use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::{
    http::HeaderMap,
    middleware,
    routing::{delete, get, post, put},
    Router,
};
use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use michi_config::Config;
use michi_security::SecurityState;
use michi_sync::PlaybackState;
use michi_sync::SyncManager;
use sqlx::SqlitePool;
use tokio::sync::{broadcast, RwLock};
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;
use tower_http::compression::CompressionLayer;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing::{info, warn};
use utoipa::OpenApi;
use uuid::Uuid;

pub mod aircat_grants;
pub mod aircat_original_file;
pub mod aircat_producer_security;
pub mod assets;
pub mod auth;
pub mod home_tls_policy;
mod library;
mod openapi;
mod players;
pub mod pwa;
mod rooms;
pub mod root;
mod scrobble;
pub mod server_caps;
mod static_files;
mod status;
mod stream;
mod sync_api;
pub mod sync_ws;
mod transcode;
mod ws;

pub mod output;
pub use output::{PlaybackOutputSelection, ResolvedOutputPlan};
pub mod playback_projection;
pub use playback_projection::PlaybackProjectionHealth;
pub mod playback_queue;
pub mod routes;
pub use routes::v1::audit::record_audit;

use openapi::ApiDoc;

pub use status::StatusResponse;

#[derive(Clone)]
pub struct AppHomeAuthority(pub Arc<michi_identity::HomeRootAuthority>);

impl std::fmt::Debug for AppHomeAuthority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HomeRootAuthority")
            .field("home_id", &self.0.home_id())
            .field("public_key", &self.0.public_key_base64url())
            .finish()
    }
}

impl std::ops::Deref for AppHomeAuthority {
    type Target = michi_identity::HomeRootAuthority;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug, Clone)]
pub struct AppState {
    pub config: Config,
    pub db: SqlitePool,
    pub tx: broadcast::Sender<String>,
    pub playback_state: Arc<RwLock<PlaybackState>>,
    pub sync_tx: broadcast::Sender<michi_sync::SyncMessage>,
    pub auth_sessions: auth::AuthState,
    pub auth_enabled: bool,
    pub admin_user_id: Option<Uuid>,
    pub started_at: Instant,
    pub transcode_profiles: Arc<RwLock<Vec<crate::transcode::TranscodeProfile>>>,
    pub token_store: michi_link::TokenStore,
    pub receiver_manager: michi_receivers::ReceiverSessionManager,
    pub sync_manager: SyncManager,
    pub security_state: SecurityState,
    pub disabled_modules: Arc<RwLock<HashSet<String>>>,
    pub shutdown_token: CancellationToken,
    pub module_tokens: Arc<RwLock<HashMap<String, CancellationToken>>>,
    pub job_cancel_tokens: Arc<RwLock<HashMap<String, CancellationToken>>>,
    pub task_handles: Arc<std::sync::Mutex<Vec<tokio::task::JoinHandle<()>>>>,
    /// Canonical server identity (Ed25519 + BLAKE3), shared with michi-connect.
    pub identity: Arc<michi_identity::IdentityManager>,
    /// Canonical v1-lite pairing registry (RAM-only sessions, rate limited).
    pub pairing_registry: Arc<michi_identity::PairingRegistry>,
    /// Testability observer for generated pairing PINs without leaking over network.
    pub pairing_display: Arc<RwLock<Option<String>>>,
    /// Testability observer mapping session_id -> PIN for safe concurrent pairing.
    pub pairing_sessions_display: Arc<RwLock<HashMap<String, String>>>,
    /// Real autonomous playback engine handle.
    pub playback_engine: michi_playback::PlaybackEngineHandle,
    /// Playback projection coordinator for authorative single-writer persistence.
    pub playback_projection: playback_projection::PlaybackProjectionCoordinator,
    /// Observable health metrics of the playback projection coordinator.
    pub playback_projection_health: Arc<RwLock<PlaybackProjectionHealth>>,
    /// Explicit output target selection (Receiver, RoomGroup, or Chain).
    pub playback_output_selection: Arc<RwLock<Option<output::PlaybackOutputSelection>>>,
    /// Encrypted credential store for paired receivers (None if persistent key unavailable).
    pub receiver_credential_store: Arc<Option<michi_receivers::ReceiverCredentialStore>>,
    /// Resource-bounded transcode semaphore derived from ResourceProfile.max_transcodes.
    pub transcode_semaphore: Arc<tokio::sync::Semaphore>,
    /// In-memory store for pending PawPass playback transfers awaiting authority commit.
    pub pending_transfers: Arc<michi_sync::playback_transfer::PendingTransferStore>,
    /// Shared in-memory Scent presence store tracking live signed peer announcements.
    pub scent_store: Arc<michi_connect::ScentStore>,
    /// Observable metrics for Whisker UDP multicast discovery.
    pub whisker_metrics: Arc<michi_connect::WhiskerMetrics>,
    /// Per-module transition mutexes to serialize lifecycle changes without holding global locks during I/O.
    pub module_transition_locks:
        Arc<tokio::sync::Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>>,
    /// Authoritative module runtime observability tracking generation, health, actual state, and last error.
    pub module_runtime_info: Arc<RwLock<HashMap<String, ModuleRuntimeInfo>>>,
    /// Root Authority for Home trust domain and device credentials.
    pub home_root_authority: AppHomeAuthority,
    /// Server's signed membership certificate within this home.
    pub server_membership: michi_identity::DeviceMembershipDto,
    /// On-demand, bounded original-file snapshot and anti-replay state.
    pub aircat_security: Arc<aircat_producer_security::AirCatProducerSecurityState>,
    /// Real HTTPS/TLS configuration and RFC 7469 SPKI SHA-256 pin when TLS is active.
    pub aircat_tls: Arc<RwLock<Option<AirCatTlsInfo>>>,
    /// True ONLY once the real tokio-rustls HTTPS listener is bound and serving.
    pub aircat_tls_listener_ready: Arc<std::sync::atomic::AtomicBool>,
    /// Single authoritative coordinator for active queue mutations, monotonic versioning,
    /// and fail-closed degraded state detection when SQLite and PlaybackEngine diverge.
    pub queue_coordinator: Arc<playback_queue::ActiveQueueCoordinator>,
}

/// Marker inserted into request extensions exclusively by the HTTPS/TLS accept loop.
#[derive(Clone, Copy, Debug)]
pub struct AirCatTlsVerifiedConnection;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum TlsTrustMode {
    /// Legacy coarse structural indicator: CA-issued certificate with a verified
    /// X.509 chain present. NEVER implies Android default X509TrustManager acceptance.
    CaChained,
    /// Single self-signed or self-issued leaf certificate. Android's default
    /// X509TrustManager rejects self-signed certificates even with matching SAN + SPKI pin.
    #[default]
    SelfSigned,
    /// CA-issued leaf without a complete/valid verified chain (missing intermediate,
    /// out-of-order chain, expired intermediate, or signature verification failure).
    IncompleteChain,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum TlsCertificateProfile {
    /// Single self-issued leaf whose cryptographic signature verifies against its own SPKI.
    SelfSignedVerified,
    /// Leaf has issuer == subject (self-issued), but cryptographic self-signature did not verify.
    SelfIssuedUnverified,
    /// CA-issued leaf accompanied by an ordered, valid, cryptographically verified chain.
    CaIssuedChainPresent,
    /// CA-issued leaf missing issuer/intermediate certs, out of order, expired intermediate,
    /// non-CA intermediate, or failed cryptographic chain verification.
    IncompleteChain,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum AndroidTrustVerificationStatus {
    /// Server has NOT verified trust against Android's platform X509TrustManager.
    /// Presenting a CA chain never proves Android default trust acceptance.
    #[default]
    NotVerified,
    /// Verified cryptographically against an explicit trust root bundle.
    VerifiedWithExplicitTrustRoots,
    /// Known incompatible with default Android trust (e.g., self-signed, broken chain,
    /// or failed explicit trust root verification).
    VerificationFailed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AirCatTlsInfo {
    pub pinned_base_url: String,
    pub tls_spki_sha256: String,
    pub cert_path: std::path::PathBuf,
    pub key_path: std::path::PathBuf,
    pub tls_port: u16,
    #[serde(default)]
    pub tls_trust_mode: TlsTrustMode,
    #[serde(default)]
    pub certificate_profile: TlsCertificateProfile,
    #[serde(default)]
    pub chain_cryptographically_verified: bool,
    #[serde(default)]
    pub android_trust_verification_status: AndroidTrustVerificationStatus,
    #[serde(default)]
    pub cert_chain_len: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cert_not_before: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cert_not_after: Option<String>,
}

/// Verified X.509 metadata extracted from a PEM certificate chain and private key.
/// Strictly separates TLS readiness, X.509 structure/chain verification, SPKI pin,
/// and Android client trust verification (never inferring Android trust from chain presence).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedX509Metadata {
    pub tls_spki_sha256: String,
    pub tls_trust_mode: TlsTrustMode,
    pub certificate_profile: TlsCertificateProfile,
    pub chain_cryptographically_verified: bool,
    pub android_trust_verification_status: AndroidTrustVerificationStatus,
    pub cert_chain_len: usize,
    pub not_before: chrono::DateTime<chrono::Utc>,
    pub not_after: chrono::DateTime<chrono::Utc>,
}

/// Validates a PEM certificate chain + private key for AirCat/Home HTTPS:
/// 1. Parses PEM certs (>= 1) and private key, verifying cryptographic cert/key match via rustls.
/// 2. Verifies leaf X.509 validity window (`notBefore <= now <= notAfter`).
/// 3. Verifies leaf X.509 v3 SubjectAltName covers `pinned_base_url` host/IP.
/// 4. Verifies leaf ExtendedKeyUsage permits `serverAuth` when the EKU extension is present.
/// 5. Computes RFC 7469 SPKI SHA-256 pin and cryptographically classifies `TlsCertificateProfile`
///    and `AndroidTrustVerificationStatus` without inferring Android default trust from chain presence.
pub fn validate_x509_for_home_tls(
    cert_pem: &[u8],
    key_pem: &[u8],
    pinned_base_url: &str,
    now_utc: chrono::DateTime<chrono::Utc>,
) -> Result<VerifiedX509Metadata, String> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let certs: Vec<rustls::pki_types::CertificateDer<'static>> =
        rustls_pemfile::certs(&mut std::io::Cursor::new(cert_pem))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("invalid TLS certificate PEM: {e}"))?;
    if certs.is_empty() {
        return Err("no X.509 certificate found in TLS cert PEM".into());
    }
    let key = rustls_pemfile::private_key(&mut std::io::Cursor::new(key_pem))
        .map_err(|e| format!("invalid TLS private key PEM: {e}"))?
        .ok_or_else(|| "no private key found in TLS key PEM".to_string())?;

    // Verify cert chain + key match cryptographically via rustls ServerConfig
    let cert_chain_len = certs.len();
    let leaf_der = certs[0].as_ref().to_vec();
    rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs.clone(), key)
        .map_err(|e| format!("TLS certificate and private key mismatch or invalid: {e}"))?;

    let tls_spki_sha256 = extract_spki_sha256_from_pem(&leaf_der)
        .ok_or_else(|| "failed to extract SPKI SHA-256 from leaf X.509 certificate".to_string())?;

    let (not_before, not_after) = extract_x509_validity(&leaf_der)
        .ok_or_else(|| "failed to parse X.509 validity window (notBefore/notAfter)".to_string())?;
    if now_utc < not_before {
        return Err(format!(
            "TLS certificate is not yet valid (notBefore={})",
            not_before.to_rfc3339()
        ));
    }
    if now_utc > not_after {
        return Err(format!(
            "TLS certificate is expired (notAfter={})",
            not_after.to_rfc3339()
        ));
    }

    if !validate_x509_san_for_url(&leaf_der, pinned_base_url) {
        return Err(format!(
            "X.509 SubjectAltName does not match pinned_base_url '{pinned_base_url}'"
        ));
    }

    if !x509_eku_permits_server_auth(&leaf_der) {
        return Err(
            "X.509 ExtendedKeyUsage on leaf certificate does not permit TLS serverAuth".into(),
        );
    }

    let self_issued = is_x509_self_issued(&leaf_der);
    let (
        certificate_profile,
        chain_cryptographically_verified,
        tls_trust_mode,
        android_trust_verification_status,
    ) = if cert_chain_len == 1 {
        match self_issued {
            Some(true) => {
                if verify_x509_signature_by_issuer(&leaf_der, &leaf_der) {
                    (
                        TlsCertificateProfile::SelfSignedVerified,
                        false,
                        TlsTrustMode::SelfSigned,
                        AndroidTrustVerificationStatus::VerificationFailed,
                    )
                } else {
                    (
                        TlsCertificateProfile::SelfIssuedUnverified,
                        false,
                        TlsTrustMode::SelfSigned,
                        AndroidTrustVerificationStatus::VerificationFailed,
                    )
                }
            }
            Some(false) => {
                // CA-issued leaf presented without any intermediate/issuer certificate.
                // Chain is incomplete locally; Android trust is not verified.
                (
                    TlsCertificateProfile::IncompleteChain,
                    false,
                    TlsTrustMode::IncompleteChain,
                    AndroidTrustVerificationStatus::NotVerified,
                )
            }
            None => (
                TlsCertificateProfile::Unknown,
                false,
                TlsTrustMode::SelfSigned,
                AndroidTrustVerificationStatus::VerificationFailed,
            ),
        }
    } else if self_issued == Some(true) {
        // Multi-cert PEM starting with a self-issued leaf is structurally invalid as a CA chain
        let profile = if verify_x509_signature_by_issuer(&leaf_der, &leaf_der) {
            TlsCertificateProfile::SelfSignedVerified
        } else {
            TlsCertificateProfile::SelfIssuedUnverified
        };
        (
            profile,
            false,
            TlsTrustMode::SelfSigned,
            AndroidTrustVerificationStatus::VerificationFailed,
        )
    } else {
        match verify_presented_ca_chain(&certs, pinned_base_url, now_utc) {
            Ok(()) => (
                TlsCertificateProfile::CaIssuedChainPresent,
                true,
                TlsTrustMode::CaChained,
                // CRITICAL (R1.2-P0-B): A valid presented chain NEVER proves acceptance
                // by Android's platform X509TrustManager (e.g. could be an unknown private CA).
                AndroidTrustVerificationStatus::NotVerified,
            ),
            Err(reason) => {
                tracing::warn!(
                    pinned_base_url = %pinned_base_url,
                    reason = %reason,
                    "Presented X.509 chain failed structural/cryptographic verification"
                );
                (
                    TlsCertificateProfile::IncompleteChain,
                    false,
                    TlsTrustMode::IncompleteChain,
                    AndroidTrustVerificationStatus::VerificationFailed,
                )
            }
        }
    };

    Ok(VerifiedX509Metadata {
        tls_spki_sha256,
        tls_trust_mode,
        certificate_profile,
        chain_cryptographically_verified,
        android_trust_verification_status,
        cert_chain_len,
        not_before,
        not_after,
    })
}

/// Cryptographically verifies a presented PEM certificate chain against an explicit
/// trust root PEM bundle (using `rustls-webpki`), validating chain signatures,
/// BasicConstraints, KeyUsage/EKU `serverAuth`, validity windows, and SAN/hostname.
/// Note: Passing against a custom test/private CA bundle NEVER proves acceptance by
/// Android's default platform `X509TrustManager`.
pub fn verify_x509_against_explicit_roots(
    cert_pem: &[u8],
    trust_roots_pem: &[u8],
    pinned_base_url: &str,
    now_utc: chrono::DateTime<chrono::Utc>,
) -> Result<(), String> {
    let certs: Vec<rustls::pki_types::CertificateDer<'static>> =
        rustls_pemfile::certs(&mut std::io::Cursor::new(cert_pem))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("invalid certificate PEM: {e}"))?;
    if certs.is_empty() {
        return Err("no certificates in presented PEM".into());
    }
    let root_certs: Vec<rustls::pki_types::CertificateDer<'static>> =
        rustls_pemfile::certs(&mut std::io::Cursor::new(trust_roots_pem))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("invalid trust root PEM: {e}"))?;
    if root_certs.is_empty() {
        return Err("no certificates in trust roots PEM".into());
    }

    let anchors: Vec<rustls::pki_types::TrustAnchor<'static>> = root_certs
        .iter()
        .map(|c| {
            webpki::anchor_from_trusted_cert(c)
                .map(|a| a.to_owned())
                .map_err(|e| format!("invalid trust anchor: {e:?}"))
        })
        .collect::<Result<Vec<_>, _>>()?;

    let ee = webpki::EndEntityCert::try_from(&certs[0])
        .map_err(|e| format!("leaf certificate rejected by webpki: {e:?}"))?;
    let unix_time = rustls::pki_types::UnixTime::since_unix_epoch(std::time::Duration::from_secs(
        now_utc.timestamp().max(0) as u64,
    ));
    let intermediates = if certs.len() > 1 { &certs[1..] } else { &[] };
    ee.verify_for_usage(
        webpki::ALL_VERIFICATION_ALGS,
        &anchors,
        intermediates,
        unix_time,
        webpki::KeyUsage::server_auth(),
        None,
        None,
    )
    .map_err(|e| format!("X.509 chain verification against explicit roots failed: {e:?}"))?;

    let server_name = extract_server_name_from_https_url(pinned_base_url)?;
    ee.verify_is_valid_for_subject_name(&server_name)
        .map_err(|e| format!("X.509 subject name verification failed: {e:?}"))?;

    Ok(())
}

fn extract_server_name_from_https_url(
    https_url: &str,
) -> Result<rustls::pki_types::ServerName<'static>, String> {
    let parsed = reqwest::Url::parse(https_url)
        .map_err(|e| format!("invalid pinned_base_url '{https_url}': {e}"))?;
    if parsed.scheme() != "https" {
        return Err("pinned_base_url must use https scheme".into());
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| "pinned_base_url missing host".to_string())?;
    let trimmed = host.trim_start_matches('[').trim_end_matches(']');
    rustls::pki_types::ServerName::try_from(trimmed.to_string())
        .map_err(|_| format!("invalid TLS server name '{trimmed}'"))
}

/// Verifies that a multi-certificate chain (`certs.len() >= 2`) is strictly ordered
/// (`certs[i].issuer == certs[i+1].subject`), every issuer/intermediate cert is within
/// its validity window and carries `BasicConstraints (CA:TRUE)`, and the cryptographic
/// signatures and `serverAuth` usage verify cleanly with `rustls-webpki`.
fn verify_presented_ca_chain(
    certs: &[rustls::pki_types::CertificateDer<'static>],
    pinned_base_url: &str,
    now_utc: chrono::DateTime<chrono::Utc>,
) -> Result<(), String> {
    if certs.len() < 2 {
        return Err("chain has fewer than 2 certificates".into());
    }

    // 1. Strict order check: each certificate's issuer MUST match the next certificate's subject
    for idx in 0..certs.len() - 1 {
        let (child_issuer, _) = extract_x509_issuer_and_subject(certs[idx].as_ref())
            .ok_or_else(|| format!("failed to parse issuer/subject at chain index {idx}"))?;
        let (_, parent_subject) = extract_x509_issuer_and_subject(certs[idx + 1].as_ref())
            .ok_or_else(|| format!("failed to parse issuer/subject at chain index {}", idx + 1))?;
        if child_issuer != parent_subject {
            return Err(format!(
                "X.509 chain out of order or disconnected between index {idx} and {}",
                idx + 1
            ));
        }
    }

    // 2. Every issuer/intermediate certificate in certs[1..] MUST be within its validity window
    //    and carry BasicConstraints CA:TRUE
    for (offset, cert) in certs[1..].iter().enumerate() {
        let idx = offset + 1;
        let (nb, na) = extract_x509_validity(cert.as_ref())
            .ok_or_else(|| format!("failed to parse validity for chain cert index {idx}"))?;
        if now_utc < nb || now_utc > na {
            return Err(format!(
                "intermediate/issuer certificate at index {idx} is outside validity window"
            ));
        }
        if !x509_basic_constraints_is_ca(cert.as_ref()) {
            return Err(format!(
                "chain certificate at index {idx} lacks BasicConstraints CA:TRUE"
            ));
        }
    }

    // 3. Cryptographic verification of leaf -> intermediates -> presented chain tip using rustls-webpki
    let ee = webpki::EndEntityCert::try_from(&certs[0])
        .map_err(|e| format!("leaf cert rejected as EndEntityCert: {e:?}"))?;
    let tip_anchor = webpki::anchor_from_trusted_cert(certs.last().unwrap())
        .map_err(|e| format!("chain tip invalid as trust anchor: {e:?}"))?
        .to_owned();
    let intermediates = &certs[1..certs.len() - 1];
    let unix_time = rustls::pki_types::UnixTime::since_unix_epoch(std::time::Duration::from_secs(
        now_utc.timestamp().max(0) as u64,
    ));
    ee.verify_for_usage(
        webpki::ALL_VERIFICATION_ALGS,
        &[tip_anchor],
        intermediates,
        unix_time,
        webpki::KeyUsage::server_auth(),
        None,
        None,
    )
    .map_err(|e| format!("cryptographic X.509 chain verification failed: {e:?}"))?;

    let server_name = extract_server_name_from_https_url(pinned_base_url)?;
    ee.verify_is_valid_for_subject_name(&server_name)
        .map_err(|e| format!("leaf SAN verification failed: {e:?}"))?;

    Ok(())
}

impl AirCatTlsInfo {
    pub fn from_env_and_config(config: &michi_config::Config) -> Option<Self> {
        let cert_path = std::env::var("MICHI_TLS_CERT_PATH")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| config.config_path.join("tls.crt"));
        let key_path = std::env::var("MICHI_TLS_KEY_PATH")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| config.config_path.join("tls.key"));

        if !cert_path.is_file() || !key_path.is_file() {
            return None;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(meta) = std::fs::metadata(&key_path) {
                let mode = meta.permissions().mode() & 0o777;
                if mode & 0o077 != 0 {
                    tracing::warn!(
                        path = %key_path.display(),
                        mode = format!("{mode:04o}"),
                        "TLS private key has overly permissive permissions; tightening to 0600 is recommended"
                    );
                }
            }
        }
        let cert_pem = std::fs::read(&cert_path).ok()?;
        let key_pem = std::fs::read(&key_path).ok()?;
        let tls_port = std::env::var("MICHI_TLS_PORT")
            .ok()
            .and_then(|v| v.parse::<u16>().ok())
            .unwrap_or_else(|| config.port.saturating_add(353));
        let pinned_base_url = std::env::var("MICHI_PUBLIC_HTTPS_URL")
            .ok()
            .filter(|u| u.starts_with("https://"))
            .unwrap_or_else(|| {
                let host = michi_connect::MichiConnect::resolve_lan_ip();
                format!("https://{host}:{tls_port}")
            });

        let verified = match validate_x509_for_home_tls(
            &cert_pem,
            &key_pem,
            &pinned_base_url,
            chrono::Utc::now(),
        ) {
            Ok(v) => v,
            Err(reason) => {
                tracing::error!(
                    cert = %cert_path.display(),
                    pinned_base_url = %pinned_base_url,
                    reason = %reason,
                    "Refusing Home/AirCat TLS activation due to invalid X.509 certificate or key"
                );
                return None;
            }
        };

        if matches!(
            verified.certificate_profile,
            TlsCertificateProfile::SelfSignedVerified | TlsCertificateProfile::SelfIssuedUnverified
        ) {
            tracing::warn!(
                cert = %cert_path.display(),
                pinned_base_url = %pinned_base_url,
                profile = ?verified.certificate_profile,
                "Loaded self-signed/self-issued TLS certificate: Android's default X509TrustManager rejects self-signed certificates even with matching SAN + SPKI pin. Use a publicly trusted CA certificate (Mode A: Let's Encrypt DNS-01 / Tailscale HTTPS) for default Android trust compatibility."
            );
        } else if verified.certificate_profile == TlsCertificateProfile::IncompleteChain {
            tracing::warn!(
                cert = %cert_path.display(),
                pinned_base_url = %pinned_base_url,
                "Loaded TLS certificate with incomplete or unverified X.509 chain; clients may reject the handshake unless the full ordered chain is provided"
            );
        }

        Some(Self {
            pinned_base_url,
            tls_spki_sha256: verified.tls_spki_sha256,
            cert_path,
            key_path,
            tls_port,
            tls_trust_mode: verified.tls_trust_mode,
            certificate_profile: verified.certificate_profile,
            chain_cryptographically_verified: verified.chain_cryptographically_verified,
            android_trust_verification_status: verified.android_trust_verification_status,
            cert_chain_len: verified.cert_chain_len,
            cert_not_before: Some(verified.not_before.to_rfc3339()),
            cert_not_after: Some(verified.not_after.to_rfc3339()),
        })
    }
}

fn parse_asn1_time(tag: u8, bytes: &[u8]) -> Option<chrono::DateTime<chrono::Utc>> {
    let s = std::str::from_utf8(bytes).ok()?;
    match tag {
        // UTCTime: YYMMDDHHMMSSZ
        0x17 => {
            if s.len() != 13 || !s.ends_with('Z') {
                return None;
            }
            let yy: i32 = s[0..2].parse().ok()?;
            let year = if yy >= 50 { 1900 + yy } else { 2000 + yy };
            let month: u32 = s[2..4].parse().ok()?;
            let day: u32 = s[4..6].parse().ok()?;
            let hour: u32 = s[6..8].parse().ok()?;
            let min: u32 = s[8..10].parse().ok()?;
            let sec: u32 = s[10..12].parse().ok()?;
            chrono::NaiveDate::from_ymd_opt(year, month, day)?
                .and_hms_opt(hour, min, sec)
                .map(|ndt| {
                    chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(ndt, chrono::Utc)
                })
        }
        // GeneralizedTime: YYYYMMDDHHMMSSZ
        0x18 => {
            if s.len() != 15 || !s.ends_with('Z') {
                return None;
            }
            let year: i32 = s[0..4].parse().ok()?;
            let month: u32 = s[4..6].parse().ok()?;
            let day: u32 = s[6..8].parse().ok()?;
            let hour: u32 = s[8..10].parse().ok()?;
            let min: u32 = s[10..12].parse().ok()?;
            let sec: u32 = s[12..14].parse().ok()?;
            chrono::NaiveDate::from_ymd_opt(year, month, day)?
                .and_hms_opt(hour, min, sec)
                .map(|ndt| {
                    chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(ndt, chrono::Utc)
                })
        }
        _ => None,
    }
}

/// Extracts `(notBefore, notAfter)` UTC timestamps from a DER X.509 certificate.
pub fn extract_x509_validity(
    cert_der: &[u8],
) -> Option<(chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>)> {
    let (cert_tag, cert_body, _) = read_der_tlv(cert_der)?;
    if cert_tag != 0x30 {
        return None;
    }
    let (tbs_tag, mut tbs_rest, _) = read_der_tlv(cert_body)?;
    if tbs_tag != 0x30 {
        return None;
    }
    if tbs_rest.first().copied() == Some(0xA0) {
        let (_, _, after_ver) = read_der_tlv(tbs_rest)?;
        tbs_rest = after_ver;
    }
    // Skip serialNumber, signature, issuer (3 elements) -> validity is 4th
    for _ in 0..3 {
        let (_, _, next) = read_der_tlv(tbs_rest)?;
        tbs_rest = next;
    }
    let (val_tag, val_body, _) = read_der_tlv(tbs_rest)?;
    if val_tag != 0x30 {
        return None;
    }
    let (nb_tag, nb_bytes, after_nb) = read_der_tlv(val_body)?;
    let (na_tag, na_bytes, _) = read_der_tlv(after_nb)?;
    Some((
        parse_asn1_time(nb_tag, nb_bytes)?,
        parse_asn1_time(na_tag, na_bytes)?,
    ))
}

/// Extracts `(issuer_der_body, subject_der_body)` from a DER X.509 certificate.
pub fn extract_x509_issuer_and_subject(cert_der: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    let (cert_tag, cert_body, _) = read_der_tlv(cert_der)?;
    if cert_tag != 0x30 {
        return None;
    }
    let (tbs_tag, mut tbs_rest, _) = read_der_tlv(cert_body)?;
    if tbs_tag != 0x30 {
        return None;
    }
    if tbs_rest.first().copied() == Some(0xA0) {
        let (_, _, after_ver) = read_der_tlv(tbs_rest)?;
        tbs_rest = after_ver;
    }
    // Skip serialNumber, signature
    for _ in 0..2 {
        let (_, _, next) = read_der_tlv(tbs_rest)?;
        tbs_rest = next;
    }
    let (_, issuer_body, after_issuer) = read_der_tlv(tbs_rest)?;
    let (_, _, after_validity) = read_der_tlv(after_issuer)?;
    let (_, subject_body, _) = read_der_tlv(after_validity)?;
    Some((issuer_body.to_vec(), subject_body.to_vec()))
}

/// Returns `true` when the X.509 DER certificate's issuer Name equals its subject Name.
/// Note: equality of issuer and subject indicates *self-issued*, NOT cryptographic self-signature.
pub fn is_x509_self_issued(cert_der: &[u8]) -> Option<bool> {
    let (issuer, subject) = extract_x509_issuer_and_subject(cert_der)?;
    Some(issuer == subject)
}

/// Finds an X.509 v3 extension's OCTET STRING value by raw DER OID bytes.
fn find_x509_extension<'a>(cert_der: &'a [u8], target_oid: &[u8]) -> Option<&'a [u8]> {
    let (cert_tag, cert_body, _) = read_der_tlv(cert_der)?;
    if cert_tag != 0x30 {
        return None;
    }
    let (tbs_tag, mut tbs_rest, _) = read_der_tlv(cert_body)?;
    if tbs_tag != 0x30 {
        return None;
    }
    if tbs_rest.first().copied() == Some(0xA0) {
        let (_, _, after_ver) = read_der_tlv(tbs_rest)?;
        tbs_rest = after_ver;
    }
    // Skip: serialNumber, signature, issuer, validity, subject, subjectPublicKeyInfo
    for _ in 0..6 {
        let (_, _, next) = read_der_tlv(tbs_rest)?;
        tbs_rest = next;
    }
    while !tbs_rest.is_empty() {
        let (tag, body, next) = read_der_tlv(tbs_rest)?;
        tbs_rest = next;
        if tag != 0xA3 {
            continue;
        }
        let (ext_seq_tag, mut ext_rest, _) = read_der_tlv(body)?;
        if ext_seq_tag != 0x30 {
            return None;
        }
        while !ext_rest.is_empty() {
            let (ext_tag, ext_body, next_ext) = read_der_tlv(ext_rest)?;
            ext_rest = next_ext;
            if ext_tag != 0x30 {
                continue;
            }
            let (oid_tag, oid_bytes, mut after_oid) = read_der_tlv(ext_body)?;
            if oid_tag != 0x06 || oid_bytes != target_oid {
                continue;
            }
            if after_oid.first().copied() == Some(0x01) {
                let (_, _, next_f) = read_der_tlv(after_oid)?;
                after_oid = next_f;
            }
            let (oct_tag, oct_body, _) = read_der_tlv(after_oid)?;
            if oct_tag == 0x04 {
                return Some(oct_body);
            }
        }
    }
    None
}

/// Returns `true` if BasicConstraints (OID 2.5.29.19) is present and has `cA = TRUE`.
pub fn x509_basic_constraints_is_ca(cert_der: &[u8]) -> bool {
    // OID 2.5.29.19 (basicConstraints) = [0x55, 0x1D, 0x13]
    let Some(ext_val) = find_x509_extension(cert_der, &[0x55, 0x1D, 0x13]) else {
        return false;
    };
    let Some((seq_tag, seq_body, _)) = read_der_tlv(ext_val) else {
        return false;
    };
    if seq_tag != 0x30 || seq_body.is_empty() {
        return false;
    }
    if let Some((tag, val, _)) = read_der_tlv(seq_body) {
        tag == 0x01 && val.first().copied().unwrap_or(0) != 0
    } else {
        false
    }
}

/// Returns `true` if ExtendedKeyUsage (OID 2.5.29.37) is absent OR permits
/// `id-kp-serverAuth` (1.3.6.1.5.5.7.3.1) / `anyExtendedKeyUsage` (2.5.29.37.0).
pub fn x509_eku_permits_server_auth(cert_der: &[u8]) -> bool {
    // OID 2.5.29.37 (extKeyUsage) = [0x55, 0x1D, 0x25]
    let Some(ext_val) = find_x509_extension(cert_der, &[0x55, 0x1D, 0x25]) else {
        return true;
    };
    let Some((seq_tag, mut seq_rest, _)) = read_der_tlv(ext_val) else {
        return false;
    };
    if seq_tag != 0x30 {
        return false;
    }
    const OID_SERVER_AUTH: &[u8] = &[0x2B, 0x06, 0x01, 0x05, 0x05, 0x07, 0x03, 0x01];
    const OID_ANY_EKU: &[u8] = &[0x55, 0x1D, 0x25, 0x00];
    while !seq_rest.is_empty() {
        let Some((tag, oid_bytes, next)) = read_der_tlv(seq_rest) else {
            return false;
        };
        seq_rest = next;
        if tag == 0x06 && (oid_bytes == OID_SERVER_AUTH || oid_bytes == OID_ANY_EKU) {
            return true;
        }
    }
    false
}

/// Cryptographically verifies `child_der`'s X.509 signature using `issuer_der`'s
/// SubjectPublicKeyInfo via `ring`. Used to distinguish a genuinely self-signed
/// certificate (`SelfSignedVerified`) from a self-issued certificate signed by a
/// different key (`SelfIssuedUnverified`).
pub fn verify_x509_signature_by_issuer(child_der: &[u8], issuer_der: &[u8]) -> bool {
    let Some((cert_tag, cert_body, _)) = read_der_tlv(child_der) else {
        return false;
    };
    if cert_tag != 0x30 {
        return false;
    }
    // tbsCertificate exact TLV slice (bytes covered by X.509 signature)
    let Some((tbs_tag, _, after_tbs)) = read_der_tlv(cert_body) else {
        return false;
    };
    if tbs_tag != 0x30 {
        return false;
    }
    let tbs_len = cert_body.len().saturating_sub(after_tbs.len());
    let tbs_tlv = &cert_body[..tbs_len];

    // signatureAlgorithm AlgorithmIdentifier SEQUENCE
    let Some((alg_tag, alg_body, after_alg)) = read_der_tlv(after_tbs) else {
        return false;
    };
    if alg_tag != 0x30 {
        return false;
    }
    let Some((oid_tag, sig_alg_oid, _)) = read_der_tlv(alg_body) else {
        return false;
    };
    if oid_tag != 0x06 {
        return false;
    }

    // signatureValue BIT STRING (leading unused-bits octet must be 0)
    let Some((sig_tag, sig_bits, _)) = read_der_tlv(after_alg) else {
        return false;
    };
    if sig_tag != 0x03 || sig_bits.len() < 2 || sig_bits[0] != 0 {
        return false;
    }
    let sig_bytes = &sig_bits[1..];

    // Extract issuer SPKI public key BIT STRING bytes
    let Some(spki_tlv) = extract_x509_spki_der(issuer_der) else {
        return false;
    };
    let Some((spki_tag, spki_body, _)) = read_der_tlv(spki_tlv) else {
        return false;
    };
    if spki_tag != 0x30 {
        return false;
    }
    let Some((_, _, after_spki_alg)) = read_der_tlv(spki_body) else {
        return false;
    };
    let Some((pk_tag, pk_bits, _)) = read_der_tlv(after_spki_alg) else {
        return false;
    };
    if pk_tag != 0x03 || pk_bits.len() < 2 || pk_bits[0] != 0 {
        return false;
    }
    let pk_bytes = &pk_bits[1..];

    // Map X.509 signatureAlgorithm OID -> ring::signature::VerificationAlgorithm
    let alg: &'static dyn ring::signature::VerificationAlgorithm = match sig_alg_oid {
        // ecdsa-with-SHA256 (1.2.840.10045.4.3.2)
        [0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x04, 0x03, 0x02] => {
            &ring::signature::ECDSA_P256_SHA256_ASN1
        }
        // ecdsa-with-SHA384 (1.2.840.10045.4.3.3)
        [0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x04, 0x03, 0x03] => {
            &ring::signature::ECDSA_P384_SHA384_ASN1
        }
        // id-Ed25519 (1.3.101.112)
        [0x2B, 0x65, 0x70] => &ring::signature::ED25519,
        // sha256WithRSAEncryption (1.2.840.113549.1.1.11)
        [0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x0B] => {
            &ring::signature::RSA_PKCS1_2048_8192_SHA256
        }
        // sha384WithRSAEncryption (1.2.840.113549.1.1.12)
        [0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x0C] => {
            &ring::signature::RSA_PKCS1_2048_8192_SHA384
        }
        // sha512WithRSAEncryption (1.2.840.113549.1.1.13)
        [0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x0D] => {
            &ring::signature::RSA_PKCS1_2048_8192_SHA512
        }
        _ => return false,
    };

    ring::signature::UnparsedPublicKey::new(alg, pk_bytes)
        .verify(tbs_tlv, sig_bytes)
        .is_ok()
}

/// Parses the first X.509 PEM certificate (or raw DER) into DER bytes.
fn pem_or_der_to_cert_der(pem_or_der: &[u8]) -> Option<Vec<u8>> {
    use base64::Engine;
    if let Ok(text) = std::str::from_utf8(pem_or_der) {
        if let Some(start) = text.find("-----BEGIN CERTIFICATE-----") {
            let after_header = &text[start + "-----BEGIN CERTIFICATE-----".len()..];
            let end = after_header.find("-----END CERTIFICATE-----")?;
            let b64: String = after_header[..end]
                .chars()
                .filter(|c| !c.is_ascii_whitespace())
                .collect();
            return base64::engine::general_purpose::STANDARD.decode(b64).ok();
        }
    }
    Some(pem_or_der.to_vec())
}

/// Validates that the X.509 v3 certificate includes a SubjectAltName (OID 2.5.29.17)
/// matching the hostname or IP address of `https_url`, as required by Android
/// `X509TrustManager` + hostname verification alongside RFC 7469 SPKI pinning.
pub fn validate_x509_san_for_url(pem_or_der: &[u8], https_url: &str) -> bool {
    let Ok(parsed_url) = reqwest::Url::parse(https_url) else {
        return false;
    };
    if parsed_url.scheme() != "https" {
        return false;
    }
    let Some(host_str) = parsed_url.host_str() else {
        return false;
    };
    let Some(der) = pem_or_der_to_cert_der(pem_or_der) else {
        return false;
    };
    let Some((dns_names, ip_addrs)) = extract_x509_san_entries(&der) else {
        return false;
    };

    let trimmed = host_str.trim_start_matches('[').trim_end_matches(']');
    if let Ok(ip) = trimmed.parse::<std::net::IpAddr>() {
        ip_addrs.contains(&ip)
    } else {
        let target = trimmed.to_ascii_lowercase();
        dns_names.iter().any(|san| {
            let san_l = san.to_ascii_lowercase();
            if san_l == target {
                true
            } else if let Some(suffix) = san_l.strip_prefix("*.") {
                if let Some((_, rest)) = target.split_once('.') {
                    rest == suffix && !rest.is_empty()
                } else {
                    false
                }
            } else {
                false
            }
        })
    }
}

/// Extracts DNS (dNSName, tag 0x82) and IP (iPAddress, tag 0x87) SubjectAltName
/// entries from an X.509 v3 DER certificate.
pub fn extract_x509_san_entries(cert_der: &[u8]) -> Option<(Vec<String>, Vec<std::net::IpAddr>)> {
    let (cert_tag, cert_body, _) = read_der_tlv(cert_der)?;
    if cert_tag != 0x30 {
        return None;
    }
    let (tbs_tag, mut tbs_rest, _) = read_der_tlv(cert_body)?;
    if tbs_tag != 0x30 {
        return None;
    }
    // Optional explicit [0] version
    if tbs_rest.first().copied() == Some(0xA0) {
        let (_, _, after_ver) = read_der_tlv(tbs_rest)?;
        tbs_rest = after_ver;
    }
    // Skip: serialNumber, signature, issuer, validity, subject, subjectPublicKeyInfo (6 elements)
    for _ in 0..6 {
        let (_, _, next) = read_der_tlv(tbs_rest)?;
        tbs_rest = next;
    }
    // Scan optional trailing fields for explicit [3] extensions (tag 0xA3)
    while !tbs_rest.is_empty() {
        let (tag, body, next) = read_der_tlv(tbs_rest)?;
        tbs_rest = next;
        if tag != 0xA3 {
            continue;
        }
        let (ext_seq_tag, mut ext_rest, _) = read_der_tlv(body)?;
        if ext_seq_tag != 0x30 {
            return None;
        }
        while !ext_rest.is_empty() {
            let (ext_tag, ext_body, next_ext) = read_der_tlv(ext_rest)?;
            ext_rest = next_ext;
            if ext_tag != 0x30 {
                continue;
            }
            let (oid_tag, oid_bytes, mut after_oid) = read_der_tlv(ext_body)?;
            // OID 2.5.29.17 (subjectAltName) DER encoding: 55 1D 11
            if oid_tag != 0x06 || oid_bytes != [0x55, 0x1D, 0x11] {
                continue;
            }
            // Optional critical BOOLEAN (tag 0x01)
            if after_oid.first().copied() == Some(0x01) {
                let (_, _, next_f) = read_der_tlv(after_oid)?;
                after_oid = next_f;
            }
            let (oct_tag, oct_body, _) = read_der_tlv(after_oid)?;
            if oct_tag != 0x04 {
                return None;
            }
            let (gn_seq_tag, mut gn_rest, _) = read_der_tlv(oct_body)?;
            if gn_seq_tag != 0x30 {
                return None;
            }
            let mut dns_names = Vec::new();
            let mut ip_addrs = Vec::new();
            while !gn_rest.is_empty() {
                let (gn_tag, gn_val, next_gn) = read_der_tlv(gn_rest)?;
                gn_rest = next_gn;
                match gn_tag {
                    0x82 => {
                        if let Ok(s) = std::str::from_utf8(gn_val) {
                            dns_names.push(s.to_string());
                        }
                    }
                    0x87 => {
                        if gn_val.len() == 4 {
                            let octets: [u8; 4] = gn_val.try_into().ok()?;
                            ip_addrs.push(std::net::IpAddr::V4(std::net::Ipv4Addr::from(octets)));
                        } else if gn_val.len() == 16 {
                            let octets: [u8; 16] = gn_val.try_into().ok()?;
                            ip_addrs.push(std::net::IpAddr::V6(std::net::Ipv6Addr::from(octets)));
                        }
                    }
                    _ => {}
                }
            }
            return Some((dns_names, ip_addrs));
        }
    }
    None
}

/// Parses the first X.509 PEM certificate (or raw DER) and computes its RFC 7469
/// SubjectPublicKeyInfo SHA-256 pin (`sha256/<BASE64_STANDARD>`).
pub fn extract_spki_sha256_from_pem(pem_or_der: &[u8]) -> Option<String> {
    use base64::Engine;
    use sha2::{Digest, Sha256};

    let der = pem_or_der_to_cert_der(pem_or_der)?;
    let spki = extract_x509_spki_der(&der)?;
    let digest = Sha256::digest(spki);
    Some(format!(
        "sha256/{}",
        base64::engine::general_purpose::STANDARD.encode(digest)
    ))
}

fn read_der_tlv(input: &[u8]) -> Option<(u8, &[u8], &[u8])> {
    let &tag = input.first()?;
    let &len_byte = input.get(1)?;
    let (len, header_len) = if len_byte & 0x80 == 0 {
        (len_byte as usize, 2usize)
    } else {
        let num_bytes = (len_byte & 0x7f) as usize;
        if num_bytes == 0 || num_bytes > 4 || input.len() < 2 + num_bytes {
            return None;
        }
        let mut len = 0usize;
        for &b in &input[2..2 + num_bytes] {
            len = (len << 8) | (b as usize);
        }
        (len, 2 + num_bytes)
    };
    let end = header_len.checked_add(len)?;
    if end > input.len() {
        return None;
    }
    Some((tag, &input[header_len..end], &input[end..]))
}

fn extract_x509_spki_der(cert_der: &[u8]) -> Option<&[u8]> {
    let (cert_tag, cert_body, _) = read_der_tlv(cert_der)?;
    if cert_tag != 0x30 {
        return None;
    }
    let (tbs_tag, mut tbs_rest, _) = read_der_tlv(cert_body)?;
    if tbs_tag != 0x30 {
        return None;
    }
    // Optional explicit [0] version
    if tbs_rest.first().copied() == Some(0xA0) {
        let (_, _, after_ver) = read_der_tlv(tbs_rest)?;
        tbs_rest = after_ver;
    }
    // Skip: serialNumber, signature, issuer, validity, subject (5 elements)
    for _ in 0..5 {
        let (_, _, next) = read_der_tlv(tbs_rest)?;
        tbs_rest = next;
    }
    // Next element is subjectPublicKeyInfo (SEQUENCE 0x30); include full TLV header+body.
    let (spki_tag, spki_body, _) = read_der_tlv(tbs_rest)?;
    if spki_tag != 0x30 {
        return None;
    }
    let header_len = (spki_body.as_ptr() as usize) - (tbs_rest.as_ptr() as usize);
    Some(&tbs_rest[..header_len + spki_body.len()])
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModuleRuntimeInfo {
    pub generation: u64,
    pub actual_state: String,
    pub health: String,
    pub last_error: Option<String>,
}

impl AppState {
    pub fn set_aircat_tls_listener_ready(&self, ready: bool) {
        self.aircat_tls_listener_ready
            .store(ready, std::sync::atomic::Ordering::Release);
    }

    pub async fn is_aircat_tls_ready(&self) -> bool {
        self.aircat_tls.read().await.is_some()
            && self
                .aircat_tls_listener_ready
                .load(std::sync::atomic::Ordering::Acquire)
    }

    pub async fn get_module_transition_lock(
        &self,
        module_name: &str,
    ) -> Arc<tokio::sync::Mutex<()>> {
        let mut locks = self.module_transition_locks.lock().await;
        locks
            .entry(module_name.to_string())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    }

    pub fn track_task(&self, handle: tokio::task::JoinHandle<()>) {
        self.task_handles.lock().unwrap().push(handle);
    }

    pub async fn shutdown_and_wait(&self, timeout: Duration) {
        // 1. Explicit bounded projection flush while PlaybackEngine is active
        let flush_timeout = Duration::from_millis(1500).min(timeout);
        if let Err(e) =
            tokio::time::timeout(flush_timeout, self.playback_projection.flush_shutdown()).await
        {
            tracing::warn!("shutdown playback projection flush timed out: {e:?}");
        }
        // 2. Shutdown PlaybackEngine cleanly
        self.playback_engine.shutdown().await;
        // 3. Cancel remaining background tasks
        self.shutdown_token.cancel();
        // 4. Join all tracked tasks
        let handles: Vec<tokio::task::JoinHandle<()>> =
            std::mem::take(&mut *self.task_handles.lock().unwrap());
        for handle in handles {
            let _ = tokio::time::timeout(timeout, handle).await;
        }
        // 5. Explicit WAL checkpoint
        let _ = sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
            .execute(&self.db)
            .await;
    }

    /// Starts persistent discovery services: Scent sweeper (90s expiry), Whisker UDP
    /// multicast listener (224.0.0.167:53318), mDNS resolver, and ReceiverDiscoveryBridge.
    pub fn start_discovery_services(&self) {
        let cancel = self.shutdown_token.clone();

        // 1. Scent 90s presence expiry sweeper
        let sweeper_handle = self.scent_store.spawn_expiry_sweeper(cancel.clone());
        self.track_task(sweeper_handle);

        // 2. Whisker UDP multicast discovery listener
        let engine = Arc::new(michi_identity::discovery::DiscoveryEngine::new(
            self.identity.clone(),
        ));
        let whisker = Arc::new(michi_connect::WhiskerDiscoveryListener::new_with_metrics(
            engine,
            self.scent_store.clone(),
            self.whisker_metrics.clone(),
        ));
        match whisker.bind_socket() {
            Ok(socket) => {
                let whisker_clone = whisker.clone();
                let cancel_clone = cancel.clone();
                let whisker_handle = tokio::spawn(async move {
                    whisker_clone.run(socket, cancel_clone).await;
                });
                self.track_task(whisker_handle);
                tracing::info!("WhiskerDiscoveryListener started on 224.0.0.167:53318");
            }
            Err(e) => {
                tracing::warn!(
                    "Failed to bind Whisker UDP multicast socket (224.0.0.167:53318): {e}"
                );
            }
        }

        // 3. Persistent mDNS service resolver
        match michi_connect::mdns_sd::ServiceDaemon::new() {
            Ok(daemon) => {
                let resolver = michi_connect::MdnsResolver::new(self.scent_store.clone());
                let cancel_clone = cancel.clone();
                let resolver_handle = tokio::spawn(async move {
                    resolver.run(daemon, cancel_clone).await;
                });
                self.track_task(resolver_handle);
                tracing::info!("MdnsResolver started for _michi-link._tcp.local.");
            }
            Err(e) => {
                tracing::warn!("Failed to create ServiceDaemon for MdnsResolver: {e}");
            }
        }

        // 4. Discovery Bridge projecting Scent into ReceiverRegistry
        let bridge = michi_receivers::ReceiverDiscoveryBridge::new(
            self.scent_store.clone(),
            self.receiver_manager.clone(),
        );
        let bridge_handle = bridge.spawn(cancel);
        self.track_task(bridge_handle);
        tracing::info!("ReceiverDiscoveryBridge started projecting Scent into ReceiverRegistry");
    }

    pub async fn bootstrap_runtime(&self) -> Result<(), String> {
        // Synchronous restoration of persisted revocations *before* enabling auth/discovery or restoring receivers
        match self.receiver_manager.load_persisted_revocations().await {
            Ok(n) => tracing::info!(
                "loaded {} persisted home revocations synchronously from DB",
                n
            ),
            Err(e) => tracing::warn!(
                "failed to load persisted home revocations during bootstrap: {}",
                e
            ),
        }

        tracing::info!(
            "bootstrapping runtime: loading persisted receivers and restoring credentials"
        );
        match michi_db::list_receivers_db(&self.db).await {
            Ok(persisted_list) => {
                let registry_arc = self.receiver_manager.registry().await;
                let mut registry = registry_arc.write().await;
                let mut restored_count = 0;

                for rec in persisted_list {
                    let mut token: Option<String> = None;
                    if rec.paired {
                        if let Some(store) = self.receiver_credential_store.as_ref() {
                            match michi_db::get_receiver_credential_db(&self.db, &rec.id).await {
                                Ok(Some(cred)) => {
                                    match store.decrypt_token(
                                        &rec.id,
                                        &cred.ciphertext,
                                        &cred.nonce,
                                    ) {
                                        Ok(decrypted) => {
                                            token = Some(decrypted);
                                        }
                                        Err(e) => {
                                            tracing::warn!(
                                                "failed to decrypt credential for receiver {}: {} (marking unpaired/fail-closed)",
                                                rec.id,
                                                e
                                            );
                                        }
                                    }
                                }
                                Ok(None) => {
                                    tracing::warn!(
                                        "no encrypted credential record found for paired receiver {}",
                                        rec.id
                                    );
                                }
                                Err(e) => {
                                    tracing::warn!(
                                        "error querying credentials for receiver {}: {}",
                                        rec.id,
                                        e
                                    );
                                }
                            }
                        } else {
                            tracing::warn!(
                                "receiver credential store unavailable, cannot restore token for receiver {}",
                                rec.id
                            );
                        }
                    }

                    let is_paired = rec.paired && token.is_some();

                    let (
                        supported_transports,
                        supported_codecs,
                        supported_sample_rates,
                        supported_bit_depths,
                        supported_channels,
                        max_sr,
                        max_bd,
                    ) = if let Some(ref json_str) = rec.capabilities_json {
                        if let Ok(caps) =
                            serde_json::from_str::<michi_receivers::ReceiverCapabilities>(json_str)
                        {
                            (
                                caps.supported_transports,
                                caps.supported_codecs,
                                caps.supported_sample_rates,
                                caps.supported_bit_depths,
                                caps.supported_channels,
                                caps.max_sample_rate,
                                caps.max_bit_depth,
                            )
                        } else {
                            (
                                Vec::new(),
                                Vec::new(),
                                Vec::new(),
                                Vec::new(),
                                Vec::new(),
                                0,
                                0,
                            )
                        }
                    } else {
                        (
                            Vec::new(),
                            Vec::new(),
                            Vec::new(),
                            Vec::new(),
                            Vec::new(),
                            0,
                            0,
                        )
                    };

                    let capabilities_observed_at =
                        rec.capabilities_observed_at.as_deref().and_then(|s| {
                            chrono::DateTime::parse_from_rfc3339(s)
                                .ok()
                                .map(|d| d.with_timezone(&chrono::Utc))
                        });

                    let entry = michi_receivers::ReceiverRegistryEntry {
                        receiver_id: rec.id.clone(),
                        michi_id: rec.michi_id.clone(),
                        name: rec.name,
                        device_type: rec.device_type,
                        base_url: rec.base_url,
                        paired: is_paired,
                        token,
                        presence: michi_receivers::ReceiverPresence::Offline,
                        last_seen: rec.last_seen.and_then(|s| {
                            chrono::DateTime::parse_from_rfc3339(&s)
                                .ok()
                                .map(|d| d.with_timezone(&chrono::Utc))
                        }),
                        capabilities: vec!["pcm".to_string(), "rtp".to_string()],
                        capabilities_verified_at: capabilities_observed_at,
                        capabilities_stale: true,
                        authority_supported: rec.authority_supported,
                        owner_michi_id: None,
                        owner_name: None,
                        active_session_id: None,
                        max_sample_rate: max_sr,
                        max_bit_depth: max_bd,
                        supported_transports,
                        supported_codecs,
                        supported_sample_rates,
                        supported_bit_depths,
                        supported_channels,
                        maximum_safe_volume: Some(100),
                        qualification:
                            michi_receivers::models::ReceiverQualification::NeedsCapabilityRefresh,
                        authenticated: is_paired,
                        ..Default::default()
                    };

                    registry.add(entry);
                    restored_count += 1;
                }
                tracing::info!(
                    "restored {} persisted receivers into registry",
                    restored_count
                );
            }
            Err(e) => {
                tracing::warn!("failed to list persisted receivers from DB: {}", e);
            }
        }

        // Reconcile unrecovered pairing journals (crash recovery / forward recovery)
        match crate::routes::v1::receivers::reconcile_unrecovered_pairings(self).await {
            Ok(count) if count > 0 => {
                tracing::info!(
                    "pairing journal recovery: restored {} unrecovered pairings",
                    count
                );
            }
            Ok(_) => {}
            Err(e) => {
                tracing::warn!(
                    "pairing journal recovery encountered error during bootstrap: {}",
                    e
                );
            }
        }

        tracing::info!("spawning sync upload startup crash recovery scan in background");
        let sync_mgr = self.sync_manager.clone();
        let shutdown_tok = self.shutdown_token.clone();
        self.track_task(tokio::spawn(async move {
            tokio::select! {
                _ = shutdown_tok.cancelled() => {
                    tracing::info!("sync startup recovery aborted by shutdown");
                }
                res = sync_mgr.recover_incomplete_uploads() => {
                    match res {
                        Ok(report) => {
                            tracing::info!(
                                candidates = report.candidates,
                                completed = report.completed,
                                deferred = report.deferred,
                                terminal = report.terminal_failures,
                                transient = report.transient_failures,
                                invalid = report.invalid_rows,
                                "sync upload startup crash recovery completed"
                            );
                        }
                        Err(e) => {
                            tracing::warn!("sync upload startup recovery scan encountered error: {}", e);
                        }
                    }
                }
            }
        }));

        tracing::info!("initializing playback projection db epoch and lamport state");
        self.playback_projection.initialize_db_state().await;

        Ok(())
    }
}

impl AppState {
    pub fn new(config: Config, db: SqlitePool, admin_user_id: Option<Uuid>) -> Self {
        let identity = Arc::new(
            michi_identity::IdentityManager::load_or_generate(
                &config.config_path,
                "Michi Micro Server",
                "",
            )
            .unwrap_or_else(|e| {
                tracing::warn!("failed to load identity from config dir: {e}; using ephemeral");
                let ephemeral_dir = std::env::temp_dir()
                    .join(format!("michi-ephemeral-identity-{}", uuid::Uuid::new_v4()));
                let _ = std::fs::create_dir_all(&ephemeral_dir);
                michi_identity::IdentityManager::generate(&ephemeral_dir, "Michi Micro Server", "")
                    .expect("ephemeral identity generation must not fail")
            }),
        );
        Self::new_with_identity(config, db, admin_user_id, identity)
    }

    pub fn new_with_identity(
        config: Config,
        db: SqlitePool,
        admin_user_id: Option<Uuid>,
        identity: Arc<michi_identity::IdentityManager>,
    ) -> Self {
        let (tx, _) = broadcast::channel(64);
        let (sync_tx, _) = broadcast::channel(64);
        let auth_sessions = auth::AuthState::new_with_db(db.clone());
        let auth_enabled = config.auth_enabled;
        if auth_enabled {
            auth::spawn_session_cleanup(auth_sessions.clone());
        }
        let token_store = michi_link::TokenStore::new();
        let playback_state = Arc::new(RwLock::new(PlaybackState::default()));
        let upload_dir = config.cache_path.join("uploads");
        let _ = std::fs::create_dir_all(&upload_dir);
        let sync_manager = michi_sync::SyncManager::new(db.clone(), upload_dir);
        let security_config = michi_security::SecurityConfig::default();
        let security_state = michi_security::SecurityState::new(security_config);

        let disabled_modules: Arc<RwLock<HashSet<String>>> = Arc::new(RwLock::new(HashSet::new()));
        let shutdown_token = CancellationToken::new();

        let mut module_tokens = HashMap::new();
        module_tokens.insert("scan".to_string(), CancellationToken::new());
        module_tokens.insert("sync".to_string(), CancellationToken::new());
        module_tokens.insert("playback".to_string(), CancellationToken::new());
        module_tokens.insert("backup".to_string(), CancellationToken::new());
        module_tokens.insert("webhook".to_string(), CancellationToken::new());
        module_tokens.insert("homeassistant".to_string(), CancellationToken::new());
        let module_tokens = Arc::new(RwLock::new(module_tokens));
        let job_cancel_tokens: Arc<RwLock<HashMap<String, CancellationToken>>> =
            Arc::new(RwLock::new(HashMap::new()));
        let task_handles: Arc<std::sync::Mutex<Vec<tokio::task::JoinHandle<()>>>> =
            Arc::new(std::sync::Mutex::new(Vec::new()));

        let db_for_tokens = db.clone();
        let ts = token_store.clone();
        let th = task_handles.clone();
        tokio::spawn(async move {
            match michi_link::load_tokens_from_db(&ts, &db_for_tokens).await {
                Ok(n) => tracing::info!("loaded {} device tokens from DB", n),
                Err(e) => tracing::warn!("failed to load device tokens from DB: {}", e),
            }
            th.lock().unwrap().retain(|h| !h.is_finished());
        });
        michi_link::spawn_token_cleanup(token_store.clone());

        let scent_store = Arc::new(michi_connect::ScentStore::new());
        let mut receiver_manager =
            michi_receivers::ReceiverSessionManager::new_with_identity_and_scent(
                identity.clone(),
                scent_store.clone(),
            )
            .with_db_pool(db.clone());
        let pairing_registry = Arc::new(michi_identity::PairingRegistry::new());
        let pairing_display = Arc::new(RwLock::new(None));
        let pairing_sessions_display = Arc::new(RwLock::new(HashMap::new()));

        let cred_key_path = config.config_path.join("receiver_credentials.key");
        let receiver_credential_store = Arc::new(
            michi_receivers::ReceiverCredentialStore::load_or_create_key(&cred_key_path)
                .map_err(|e| {
                    tracing::warn!(
                        "failed to load receiver credentials key from {:?}: {}",
                        cred_key_path,
                        e
                    );
                    e
                })
                .ok(),
        );

        if let Some(ref store) = *receiver_credential_store {
            receiver_manager.set_credential_store(Arc::new(store.clone()));
        }

        let home_key_path = config.config_path.join("home_root_authority.key");
        let home_root_authority = AppHomeAuthority(Arc::new(if home_key_path.exists() {
            match std::fs::read(&home_key_path) {
                Ok(bytes) if bytes.len() == 32 => {
                    let mut key_bytes = [0u8; 32];
                    key_bytes.copy_from_slice(&bytes);
                    let sk = ed25519_dalek::SigningKey::from_bytes(&key_bytes);
                    michi_identity::HomeRootAuthority::from_signing_key(sk)
                }
                Ok(bytes) => {
                    tracing::error!(
                        path = ?home_key_path,
                        len = bytes.len(),
                        "FATAL: home_root_authority.key exists but has invalid length (expected 32 bytes). HOME_AUTHORITY_RECOVERY_REQUIRED: silent key regeneration refused to prevent breaking trust domain"
                    );
                    panic!("HOME_AUTHORITY_RECOVERY_REQUIRED: home_root_authority.key corrupt or invalid length");
                }
                Err(e) => {
                    tracing::error!(
                        path = ?home_key_path,
                        error = %e,
                        "FATAL: home_root_authority.key exists but cannot be read. HOME_AUTHORITY_RECOVERY_REQUIRED: silent key regeneration refused to prevent breaking trust domain"
                    );
                    panic!(
                        "HOME_AUTHORITY_RECOVERY_REQUIRED: home_root_authority.key unreadable: {e}"
                    );
                }
            }
        } else {
            if let Some(parent) = home_key_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let sk = ed25519_dalek::SigningKey::generate(&mut rand::rngs::OsRng);
            let key_bytes = sk.to_bytes();
            let tmp_key_path =
                home_key_path.with_extension(format!("tmp.{}", uuid::Uuid::new_v4()));

            #[cfg(unix)]
            {
                use std::io::Write;
                use std::os::unix::fs::OpenOptionsExt;
                if let Ok(mut f) = std::fs::OpenOptions::new()
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .mode(0o600)
                    .open(&tmp_key_path)
                {
                    let _ = f.write_all(&key_bytes);
                    let _ = f.flush();
                    let _ = f.sync_all();
                    drop(f);
                    if let Err(e) = std::fs::rename(&tmp_key_path, &home_key_path) {
                        tracing::warn!("Failed to atomically rename home_root_authority key file: {e}; falling back to direct write");
                        let _ = std::fs::write(&home_key_path, key_bytes);
                        let _ = std::fs::remove_file(&tmp_key_path);
                    }
                } else {
                    let _ = std::fs::write(&home_key_path, key_bytes);
                }
            }
            #[cfg(not(unix))]
            {
                if let Err(e) = std::fs::write(&tmp_key_path, key_bytes) {
                    tracing::warn!(
                        "Failed to write tmp home_root_authority key file: {e}; writing directly"
                    );
                    let _ = std::fs::write(&home_key_path, key_bytes);
                } else {
                    let _ = std::fs::rename(&tmp_key_path, &home_key_path);
                }
            }

            michi_identity::HomeRootAuthority::from_signing_key(sk)
        }));

        let server_michi_id = identity.michi_id().to_base64url();
        let server_public_key = identity.public_key_base64url();
        let server_membership = home_root_authority.issue_membership(
            &home_root_authority.home_id(),
            &server_michi_id,
            &server_public_key,
            "server",
            vec![
                michi_identity::Role::MusicServer,
                michi_identity::Role::PlaybackHost,
            ],
            &chrono::Utc::now().to_rfc3339(),
            1,
        );

        receiver_manager = receiver_manager
            .with_home_authority(home_root_authority.0.clone(), server_membership.clone());

        // Persisted revocations are restored synchronously in bootstrap_runtime() before discovery and networking start.

        let resolver = Arc::new(michi_playback::SqliteTrackResolver::new(
            db.clone(),
            config.music_paths.clone(),
        ));
        let (playback_engine, engine_join) =
            michi_playback::spawn_playback_engine(resolver, michi_playback::PcmFormat::default());
        task_handles.lock().unwrap().push(engine_join);

        let playback_output_selection = Arc::new(RwLock::new(None));

        let (playback_projector, playback_projection_health) =
            playback_projection::PlaybackProjectionCoordinator::new(
                db.clone(),
                playback_state.clone(),
                playback_engine.clone(),
                sync_tx.clone(),
                config.server_id.to_string(),
            );
        let projector_handle = playback_projector.clone().spawn(shutdown_token.clone());
        task_handles.lock().unwrap().push(projector_handle);

        let max_transcodes = config.resource_profile.max_transcodes();
        let transcode_semaphore = Arc::new(tokio::sync::Semaphore::new(max_transcodes));

        let mut initial_runtime = HashMap::new();
        for mod_name in &[
            "scan",
            "sync",
            "stream",
            "playback",
            "backup",
            "webhook",
            "homeassistant",
        ] {
            let (actual_state, health, last_error) =
                if *mod_name == "homeassistant" && std::env::var("MICHI_MQTT_HOST").is_err() {
                    (
                        "idle".to_string(),
                        "disabled".to_string(),
                        Some("MICHI_MQTT_HOST not set".to_string()),
                    )
                } else {
                    ("active".to_string(), "healthy".to_string(), None)
                };
            initial_runtime.insert(
                mod_name.to_string(),
                ModuleRuntimeInfo {
                    generation: 1,
                    actual_state,
                    health,
                    last_error,
                },
            );
        }
        let module_runtime_info = Arc::new(RwLock::new(initial_runtime));

        let aircat_tls = Arc::new(RwLock::new(AirCatTlsInfo::from_env_and_config(&config)));

        let state = Self {
            config,
            db,
            tx,
            playback_state,
            sync_tx,
            auth_sessions,
            auth_enabled,
            admin_user_id,
            started_at: Instant::now(),
            transcode_profiles: Arc::new(RwLock::new(crate::transcode::default_profiles())),
            token_store,
            receiver_manager,
            sync_manager,
            security_state,
            disabled_modules,
            shutdown_token,
            module_tokens,
            job_cancel_tokens,
            task_handles,
            identity,
            pairing_registry,
            pairing_display,
            pairing_sessions_display,
            playback_engine,
            playback_projection: playback_projector,
            playback_projection_health,
            playback_output_selection,
            receiver_credential_store,
            transcode_semaphore,
            pending_transfers: Arc::new(michi_sync::playback_transfer::PendingTransferStore::new()),
            scent_store,
            whisker_metrics: Arc::new(michi_connect::WhiskerMetrics::default()),
            module_transition_locks: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            module_runtime_info,
            home_root_authority,
            server_membership,
            aircat_security: Arc::new(aircat_producer_security::AirCatProducerSecurityState::new()),
            aircat_tls,
            aircat_tls_listener_ready: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            queue_coordinator: Arc::new(playback_queue::ActiveQueueCoordinator::new()),
        };

        state.spawn_background_tasks();
        state
    }

    fn spawn_background_tasks(&self) {
        let db = self.db.clone();
        let dm = self.disabled_modules.clone();
        let shutdown = self.shutdown_token.clone();

        // Import cleanup (siempre corre)
        let import_handle =
            routes::v1::import::spawn_import_cleanup(&self.config, db.clone(), shutdown.clone());
        self.track_task(import_handle);

        // Restore playback state (siempre corre)
        let playback_restore_handle = routes::v1::playback::auto_restore_playback_state(
            db.clone(),
            self.playback_state.clone(),
            self.playback_engine.clone(),
            self.config.music_paths.clone(),
            shutdown.clone(),
        );
        self.track_task(playback_restore_handle);

        // DB maintenance scheduler (siempre corre)
        let maintenance_db = db.clone();
        let maint_shutdown = shutdown.clone();
        self.track_task(tokio::spawn(async move {
            let mut hourly = tokio::time::interval(Duration::from_secs(3600));
            let mut daily = tokio::time::interval(Duration::from_secs(86400));
            let mut weekly = tokio::time::interval(Duration::from_secs(604800));
            hourly.tick().await;
            daily.tick().await;
            weekly.tick().await;
            loop {
                tokio::select! {
                    _ = maint_shutdown.cancelled() => break,
                    _ = hourly.tick() => {
                        if let Err(e) = michi_db::run_hourly_maintenance(&maintenance_db).await {
                            tracing::warn!(error = %e, "hourly database maintenance failed");
                        }
                    }
                    _ = daily.tick() => {
                        if let Err(e) = michi_db::run_daily_maintenance(&maintenance_db).await {
                            tracing::warn!(error = %e, "daily database maintenance failed");
                        }
                    }
                    _ = weekly.tick() => {
                        if let Err(e) = michi_db::run_weekly_maintenance(&maintenance_db).await {
                            tracing::warn!(error = %e, "weekly database maintenance failed");
                        }
                    }
                }
            }
            info!("maintenance scheduler stopped");
        }));

        // Periodic rate limiter & pairing attempt cache pruner (runs every 60s)
        let sec_state_for_prune = self.security_state.clone();
        let sec_prune_shutdown = shutdown.clone();
        self.track_task(tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));
            interval.tick().await;
            loop {
                tokio::select! {
                    _ = sec_prune_shutdown.cancelled() => break,
                    _ = interval.tick() => {
                        sec_state_for_prune.prune_stale_limiters();
                    }
                }
            }
        }));

        // Periodic revocation outbox delivery loop (runs every 30s)
        let outbox_mgr = self.receiver_manager.clone();
        let outbox_shutdown = shutdown.clone();
        self.track_task(tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(30));
            interval.tick().await;
            loop {
                tokio::select! {
                    _ = outbox_shutdown.cancelled() => break,
                    _ = interval.tick() => {
                        outbox_mgr.flush_revocation_outbox().await;
                    }
                }
            }
        }));

        // Daily library integrity & reconciliation cron (solo si scan habilitado)

        let integrity_db = db.clone();
        let integrity_paths = self.config.music_paths.clone();
        let integrity_profile = self.config.resource_profile;
        let integrity_shutdown = shutdown.clone();
        let integrity_dm = dm.clone();
        let integrity_tokens = self.module_tokens.clone();
        self.track_task(tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(86400));
            interval.tick().await;
            loop {
                tokio::select! {
                    _ = integrity_shutdown.cancelled() => break,
                    _ = interval.tick() => {
                        if integrity_dm.read().await.contains("scan") {
                            continue;
                        }
                        let current_cancel = integrity_tokens
                            .read()
                            .await
                            .get("scan")
                            .cloned()
                            .unwrap_or_default();
                        if current_cancel.is_cancelled() {
                            continue;
                        }
                        let _concurrency = integrity_profile.scan_concurrency();
                        for path in &integrity_paths {
                            if current_cancel.is_cancelled() {
                                break;
                            }
                            let scan_res = michi_scanner::scan_root_cancellable(
                                path,
                                current_cancel.clone(),
                            )
                            .await;
                            let _ = michi_scanner::reconcile_root(
                                &integrity_db,
                                path,
                                &scan_res,
                                &current_cancel,
                            )
                            .await;
                        }
                    }
                }
            }
            info!("integrity cron stopped");
        }));

        // Inotify file system watcher (respeta módulo scan con re-habilitación dinámica)
        let watch_paths = self.config.music_paths.clone();
        let watch_db = db.clone();
        let watch_shutdown = shutdown.clone();
        let watch_dm = dm.clone();
        let watch_tokens = self.module_tokens.clone();
        self.track_task(tokio::spawn(async move {
            if !watch_paths.is_empty() {
                let watcher = michi_scanner::watcher::LibraryWatcher::new(
                    watch_paths.clone(),
                    watch_db.clone(),
                );
                let mut was_disabled = false;
                loop {
                    if watch_shutdown.is_cancelled() {
                        break;
                    }
                    if watch_dm.read().await.contains("scan") {
                        was_disabled = true;
                        tokio::select! {
                            _ = watch_shutdown.cancelled() => break,
                            _ = tokio::time::sleep(Duration::from_secs(1)) => continue,
                        }
                    }
                    let current_cancel = watch_tokens
                        .read()
                        .await
                        .get("scan")
                        .cloned()
                        .unwrap_or_default();
                    if current_cancel.is_cancelled() {
                        was_disabled = true;
                        tokio::select! {
                            _ = watch_shutdown.cancelled() => break,
                            _ = tokio::time::sleep(Duration::from_secs(1)) => continue,
                        }
                    } else {
                        if was_disabled {
                            info!(
                                "scan module re-enabled; reconciling library roots with filesystem"
                            );
                            for path in &watch_paths {
                                if current_cancel.is_cancelled() {
                                    break;
                                }
                                let scan_res = michi_scanner::scan_root_cancellable(
                                    path,
                                    current_cancel.clone(),
                                )
                                .await;
                                let _ = michi_scanner::reconcile_root(
                                    &watch_db,
                                    path,
                                    &scan_res,
                                    &current_cancel,
                                )
                                .await;
                            }
                            was_disabled = false;
                        }
                        watcher
                            .run(
                                current_cancel,
                                watch_shutdown.clone(),
                                Duration::from_secs(5),
                            )
                            .await;
                    }
                }
            }
            info!("watcher stopped");
        }));

        // Job Queue supervisor
        let supervisor_db = db.clone();
        let supervisor_state = self.clone();
        let supervisor_shutdown = shutdown.clone();
        let supervisor_dm = dm.clone();
        self.track_task(tokio::spawn(async move {
            let max_jobs = (supervisor_state.config.job_max_concurrent as usize).max(1);
            let semaphore = Arc::new(tokio::sync::Semaphore::new(max_jobs));
            let mut interval = tokio::time::interval(Duration::from_secs(2));

            loop {
                tokio::select! {
                    _ = supervisor_shutdown.cancelled() => break,
                    _ = interval.tick() => {
                        if supervisor_dm.read().await.contains("scan") && supervisor_dm.read().await.contains("sync") {
                            continue;
                        }
                        let pending = match michi_db::get_pending_jobs(&supervisor_db, 5).await {
                            Ok(jobs) => jobs,
                            Err(e) => {
                                tracing::warn!("job supervisor: failed to query pending jobs: {}", e);
                                continue;
                            }
                        };
                        for job in &pending {
                            let permit = match tokio::select! {
                                _ = supervisor_shutdown.cancelled() => None,
                                result = semaphore.clone().acquire_owned() => result.ok(),
                            } {
                                Some(permit) => permit,
                                None => break,
                            };
                            let claimed = match michi_db::claim_job(&supervisor_db, &job.id).await {
                                Ok(true) => true,
                                Ok(false) => continue,
                                Err(e) => {
                                    tracing::warn!("job supervisor: failed to claim job {}: {}", job.id, e);
                                    continue;
                                }
                            };
                            if !claimed {
                                continue;
                            }

                            let worker_db = supervisor_db.clone();
                            let worker_state = supervisor_state.clone();
                            let job_id = job.id.clone();
                            let job_kind = job.kind.clone();
                            let job_payload = job.payload.clone();
                            let worker_dm = supervisor_dm.clone();
                            let cancel = supervisor_shutdown.child_token();
                            supervisor_state
                                .job_cancel_tokens
                                .write()
                                .await
                                .insert(job_id.clone(), cancel.clone());

                            tokio::spawn(async move {
                                let _permit = permit;
                                let result = run_job_worker(
                                    &worker_db,
                                    &worker_state,
                                    &job_id,
                                    &job_kind,
                                    job_payload.as_ref(),
                                    &worker_dm,
                                    &cancel,
                                )
                                .await;
                                if cancel.is_cancelled() {
                                    tracing::info!("job {} cancelled", job_id);
                                } else {
                                    match result {
                                    Ok(msg) => {
                                        tracing::info!("job {} completed: {}", job_id, msg);
                                        let _ = michi_db::complete_job(&worker_db, &job_id).await;
                                    }
                                    Err(e) => {
                                        tracing::error!("job {} failed: {}", job_id, e);
                                        let _ = michi_db::fail_job(&worker_db, &job_id, &e).await;
                                    }
                                    }
                                }
                                worker_state.job_cancel_tokens.write().await.remove(&job_id);
                            });
                        }
                    }
                }
            }
            info!("job supervisor stopped");
        }));

        // Auto-Backup Scheduler
        if self.config.auto_backup_enabled {
            let backup_state = self.clone();
            let backup_shutdown = shutdown.clone();
            let backup_dm = dm.clone();
            self.track_task(tokio::spawn(async move {
                let mut interval = tokio::time::interval(Duration::from_secs(3600));
                loop {
                    tokio::select! {
                        _ = backup_shutdown.cancelled() => break,
                        _ = interval.tick() => {
                            if backup_dm.read().await.contains("backup") {
                                continue;
                            }
                            if let Err(e) = routes::v1::backup::run_auto_backup_cycle(&backup_state).await {
                                tracing::warn!("auto-backup cycle encountered error: {}", e);
                            }
                        }
                    }
                }
                info!("auto-backup scheduler stopped");
            }));
        }

        // Start sync peers (solo si sync habilitado)
        // Se hace desde main.rs después de AppState::new()
    }

    pub fn server_id(&self) -> Uuid {
        self.config.server_id
    }

    pub async fn get_user_id(&self, headers: &HeaderMap) -> Option<Uuid> {
        if !self.auth_enabled {
            return None;
        }
        let auth_header = headers.get("Authorization")?.to_str().ok()?;
        let token = auth_header.strip_prefix("Bearer ")?;
        self.auth_sessions.extract_user_id(token).await
    }
}

/// Get the current CancellationToken for a module (supports dynamic replacement)
impl AppState {
    pub async fn scan_token(&self) -> CancellationToken {
        self.module_tokens
            .read()
            .await
            .get("scan")
            .cloned()
            .unwrap_or_else(CancellationToken::new)
    }
    pub async fn sync_token(&self) -> CancellationToken {
        self.module_tokens
            .read()
            .await
            .get("sync")
            .cloned()
            .unwrap_or_else(CancellationToken::new)
    }
    pub async fn playback_token(&self) -> CancellationToken {
        self.module_tokens
            .read()
            .await
            .get("playback")
            .cloned()
            .unwrap_or_else(CancellationToken::new)
    }
    pub async fn backup_token(&self) -> CancellationToken {
        self.module_tokens
            .read()
            .await
            .get("backup")
            .cloned()
            .unwrap_or_else(CancellationToken::new)
    }
    pub async fn webhook_token(&self) -> CancellationToken {
        self.module_tokens
            .read()
            .await
            .get("webhook")
            .cloned()
            .unwrap_or_else(CancellationToken::new)
    }
    pub async fn homeassistant_token(&self) -> CancellationToken {
        self.module_tokens
            .read()
            .await
            .get("homeassistant")
            .cloned()
            .unwrap_or_else(CancellationToken::new)
    }
}

async fn run_job_worker(
    db: &SqlitePool,
    state: &AppState,
    job_id: &str,
    kind: &str,
    _payload: Option<&serde_json::Value>,
    _dm: &Arc<RwLock<HashSet<String>>>,
    cancel: &CancellationToken,
) -> Result<String, String> {
    if cancel.is_cancelled() {
        return Err("cancelled".into());
    }
    match kind {
        "scan" => {
            tracing::info!("job {}: starting library scan", job_id);
            let paths = &state.config.music_paths;
            let mut total = 0usize;
            for (index, path) in paths.iter().enumerate() {
                if cancel.is_cancelled() {
                    return Err("cancelled".into());
                }
                let probe_path = path.clone();
                let available = tokio::task::spawn_blocking(move || {
                    std::fs::read_dir(probe_path)
                        .map(|_| ())
                        .map_err(|error| error.to_string())
                })
                .await
                .map_err(|error| format!("mount probe failed: {error}"))?;
                if let Err(error) = available {
                    let _ = michi_db::update_mount_state(
                        db,
                        &path.display().to_string(),
                        "unavailable",
                        &error,
                    )
                    .await;
                    tracing::warn!(path = %path.display(), %error, "scan skipped unavailable mount");
                    continue;
                }
                let scan_res = michi_scanner::scan_root_cancellable(path, cancel.clone()).await;
                michi_scanner::reconcile_root(db, path, &scan_res, cancel)
                    .await
                    .map_err(|error| format!("reconcile error: {error}"))?;
                total += scan_res.tracks().len();
                let progress = (index + 1) as f64 / paths.len().max(1) as f64;
                let _ = michi_db::update_job_progress(db, job_id, progress).await;
            }
            record_audit(db, "scan_completed", Some("library"), None, None).await;
            Ok(format!("scanned {total} tracks"))
        }
        "sync" => {
            tracing::info!("job {}: triggering sync", job_id);
            // Sync is triggered via WebSocket; the broadcast sends a Ping
            // which peers interpret as a sync heartbeat
            let _ = state.sync_tx.send(michi_sync::SyncMessage::Ping);
            record_audit(db, "sync_triggered", Some("sync"), None, None).await;
            Ok("sync triggered".to_string())
        }
        "backup" => {
            if cancel.is_cancelled() {
                return Err("cancelled".into());
            }
            tracing::info!("job {}: running backup", job_id);
            let tracks = michi_db::list_tracks(db)
                .await
                .map_err(|e| format!("list tracks error: {e}"))?;
            let playlists = michi_db::list_playlists(db, None)
                .await
                .map_err(|e| format!("list playlists error: {e}"))?;
            let output = serde_json::json!({
                "exported_at": Utc::now().to_rfc3339(),
                "tracks_count": tracks.len(),
                "playlists_count": playlists.len(),
            });
            record_audit(db, "backup_completed", Some("backup"), None, Some(output)).await;
            let _ = michi_db::update_job_progress(db, job_id, 1.0).await;
            Ok(format!(
                "backup complete: {} tracks, {} playlists",
                tracks.len(),
                playlists.len()
            ))
        }
        "cleanup" => {
            if cancel.is_cancelled() {
                return Err("cancelled".into());
            }
            tracing::info!("job {}: running cleanup", job_id);
            michi_db::run_hourly_maintenance(db)
                .await
                .map_err(|e| format!("cleanup error: {e}"))?;
            sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
                .execute(db)
                .await
                .map_err(|e| format!("checkpoint error: {e}"))?;
            let _ = michi_db::update_job_progress(db, job_id, 0.5).await;
            // Clean stale jobs older than 7 days
            sqlx::query(
                "DELETE FROM job_queue WHERE created_at < datetime('now', '-7 days') AND state IN ('completed', 'failed', 'cancelled')"
            )
                .execute(db)
                .await
                .ok();
            let _ = michi_db::update_job_progress(db, job_id, 1.0).await;
            record_audit(db, "cleanup_completed", Some("system"), None, None).await;
            Ok("cleanup complete".to_string())
        }
        _ => Err(format!("unknown job kind: {kind}")),
    }
}

pub async fn init_admin_user(config: &Config, db: &SqlitePool) -> Result<Option<Uuid>, String> {
    if !config.auth_enabled {
        return Ok(None);
    }
    let username = config
        .auth_username
        .as_deref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            "MICHI_AUTH_USERNAME is missing or empty while auth is enabled".to_string()
        })?;

    let password = config
        .auth_password
        .as_deref()
        .map(|s| s.trim())
        .filter(|s| s.len() >= 8)
        .ok_or_else(|| "MICHI_AUTH_PASSWORD is missing or shorter than 8 characters".to_string())?;

    // A. Si ya existe usuario marcado managed_by_environment:
    let managed_admin_opt = michi_db::get_managed_admin_user(db)
        .await
        .map_err(|e| format!("failed to query managed admin from database: {e}"))?;

    if let Some(managed) = managed_admin_opt {
        // Conservar exactamente su UUID; reconciliar username, password hash, asegurar is_admin=true.
        let password_matches =
            auth::verify_password(password, &managed.password_hash).unwrap_or(false);
        let username_matches = managed.username == username;
        let credentials_changed = !password_matches || !username_matches;

        let target_hash = if !password_matches {
            auth::hash_password(password)
                .map_err(|e| format!("failed to hash admin password: {e}"))?
        } else {
            managed.password_hash.clone()
        };

        if credentials_changed || !managed.is_admin {
            michi_db::reconcile_managed_admin(
                db,
                &managed.id,
                username,
                &target_hash,
                credentials_changed,
            )
            .await
            .map_err(|e| format!("failed to reconcile managed admin: {e}"))?;

            if credentials_changed {
                info!(
                    "environment-managed administrator credentials updated from '{}:***' to '{}:***'; previous sessions revoked",
                    managed.username, username
                );
            }
        }
        return Ok(Some(managed.id));
    }

    // B. Primera ejecución después de migración sin usuario marcado:
    // B1. Si existe usuario cuyo username coincide exactamente con MICHI_AUTH_USERNAME:
    if let Some((user_id, _, current_password_hash, _)) =
        michi_db::get_user_by_username(db, username)
            .await
            .map_err(|e| format!("failed to check user by username: {e}"))?
    {
        let password_matches =
            auth::verify_password(password, &current_password_hash).unwrap_or(false);
        let target_hash = if !password_matches {
            auth::hash_password(password)
                .map_err(|e| format!("failed to hash admin password: {e}"))?
        } else {
            current_password_hash
        };

        michi_db::reconcile_managed_admin(db, &user_id, username, &target_hash, !password_matches)
            .await
            .map_err(|e| format!("failed to adopt matching user as managed admin: {e}"))?;

        info!(
            "adopted existing user '{}' as environment-managed administrator",
            username
        );
        return Ok(Some(user_id));
    }

    // Inspect all admin users
    let existing_admins = michi_db::list_admin_users(db)
        .await
        .map_err(|e| format!("failed to list admin users: {e}"))?;

    // B2. Si no coincide ninguno y existe exactamente UN administrador:
    if existing_admins.len() == 1 {
        let legacy_admin = &existing_admins[0];
        let target_hash = auth::hash_password(password)
            .map_err(|e| format!("failed to hash admin password: {e}"))?;

        michi_db::reconcile_managed_admin(db, &legacy_admin.id, username, &target_hash, true)
            .await
            .map_err(|e| format!("failed to adopt legacy admin: {e}"))?;

        info!(
            "adopted single legacy administrator '{}' as environment-managed administrator and reconciled credentials to '{}'",
            legacy_admin.username, username
        );
        return Ok(Some(legacy_admin.id));
    }

    // B3. Si no existen administradores:
    if existing_admins.is_empty() {
        let user_exists = michi_db::get_user_by_username(db, username)
            .await
            .map_err(|e| format!("failed to check username uniqueness: {e}"))?
            .is_some();
        if user_exists {
            return Err(format!(
                "Cannot create managed admin with username '{username}': username is already taken by a non-admin user"
            ));
        }

        let new_id = Uuid::new_v4();
        let target_hash = auth::hash_password(password)
            .map_err(|e| format!("failed to hash admin password: {e}"))?;

        michi_db::create_managed_admin_user(db, &new_id, username, &target_hash)
            .await
            .map_err(|e| format!("failed to create managed admin user: {e}"))?;

        info!("created environment-managed administrator: {}", username);
        return Ok(Some(new_id));
    }

    // B4. Si existen DOS O MÁS admins y ninguno coincide con MICHI_AUTH_USERNAME:
    // ESTADO AMBIGUO. FALLAR CERRADO.
    let err_msg = format!(
        "Ambiguous administrator state: {} admin users exist, but none matches configured MICHI_AUTH_USERNAME '{}' and none is marked as managed_by_environment. Refusing to initialize administrator to prevent unauthorized privilege escalation or data corruption.",
        existing_admins.len(),
        username
    );
    tracing::error!("{err_msg}");
    Err(err_msg)
}

pub fn start_sync_peers(state: &AppState, cancel_token: CancellationToken) {
    let peers = state.config.sync_peers.clone();
    let sync_name = state.config.sync_name.clone();
    let sync_tx = state.sync_tx.clone();
    let reconnect_max = state.config.reconnect_delay_max as u64;
    let shutdown = state.shutdown_token.clone();
    let dm = state.disabled_modules.clone();
    let sync_cancel = cancel_token;
    let worker_state = state.clone();

    state.track_task(tokio::spawn(async move {
        tokio::select! {
            _ = sync_cancel.cancelled() => {
                info!("sync module cancelled, sync peers stopped");
            }
            _ = shutdown.cancelled() => {
                info!("shutdown received, sync peers stopped");
            }
            _ = async {
                if dm.read().await.contains("sync") {
                    info!("sync module disabled, sync peers not started");
                    return;
                }
                for peer in &peers {
                    let peer = peer.clone();
                    let sync_name = sync_name.clone();
                    let sync_tx = sync_tx.clone();
                    let peer_state = worker_state.clone();
                    let peer_shutdown = shutdown.clone();
                    let peer_cancel = sync_cancel.clone();
                    let peer_dm = dm.clone();

                    tokio::spawn(async move {
                        let url = format!("ws://{peer}/api/sync");
                        let mut attempt = 0u64;

                        loop {
                            tokio::select! {
                                _ = peer_shutdown.cancelled() => break,
                                _ = peer_cancel.cancelled() => {
                                    info!("sync peer worker for {peer} cancelled");
                                    break;
                                }
                                _ = async {
                                    if peer_dm.read().await.contains("sync") {
                                        return;
                                    }
                                    info!("connecting to sync peer: {} (attempt {})", url, attempt + 1);
                                    match tokio_tungstenite::connect_async(&url).await {
                                        Ok((ws_stream, _)) => {
                                            info!("connected to sync peer: {}", peer);
                                            attempt = 0;
                                            let (mut sender, mut receiver) = ws_stream.split();
                                            let mut local_sync_rx = sync_tx.subscribe();

                                            let identify = michi_sync::SyncMessage::Identify {
                                                name: sync_name.clone(),
                                                version: env!("CARGO_PKG_VERSION").into(),
                                                device_type: michi_sync::DeviceType::Server,
                                            };
                                            if let Ok(json) = identify.serialize() {
                                                let _ = sender.send(Message::Text(json)).await;
                                            }

                                            let send_cancel = peer_cancel.clone();
                                            let send_shutdown = peer_shutdown.clone();
                                            let mut send_task = tokio::spawn(async move {
                                                loop {
                                                    tokio::select! {
                                                        _ = send_shutdown.cancelled() => break,
                                                        _ = send_cancel.cancelled() => break,
                                                        msg = local_sync_rx.recv() => {
                                                            match msg {
                                                                Ok(msg) => {
                                                                    if let Ok(json) = msg.serialize() {
                                                                        if sender.send(Message::Text(json)).await.is_err() {
                                                                            break;
                                                                        }
                                                                    }
                                                                }
                                                                Err(_) => break,
                                                            }
                                                        }
                                                    }
                                                }
                                            });

                                            let recv_state = peer_state.clone();
                                            let recv_cancel = peer_cancel.clone();
                                            let recv_shutdown = peer_shutdown.clone();
                                            let mut recv_task = tokio::spawn(async move {
                                                loop {
                                                    tokio::select! {
                                                        _ = recv_shutdown.cancelled() => break,
                                                        _ = recv_cancel.cancelled() => break,
                                                        msg = receiver.next() => {
                                                            match msg {
                                                                Some(Ok(Message::Text(text))) => {
                                                                    if let Ok(michi_sync::SyncMessage::State {
                                                                        track_id,
                                                                        position_ms,
                                                                        playing,
                                                                        volume,
                                                                        updated_at,
                                                                        origin_device_id,
                                                                        event_id,
                                                                        sequence,
                                                                        epoch,
                                                                        boot_id,
                                                                        lamport,
                                                                        ..
                                                                    }) = michi_sync::SyncMessage::deserialize(&text)
                                                                    {
                                                                        crate::sync_ws::apply_remote_playback_state(
                                                                            &recv_state,
                                                                            track_id,
                                                                            position_ms,
                                                                            playing,
                                                                            volume,
                                                                            updated_at,
                                                                            origin_device_id,
                                                                            event_id,
                                                                            sequence,
                                                                            epoch,
                                                                            boot_id,
                                                                            lamport,
                                                                        )
                                                                        .await;
                                                                    }
                                                                }
                                                                Some(Ok(Message::Close(_))) | None => break,
                                                                _ => {}
                                                            }
                                                        }
                                                    }
                                                }
                                            });

                                            tokio::select! {
                                                _ = peer_shutdown.cancelled() => {},
                                                _ = peer_cancel.cancelled() => {},
                                                _ = (&mut send_task) => {},
                                                _ = (&mut recv_task) => {},
                                            }
                                            send_task.abort();
                                            recv_task.abort();
                                            let _ = send_task.await;
                                            let _ = recv_task.await;
                                        }
                                        Err(e) => {
                                            warn!(
                                                "failed to connect to sync peer {} (attempt {}): {}",
                                                peer,
                                                attempt + 1,
                                                e
                                            );
                                        }
                                    }

                                    attempt += 1;
                                    let delay = michi_config::Config::compute_reconnect_backoff(attempt, reconnect_max);
                                    info!("sync peer {}: reconnecting in {}s", peer, delay.as_secs());
                                    tokio::select! {
                                        _ = peer_shutdown.cancelled() => {},
                                        _ = peer_cancel.cancelled() => {},
                                        _ = tokio::time::sleep(delay) => {},
                                    }
                                } => {}
                            }
                        }
                        info!("sync peer {} stopped", peer);
                    });
                }
                futures_util::future::pending::<()>().await;
            } => {}
        }
    }));
}
fn v1_link_routes() -> Router<AppState> {
    let api_v1 = Router::new()
        .route(
            "/api/v1/pair/qr",
            post(routes::v1::pair::qr_generate_handler),
        )
        .route(
            "/api/v1/pair/qr/:qr_code/status",
            get(routes::v1::pair::qr_status_handler),
        )
        .route(
            "/api/v1/devices/revoke",
            post(routes::v1::pair::link_devices_revoke),
        )
        .route(
            "/api/v1/link/devices",
            get(routes::v1::pair::list_devices_handler),
        )
        .route(
            "/api/v1/library/stats",
            get(routes::v1::library::library_stats_handler),
        )
        .route(
            "/api/v1/library/health",
            get(routes::v1::library::library_health_handler),
        )
        .route(
            "/api/v1/library/scan",
            post(routes::v1::library::library_scan_handler),
        )
        .route("/api/v1/tracks", get(routes::v1::tracks::tracks_handler))
        .route("/api/v1/tracks/:id", get(routes::v1::tracks::track_handler))
        .route(
            "/api/v1/tracks/:id/stream",
            get(routes::v1::stream::stream_handler),
        )
        .route(
            "/api/v1/tracks/:id/download",
            get(routes::v1::stream::download_handler),
        )
        .route("/api/v1/search", get(routes::v1::tracks::search_handler))
        .route(
            "/api/v1/search/advanced",
            get(routes::v1::search::search_advanced_handler),
        )
        .route(
            "/api/v1/stream/:id",
            get(routes::v1::stream::stream_handler),
        )
        .route("/api/v1/hls/:id/:segment", get(stream::hls_segment_handler))
        .route(
            "/api/v1/download/:id",
            get(routes::v1::stream::download_handler),
        )
        .route(
            "/api/v1/aircat/manifest",
            get(routes::v1::aircat::aircat_manifest_handler),
        )
        .route(
            "/api/v1/aircat/authorize",
            post(routes::v1::aircat::aircat_authorize_handler)
                .layer(axum::extract::DefaultBodyLimit::max(128 * 1024)),
        )
        .route(
            "/api/v1/aircat/download/:remote_id",
            get(routes::v1::aircat::aircat_download_handler),
        )
        .route(
            "/api/v1/aircat/enrollment-qr",
            get(routes::v1::aircat::aircat_enrollment_qr_handler),
        )
        .route(
            "/api/v1/aircat/status",
            get(routes::v1::aircat::aircat_status_handler),
        )
        .route(
            "/api/v1/artwork/:id",
            get(routes::v1::artwork::artwork_handler),
        )
        .route(
            "/api/v1/playlists",
            get(routes::v1::playlists::playlists_handler)
                .post(routes::v1::playlists::create_playlist_handler),
        )
        .route(
            "/api/v1/playlists/:id",
            get(routes::v1::playlists::get_playlist_handler)
                .put(routes::v1::playlists::update_playlist_handler)
                .delete(routes::v1::playlists::delete_playlist_handler),
        )
        .route(
            "/api/v1/playlists/:id/tracks",
            get(routes::v1::playlists::get_playlist_tracks_handler),
        )
        .route(
            "/api/v1/playlists/:playlist_id/tracks/:track_id",
            post(routes::v1::playlists::add_playlist_track_handler)
                .delete(routes::v1::playlists::remove_playlist_track_handler),
        )
        .route(
            "/api/v1/playlists/:id/reorder",
            put(routes::v1::playlists::reorder_playlist_tracks_handler),
        )
        .route(
            "/api/v1/playlists/:id/export/m3u",
            get(routes::v1::playlists::export_playlist_m3u_handler),
        )
        .route(
            "/api/v1/playlists/smart",
            post(routes::v1::playlists::smart_playlist_handler),
        )
        .route(
            "/api/v1/chains",
            get(routes::v1::chains::list_chains_handler)
                .post(routes::v1::chains::create_chain_handler),
        )
        .route(
            "/api/v1/chains/:id",
            get(routes::v1::chains::get_chain_handler)
                .put(routes::v1::chains::update_chain_handler)
                .delete(routes::v1::chains::delete_chain_handler),
        )
        .route(
            "/api/v1/chains/:id/links",
            post(routes::v1::chains::add_link_handler),
        )
        .route(
            "/api/v1/chains/:chain_id/links/:link_id",
            put(routes::v1::chains::update_link_handler)
                .delete(routes::v1::chains::delete_link_handler),
        )
        .route(
            "/api/v1/chains/:id/links/reorder",
            post(routes::v1::chains::reorder_links_handler),
        )
        .route(
            "/api/v1/chains/:id/play",
            post(routes::v1::chains::play_chain_handler),
        )
        .route(
            "/api/v1/chains/:id/stop",
            post(routes::v1::chains::stop_chain_handler),
        )
        .route(
            "/api/v1/chains/:id/volume",
            post(routes::v1::chains::chain_volume_handler),
        )
        .route(
            "/api/v1/starred",
            get(routes::v1::favorites::starred_tracks_handler),
        )
        .route(
            "/api/v1/star/:id",
            post(routes::v1::favorites::star_track_handler),
        )
        .route(
            "/api/v1/rate/:id",
            post(routes::v1::favorites::rate_track_handler),
        )
        .route(
            "/api/v1/artists/:name/insights",
            get(routes::v1::insights::artist_insights_handler),
        )
        .route(
            "/api/v1/albums/:key/health",
            get(routes::v1::insights::album_health_handler),
        )
        .route(
            "/api/v1/import/session",
            post(routes::v1::import::import_session_handler),
        )
        .route(
            "/api/v1/import/session/create",
            post(routes::v1::import::import_session_handler),
        )
        .route(
            "/api/v1/import/preflight",
            post(routes::v1::import::import_preflight_handler),
        )
        .route(
            "/api/v1/import/upload/:session_id",
            post(routes::v1::import::import_upload_handler),
        )
        .route(
            "/api/v1/import/session/:session_id/upload",
            post(routes::v1::import::import_upload_handler),
        )
        .route(
            "/api/v1/import/commit/:session_id",
            post(routes::v1::import::import_commit_handler),
        )
        .route(
            "/api/v1/import/session/commit/:session_id",
            post(routes::v1::import::import_commit_handler),
        )
        .route(
            "/api/v1/import/rollback/:session_id",
            post(routes::v1::import::import_rollback_handler),
        )
        .route(
            "/api/v1/import/session/:session_id/status",
            get(routes::v1::import::import_session_status_handler),
        )
        .route(
            "/api/v1/diagnostics",
            get(routes::v1::diagnostics::diagnostics_handler),
        )
        .route("/api/v1/backup", get(routes::v1::backup::backup_handler))
        .route(
            "/api/v1/backup/restore",
            post(routes::v1::backup::restore_handler),
        )
        .route(
            "/api/v1/home/dashboard",
            get(routes::v1::dashboard::dashboard_handler),
        )
        .route(
            "/api/v1/history",
            get(routes::v1::history::history_handler)
                .delete(routes::v1::history::clear_history_handler),
        )
        .route(
            "/api/v1/history/stats",
            get(routes::v1::history::history_stats_handler),
        )
        .route(
            "/api/v1/history/export",
            get(routes::v1::history::history_export_handler),
        )
        .route(
            "/api/v1/bookmarks",
            get(routes::v1::bookmarks::list_bookmarks_handler)
                .post(routes::v1::bookmarks::upsert_bookmark_handler),
        )
        .route(
            "/api/v1/bookmarks/:track_id",
            get(routes::v1::bookmarks::get_bookmark_handler)
                .delete(routes::v1::bookmarks::delete_bookmark_handler),
        )
        .route(
            "/api/v1/backup/snapshot",
            post(routes::v1::backup::snapshot_handler),
        )
        .route(
            "/api/v1/radio/stations",
            get(routes::v1::radio::list_radio_stations_handler)
                .post(routes::v1::radio::create_radio_station_handler),
        )
        .route(
            "/api/v1/radio/stations/:id",
            put(routes::v1::radio::update_radio_station_handler)
                .delete(routes::v1::radio::delete_radio_station_handler),
        )
        .route(
            "/api/v1/radio/stations/:id/test",
            post(routes::v1::radio::test_radio_station_handler),
        )
        .route(
            "/api/v1/radio/stations/:id/favorite",
            post(routes::v1::radio::toggle_favorite_handler),
        )
        .route(
            "/api/v1/backup/snapshot/last",
            get(routes::v1::backup::last_snapshot_handler),
        )
        .route(
            "/api/v1/backup/bundle",
            get(routes::v1::backup::backup_bundle_handler)
                .post(routes::v1::backup::restore_backup_bundle_handler),
        )
        .route(
            "/api/v1/webhook",
            get(routes::v1::backup::get_webhook_handler)
                .post(routes::v1::backup::set_webhook_handler)
                .delete(routes::v1::backup::delete_webhook_handler),
        )
        .route(
            "/api/v1/webhook/test",
            post(routes::v1::backup::test_webhook_handler),
        )
        .route(
            "/api/v1/health/verify",
            get(routes::v1::backup::verify_integrity_handler)
                .post(routes::v1::backup::verify_integrity_handler),
        )
        .route(
            "/api/v1/backup/verify",
            get(routes::v1::backup::verify_integrity_handler)
                .post(routes::v1::backup::verify_integrity_handler),
        )
        .route(
            "/api/v1/backup/download",
            get(routes::v1::backup::download_backup_handler),
        )
        .route(
            "/api/v1/health/mounts",
            get(routes::v1::backup::mount_health_handler),
        )
        .route(
            "/api/v1/health/storage",
            get(routes::v1::storage::storage_health_handler),
        )
        .route(
            "/api/v1/config/validate",
            get(routes::v1::validate::config_validate_handler),
        )
        .route(
            "/api/v1/audit/log",
            get(routes::v1::audit::audit_log_handler),
        )
        .route("/api/v1/modules", get(routes::v1::modules::modules_handler))
        .route(
            "/api/v1/modules/toggle",
            post(routes::v1::modules::toggle_module_handler),
        )
        .route(
            "/api/v1/modules/:name",
            post(routes::v1::modules::toggle_module_path_handler),
        )
        .route(
            "/api/v1/changes",
            get(routes::v1::modules::change_journal_handler),
        )
        .route(
            "/api/v1/stream/handoff/offer",
            post(routes::v1::modules::handoff_handler),
        )
        .route(
            "/api/v1/jobs",
            get(routes::v1::jobs::list_jobs_handler).post(routes::v1::jobs::create_job_handler),
        )
        .route("/api/v1/jobs/:id", get(routes::v1::jobs::get_job_handler))
        .route(
            "/api/v1/jobs/:id/cancel",
            post(routes::v1::jobs::cancel_job_handler),
        )
        .route(
            "/api/v1/playback/state",
            get(routes::v1::playback::playback_state_handler),
        )
        .route(
            "/api/v1/playback/output",
            get(routes::v1::playback::get_playback_output_handler)
                .put(routes::v1::playback::set_playback_output_handler)
                .post(routes::v1::playback::set_playback_output_handler),
        )
        .route(
            "/api/v1/playback/control",
            post(routes::v1::playback::playback_control_handler),
        )
        .route(
            "/api/v1/playback/seek",
            post(routes::v1::playback::playback_seek_handler),
        )
        .route(
            "/api/v1/playback/session",
            post(routes::v1::playback::playback_session_handler),
        )
        .route(
            "/api/v1/playback/session/:session_id",
            get(routes::v1::playback::playback_session_get_handler),
        )
        .route(
            "/api/v1/playback/session/restore",
            post(routes::v1::playback::playback_session_restore_handler),
        )
        .route(
            "/api/v1/playback/restore",
            post(routes::v1::playback::playback_session_restore_handler),
        )
        .route(
            "/api/v1/player/handoff",
            post(routes::v1::playback::handoff_handler),
        )
        .route(
            "/api/v1/playback/handoff",
            post(routes::v1::playback::handoff_handler),
        )
        .route(
            "/api/v1/playback/record",
            post(routes::v1::playback::record_play_handler),
        )
        .route(
            "/api/v1/sessions/active",
            get(routes::v1::sessions::active_streams_handler),
        )
        .route(
            "/api/v1/library/duplicates",
            get(routes::v1::duplicates::duplicates_handler),
        )
        .route(
            "/api/v1/player/announce",
            post(routes::v1::announce::announce_handler),
        )
        .route(
            "/api/v1/settings",
            get(routes::v1::settings::get_settings_handler)
                .put(routes::v1::settings::update_settings_handler),
        )
        .route(
            "/api/v1/update/status",
            get(routes::v1::update::update_status_handler),
        )
        .route(
            "/api/v1/update/check",
            post(routes::v1::update::update_check_handler),
        )
        .route(
            "/api/v1/setup/status",
            get(routes::v1::setup::setup_status_handler),
        )
        .route(
            "/api/v1/setup/scan",
            post(routes::v1::setup::setup_scan_handler),
        )
        .route(
            "/api/v1/setup/fix-perms",
            post(routes::v1::setup::setup_fix_perms_handler),
        )
        .route(
            "/api/v1/sources",
            get(routes::v1::sources::list_sources_handler)
                .post(routes::v1::sources::add_source_handler),
        )
        .route(
            "/api/v1/sources/:id",
            delete(routes::v1::sources::delete_source_handler),
        )
        .route(
            "/api/v1/sources/:source_id/episodes",
            get(routes::v1::sources::get_episodes_handler),
        )
        .route(
            "/api/v1/sources/episodes/:id",
            put(routes::v1::sources::update_episode_handler),
        )
        .route(
            "/api/v1/stream/proxy/:source_id",
            get(routes::v1::sources::proxy_stream_handler),
        )
        .route(
            "/api/v1/stream/proxy/episode/:episode_id",
            get(routes::v1::sources::proxy_episode_handler),
        )
        .route(
            "/api/v1/shares",
            get(routes::v1::shares::list_shares_handler)
                .post(routes::v1::shares::create_share_handler),
        )
        .route(
            "/api/v1/shares/:id",
            delete(routes::v1::shares::delete_share_handler),
        )
        .route("/api/v1/queue", get(routes::v1::queue::queue_handler))
        .route(
            "/api/v1/queue/items",
            post(routes::v1::queue::queue_items_handler),
        )
        .route(
            "/api/v1/queue/items/:item_id",
            delete(routes::v1::queue::queue_item_delete_handler),
        )
        .route(
            "/api/v1/queue/repeat",
            post(routes::v1::queue::queue_repeat_handler),
        )
        .route(
            "/api/v1/queue/shuffle",
            post(routes::v1::queue::queue_shuffle_handler),
        )
        .route(
            "/api/v1/queue/jump",
            post(routes::v1::queue::queue_jump_handler),
        )
        .route(
            "/api/v1/queue/transfer",
            post(routes::v1::queue::queue_transfer_handler),
        )
        .route(
            "/api/v1/queue/reorder",
            put(routes::v1::queue::queue_reorder_handler),
        )
        .route(
            "/api/v1/queue/:queue_id",
            delete(routes::v1::queue::queue_delete_handler),
        )
        .route(
            "/api/v1/queue/save",
            post(routes::v1::queue::queue_save_handler),
        )
        .route(
            "/api/v1/queue/saved",
            get(routes::v1::queue::queue_saved_handler),
        )
        .route(
            "/api/v1/queue/reconcile",
            post(routes::v1::queue::queue_reconcile_handler),
        )
        .route(
            "/api/v1/discovery/whisker",
            get(routes::v1::receivers::whisker_discovery_handler),
        )
        .route(
            "/api/v1/receivers",
            get(routes::v1::receivers::receivers_handler),
        )
        .route(
            "/api/v1/home/info",
            get(routes::v1::receivers::home_info_handler),
        )
        .route(
            "/api/v1/home/roster",
            get(routes::v1::receivers::home_roster_handler),
        )
        .route(
            "/api/v1/home/revocations",
            get(routes::v1::receivers::home_revocations_handler),
        )
        .route(
            "/api/v1/home/revoke",
            post(routes::v1::receivers::home_revoke_handler),
        )
        .route(
            "/api/v1/home/outbox",
            get(routes::v1::receivers::home_revocation_outbox_handler),
        )
        .route(
            "/api/v1/home/outbox/retry",
            post(routes::v1::receivers::home_retry_outbox_handler),
        )
        .route(
            "/api/v1/home/enrollment/pending",
            get(routes::v1::aircat::home_enrollment_pending_handler),
        )
        .route(
            "/api/v1/home/enrollment/:request_id/approve",
            post(routes::v1::aircat::home_enrollment_approve_handler),
        )
        .route(
            "/api/v1/home/enrollment/:request_id/reject",
            post(routes::v1::aircat::home_enrollment_reject_handler),
        )
        .route(
            "/api/v1/home/members/enroll",
            post(routes::v1::aircat::home_members_direct_enroll_handler),
        )
        .route(
            "/api/v1/receivers/:id/auth",
            post(routes::v1::receivers::receiver_auth_handler),
        )
        .route(
            "/api/v1/receivers/pair/start",
            post(routes::v1::receivers::receiver_pair_start_handler),
        )
        .route(
            "/api/v1/receivers/pair/confirm",
            post(routes::v1::receivers::receiver_pair_confirm_handler),
        )
        .route(
            "/api/v1/receivers/discover/pair",
            post(routes::v1::receivers::discover_receiver_handler),
        )
        .route(
            "/api/v1/receivers/:id",
            get(routes::v1::receivers::get_receiver_handler)
                .delete(routes::v1::receivers::delete_receiver_handler),
        )
        .route(
            "/api/v1/receivers/:id/session/start",
            post(routes::v1::receivers::receiver_session_start_handler),
        )
        .route(
            "/api/v1/receivers/:id/session/stop",
            post(routes::v1::receivers::receiver_session_stop_handler),
        )
        .route(
            "/api/v1/receivers/:id/volume",
            post(routes::v1::receivers::receiver_volume_handler),
        )
        .route(
            "/api/v1/receivers/:id/heartbeat",
            post(routes::v1::receivers::receiver_heartbeat_handler),
        )
        .route(
            "/api/v1/receivers/:id/takeover",
            post(routes::v1::receivers::receiver_takeover_handler),
        )
        .route(
            "/api/v1/playback-transfer/prepare",
            post(routes::v1::playback_transfer::playback_transfer_prepare_handler),
        )
        .route(
            "/api/v1/playback-transfer/commit",
            post(routes::v1::playback_transfer::playback_transfer_commit_handler),
        )
        .route(
            "/api/v1/playback-transfer/abort",
            post(routes::v1::playback_transfer::playback_transfer_abort_handler),
        );

    // /stream/test_pcm is an engineering verification route, gated behind hardware-gate or debug/test builds
    #[cfg(any(feature = "hardware-gate", debug_assertions, test))]
    let api_v1 = api_v1.route(
        "/api/v1/receivers/:id/stream/test_pcm",
        post(routes::v1::receivers::receiver_stream_test_pcm_handler),
    );

    api_v1
        .route(
            "/api/v1/devices/discover",
            post(routes::v1::receivers::discover_mdns_handler),
        )
        .route(
            "/api/v1/receivers/groups",
            get(routes::v1::receivers::list_groups_handler)
                .post(routes::v1::receivers::create_group_handler),
        )
        .route(
            "/api/v1/receivers/groups/:group_id/sync",
            post(routes::v1::receivers::sync_group_handler),
        )
        .route(
            "/api/v1/rooms",
            get(routes::v1::rooms::rooms_handler).post(routes::v1::rooms::create_room_handler),
        )
        .route(
            "/api/v1/rooms/:id/play",
            post(routes::v1::rooms::room_play_handler),
        )
        .route(
            "/api/v1/rooms/groups",
            get(routes::v1::receivers::list_room_groups_handler)
                .post(routes::v1::receivers::create_room_group_handler),
        )
        .route(
            "/api/v1/rooms/groups/:id",
            get(routes::v1::receivers::get_room_group_handler)
                .put(routes::v1::receivers::update_room_group_handler)
                .delete(routes::v1::receivers::delete_room_group_handler),
        )
        .route(
            "/api/v1/rooms/groups/:id/activate",
            post(routes::v1::receivers::activate_room_group_handler),
        )
        .route(
            "/api/v1/rooms/groups/:id/deactivate",
            post(routes::v1::receivers::deactivate_room_group_handler),
        )
        .route(
            "/api/v1/rooms/groups/:id/mode",
            post(routes::v1::receivers::set_room_mode_handler),
        )
}

fn v1_home_auth_routes(state: AppState) -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/auth/challenge",
            post(routes::v1::aircat::home_auth_challenge_handler),
        )
        .route(
            "/api/v1/auth/session",
            post(routes::v1::aircat::home_auth_session_handler),
        )
        .route(
            "/api/v1/home/enrollment/challenge",
            post(routes::v1::aircat::home_enrollment_challenge_handler),
        )
        .route(
            "/api/v1/home/enrollment/request",
            post(routes::v1::aircat::home_enrollment_request_handler),
        )
        .route(
            "/api/v1/home/enrollment/:request_id/status",
            get(routes::v1::aircat::home_enrollment_status_handler),
        )
        .layer(middleware::from_fn_with_state(
            state.clone(),
            home_tls_policy::home_tls_required_middleware,
        ))
        .layer(middleware::from_fn_with_state(
            state.security_state,
            michi_security::rate_limit_middleware,
        ))
}

fn v1_public_routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/server/info",
            get(routes::v1::server::server_info_handler),
        )
        .route("/api/v1/status", get(routes::v1::server::status_handler))
        .route("/health/live", get(routes::v1::server::health_live_handler))
        .route(
            "/health/ready",
            get(routes::v1::server::health_ready_handler),
        )
        .route(
            "/api/v1/capabilities",
            get(routes::v1::modules::capabilities_handler),
        )
        .route("/api/v1/policy", get(routes::v1::modules::policy_handler))
        .route(
            "/api/v1/policy/lan",
            post(routes::v1::modules::lan_policy_handler),
        )
        .route(
            "/api/v1/pair/qr/:qr_code/claim",
            post(routes::v1::pair::qr_claim_handler),
        )
        .route(
            "/api/v1/pair/qr/:qr_code/svg",
            get(routes::v1::pair::qr_svg_handler),
        )
        .route(
            "/api/v1/pair/confirm",
            post(routes::v1::pair::link_pair_confirm),
        )
        .route(
            "/api/v1/token/refresh",
            post(routes::v1::pair::link_token_refresh),
        )
}

fn v1_link_routes_with_auth(state: AppState) -> Router<AppState> {
    v1_link_routes()
        .route(
            "/api/v1/pair/start",
            post(routes::v1::pair::link_pair_start),
        )
        .route("/api/v1/events", get(routes::v1::events::events_handler))
        .route(
            "/api/v1/events/sse",
            get(routes::v1::events::events_sse_handler),
        )
        .route(
            "/api/v1/health/self-test",
            get(routes::v1::modules::self_test_handler),
        )
        .route(
            "/api/v1/link/self-test",
            get(routes::v1::modules::link_self_test_handler),
        )
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::v1_auth_middleware,
        ))
        .layer(middleware::from_fn_with_state(
            state.security_state.clone(),
            michi_security::rate_limit_middleware,
        ))
        .layer(middleware::from_fn(
            michi_security::security_headers_middleware,
        ))
}

pub fn create_router(state: AppState) -> Router {
    let protected = Router::new()
        .route("/api/status", get(status::status_handler))
        .route("/api/library/scan", post(library::scan_handler))
        .route("/api/library/stats", get(library::stats_handler))
        .route(
            "/api/library/tracks",
            delete(library::delete_all_tracks_handler),
        )
        .route("/api/tracks", get(library::tracks_handler))
        .route("/api/search", get(library::search_handler))
        .route(
            "/api/tracks/:id",
            get(library::track_handler)
                .delete(library::delete_track_handler)
                .put(library::update_track_handler),
        )
        .route("/api/albums", get(library::albums_handler))
        .route("/api/artists", get(library::artists_handler))
        .route("/api/albums/:album", get(library::album_tracks_handler))
        .route("/api/artists/:artist", get(library::artist_tracks_handler))
        .route("/api/artwork/:id", get(library::artwork_handler))
        .route(
            "/api/playlists",
            get(library::playlists_handler).post(library::create_playlist_handler),
        )
        .route(
            "/api/playlists/:id",
            get(library::get_playlist_handler).delete(library::delete_playlist_handler),
        )
        .route(
            "/api/playlists/:playlist_id/tracks/:track_id",
            post(library::add_playlist_track_handler)
                .delete(library::remove_playlist_track_handler),
        )
        .route(
            "/api/playlists/:id/tracks",
            get(library::get_playlist_tracks_handler),
        )
        .route(
            "/api/playlists/:id/reorder",
            put(library::reorder_playlist_handler),
        )
        .route(
            "/api/playlists/:id/export",
            get(library::export_playlist_handler),
        )
        .route(
            "/api/playlists/import",
            post(library::import_playlist_handler),
        )
        .route(
            "/api/playlists/:id/share",
            get(library::get_share_handler)
                .post(library::enable_share_handler)
                .delete(library::disable_share_handler),
        )
        .route("/api/ws", get(ws::ws_handler))
        .route("/api/sync", get(sync_ws::sync_handler))
        .route(
            "/api/playback/state",
            get(library::get_playback_state_handler).post(library::set_playback_state_handler),
        )
        .route("/api/stream/:id", get(stream::stream_handler))
        .route("/api/hls/:id/:segment", get(stream::hls_segment_handler))
        .route("/api/playback/record", post(scrobble::record_play_handler))
        .route("/api/history", get(scrobble::history_handler))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::auth_middleware,
        ))
        .layer(middleware::from_fn_with_state(
            state.security_state.clone(),
            michi_security::rate_limit_middleware,
        ))
        .layer(middleware::from_fn(
            michi_security::security_headers_middleware,
        ))
        .with_state(state.clone());

    Router::new()
        .route("/", get(root::root_handler))
        .route("/static/styles.css", get(static_files::styles_css))
        .route("/static/hero-cat.css", get(static_files::hero_cat_css))
        .route("/static/app.js", get(static_files::app_js))
        .route("/static/assets/michi-logo.svg", get(static_files::logo_svg))
        .route(
            "/static/assets/michi-micro-server.svg",
            get(static_files::favicon_svg),
        )
        .route(
            "/static/assets/michi-micro-server.png",
            get(static_files::favicon_png),
        )
        .route(
            "/static/assets/michi-micro-server-180.png",
            get(static_files::icon_180_png),
        )
        .route(
            "/static/assets/michi-micro-server-192.png",
            get(static_files::icon_192_png),
        )
        .route(
            "/static/assets/michi-micro-server-512.png",
            get(static_files::icon_512_png),
        )
        .route(
            "/static/assets/michi-hero-cat.webp",
            get(static_files::hero_cat_webp),
        )
        .route("/static/i18n/:lang", get(static_files::i18n_handler))
        .route("/manifest.json", get(pwa::manifest_json))
        .route("/sw.js", get(pwa::sw_js))
        .route("/api/shared/:code", get(library::shared_playlist_handler))
        .merge(auth::auth_router())
        .merge(
            utoipa_swagger_ui::SwaggerUi::new("/api/docs")
                .url("/api-docs/openapi.json", ApiDoc::openapi()),
        )
        .merge(protected)
        .merge(
            sync_api::sync_router()
                .layer(middleware::from_fn_with_state(
                    state.clone(),
                    routes::v1::sync::enforce_sync_network_policy,
                ))
                .layer(middleware::from_fn_with_state(
                    state.clone(),
                    auth::v1_auth_middleware,
                )),
        )
        .merge(
            routes::v1::sync::sync_router()
                .layer(middleware::from_fn_with_state(
                    state.clone(),
                    routes::v1::sync::enforce_sync_network_policy,
                ))
                .layer(middleware::from_fn_with_state(
                    state.clone(),
                    auth::v1_auth_middleware,
                )),
        )
        .merge(rooms::rooms_router().layer(middleware::from_fn_with_state(
            state.clone(),
            auth::v1_auth_middleware,
        )))
        .merge(
            players::players_router().layer(middleware::from_fn_with_state(
                state.clone(),
                auth::v1_auth_middleware,
            )),
        )
        .merge(
            transcode::transcode_router().layer(middleware::from_fn_with_state(
                state.clone(),
                auth::v1_auth_middleware,
            )),
        )
        .merge(
            scrobble::scrobble_router().layer(middleware::from_fn_with_state(
                state.clone(),
                auth::v1_auth_middleware,
            )),
        )
        .merge(v1_link_routes_with_auth(state.clone()))
        .merge(v1_home_auth_routes(state.clone()))
        .merge(v1_public_routes())
        .layer(middleware::from_fn(michi_security::content_type_middleware))
        .layer(TraceLayer::new_for_http())
        .layer(CompressionLayer::new().compress_when({
            use tower_http::compression::predicate::{
                DefaultPredicate, NotForContentType, Predicate,
            };
            DefaultPredicate::new().and(NotForContentType::const_new("application/octet-stream"))
        }))
        .layer(cors_layer(&state))
        .with_state(state)
}

fn cors_layer(state: &AppState) -> CorsLayer {
    if state.config.dev_mode {
        return CorsLayer::permissive();
    }
    let methods = [
        axum::http::Method::GET,
        axum::http::Method::POST,
        axum::http::Method::PUT,
        axum::http::Method::DELETE,
    ];
    let headers = [
        axum::http::header::CONTENT_TYPE,
        axum::http::header::AUTHORIZATION,
    ];
    if let Some(ref origin) = state.config.cors_origin {
        match origin.parse::<axum::http::HeaderValue>() {
            Ok(header_origin) => CorsLayer::new()
                .allow_origin(tower_http::cors::AllowOrigin::exact(header_origin))
                .allow_methods(methods)
                .allow_headers(headers),
            Err(_) => {
                tracing::warn!("invalid MICHI_CORS_ORIGIN value, using restrictive CORS");
                CorsLayer::new()
            }
        }
    } else {
        CorsLayer::new()
    }
}

pub fn resolve_client_ip(
    connect_info: Option<axum::extract::ConnectInfo<std::net::SocketAddr>>,
    headers: &axum::http::HeaderMap,
    config: &michi_config::Config,
) -> Option<std::net::IpAddr> {
    let peer_addr = connect_info.map(|axum::extract::ConnectInfo(addr)| addr);
    let peer_ip = peer_addr.map(|a| a.ip())?;

    if config.is_trusted_proxy(&peer_ip) {
        // Parse X-Forwarded-For from right to left (nearest proxy hop to furthest)
        if let Some(forwarded_header) = headers.get("X-Forwarded-For").and_then(|v| v.to_str().ok())
        {
            let mut resolved = None;
            for token in forwarded_header.split(',').map(|s| s.trim()).rev() {
                if token.is_empty() {
                    return None;
                }
                match token.parse::<std::net::IpAddr>() {
                    Ok(ip) => {
                        if config.is_trusted_proxy(&ip) {
                            continue; // Skip intermediate trusted proxy hop
                        }
                        resolved = Some(ip);
                        break;
                    }
                    Err(_) => {
                        // Unparseable token in proxy chain before finding client IP -> fail closed!
                        return None;
                    }
                }
            }
            if let Some(ip) = resolved {
                return Some(ip);
            }
        }

        // Fallback to X-Real-IP if present and valid
        if let Some(real_ip_header) = headers.get("X-Real-IP").and_then(|v| v.to_str().ok()) {
            if let Ok(ip) = real_ip_header.trim().parse::<std::net::IpAddr>() {
                return Some(ip);
            }
        }

        // The TCP peer is a trusted proxy, but no valid client IP could be extracted.
        // Fail-closed by returning None so we don't accidentally treat the proxy's private IP as the client.
        None
    } else {
        Some(peer_ip)
    }
}

pub fn extract_client_ip(
    connect_info: Option<axum::extract::ConnectInfo<std::net::SocketAddr>>,
    headers: &axum::http::HeaderMap,
    config: &michi_config::Config,
) -> String {
    resolve_client_ip(connect_info, headers, config)
        .map(|ip| ip.to_string())
        .unwrap_or_else(|| "127.0.0.1".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::ConnectInfo;
    use axum::http::{HeaderMap, HeaderValue};
    use std::net::SocketAddr;

    fn make_test_config(trust_proxy: bool, trusted_proxies: Vec<&str>) -> michi_config::Config {
        michi_config::Config {
            port: 9090,
            music_paths: vec![std::path::PathBuf::from("/music")],
            config_path: std::path::PathBuf::from("/config"),
            cache_path: std::path::PathBuf::from("/cache"),
            database_url: "sqlite::memory:".to_string(),
            version: "test",
            sync_peers: Vec::new(),
            sync_name: "test".to_string(),
            listenbrainz_token: None,
            lastfm_token: None,
            scrobble_enabled: false,
            auth_username: None,
            auth_password: None,
            auth_enabled: false,
            allow_registration: false,
            server_id: uuid::Uuid::new_v4(),
            cors_origin: None,
            dev_mode: false,
            resource_profile: michi_core::ResourceProfile::from_config_str("eco"),
            stream_profile: michi_core::StreamProfile::from_config_str("original"),
            format_policy: michi_core::AudioFormatPolicy::from_config_str("lossless"),
            max_remote_bitrate: 320_000,
            remote_sync: false,
            language: "en".into(),
            ui: michi_config::UiConfig::default(),
            auto_backup_enabled: false,
            backup_max_keep: 7,
            job_max_concurrent: 3,
            reconnect_delay_max: 300,
            opensubsonic_enabled: false,
            trust_proxy,
            trusted_proxies: trusted_proxies
                .into_iter()
                .map(|s| s.parse().unwrap())
                .collect(),
            deployment_platform: "unknown".into(),
        }
    }

    #[test]
    fn test_extract_client_ip_no_trust_proxy_ignores_headers() {
        let cfg = make_test_config(false, vec!["127.0.0.1"]);
        let addr: SocketAddr = "192.168.1.100:54321".parse().unwrap();
        let mut headers = HeaderMap::new();
        headers.insert("X-Forwarded-For", HeaderValue::from_static("8.8.8.8"));
        headers.insert("X-Real-IP", HeaderValue::from_static("8.8.4.4"));

        let ip = extract_client_ip(Some(ConnectInfo(addr)), &headers, &cfg);
        assert_eq!(ip, "192.168.1.100");
    }

    #[test]
    fn test_extract_client_ip_untrusted_peer_cannot_spoof() {
        let cfg = make_test_config(true, vec!["127.0.0.1", "10.0.0.1"]);
        // Untrusted LAN attacker connecting directly
        let addr: SocketAddr = "192.168.1.50:41234".parse().unwrap();
        let mut headers = HeaderMap::new();
        headers.insert("X-Forwarded-For", HeaderValue::from_static("1.1.1.1"));

        let ip = extract_client_ip(Some(ConnectInfo(addr)), &headers, &cfg);
        assert_eq!(ip, "192.168.1.50");
    }

    #[test]
    fn test_extract_client_ip_trusted_proxy_accepts_forwarded() {
        let cfg = make_test_config(true, vec!["127.0.0.1", "10.0.0.1"]);
        // Trusted proxy connecting
        let addr: SocketAddr = "10.0.0.1:41234".parse().unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Forwarded-For",
            HeaderValue::from_static("203.0.113.195, 10.0.0.1"),
        );

        let ip = extract_client_ip(Some(ConnectInfo(addr)), &headers, &cfg);
        assert_eq!(ip, "203.0.113.195");
    }

    #[test]
    fn test_extract_client_ip_trusted_proxy_handles_spoofed_prefix_right_to_left() {
        let cfg = make_test_config(true, vec!["10.0.0.1"]);
        // Attacker sent XFF: 127.0.0.1, proxy appended actual remote client 203.0.113.195
        let addr: SocketAddr = "10.0.0.1:41234".parse().unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Forwarded-For",
            HeaderValue::from_static("127.0.0.1, 203.0.113.195"),
        );

        let ip = extract_client_ip(Some(ConnectInfo(addr)), &headers, &cfg);
        assert_eq!(ip, "203.0.113.195", "Must resolve to the true client IP (203.0.113.195) rather than the spoofed prefix (127.0.0.1)");
    }

    #[test]
    fn test_extract_client_ip_trusted_proxy_accepts_real_ip() {
        let cfg = make_test_config(true, vec!["127.0.0.1"]);
        let addr: SocketAddr = "127.0.0.1:41234".parse().unwrap();
        let mut headers = HeaderMap::new();
        headers.insert("X-Real-IP", HeaderValue::from_static("198.51.100.4"));

        let ip = extract_client_ip(Some(ConnectInfo(addr)), &headers, &cfg);
        assert_eq!(ip, "198.51.100.4");
    }

    #[test]
    fn test_resolve_client_ip_trusted_proxy_malformed_token_fails_closed() {
        let cfg = make_test_config(true, vec!["10.0.0.1"]);
        let addr: SocketAddr = "10.0.0.1:41234".parse().unwrap();
        let mut headers = HeaderMap::new();
        // Malformed token "garbage" precedes 127.0.0.1 from right-to-left
        headers.insert(
            "X-Forwarded-For",
            HeaderValue::from_static("127.0.0.1, garbage"),
        );

        let resolved = resolve_client_ip(Some(ConnectInfo(addr)), &headers, &cfg);
        assert!(
            resolved.is_none(),
            "Malformed token in XFF must fail closed (return None)"
        );
    }
}
