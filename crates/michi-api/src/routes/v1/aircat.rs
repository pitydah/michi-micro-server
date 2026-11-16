//! AirCat v1 dedicated HTTP producer routes for michi-micro-server.
//!
//! Provides:
//! - GET  /api/v1/aircat/manifest
//! - POST /api/v1/aircat/authorize
//! - GET  /api/v1/aircat/download/:remote_id
//! - GET  /api/v1/aircat/enrollment-qr
//!
//! All routes require authenticated Home membership. Original files are served
//! strictly without transcoding, with strong quoted ETags and receipt header
//! X-Michi-AirCat-Authorized-Grant-SHA256.

use axum::{
    async_trait,
    body::Body,
    extract::{ConnectInfo, FromRequestParts, Path, State},
    http::{header, request::Parts, HeaderMap, StatusCode},
    response::Response,
    Json,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use ed25519_dalek::VerifyingKey;
use michi_identity::types::decode_base64url_strict;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use uuid::Uuid;

use crate::{
    aircat_grants::{self, ApprovedAsset, AuthorizedContext, Refusal},
    aircat_original_file,
    aircat_producer_security::ProducerSnapshot,
    AirCatTlsVerifiedConnection, AppState,
};

pub const AIRCAT_PROTOCOL_HEADER: &str = "X-Michi-AirCat-Protocol";
pub const AIRCAT_PROTOCOL_VALUE: &str = "michi.aircat.original.v1";
const MAX_MANIFEST_TRACKS: usize = 10_000;
const AIRCAT_CHUNK_BYTES: usize = 64 * 1024;
const DB_REVOCATION_RECHECK_INTERVAL: Duration = Duration::from_secs(2);
static AIRCAT_HASH_SEMAPHORE: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(4);

#[derive(Debug, Clone, Copy)]
pub struct AirCatTransportInfo {
    pub has_tls_ext: bool,
    pub has_peer_socket: bool,
    pub is_trusted_proxy_https: bool,
}

#[async_trait]
impl FromRequestParts<AppState> for AirCatTransportInfo {
    type Rejection = (StatusCode, Json<serde_json::Value>);

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let has_tls_ext = parts
            .extensions
            .get::<AirCatTlsVerifiedConnection>()
            .is_some();
        let peer_ip = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|c| c.0.ip());
        let has_peer_socket = peer_ip.is_some();
        let is_trusted_proxy_https = state.config.trust_proxy
            && peer_ip
                .map(|ip| state.config.is_trusted_proxy(&ip))
                .unwrap_or(false)
            && parts
                .headers
                .get("x-forwarded-proto")
                .and_then(|v| v.to_str().ok())
                .map(|p| p.trim().eq_ignore_ascii_case("https"))
                .unwrap_or(false);

        Ok(Self {
            has_tls_ext,
            has_peer_socket,
            is_trusted_proxy_https,
        })
    }
}

async fn enforce_aircat_tls_transport(
    state: &AppState,
    transport: &AirCatTransportInfo,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    if state.aircat_tls.read().await.is_some()
        && transport.has_peer_socket
        && !transport.has_tls_ext
        && !transport.is_trusted_proxy_https
    {
        state.aircat_security.record_tls_rejection();
        return Err(aircat_error(
            StatusCode::FORBIDDEN,
            "TLS_REQUIRED",
            "AirCat endpoints forbid cleartext HTTP when HTTPS/TLS is active",
        ));
    }
    Ok(())
}

pub fn aircat_json_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::HeaderName::from_static("x-michi-aircat-protocol"),
        header::HeaderValue::from_static(AIRCAT_PROTOCOL_VALUE),
    );
    headers.insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    headers
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AirCatCatalogTrack {
    pub track_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logical_track_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_track_id: Option<String>,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub format: String,
    pub size: i64,
    pub checksum: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AirCatCatalogManifest {
    pub schema: String,
    pub version: i32,
    pub device_id: String,
    pub total_tracks: i32,
    pub total_size: i64,
    pub tracks: Vec<AirCatCatalogTrack>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AirCatTrustedEnrollmentQr {
    #[serde(rename = "type")]
    pub qr_type: String,
    pub home_id: String,
    pub authority_michi_id: String,
    pub authority_public_key_base64_url: String,
    pub source_michi_id: String,
    pub pinned_base_url: String,
    pub tls_spki_sha256: String,
    pub nonce_base64_url: String,
    pub issued_at_millis: i64,
    pub expires_at_millis: i64,
}

pub fn aircat_error(
    status: StatusCode,
    code: &str,
    message: &str,
) -> (StatusCode, Json<serde_json::Value>) {
    (
        status,
        Json(serde_json::json!({
            "error": {
                "code": code,
                "message": message,
                "details": {}
            }
        })),
    )
}

fn refusal_to_status(refusal: Refusal) -> (StatusCode, &'static str, &'static str) {
    match refusal {
        Refusal::UntrustedHomeMember => (
            StatusCode::FORBIDDEN,
            "UNTRUSTED_MEMBER",
            "caller is not an active Home member",
        ),
        Refusal::WrongAudience => (
            StatusCode::FORBIDDEN,
            "WRONG_AUDIENCE",
            "destination or audience mismatch",
        ),
        Refusal::Revoked => (
            StatusCode::FORBIDDEN,
            "MEMBER_REVOKED",
            "member identity is revoked",
        ),
        Refusal::Expired => (
            StatusCode::FORBIDDEN,
            "GRANT_EXPIRED",
            "grant or intent time window expired",
        ),
        Refusal::NotOriginalBytes => (
            StatusCode::NOT_FOUND,
            "ASSET_NOT_FOUND",
            "asset not found or bytes do not match original library files",
        ),
        Refusal::BadSignature => (
            StatusCode::FORBIDDEN,
            "BAD_SIGNATURE",
            "invalid Home signature",
        ),
        Refusal::InvalidFields => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "INVALID_FIELDS",
            "invalid fields or canonical schema violation",
        ),
    }
}

/// Checks whether a given device/member is revoked in this Home.
pub async fn is_member_revoked(state: &AppState, home_id: &str, device_michi_id: &str) -> bool {
    if state
        .aircat_security
        .is_member_revoked_in_memory(device_michi_id)
    {
        return true;
    }

    // 1. In-memory session manager revocations
    let mem_revs = state.receiver_manager.get_revocations().await;
    if mem_revs
        .iter()
        .any(|r| r.revoked_device_michi_id == device_michi_id)
    {
        state.aircat_security.revoke_member(device_michi_id);
        return true;
    }

    // 2. Persisted SQLite home_revocations
    if let Ok(db_revs) = michi_db::list_home_revocations_db(&state.db, home_id).await {
        if db_revs
            .iter()
            .any(|r| r.revoked_device_michi_id == device_michi_id)
        {
            state.aircat_security.revoke_member(device_michi_id);
            return true;
        }
    }

    false
}

async fn resolve_caller_with_token(
    state: &AppState,
    headers: &HeaderMap,
    token: &str,
    claimed_destination: Option<&str>,
    require_admin_user: bool,
) -> Result<String, (StatusCode, Json<serde_json::Value>)> {
    let now_ms = chrono::Utc::now().timestamp_millis();
    if let Some(verified_home_member) = state.aircat_security.lookup_home_session(token, now_ms) {
        if require_admin_user {
            return Err(aircat_error(
                StatusCode::FORBIDDEN,
                "ADMIN_REQUIRED",
                "administrator session required",
            ));
        }
        if let Some(claimed) = claimed_destination {
            if claimed != verified_home_member {
                return Err(aircat_error(
                    StatusCode::FORBIDDEN,
                    "WRONG_AUDIENCE",
                    "destinationMichiId does not match cryptographically verified Home member session",
                ));
            }
        }
        if let Some(hdr_dev) = headers
            .get("X-Michi-Device-Id")
            .and_then(|h| h.to_str().ok())
        {
            if !hdr_dev.is_empty() && hdr_dev != verified_home_member {
                return Err(aircat_error(
                    StatusCode::FORBIDDEN,
                    "IDENTITY_MISMATCH",
                    "X-Michi-Device-Id header does not match verified Home member session",
                ));
            }
        }
        let home_id = state.home_root_authority.home_id();
        if is_member_revoked(state, &home_id, &verified_home_member).await {
            return Err(aircat_error(
                StatusCode::FORBIDDEN,
                "MEMBER_REVOKED",
                "authenticated member is revoked in this Home",
            ));
        }
        return Ok(verified_home_member);
    }

    let user_id_opt = state.auth_sessions.extract_user_id(token).await;
    let device_uuid_opt = if user_id_opt.is_none() {
        state
            .token_store
            .validate(token, michi_link::TokenType::Device)
            .await
            .ok()
    } else {
        None
    };

    let authenticated_member = if let Some(user_id) = user_id_opt {
        let is_admin = michi_db::get_user_by_id(&state.db, &user_id)
            .await
            .map_err(|e| {
                aircat_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "DB_ERROR",
                    &e.to_string(),
                )
            })?
            .map(|(_, _, _, admin)| admin)
            .unwrap_or(false);

        if require_admin_user && !is_admin {
            return Err(aircat_error(
                StatusCode::FORBIDDEN,
                "ADMIN_REQUIRED",
                "administrator privileges required",
            ));
        }

        // Active user/admin session (WebUI / paired admin operator)
        if let Some(claimed) = claimed_destination {
            if !is_admin {
                return Err(aircat_error(
                    StatusCode::FORBIDDEN,
                    "ADMIN_REQUIRED",
                    "non-admin user session cannot authorize transfers for another destinationMichiId",
                ));
            }
            let valid_id = claimed.len() == 43
                && decode_base64url_strict(claimed)
                    .map(|b| b.len() == 32)
                    .unwrap_or(false);
            if !valid_id {
                return Err(aircat_error(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "INVALID_FIELDS",
                    "destinationMichiId must be a canonical 43-character base64url Michi ID",
                ));
            }
            claimed.to_string()
        } else if let Some(hdr_dev) = headers
            .get("X-Michi-Device-Id")
            .and_then(|h| h.to_str().ok())
        {
            hdr_dev.to_string()
        } else {
            state.identity.michi_id().to_base64url()
        }
    } else if let Some(device_uuid) = device_uuid_opt {
        if require_admin_user {
            return Err(aircat_error(
                StatusCode::UNAUTHORIZED,
                "ADMIN_REQUIRED",
                "administrator session required",
            ));
        }
        let dev = michi_db::get_link_device(&state.db, &device_uuid)
            .await
            .map_err(|e| {
                aircat_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "DB_ERROR",
                    &e.to_string(),
                )
            })?
            .ok_or_else(|| {
                aircat_error(
                    StatusCode::FORBIDDEN,
                    "UNTRUSTED_MEMBER",
                    "authenticated device token has no registered link device",
                )
            })?;
        if dev.revoked {
            return Err(aircat_error(
                StatusCode::FORBIDDEN,
                "DEVICE_REVOKED",
                "device is revoked",
            ));
        }
        if let Some(claimed) = claimed_destination {
            if dev.device_id.to_string() == claimed
                || (!dev.alias.is_empty() && dev.alias == claimed)
            {
                claimed.to_string()
            } else {
                return Err(aircat_error(
                    StatusCode::FORBIDDEN,
                    "WRONG_AUDIENCE",
                    "destinationMichiId does not match authenticated device identity",
                ));
            }
        } else if !dev.alias.is_empty() {
            dev.alias
        } else {
            dev.device_id.to_string()
        }
    } else {
        return Err(aircat_error(
            StatusCode::UNAUTHORIZED,
            "INVALID_TOKEN",
            "invalid or expired session token",
        ));
    };

    // Header identity coherence check: if X-Michi-Device-Id header is provided, it MUST match
    if let Some(hdr_dev) = headers
        .get("X-Michi-Device-Id")
        .and_then(|h| h.to_str().ok())
    {
        if !hdr_dev.is_empty() && hdr_dev != authenticated_member {
            return Err(aircat_error(
                StatusCode::FORBIDDEN,
                "IDENTITY_MISMATCH",
                "X-Michi-Device-Id header does not match authenticated member",
            ));
        }
    }

    // Check Home revocation status
    let home_id = state.home_root_authority.home_id();
    if is_member_revoked(state, &home_id, &authenticated_member).await {
        return Err(aircat_error(
            StatusCode::FORBIDDEN,
            "MEMBER_REVOKED",
            "authenticated member is revoked in this Home",
        ));
    }

    Ok(authenticated_member)
}

/// Resolves and cryptographically checks the caller's Home session and identity via
/// `Authorization: Bearer ...` strictly (no browser cookies on AirCat wire transfer APIs).
pub async fn resolve_authenticated_caller(
    state: &AppState,
    headers: &HeaderMap,
    claimed_destination: Option<&str>,
) -> Result<String, (StatusCode, Json<serde_json::Value>)> {
    let auth_header = headers
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok());

    let token = match auth_header {
        Some(h) if h.starts_with("Bearer ") => h[7..].trim(),
        _ => {
            return Err(aircat_error(
                StatusCode::UNAUTHORIZED,
                "UNAUTHORIZED",
                "missing or malformed authorization header",
            ));
        }
    };

    resolve_caller_with_token(state, headers, token, claimed_destination, false).await
}

/// Resolves WebUI (`michi_web_session` cookie) or Bearer callers for WebUI/admin AirCat panels.
async fn resolve_webui_or_bearer_caller(
    state: &AppState,
    headers: &HeaderMap,
    require_admin_user: bool,
) -> Result<String, (StatusCode, Json<serde_json::Value>)> {
    let token = crate::auth::extract_token_from_headers(headers).ok_or_else(|| {
        aircat_error(
            StatusCode::UNAUTHORIZED,
            "UNAUTHORIZED",
            "missing authorization header or session cookie",
        )
    })?;
    resolve_caller_with_token(state, headers, &token, None, require_admin_user).await
}

/// GET /api/v1/aircat/manifest
///
/// Returns the original track catalog for AirCat transfer.
pub async fn aircat_manifest_handler(
    State(state): State<AppState>,
    transport: AirCatTransportInfo,
    headers: HeaderMap,
) -> Result<(HeaderMap, Json<AirCatCatalogManifest>), (StatusCode, Json<serde_json::Value>)> {
    enforce_aircat_tls_transport(&state, &transport).await?;
    let _authenticated_member = resolve_authenticated_caller(&state, &headers, None).await?;

    let producer_michi_id = state.identity.michi_id().to_base64url();

    let tracks = michi_db::list_tracks(&state.db).await.map_err(|e| {
        aircat_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DB_ERROR",
            &e.to_string(),
        )
    })?;

    let mut catalog_tracks = Vec::new();
    let mut total_size: i64 = 0;

    for track in tracks.into_iter().take(MAX_MANIFEST_TRACKS) {
        let _permit = AIRCAT_HASH_SEMAPHORE.acquire().await.map_err(|_| {
            aircat_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "CONCURRENCY_LIMIT",
                "aircat hashing semaphore unavailable",
            )
        })?;
        if let Ok((_item, revision)) = aircat_original_file::inspect_original_asset_cached(
            &state.config.music_paths,
            &track,
            &state.aircat_security,
        )
        .await
        {
            total_size = total_size.saturating_add(revision.exact_size);
            let track_id_str = track.id.to_string();
            catalog_tracks.push(AirCatCatalogTrack {
                track_id: track_id_str.clone(),
                logical_track_id: Some(track_id_str.clone()),
                remote_track_id: Some(track_id_str),
                title: track.title.unwrap_or_default(),
                artist: track.artist.unwrap_or_default(),
                album: track.album.unwrap_or_default(),
                format: track.format.as_str().to_string(),
                size: revision.exact_size,
                checksum: revision.exact_sha256,
            });
        }
    }

    let total_tracks = catalog_tracks.len() as i32;

    Ok((
        aircat_json_headers(),
        Json(AirCatCatalogManifest {
            schema: "michi.aircat.manifest.v1".to_string(),
            version: 1,
            device_id: producer_michi_id,
            total_tracks,
            total_size,
            tracks: catalog_tracks,
        }),
    ))
}

/// POST /api/v1/aircat/authorize
///
/// Validates selection and intent against real original assets, records a producer
/// snapshot, reserves the challenge nonce, and issues a signed Ed25519 grant.
pub async fn aircat_authorize_handler(
    State(state): State<AppState>,
    transport: AirCatTransportInfo,
    headers: HeaderMap,
    Json(body): Json<aircat_grants::Request>,
) -> Result<(HeaderMap, Json<aircat_grants::Grant>), (StatusCode, Json<serde_json::Value>)> {
    enforce_aircat_tls_transport(&state, &transport).await?;
    let authenticated_member =
        resolve_authenticated_caller(&state, &headers, Some(&body.intent.destination_michi_id))
            .await?;

    // AUTH-02: destinationMichiId in body MUST match authenticated member
    if body.intent.destination_michi_id != authenticated_member {
        return Err(aircat_error(
            StatusCode::FORBIDDEN,
            "WRONG_AUDIENCE",
            "destinationMichiId does not match authenticated member",
        ));
    }

    let now_ms = chrono::Utc::now().timestamp_millis();
    if body.intent.expires_at <= now_ms {
        return Err(aircat_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "GRANT_EXPIRED",
            "intent.expiresAt must be in the future",
        ));
    }
    let home_id = state.home_root_authority.home_id();
    let producer_id = state.identity.michi_id().to_base64url();

    // Verify intent source matches this server
    if body.intent.source_michi_id != producer_id {
        return Err(aircat_error(
            StatusCode::FORBIDDEN,
            "WRONG_SOURCE",
            "sourceMichiId does not match producer identity",
        ));
    }

    // Verify manifest hash matching canonical computation
    let computed_manifest_sha = aircat_grants::manifest_sha256(&body.manifest).map_err(|r| {
        let (status, code, msg) = refusal_to_status(r);
        aircat_error(status, code, msg)
    })?;

    if body.manifest_sha256 != computed_manifest_sha {
        return Err(aircat_error(
            StatusCode::CONFLICT,
            "MANIFEST_ALTERED",
            "manifestSha256 does not match canonical manifest calculation",
        ));
    }

    // Verify each selected item exists in server library and matches exact bytes
    let mut approved_assets = Vec::with_capacity(body.manifest.items.len());
    let mut original_revisions = Vec::with_capacity(body.manifest.items.len());

    for item in &body.manifest.items {
        let track_uuid = Uuid::parse_str(&item.remote_track_id).map_err(|_| {
            aircat_error(
                StatusCode::UNPROCESSABLE_ENTITY,
                "INVALID_TRACK_ID",
                "remoteTrackId is not a valid UUID",
            )
        })?;

        let track = michi_db::get_track(&state.db, &track_uuid)
            .await
            .map_err(|e| {
                aircat_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "DB_ERROR",
                    &e.to_string(),
                )
            })?
            .ok_or_else(|| {
                aircat_error(
                    StatusCode::NOT_FOUND,
                    "TRACK_NOT_FOUND",
                    &format!("track {} not found in library", item.remote_track_id),
                )
            })?;

        let _permit = AIRCAT_HASH_SEMAPHORE.acquire().await.map_err(|_| {
            aircat_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "CONCURRENCY_LIMIT",
                "aircat hashing semaphore unavailable",
            )
        })?;
        let (_file_item, revision) =
            aircat_original_file::inspect_original_asset(&state.config.music_paths, &track)
                .await
                .map_err(|r| {
                    let (status, code, msg) = refusal_to_status(r);
                    aircat_error(status, code, msg)
                })?;

        if revision.exact_size != item.size
            || !revision.exact_sha256.eq_ignore_ascii_case(&item.sha256)
        {
            return Err(aircat_error(
                StatusCode::CONFLICT,
                "ORIGINAL_BYTES_MISMATCH",
                "requested item size or sha256 does not match server original file",
            ));
        }

        approved_assets.push(ApprovedAsset {
            remote_track_id: item.remote_track_id.clone(),
            exact_size: revision.exact_size,
            exact_sha256: revision.exact_sha256.clone(),
        });

        original_revisions.push(revision);
    }

    // Construct and record ProducerSnapshot (clamp lifetime to 600s max matching issue_grant)
    let clamped_expires_at = now_ms.saturating_add(600_000).min(body.intent.expires_at);
    let snapshot = ProducerSnapshot {
        manifest: body.manifest.clone(),
        manifest_sha256: body.manifest_sha256.clone(),
        authenticated_destination_michi_id: authenticated_member.clone(),
        expires_at_ms: clamped_expires_at,
        originals: original_revisions,
    };

    if !state
        .aircat_security
        .has_snapshot(&body.manifest.transfer_id, &body.manifest_sha256)
    {
        state
            .aircat_security
            .record_producer_snapshot(snapshot, now_ms)
            .map_err(|r| {
                let (status, code, msg) = refusal_to_status(r);
                aircat_error(status, code, msg)
            })?;
    }

    // Reserve nonce and authorization challenge atomically
    let _reserved_snapshot = state
        .aircat_security
        .reserve_authorization(&body, &authenticated_member, now_ms)
        .map_err(|r| {
            if r == Refusal::InvalidFields {
                aircat_error(
                    StatusCode::CONFLICT,
                    "NONCE_REPLAY",
                    "authorization challenge nonce already consumed or invalid",
                )
            } else {
                let (status, code, msg) = refusal_to_status(r);
                aircat_error(status, code, msg)
            }
        })?;

    // Build AuthorizedContext
    let ctx = AuthorizedContext {
        home_id,
        source_michi_id: producer_id,
        destination_michi_id: authenticated_member,
        source_library_id: body.intent.source_library_id.clone(),
        allowed_destination_library_id: body.intent.destination_library_id.clone(),
        is_active_home_member: true,
        may_read_original_files: true,
        approved_assets,
    };

    // Issue grant using HomeRootAuthority
    let grant = aircat_grants::issue_grant(&body, &ctx, &state.home_root_authority, now_ms)
        .map_err(|r| {
            let (status, code, msg) = refusal_to_status(r);
            aircat_error(status, code, msg)
        })?;

    Ok((aircat_json_headers(), Json(grant)))
}

fn accept_encoding_forbids_identity(headers: &HeaderMap) -> bool {
    let Some(raw) = headers
        .get(header::ACCEPT_ENCODING)
        .and_then(|v| v.to_str().ok())
    else {
        return false;
    };
    let lower = raw.to_ascii_lowercase();
    let has_transform = ["gzip", "br", "deflate", "zstd"]
        .iter()
        .any(|enc| lower.contains(enc));
    has_transform && !lower.contains("identity") && !lower.contains('*')
}

struct DownloadStreamGuard {
    state: AppState,
    home_id: String,
    authenticated_member: String,
    transfer_id: String,
    manifest_sha256: String,
    remote_id: String,
    expires_at_millis: i64,
    remaining_bytes: u64,
    expected_file_size: i64,
    expected_modified_unix_ns: Option<i128>,
}

fn checked_download_stream(
    file: tokio::fs::File,
    guard: DownloadStreamGuard,
) -> impl futures_util::Stream<Item = Result<axum::body::Bytes, std::io::Error>> + Send + 'static {
    let last_db_check = tokio::time::Instant::now();
    futures_util::stream::unfold(
        (Some((file, guard)), last_db_check),
        move |(state_tuple, mut last_db_check)| async move {
            let (mut file, mut guard) = state_tuple?;
            if guard.remaining_bytes == 0 {
                return None;
            }
            let now_ms = chrono::Utc::now().timestamp_millis();
            let mut revoked = guard
                .state
                .aircat_security
                .is_member_revoked_in_memory(&guard.authenticated_member);
            if !revoked && last_db_check.elapsed() >= DB_REVOCATION_RECHECK_INTERVAL {
                revoked =
                    is_member_revoked(&guard.state, &guard.home_id, &guard.authenticated_member)
                        .await;
                last_db_check = tokio::time::Instant::now();
            }

            if now_ms >= guard.expires_at_millis
                || revoked
                || guard
                    .state
                    .aircat_security
                    .selected_original(
                        &guard.transfer_id,
                        &guard.manifest_sha256,
                        &guard.authenticated_member,
                        &guard.remote_id,
                        now_ms,
                    )
                    .is_err()
            {
                guard.state.aircat_security.record_midstream_revocation();
                return Some((
                    Err(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        "aircat grant expired or member revoked mid-stream",
                    )),
                    (None, last_db_check),
                ));
            }

            match file.metadata().await {
                Ok(meta)
                    if meta.is_file()
                        && meta.len() as i64 == guard.expected_file_size
                        && (guard.expected_modified_unix_ns.is_none()
                            || aircat_original_file::metadata_modified_unix_ns(&meta)
                                == guard.expected_modified_unix_ns) => {}
                _ => {
                    return Some((
                        Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "original file modified mid-stream",
                        )),
                        (None, last_db_check),
                    ));
                }
            }

            let to_read = AIRCAT_CHUNK_BYTES.min(guard.remaining_bytes as usize);
            let mut buf = vec![0u8; to_read];
            match file.read(&mut buf).await {
                Ok(0) => Some((
                    Err(std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        "original file truncated mid-stream",
                    )),
                    (None, last_db_check),
                )),
                Ok(n) => {
                    guard.remaining_bytes -= n as u64;
                    guard.state.aircat_security.record_bytes_served(n as u64);
                    buf.truncate(n);
                    Some((
                        Ok(axum::body::Bytes::from(buf)),
                        (Some((file, guard)), last_db_check),
                    ))
                }
                Err(e) => Some((Err(e), (None, last_db_check))),
            }
        },
    )
}

/// GET /api/v1/aircat/download/:remote_id
///
/// Streams raw original uncompressed/original file bytes with ETag and receipt header.
pub async fn aircat_download_handler(
    State(state): State<AppState>,
    transport: AirCatTransportInfo,
    Path(remote_id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, (StatusCode, Json<serde_json::Value>)> {
    enforce_aircat_tls_transport(&state, &transport).await?;
    if accept_encoding_forbids_identity(&headers) {
        return Err(aircat_error(
            StatusCode::NOT_ACCEPTABLE,
            "TRANSFORM_ENCODING_REJECTED",
            "AirCat original downloads require identity encoding without compression",
        ));
    }

    let now_ms = chrono::Utc::now().timestamp_millis();

    let grant_header_raw = match headers
        .get("X-Michi-AirCat-Grant")
        .and_then(|v| v.to_str().ok())
    {
        Some(raw) => raw,
        None => {
            let _ = resolve_authenticated_caller(&state, &headers, None).await?;
            return Err(aircat_error(
                StatusCode::UNAUTHORIZED,
                "MISSING_GRANT",
                "missing X-Michi-AirCat-Grant header",
            ));
        }
    };

    // Compute mandatory protocol receipt header: lowercase_hex(sha256(ASCII(header_value)))
    let receipt = match aircat_grants::receipt_for_grant_header(grant_header_raw) {
        Ok(r) => r,
        Err(_) => {
            let _ = resolve_authenticated_caller(&state, &headers, None).await?;
            return Err(aircat_error(
                StatusCode::UNPROCESSABLE_ENTITY,
                "INVALID_GRANT_HEADER",
                "grant header is not valid base64url or exceeds size limits",
            ));
        }
    };

    // Decode grant JSON
    let grant_json_bytes = match URL_SAFE_NO_PAD.decode(grant_header_raw) {
        Ok(b) => b,
        Err(_) => {
            let _ = resolve_authenticated_caller(&state, &headers, None).await?;
            return Err(aircat_error(
                StatusCode::UNPROCESSABLE_ENTITY,
                "INVALID_GRANT_BASE64",
                "failed to decode base64url grant payload",
            ));
        }
    };

    let grant: aircat_grants::Grant = match serde_json::from_slice(&grant_json_bytes) {
        Ok(g) => g,
        Err(_) => {
            let _ = resolve_authenticated_caller(&state, &headers, None).await?;
            return Err(aircat_error(
                StatusCode::UNPROCESSABLE_ENTITY,
                "INVALID_GRANT_JSON",
                "failed to parse grant json",
            ));
        }
    };

    // Authenticate caller session
    let authenticated_member =
        resolve_authenticated_caller(&state, &headers, Some(&grant.destination_michi_id)).await?;

    // Verify audience matches authenticated caller
    if grant.destination_michi_id != authenticated_member {
        return Err(aircat_error(
            StatusCode::FORBIDDEN,
            "WRONG_AUDIENCE",
            "grant destinationMichiId does not match authenticated member",
        ));
    }

    // Verify Home signature and grant parameters
    let root_pk_bytes = decode_base64url_strict(&state.home_root_authority.public_key_base64url())
        .map_err(|_| {
            aircat_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "CRYPTO_ERROR",
                "failed to decode root authority public key",
            )
        })?;
    let root_key = VerifyingKey::from_bytes(&root_pk_bytes.try_into().map_err(|_| {
        aircat_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "CRYPTO_ERROR",
            "invalid root authority public key length",
        )
    })?)
    .map_err(|_| {
        aircat_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "CRYPTO_ERROR",
            "invalid root authority public key format",
        )
    })?;

    let is_revoked = is_member_revoked(&state, &grant.home_id, &grant.destination_michi_id).await;

    aircat_grants::verify_grant(&grant, &root_key, &authenticated_member, now_ms, is_revoked)
        .map_err(|r| {
            let (status, code, msg) = refusal_to_status(r);
            aircat_error(status, code, msg)
        })?;

    // Verify remote_id is in retained authorized snapshot
    let revision = state
        .aircat_security
        .selected_original(
            &grant.transfer_id,
            &grant.manifest_sha256,
            &authenticated_member,
            &remote_id,
            now_ms,
        )
        .map_err(|r| {
            let (status, code, msg) = refusal_to_status(r);
            aircat_error(status, code, msg)
        })?;

    // Lookup track in DB
    let track_uuid = Uuid::parse_str(&remote_id).map_err(|_| {
        aircat_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "INVALID_TRACK_ID",
            "remote_id is not a valid UUID",
        )
    })?;

    let track = michi_db::get_track(&state.db, &track_uuid)
        .await
        .map_err(|e| {
            aircat_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DB_ERROR",
                &e.to_string(),
            )
        })?
        .ok_or_else(|| {
            aircat_error(
                StatusCode::NOT_FOUND,
                "TRACK_NOT_FOUND",
                "track not found in server library",
            )
        })?;

    // Open original file descriptor without following symlinks
    let (_path, mut file) =
        aircat_original_file::open_original_file_no_symlinks(&state.config.music_paths, &track)
            .await
            .map_err(|r| {
                let (status, code, msg) = refusal_to_status(r);
                aircat_error(status, code, msg)
            })?;

    // Verify file revision (size + inode + mtime) in descriptor has not changed
    let metadata = file.metadata().await.map_err(|e| {
        aircat_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "IO_ERROR",
            &e.to_string(),
        )
    })?;

    let file_len = metadata.len() as i64;
    if !aircat_original_file::revision_matches_metadata(&revision, &metadata) {
        return Err(aircat_error(
            StatusCode::CONFLICT,
            "FILE_REVISION_CHANGED",
            "original file size, inode, or mtime changed during transfer",
        ));
    }

    let filename = format!("{}.{}", track.id, track.format.as_str());

    // 304 Not Modified when If-None-Match matches strong ETag and no Range is requested
    if !headers.contains_key(header::RANGE) {
        if let Some(inm) = headers
            .get(header::IF_NONE_MATCH)
            .and_then(|v| v.to_str().ok())
        {
            if inm
                .split(',')
                .any(|tag| tag.trim() == revision.strong_etag || tag.trim() == "*")
            {
                let resp = Response::builder()
                    .status(StatusCode::NOT_MODIFIED)
                    .header(AIRCAT_PROTOCOL_HEADER, AIRCAT_PROTOCOL_VALUE)
                    .header(header::CACHE_CONTROL, "no-store, no-transform")
                    .header(header::ETAG, &revision.strong_etag)
                    .header("X-Michi-AirCat-Authorized-Grant-SHA256", &receipt)
                    .body(Body::empty())
                    .map_err(|e| {
                        aircat_error(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            "RESPONSE_BUILD_ERROR",
                            &e.to_string(),
                        )
                    })?;
                return Ok(resp);
            }
        }
    }

    state.aircat_security.record_download_started();

    // Check If-Range header
    let if_range_matched =
        if let Some(if_range) = headers.get(header::IF_RANGE).and_then(|h| h.to_str().ok()) {
            if_range.trim() == revision.strong_etag
        } else {
            true
        };

    // Range handling
    if if_range_matched {
        if let Some(range_header) = headers.get(header::RANGE).and_then(|h| h.to_str().ok()) {
            match michi_streaming::parse_range(range_header, file_len as u64) {
                Ok(range) => {
                    file.seek(std::io::SeekFrom::Start(range.start))
                        .await
                        .map_err(|e| {
                            aircat_error(
                                StatusCode::INTERNAL_SERVER_ERROR,
                                "IO_ERROR",
                                &e.to_string(),
                            )
                        })?;

                    let stream = checked_download_stream(
                        file,
                        DownloadStreamGuard {
                            state: state.clone(),
                            home_id: grant.home_id.clone(),
                            authenticated_member: authenticated_member.clone(),
                            transfer_id: grant.transfer_id.clone(),
                            manifest_sha256: grant.manifest_sha256.clone(),
                            remote_id: remote_id.clone(),
                            expires_at_millis: grant.expires_at_millis,
                            remaining_bytes: range.content_length(),
                            expected_file_size: revision.exact_size,
                            expected_modified_unix_ns: revision.modified_unix_ns,
                        },
                    );

                    let response = Response::builder()
                        .status(StatusCode::PARTIAL_CONTENT)
                        .header(AIRCAT_PROTOCOL_HEADER, AIRCAT_PROTOCOL_VALUE)
                        .header(header::CONTENT_TYPE, "application/octet-stream")
                        .header(header::CACHE_CONTROL, "no-store, no-transform")
                        .header(header::CONTENT_RANGE, range.content_range_header())
                        .header(header::CONTENT_LENGTH, range.content_length().to_string())
                        .header(header::ACCEPT_RANGES, "bytes")
                        .header(header::ETAG, &revision.strong_etag)
                        .header("X-Michi-AirCat-Authorized-Grant-SHA256", receipt)
                        .header(
                            header::CONTENT_DISPOSITION,
                            format!("attachment; filename=\"{filename}\""),
                        )
                        .body(Body::from_stream(stream))
                        .map_err(|e| {
                            aircat_error(
                                StatusCode::INTERNAL_SERVER_ERROR,
                                "RESPONSE_BUILD_ERROR",
                                &e.to_string(),
                            )
                        })?;

                    return Ok(response);
                }
                Err(_) => {
                    let resp = Response::builder()
                        .status(StatusCode::RANGE_NOT_SATISFIABLE)
                        .header(AIRCAT_PROTOCOL_HEADER, AIRCAT_PROTOCOL_VALUE)
                        .header(header::CACHE_CONTROL, "no-store, no-transform")
                        .header(header::CONTENT_RANGE, format!("bytes */{file_len}"))
                        .header("X-Michi-AirCat-Authorized-Grant-SHA256", receipt)
                        .body(Body::empty())
                        .map_err(|e| {
                            aircat_error(
                                StatusCode::INTERNAL_SERVER_ERROR,
                                "RESPONSE_BUILD_ERROR",
                                &e.to_string(),
                            )
                        })?;
                    return Ok(resp);
                }
            }
        }
    }

    // Full 200 OK download
    let stream = checked_download_stream(
        file,
        DownloadStreamGuard {
            state: state.clone(),
            home_id: grant.home_id.clone(),
            authenticated_member,
            transfer_id: grant.transfer_id,
            manifest_sha256: grant.manifest_sha256,
            remote_id,
            expires_at_millis: grant.expires_at_millis,
            remaining_bytes: file_len as u64,
            expected_file_size: revision.exact_size,
            expected_modified_unix_ns: revision.modified_unix_ns,
        },
    );
    let response = Response::builder()
        .status(StatusCode::OK)
        .header(AIRCAT_PROTOCOL_HEADER, AIRCAT_PROTOCOL_VALUE)
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(header::CACHE_CONTROL, "no-store, no-transform")
        .header(header::CONTENT_LENGTH, file_len.to_string())
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::ETAG, &revision.strong_etag)
        .header("X-Michi-AirCat-Authorized-Grant-SHA256", receipt)
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{filename}\""),
        )
        .body(Body::from_stream(stream))
        .map_err(|e| {
            aircat_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "RESPONSE_BUILD_ERROR",
                &e.to_string(),
            )
        })?;

    Ok(response)
}

/// GET /api/v1/aircat/enrollment-qr
///
/// Generates an administrative AirCat enrollment QR payload (michi.aircat.home.enrollment.v1).
/// Fails closed with 503 if real HTTPS/TLS + SPKI pinning is not configured.
pub async fn aircat_enrollment_qr_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(HeaderMap, Json<AirCatTrustedEnrollmentQr>), (StatusCode, Json<serde_json::Value>)> {
    let _authenticated_admin = resolve_webui_or_bearer_caller(&state, &headers, true).await?;

    let tls_info = state.aircat_tls.read().await.clone().ok_or_else(|| {
        aircat_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "TLS_NOT_CONFIGURED",
            "HTTPS/TLS certificate and SPKI pin are not configured for AirCat enrollment",
        )
    })?;

    let now_ms = chrono::Utc::now().timestamp_millis();
    let home_id = state.home_root_authority.home_id();
    let root_pk = state.home_root_authority.public_key_base64url();
    let source_michi_id = state.identity.michi_id().to_base64url();

    let mut random_nonce = [0u8; 24];
    use rand::RngCore;
    rand::rngs::OsRng.fill_bytes(&mut random_nonce);
    let nonce_base64_url = URL_SAFE_NO_PAD.encode(random_nonce);

    Ok((
        aircat_json_headers(),
        Json(AirCatTrustedEnrollmentQr {
            qr_type: "michi.aircat.home.enrollment.v1".to_string(),
            home_id: home_id.clone(),
            authority_michi_id: home_id,
            authority_public_key_base64_url: root_pk,
            source_michi_id,
            pinned_base_url: tls_info.pinned_base_url,
            tls_spki_sha256: tls_info.tls_spki_sha256,
            nonce_base64_url,
            issued_at_millis: now_ms,
            expires_at_millis: now_ms + 600_000,
        }),
    ))
}

/// POST /api/v1/auth/challenge
///
/// Issues a one-time cryptographic challenge for HOME_MEMBERSHIP mutual device authentication.
pub async fn home_auth_challenge_handler(
    State(state): State<AppState>,
    Json(body): Json<michi_identity::types::DeviceAuthChallengeRequest>,
) -> Result<
    Json<michi_identity::types::DeviceAuthChallengeResponse>,
    (StatusCode, Json<serde_json::Value>),
> {
    if body.client_michi_id.len() != 43
        || decode_base64url_strict(&body.client_michi_id)
            .map(|b| b.len() != 32)
            .unwrap_or(true)
    {
        return Err(aircat_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "INVALID_FIELDS",
            "client_michi_id must be a canonical 43-character base64url Michi ID",
        ));
    }

    let home_id = state.home_root_authority.home_id();
    if body.home_id != home_id {
        return Err(aircat_error(
            StatusCode::FORBIDDEN,
            "HOME_ID_MISMATCH",
            "home_id does not match this server's Home authority",
        ));
    }
    let derived_client_id = michi_identity::IdentityManager::derive_michi_id(
        &body.client_public_key,
    )
    .map_err(|_| {
        aircat_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "INVALID_FIELDS",
            "invalid client_public_key",
        )
    })?;
    if derived_client_id.to_base64url() != body.client_michi_id {
        return Err(aircat_error(
            StatusCode::FORBIDDEN,
            "IDENTITY_MISMATCH",
            "client_michi_id does not match client_public_key",
        ));
    }

    if is_member_revoked(&state, &home_id, &body.client_michi_id).await {
        return Err(aircat_error(
            StatusCode::FORBIDDEN,
            "MEMBER_REVOKED",
            "device identity is revoked in this Home",
        ));
    }

    let mut nonce_bytes = [0u8; 32];
    use rand::RngCore;
    rand::rngs::OsRng.fill_bytes(&mut nonce_bytes);
    let challenge_nonce = URL_SAFE_NO_PAD.encode(nonce_bytes);
    let now_ms = chrono::Utc::now().timestamp_millis();

    let challenge_id = state
        .aircat_security
        .issue_home_challenge(
            &body.client_michi_id,
            challenge_nonce.clone(),
            now_ms,
            60_000,
        )
        .map_err(|r| {
            let (status, code, msg) = refusal_to_status(r);
            aircat_error(status, code, msg)
        })?;

    Ok(Json(michi_identity::types::DeviceAuthChallengeResponse {
        challenge_id,
        challenge_nonce,
        server_michi_id: state.identity.michi_id().to_base64url(),
        server_public_key: state.identity.public_key_base64url(),
        expires_in: 60,
    }))
}

/// POST /api/v1/auth/session
///
/// Verifies Home membership certificate + Ed25519 challenge signature and establishes a
/// bound HOME_MEMBERSHIP bearer session.
pub async fn home_auth_session_handler(
    State(state): State<AppState>,
    Json(body): Json<michi_identity::types::DeviceAuthSessionRequest>,
) -> Result<
    Json<michi_identity::types::DeviceAuthSessionResponse>,
    (StatusCode, Json<serde_json::Value>),
> {
    let now_ms = chrono::Utc::now().timestamp_millis();
    let home_id = state.home_root_authority.home_id();
    let root_pk = state.home_root_authority.public_key_base64url();

    if body.client_michi_id != body.membership.device_michi_id {
        return Err(aircat_error(
            StatusCode::FORBIDDEN,
            "IDENTITY_MISMATCH",
            "client_michi_id does not match membership.device_michi_id",
        ));
    }

    if is_member_revoked(&state, &home_id, &body.client_michi_id).await {
        return Err(aircat_error(
            StatusCode::FORBIDDEN,
            "MEMBER_REVOKED",
            "member is revoked",
        ));
    }

    let challenge_nonce = state
        .aircat_security
        .consume_home_challenge(&body.challenge_id, &body.client_michi_id, now_ms)
        .map_err(|r| {
            let (status, code, msg) = refusal_to_status(r);
            aircat_error(status, code, msg)
        })?;

    let revocations = state.receiver_manager.get_revocations().await;

    michi_identity::home::verify_membership(&body.membership, &root_pk, &home_id, &revocations)
        .map_err(|e| match e {
            michi_identity::home::HomeAuthError::DeviceRevoked => {
                aircat_error(StatusCode::FORBIDDEN, "MEMBER_REVOKED", "member is revoked")
            }
            _ => aircat_error(
                StatusCode::FORBIDDEN,
                "UNTRUSTED_MEMBER",
                &format!("membership verification failed: {e}"),
            ),
        })?;

    let server_michi_id = state.identity.michi_id().to_base64url();
    let challenge_id_str = body.challenge_id.to_string();
    let sig_ok = michi_identity::home::verify_device_auth_challenge(
        &body.membership.device_public_key,
        &body.client_signature,
        &home_id,
        &server_michi_id,
        &body.client_michi_id,
        &challenge_id_str,
        &challenge_nonce,
    )
    .unwrap_or(false);

    if !sig_ok {
        return Err(aircat_error(
            StatusCode::FORBIDDEN,
            "BAD_SIGNATURE",
            "invalid device challenge signature",
        ));
    }

    let mut token_bytes = [0u8; 32];
    use rand::RngCore;
    rand::rngs::OsRng.fill_bytes(&mut token_bytes);
    let session_token = format!("michi_home_{}", URL_SAFE_NO_PAD.encode(token_bytes));

    let ttl_secs: u64 = 3600;
    let synthetic_device_uuid = Uuid::new_v5(&Uuid::NAMESPACE_OID, body.client_michi_id.as_bytes());
    state
        .token_store
        .store(
            &session_token,
            michi_link::TokenType::Device,
            synthetic_device_uuid,
        )
        .await;
    state.aircat_security.record_home_session(
        session_token.clone(),
        body.client_michi_id.clone(),
        now_ms,
        (ttl_secs as i64) * 1000,
    );

    let confirm_payload = michi_identity::home::server_auth_confirm_payload(
        &home_id,
        &server_michi_id,
        &body.client_michi_id,
        &challenge_id_str,
        &challenge_nonce,
    );
    let (server_signature, _) = state.identity.sign_base64url(&confirm_payload);

    Ok(Json(michi_identity::types::DeviceAuthSessionResponse {
        session_token,
        token_type: "Bearer".to_string(),
        expires_in: ttl_secs,
        server_michi_id,
        server_membership: state.server_membership.clone(),
        server_signature,
    }))
}

/// GET /api/v1/aircat/status
///
/// Returns AirCat TLS/SPKI readiness and runtime transfer/revocation observability metrics.
pub async fn aircat_status_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(HeaderMap, Json<serde_json::Value>), (StatusCode, Json<serde_json::Value>)> {
    let _authenticated_member = resolve_webui_or_bearer_caller(&state, &headers, false).await?;
    let now_ms = chrono::Utc::now().timestamp_millis();
    let tls_info = state.aircat_tls.read().await.clone();
    let stats = state.aircat_security.stats_snapshot(now_ms);
    Ok((
        aircat_json_headers(),
        Json(serde_json::json!({
            "protocol": AIRCAT_PROTOCOL_VALUE,
            "tls_configured": tls_info.is_some(),
            "tls_port": tls_info.as_ref().map(|t| t.tls_port),
            "pinned_base_url": tls_info.as_ref().map(|t| t.pinned_base_url.clone()),
            "tls_spki_sha256": tls_info.as_ref().map(|t| t.tls_spki_sha256.clone()),
            "stats": stats,
        })),
    ))
}
