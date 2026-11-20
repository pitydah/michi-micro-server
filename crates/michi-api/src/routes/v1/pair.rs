use axum::{
    extract::{Path, State},
    http::{header, StatusCode},
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::AppState;
use michi_identity::{
    IdentityError, PairConfirmRequest, PairConfirmResponse, PairStartRequest, PairStartResponse,
};
use michi_link::{
    generate_device_token, hash_token,
    models::{TokenRefreshRequest, TokenRefreshResponse},
    DeviceEntry, TokenType,
};

fn v1_error_code(
    status: StatusCode,
    code: michi_link::MichiLinkErrorCode,
    message: &str,
) -> (StatusCode, Json<serde_json::Value>) {
    (
        status,
        Json(serde_json::json!({
            "error": { "code": code.as_str(), "message": message, "details": {} }
        })),
    )
}

fn v1_error(
    status: StatusCode,
    code: &str,
    message: &str,
) -> (StatusCode, Json<serde_json::Value>) {
    (
        status,
        Json(serde_json::json!({
            "error": { "code": code, "message": message, "details": {} }
        })),
    )
}

fn v1_internal_error(msg: &str) -> (StatusCode, Json<serde_json::Value>) {
    v1_error_code(
        StatusCode::INTERNAL_SERVER_ERROR,
        michi_link::MichiLinkErrorCode::InternalError,
        msg,
    )
}

/// Map canonical `IdentityError` to the v1 error envelope.
fn pairing_error(e: &IdentityError) -> (StatusCode, Json<serde_json::Value>) {
    match e {
        IdentityError::RateLimited => v1_error_code(
            StatusCode::TOO_MANY_REQUESTS,
            michi_link::MichiLinkErrorCode::RateLimited,
            "Too many pairing requests. Please wait.",
        ),
        IdentityError::PairingNotFound => v1_error_code(
            StatusCode::NOT_FOUND,
            michi_link::MichiLinkErrorCode::PairingNotFound,
            "pairing session not found or expired",
        ),
        IdentityError::PairingExpired => v1_error_code(
            StatusCode::GONE,
            michi_link::MichiLinkErrorCode::PairingExpired,
            "pairing session expired",
        ),
        IdentityError::PairingAlreadyConsumed => v1_error_code(
            StatusCode::CONFLICT,
            michi_link::MichiLinkErrorCode::PairingAlreadyConsumed,
            "pairing already confirmed",
        ),
        IdentityError::PairingPinMismatch => v1_error_code(
            StatusCode::UNAUTHORIZED,
            michi_link::MichiLinkErrorCode::PairingPinMismatch,
            "invalid pairing PIN",
        ),
        IdentityError::PairingAttemptsExceeded => v1_error_code(
            StatusCode::TOO_MANY_REQUESTS,
            michi_link::MichiLinkErrorCode::PairingAttemptsExceeded,
            "maximum pairing PIN attempts exceeded",
        ),
        IdentityError::PairingKeyMismatch => v1_error_code(
            StatusCode::FORBIDDEN,
            michi_link::MichiLinkErrorCode::PairingKeyMismatch,
            "cryptographic identity key mismatch",
        ),
        _ => v1_error_code(
            StatusCode::BAD_REQUEST,
            michi_link::MichiLinkErrorCode::InvalidRequest,
            &e.to_string(),
        ),
    }
}

pub async fn link_pair_start(
    connect_info: Option<axum::extract::ConnectInfo<std::net::SocketAddr>>,
    headers: axum::http::HeaderMap,
    State(state): State<AppState>,
    Json(body): Json<PairStartRequest>,
) -> Result<Json<PairStartResponse>, (StatusCode, Json<serde_json::Value>)> {
    // Rate limit por IP: 10 intentos/minuto en pair/start (gate local, más
    // tolerante que confirm para permitir reintentos legítimos).
    let client_ip = crate::extract_client_ip(connect_info, &headers, &state.config);

    {
        let now = std::time::Instant::now();
        let mut entry = state
            .security_state
            .pairing_attempts
            .entry(client_ip.clone())
            .or_insert((0u32, now));
        let (count, last_reset) = entry.value();
        let elapsed = now.duration_since(*last_reset);

        if elapsed.as_secs() > 60 {
            *entry = (1, now);
        } else if *count >= 10 {
            tracing::warn!("Pairing start rate limit exceeded for IP: {}", client_ip);
            return Err(v1_error_code(
                StatusCode::TOO_MANY_REQUESTS,
                michi_link::MichiLinkErrorCode::RateLimited,
                "Too many pairing attempts. Please wait 60 seconds.",
            ));
        } else {
            *entry = (count + 1, *last_reset);
        }
    }

    if let Some(device_id) = headers
        .get("X-Michi-Device-Id")
        .and_then(|v| v.to_str().ok())
    {
        tracing::info!("pair/start from device: {}", device_id);
    }

    // Contrato canónico v1-lite: valida el challenge Ed25519 sobre el nonce,
    // la coherencia michi_id/public_key, el rate limit por source_key y crea
    // una sesión RAM-only (PairingRegistry del crate vendored).
    let (response, pin) = state
        .pairing_registry
        .start_server(&state.identity, &body, &client_ip)
        .map_err(|e| pairing_error(&e))?;

    // El PIN se muestra localmente en el servidor / observer in-memory y en el contrato canónico
    // NUNCA viaja por la red (cumplimiento estricto con pair-start-response.schema.json).
    *state.pairing_display.write().await = Some(pin.clone());
    state
        .pairing_sessions_display
        .write()
        .await
        .insert(response.session_id.to_string(), pin.clone());
    tracing::debug!(
        device = %body.device_name,
        session = %response.session_id,
        "pairing PIN registered for local display"
    );
    Ok(Json(response))
}

pub async fn link_pair_confirm(
    connect_info: Option<axum::extract::ConnectInfo<std::net::SocketAddr>>,
    headers: axum::http::HeaderMap,
    State(state): State<AppState>,
    Json(body): Json<PairConfirmRequest>,
) -> Result<Json<PairConfirmResponse>, (StatusCode, Json<serde_json::Value>)> {
    // Rate limit por IP: 5 intentos/minuto (gate local, se mantiene).
    let client_ip = crate::extract_client_ip(connect_info, &headers, &state.config);

    {
        let now = std::time::Instant::now();
        let mut entry = state
            .security_state
            .pairing_attempts
            .entry(client_ip.clone())
            .or_insert((0u32, now));
        let (count, last_reset) = entry.value();
        let elapsed = now.duration_since(*last_reset);

        if elapsed.as_secs() > 60 {
            *entry = (1, now);
        } else if *count >= 5 {
            tracing::warn!("Pairing rate limit exceeded for IP: {}", client_ip);
            return Err(v1_error_code(
                StatusCode::TOO_MANY_REQUESTS,
                michi_link::MichiLinkErrorCode::RateLimited,
                "Too many pairing attempts. Please wait 60 seconds.",
            ));
        } else {
            *entry = (count + 1, *last_reset);
        }
    }
    if let Some(device_id) = headers
        .get("X-Michi-Device-Id")
        .and_then(|v| v.to_str().ok())
    {
        tracing::info!("pair/confirm from device: {}", device_id);
    }

    // Validación canónica: sesión existente, no expirada, PIN con comparación
    // en tiempo constante, misma identidad cliente que en start.
    let session = state
        .pairing_registry
        .confirm(&body, &client_ip)
        .map_err(|e| pairing_error(&e))?;

    let device_token = generate_device_token();
    let refresh_token = generate_device_token();
    let device_id = Uuid::new_v4();
    let token_hash = hash_token(&device_token);

    // El contrato canónico no transporta device_name en confirm; se identifica
    // al cliente por su michi_id (derivado de su public_key).
    let client_identity = session
        .client_michi_id
        .clone()
        .unwrap_or_else(|| body.michi_id.clone());
    let device_entry = DeviceEntry::new(
        device_id,
        client_identity.clone(),
        "paired".into(),
        None,
        token_hash,
    );

    let core_device = michi_core::LinkDevice {
        device_id,
        alias: client_identity,
        device_type: "paired".into(),
        device_model: None,
        token_hash: hash_token(&device_token),
        permissions_json: serde_json::to_string(&device_entry.permissions).unwrap_or_default(),
        created_at: chrono::Utc::now(),
        last_seen: Some(chrono::Utc::now().to_rfc3339()),
        revoked: false,
    };

    michi_db::create_link_device(&state.db, &core_device)
        .await
        .map_err(|e| v1_internal_error(&e.to_string()))?;

    state
        .token_store
        .store(&device_token, TokenType::Device, device_id)
        .await;
    state
        .token_store
        .store(&refresh_token, TokenType::Refresh, device_id)
        .await;

    // Clear session display observer on successful confirm
    state
        .pairing_sessions_display
        .write()
        .await
        .remove(&body.session_id.to_string());

    Ok(Json(PairConfirmResponse {
        token: device_token,
        refresh_token: Some(refresh_token),
        expires_in: 604800,
        device_id: device_id.to_string(),
        server_id: state.server_id().to_string(),
    }))
}

pub async fn link_token_refresh(
    State(state): State<AppState>,
    Json(body): Json<TokenRefreshRequest>,
) -> Result<Json<TokenRefreshResponse>, (StatusCode, Json<serde_json::Value>)> {
    let device_id = match state
        .token_store
        .validate(&body.refresh_token, TokenType::Refresh)
        .await
    {
        Ok(id) => id,
        Err(_) => {
            return Err(v1_error(
                StatusCode::UNAUTHORIZED,
                "INVALID_TOKEN",
                "invalid or expired refresh token",
            ));
        }
    };

    let req_device_id = body.device_id.or_else(|| {
        body.client_device_id
            .as_ref()
            .and_then(|s| Uuid::parse_str(s).ok())
    });

    if let Some(req_id) = req_device_id {
        if req_id != device_id {
            return Err(v1_error(
                StatusCode::FORBIDDEN,
                "DEVICE_MISMATCH",
                "device id does not match token",
            ));
        }
    }

    let new_device_token = generate_device_token();
    let new_refresh_token = generate_device_token();

    // Revocar token anterior antes de emitir nuevo (rotación)
    state.token_store.revoke(&body.refresh_token).await;

    state
        .token_store
        .store(&new_device_token, TokenType::Device, device_id)
        .await;
    state
        .token_store
        .store(&new_refresh_token, TokenType::Refresh, device_id)
        .await;

    Ok(Json(TokenRefreshResponse {
        device_token: new_device_token,
        refresh_token: new_refresh_token,
    }))
}

pub async fn link_devices_revoke(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let device_id = body
        .get("device_id")
        .and_then(|v| v.as_str())
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| {
            v1_error(
                StatusCode::BAD_REQUEST,
                "INVALID_REQUEST",
                "device_id is required",
            )
        })?;

    let existing_dev = michi_db::get_link_device(&state.db, &device_id)
        .await
        .ok()
        .flatten();

    let revoked = michi_db::revoke_link_device(&state.db, &device_id)
        .await
        .map_err(|e| v1_internal_error(&e.to_string()))?;

    if !revoked {
        return Err(v1_error(
            StatusCode::NOT_FOUND,
            "NOT_FOUND",
            "device not found",
        ));
    }

    state.token_store.revoke_all_by_device(device_id).await;
    state.aircat_security.revoke_member(&device_id.to_string());
    if let Some(dev) = existing_dev {
        if !dev.alias.is_empty() {
            state.aircat_security.revoke_member(&dev.alias);
        }
    }

    Ok(Json(serde_json::json!({ "status": "revoked" })))
}

pub async fn list_devices_handler(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let devices = michi_db::list_link_devices(&state.db).await.map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DB_ERROR",
            &e.to_string(),
        )
    })?;

    let now = chrono::Utc::now();
    let items: Vec<serde_json::Value> = devices
        .into_iter()
        .map(|d| {
            let online = !d.revoked
                && d.last_seen
                    .as_ref()
                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                    .map(|t| (now - t.with_timezone(&chrono::Utc)).num_seconds() < 180)
                    .unwrap_or(false);
            serde_json::json!({
                "device_id": d.device_id,
                "alias": d.alias,
                "device_type": d.device_type,
                "device_model": d.device_model,
                "last_seen": d.last_seen,
                "online": online,
                "revoked": d.revoked,
            })
        })
        .collect();

    Ok(Json(serde_json::json!({ "devices": items })))
}

// ── QR Pairing (Legacy SERVER_CODE v1 — strictly distinct from Home Enrollment QR) ──

#[derive(Serialize, Debug, Deserialize, Default)]
pub struct QrGenerateBody {
    pub server_url: Option<String>,
}

pub struct QrGenerateResponse {
    pub qr_code: Uuid,
    pub expires_at: String,
    pub svg_url: String,
    pub server_url: String,
}

#[derive(Debug, Clone)]
pub struct QrUrlPolicy {
    /// Operator-configured canonical public URL (`MICHI_PUBLIC_URL` / `MICHI_PUBLIC_HTTPS_URL`).
    pub configured_public_url: Option<String>,
    /// Explicitly authorized hosts (lowercased hostnames or IP literals from operator config,
    /// `MICHI_ALLOWED_QR_HOSTS`, `MICHI_ADVERTISE_HOST`, `MICHI_TAILSCALE_HOST`, CORS, or TLS SAN).
    pub allowed_hosts: Vec<String>,
    /// Actual IP addresses owned by this server's network interfaces (plus loopback).
    pub local_interface_ips: Vec<std::net::IpAddr>,
    /// Actual `.local` mDNS hostnames advertised by this server instance.
    pub local_mdns_hosts: Vec<String>,
    /// Server HTTP listener port for local fallback.
    pub server_port: u16,
}

impl QrUrlPolicy {
    pub async fn from_env_and_state(state: &AppState) -> Self {
        let configured_public_url = std::env::var("MICHI_PUBLIC_URL")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .or_else(|| {
                std::env::var("MICHI_PUBLIC_HTTPS_URL")
                    .ok()
                    .filter(|v| !v.trim().is_empty())
            });

        let mut allowed_hosts = Vec::new();
        let mut push_allowed = |raw: &str| {
            let normalized = normalize_host_for_allowlist(raw);
            if !normalized.is_empty() && !allowed_hosts.contains(&normalized) {
                allowed_hosts.push(normalized);
            }
        };

        if let Ok(raw_hosts) = std::env::var("MICHI_ALLOWED_QR_HOSTS") {
            for h in raw_hosts.split(',') {
                push_allowed(h);
            }
        }
        if let Ok(adv_host) = std::env::var("MICHI_ADVERTISE_HOST") {
            push_allowed(&adv_host);
        }
        if let Ok(ts_host) = std::env::var("MICHI_TAILSCALE_HOST") {
            push_allowed(&ts_host);
        }
        if let Some(ref pub_url) = configured_public_url {
            if let Ok(parsed) = reqwest::Url::parse(pub_url.trim()) {
                if let Some(h) = parsed.host_str() {
                    push_allowed(h);
                }
            }
        }
        if let Some(ref cors) = state.config.cors_origin {
            if let Ok(parsed) = reqwest::Url::parse(cors.trim()) {
                if let Some(h) = parsed.host_str() {
                    push_allowed(h);
                }
            }
        }
        if let Some(ref tls) = *state.aircat_tls.read().await {
            if let Ok(parsed) = reqwest::Url::parse(tls.pinned_base_url.trim()) {
                if let Some(h) = parsed.host_str() {
                    push_allowed(h);
                }
            }
        }

        let local_interface_ips = michi_connect::MichiConnect::local_server_ips();
        let local_mdns_hosts = michi_connect::MichiConnect::local_mdns_hostnames_for_host(None);

        Self {
            configured_public_url,
            allowed_hosts,
            local_interface_ips,
            local_mdns_hosts,
            server_port: state.config.port(),
        }
    }
}

fn normalize_host_for_allowlist(raw: &str) -> String {
    raw.trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .trim_end_matches('.')
        .to_ascii_lowercase()
}

fn has_forbidden_url_chars(s: &str) -> bool {
    s.bytes()
        .any(|b| b < 0x20 || b == 0x7f || b == b' ' || b == b'\t')
}

fn is_valid_dns_hostname(host: &str) -> bool {
    if host.is_empty() || host.len() > 253 {
        return false;
    }
    let trimmed = host.trim_end_matches('.');
    if trimmed.is_empty() {
        return false;
    }
    for label in trimmed.split('.') {
        if label.is_empty() || label.len() > 63 {
            return false;
        }
        if label.starts_with('-') || label.ends_with('-') {
            return false;
        }
        if !label
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return false;
        }
    }
    true
}

/// Checks whether a parsed URL host string is authorized as a Michi QR destination
/// bound strictly to THIS server (R1.4 §3.4):
/// - Loopback (`localhost`, `127.0.0.1`, `::1`), OR
/// - An IP address actually owned by one of this server's local network interfaces
///   (`policy.local_interface_ips`), OR
/// - An mDNS `.local` hostname actually advertised by this server instance
///   (`policy.local_mdns_hosts`), OR
/// - Explicitly authorized in `policy.allowed_hosts` (`MICHI_PUBLIC_URL`,
///   `MICHI_PUBLIC_HTTPS_URL`, `MICHI_ALLOWED_QR_HOSTS`, `MICHI_ADVERTISE_HOST`,
///   `MICHI_TAILSCALE_HOST`, `MICHI_CORS_ORIGIN`, or active TLS pinned host).
///
/// Arbitrary third-party hosts — including OTHER RFC 1918 LAN IPs, other `*.local`
/// hosts, other `*.ts.net` peers, and public internet hosts — are REJECTED fail-closed.
pub fn is_authorized_qr_host(raw_host: &str, policy: &QrUrlPolicy) -> bool {
    let trimmed = raw_host
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']');
    if let Ok(ip) = trimmed.parse::<std::net::IpAddr>() {
        if ip.is_unspecified() || ip.is_multicast() {
            return false;
        }
        if let std::net::IpAddr::V4(v4) = ip {
            if v4.is_broadcast() {
                return false;
            }
        }
        if ip.is_loopback() || policy.local_interface_ips.contains(&ip) {
            return true;
        }
        let norm = ip.to_string().to_ascii_lowercase();
        return policy.allowed_hosts.iter().any(|h| h == &norm);
    }

    if !is_valid_dns_hostname(trimmed) {
        return false;
    }
    let norm = normalize_host_for_allowlist(trimmed);
    if norm == "localhost" {
        return true;
    }
    if policy.allowed_hosts.iter().any(|h| h == &norm) {
        return true;
    }
    if policy.local_mdns_hosts.iter().any(|h| h == &norm) {
        return true;
    }
    false
}

/// Strictly validates a full QR server URL (scheme, no userinfo, no control chars,
/// origin-only without path/query/fragment, and authorized destination host).
pub fn validate_and_canonicalize_qr_url(
    raw: &str,
    policy: &QrUrlPolicy,
) -> Result<String, &'static str> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || has_forbidden_url_chars(trimmed) {
        return Err("server_url contains forbidden characters or is empty");
    }
    let parsed = reqwest::Url::parse(trimmed).map_err(|_| "invalid server_url syntax")?;
    let scheme = parsed.scheme();
    if scheme != "http" && scheme != "https" {
        return Err("server_url scheme must be http or https");
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("server_url must not contain userinfo");
    }
    if parsed.query().is_some() || parsed.fragment().is_some() {
        return Err("server_url must be an origin without query or fragment");
    }
    if parsed.path() != "/" && !parsed.path().is_empty() {
        return Err("server_url must be an origin without a path");
    }
    let host_str = parsed.host_str().ok_or("server_url missing host")?;
    if !is_authorized_qr_host(host_str, policy) {
        return Err("server_url host is not authorized for QR pairing");
    }
    match parsed.port() {
        Some(port) => Ok(format!("{scheme}://{host_str}:{port}")),
        None => Ok(format!("{scheme}://{host_str}")),
    }
}

fn extract_single_host_header<'a>(
    headers: &'a axum::http::HeaderMap,
    name: &'static str,
) -> Result<Option<&'a str>, &'static str> {
    let mut values = headers.get_all(name).iter();
    let Some(first) = values.next() else {
        return Ok(None);
    };
    if values.next().is_some() {
        return Err("multiple host forwarding headers are not permitted");
    }
    let raw = first
        .to_str()
        .map_err(|_| "host header contains non-UTF-8 bytes")?;
    let trimmed = raw.trim();
    if trimmed.is_empty()
        || has_forbidden_url_chars(trimmed)
        || trimmed.contains(',')
        || trimmed.contains('/')
        || trimmed.contains('@')
        || trimmed.contains('?')
        || trimmed.contains('#')
        || trimmed.contains('\\')
    {
        return Err("invalid host header value");
    }
    Ok(Some(trimmed))
}

/// Resolves the canonical `server_url` for legacy SERVER_CODE QR generation using strict
/// 3-tier precedence and fail-closed header/origin validation:
/// 1. Explicit authorized `server_url` (rejected if invalid, unauthorized, or contradicting a configured public URL).
/// 2. Operator-configured public URL (`MICHI_PUBLIC_URL` / `MICHI_PUBLIC_HTTPS_URL`).
/// 3. Verified trusted-proxy host/proto (rejecting contradictory proxy headers or spoofed hosts)
///    or direct listener host/scheme (ignoring any forwarded headers from untrusted peers).
pub fn resolve_canonical_qr_server_url(
    config: &michi_config::Config,
    policy: &QrUrlPolicy,
    peer_ip: Option<std::net::IpAddr>,
    direct_tls: bool,
    headers: &axum::http::HeaderMap,
    explicit_url: Option<&str>,
) -> Result<String, &'static str> {
    let from_trusted_proxy = config.trust_proxy
        && peer_ip
            .map(|ip| config.is_trusted_proxy(&ip))
            .unwrap_or(false);

    let proxy_proto = if from_trusted_proxy {
        let evidence = crate::home_tls_policy::classify_proxy_proto_headers(headers);
        if evidence == crate::home_tls_policy::ProxyProtoEvidence::ContradictoryOrMalformed {
            return Err("contradictory or malformed reverse proxy forwarding headers");
        }
        evidence
    } else {
        crate::home_tls_policy::ProxyProtoEvidence::Absent
    };

    if let Some(raw_explicit) = explicit_url.map(str::trim).filter(|u| !u.is_empty()) {
        let canonical = validate_and_canonicalize_qr_url(raw_explicit, policy)?;
        if let Some(ref configured_pub) = policy.configured_public_url {
            let configured_canonical = validate_and_canonicalize_qr_url(configured_pub, policy)?;
            let exp_parsed = reqwest::Url::parse(&canonical).map_err(|_| "invalid explicit url")?;
            let exp_host = exp_parsed
                .host_str()
                .map(normalize_host_for_allowlist)
                .unwrap_or_default();
            if canonical != configured_canonical
                && !policy.allowed_hosts.iter().any(|h| h == &exp_host)
            {
                return Err(
                    "explicit server_url does not match configured public URL or allowed QR hosts",
                );
            }
        }
        return Ok(canonical);
    }

    if let Some(ref configured_pub) = policy.configured_public_url {
        return validate_and_canonicalize_qr_url(configured_pub, policy);
    }

    let scheme =
        if direct_tls || proxy_proto == crate::home_tls_policy::ProxyProtoEvidence::CoherentHttps {
            "https"
        } else {
            "http"
        };

    if from_trusted_proxy {
        let candidate_host = match extract_single_host_header(headers, "x-forwarded-host")? {
            Some(xfh) => Some(xfh),
            None => extract_single_host_header(headers, "host")?,
        };
        if let Some(host_val) = candidate_host {
            return validate_and_canonicalize_qr_url(&format!("{scheme}://{host_val}"), policy);
        }
    } else if let Some(host_val) = extract_single_host_header(headers, "host")? {
        return validate_and_canonicalize_qr_url(&format!("{scheme}://{host_val}"), policy);
    }

    Ok(format!("{scheme}://localhost:{}", policy.server_port))
}

pub async fn qr_generate_handler(
    connect_info: Option<axum::extract::ConnectInfo<std::net::SocketAddr>>,
    direct_tls: Option<axum::Extension<crate::AirCatTlsVerifiedConnection>>,
    headers: axum::http::HeaderMap,
    State(state): State<AppState>,
    body: Option<Json<QrGenerateBody>>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let qr_code = Uuid::new_v4();
    let explicit_url = body
        .and_then(|b| b.0.server_url)
        .filter(|u| !u.trim().is_empty());

    let peer_ip = connect_info.map(|axum::extract::ConnectInfo(addr)| addr.ip());
    let policy = QrUrlPolicy::from_env_and_state(&state).await;
    let server_url = resolve_canonical_qr_server_url(
        &state.config,
        &policy,
        peer_ip,
        direct_tls.is_some(),
        &headers,
        explicit_url.as_deref(),
    )
    .map_err(|reason| {
        v1_error_code(
            StatusCode::BAD_REQUEST,
            michi_link::MichiLinkErrorCode::InvalidRequest,
            reason,
        )
    })?;

    let expires_at = chrono::Utc::now() + chrono::Duration::minutes(5);
    sqlx::query(
        "INSERT INTO pairing_qr_codes (id, qr_code, server_url, expires_at, created_at)
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(qr_code.to_string())
    .bind(&server_url)
    .bind(expires_at.to_rfc3339())
    .bind(chrono::Utc::now().to_rfc3339())
    .execute(&state.db)
    .await
    .map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DB_ERROR",
            &e.to_string(),
        )
    })?;

    Ok(Json(serde_json::json!({
        "qr_code": qr_code,
        "qr_protocol": "legacy_server_code_v1",
        "expires_at": expires_at.to_rfc3339(),
        "server_url": server_url,
        "svg_url": format!("/api/v1/pair/qr/{}/svg", qr_code),
    })))
}

pub async fn qr_status_handler(
    State(state): State<AppState>,
    Path(qr_code): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let qr_str = qr_code.to_string();
    let row = sqlx::query_as::<_, (String, String, i64, Option<String>)>(
        "SELECT server_url, expires_at, claimed, claimed_at FROM pairing_qr_codes WHERE qr_code = ?"
    )
    .bind(&qr_str)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| v1_error(StatusCode::INTERNAL_SERVER_ERROR, "DB_ERROR", &e.to_string()))?;

    let (server_url, expires_at_str, claimed, claimed_at) =
        row.ok_or_else(|| v1_error(StatusCode::NOT_FOUND, "NOT_FOUND", "QR code not found"))?;

    if claimed != 0 {
        return Ok(Json(serde_json::json!({
            "status": "claimed",
            "claimed": true,
            "claimed_at": claimed_at,
            "qr_code": qr_str,
        })));
    }

    let expires_at = chrono::DateTime::parse_from_rfc3339(&expires_at_str).map_err(|_| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "PARSE_ERROR",
            "invalid expiry",
        )
    })?;

    if expires_at < chrono::Utc::now() {
        return Ok(Json(serde_json::json!({
            "status": "expired",
            "claimed": false,
            "qr_code": qr_str,
        })));
    }

    Ok(Json(serde_json::json!({
        "status": "waiting_for_scan",
        "claimed": false,
        "expires_at": expires_at_str,
        "server_url": server_url,
        "qr_code": qr_str,
    })))
}

pub async fn qr_svg_handler(
    State(state): State<AppState>,
    Path(qr_code): Path<Uuid>,
) -> Result<axum::response::Response, (StatusCode, Json<serde_json::Value>)> {
    let qr_str = qr_code.to_string();

    let row = sqlx::query_as::<_, (String, String, i64, Option<String>)>(
        "SELECT server_url, expires_at, claimed, claimed_at FROM pairing_qr_codes WHERE qr_code = ?"
    )
        .bind(&qr_str)
        .fetch_optional(&state.db)
        .await
        .map_err(|e| v1_error(StatusCode::INTERNAL_SERVER_ERROR, "DB_ERROR", &e.to_string()))?;

    let (server_url, expires_at_str, claimed, _claimed_at) =
        row.ok_or_else(|| v1_error(StatusCode::NOT_FOUND, "NOT_FOUND", "QR code not found"))?;

    if claimed != 0 {
        return Err(v1_error(
            StatusCode::GONE,
            "ALREADY_USED",
            "QR code has already been used",
        ));
    }

    let expires_at = chrono::DateTime::parse_from_rfc3339(&expires_at_str).map_err(|_| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "PARSE_ERROR",
            "invalid expiry",
        )
    })?;
    if expires_at < chrono::Utc::now() {
        return Err(v1_error(StatusCode::GONE, "EXPIRED", "QR code has expired"));
    }

    // Build QR content
    let payload = serde_json::json!({
        "michi": "v1",
        "url": server_url,
        "code": qr_str,
    });
    let payload_str = serde_json::to_string(&payload).map_err(|_| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "JSON_ERROR",
            "serialization failed",
        )
    })?;

    // Generate QR code
    let code = qrcode::QrCode::new(payload_str.as_bytes()).map_err(|_| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "QR_ERROR",
            "QR generation failed",
        )
    })?;

    // Build QR SVG with premium styling
    let modules = code.to_colors();
    let size = code.width();
    let cell_size = 5.0;
    let qr_dim = size as f64 * cell_size;
    let padding = 40.0;
    let canvas = qr_dim + padding * 2.0;
    let corner_radius = 6.0;
    let center = padding + qr_dim / 2.0;
    let logo_size = qr_dim * 0.28;

    let mut svg = String::with_capacity(16000);
    svg.push_str(&format!(
        r#"<?xml version="1.0" standalone="yes"?><svg xmlns="http://www.w3.org/2000/svg" version="1.1" width="{c}" height="{c}" viewBox="0 0 {c} {c}">"#,
        c = canvas as u32
    ));

    // Background with rounded rect
    svg.push_str(&format!(
        r##"<rect x="0" y="0" width="{c}" height="{c}" rx="24" fill="#0D1120"/>"##,
        c = canvas as u32
    ));

    // White QR background with rounded rect
    svg.push_str(&format!(
        r##"<rect x="{p}" y="{p}" width="{d}" height="{d}" rx="{cr}" fill="#ffffff"/>"##,
        p = padding,
        d = qr_dim,
        cr = 12.0
    ));

    // Draw QR modules as circles with gradient
    svg.push_str(r#"<defs><linearGradient id="qrGrad" x1="0%" y1="0%" x2="100%" y2="100%">"#);
    svg.push_str(r##"<stop offset="0%" stop-color="#8B5CF6"/>"##);
    svg.push_str(r##"<stop offset="100%" stop-color="#6D4AFF"/>"##);
    svg.push_str("</linearGradient></defs>");

    let radius = (cell_size - 0.4) / 2.0;
    for y in 0..size {
        for x in 0..size {
            if modules[y * size + x] == qrcode::types::Color::Dark {
                let cx = padding + x as f64 * cell_size + cell_size / 2.0;
                let cy = padding + y as f64 * cell_size + cell_size / 2.0;
                svg.push_str(&format!(
                    r##"<circle cx="{cx}" cy="{cy}" r="{radius}" fill="url(#qrGrad)"/>"##
                ));
            }
        }
    }

    // Corners: Finder patterns with rounded squares and inner circles
    let finder_positions = [(0, 0), (size - 7, 0), (0, size - 7)];
    for &(fx, fy) in &finder_positions {
        let fx = padding + fx as f64 * cell_size;
        let fy = padding + fy as f64 * cell_size;
        let f_size = 7.0 * cell_size;
        // Outer
        svg.push_str(&format!(
            r##"<rect x="{fx}" y="{fy}" width="{f_size}" height="{f_size}" rx="{corner_radius}" fill="url(#qrGrad)"/>"##
        ));
        // Inner
        svg.push_str(&format!(
            r##"<rect x="{x}" y="{y}" width="{s}" height="{s}" rx="{cr}" fill="#ffffff"/>"##,
            x = fx + cell_size,
            y = fy + cell_size,
            s = 5.0 * cell_size,
            cr = corner_radius - 1.0
        ));
        // Core
        svg.push_str(&format!(
            r##"<rect x="{x}" y="{y}" width="{s}" height="{s}" rx="3" fill="url(#qrGrad)"/>"##,
            x = fx + 2.0 * cell_size,
            y = fy + 2.0 * cell_size,
            s = 3.0 * cell_size
        ));
    }

    // Logo circle background (white circle with subtle border)
    let white = "#ffffff";
    let dark = "#0D1120";
    svg.push_str(&format!(
        r##"<circle cx="{cx}" cy="{cy}" r="{r}" fill="{w}" stroke="url(#qrGrad)" stroke-width="3"/>"##,
        cx = center, cy = center, r = logo_size / 2.0 + 6.0, w = white
    ));

    // Logo: Michi cat silhouette
    let logo_scale = logo_size / 100.0;
    svg.push_str(&format!(
        r##"<g transform="translate({cx}, {cy}) scale({s})">
        <polygon points="-30,-35 -15,-55 0,-35" fill="url(#qrGrad)"/>
        <polygon points="30,-35 15,-55 0,-35" fill="url(#qrGrad)"/>
        <circle cx="0" cy="-10" r="25" fill="url(#qrGrad)"/>
        <ellipse cx="-10" cy="-14" rx="5" ry="6" fill="{w}"/>
        <ellipse cx="10" cy="-14" rx="5" ry="6" fill="{w}"/>
        <ellipse cx="-10" cy="-14" rx="2.5" ry="4" fill="{d}"/>
        <ellipse cx="10" cy="-14" rx="2.5" ry="4" fill="{d}"/>
        <polygon points="0,-7 -3,-3 3,-3" fill="{w}" opacity="0.8"/>
        <path d="M-6,-1 Q0,3 6,-1" fill="none" stroke="{w}" stroke-width="1.2" opacity="0.7"/>
        <path d="M-20,5 Q-25,30 -15,45 L15,45 Q25,30 20,5" fill="url(#qrGrad)"/>
        <path d="M-18,30 Q-40,20 -35,0 Q-32,-6 -28,-2" fill="none" stroke="url(#qrGrad)" stroke-width="5" stroke-linecap="round"/>
      </g>"##,
        cx = 0, cy = 0, s = logo_scale, w = white, d = dark
    ));

    svg.push_str("</svg>");

    Ok(([(header::CONTENT_TYPE, "image/svg+xml")], svg).into_response())
}

#[derive(Deserialize)]
pub struct QrClaimBody {
    pub device_name: Option<String>,
    pub device_type: Option<String>,
}

pub async fn qr_claim_handler(
    State(state): State<AppState>,
    Path(qr_code): Path<Uuid>,
    Json(body): Json<QrClaimBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let qr_str = qr_code.to_string();

    let mut tx = state.db.begin().await.map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DB_ERROR",
            &e.to_string(),
        )
    })?;

    let row = sqlx::query_as::<_, (String, String, i64)>(
        "SELECT id, expires_at, claimed FROM pairing_qr_codes WHERE qr_code = ?",
    )
    .bind(&qr_str)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DB_ERROR",
            &e.to_string(),
        )
    })?;

    let (db_id, expires_at_str, claimed) =
        row.ok_or_else(|| v1_error(StatusCode::NOT_FOUND, "NOT_FOUND", "QR code not found"))?;

    if claimed != 0 {
        return Err(v1_error(
            StatusCode::GONE,
            "ALREADY_USED",
            "QR code has already been used",
        ));
    }

    let expires_at = chrono::DateTime::parse_from_rfc3339(&expires_at_str).map_err(|_| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "PARSE_ERROR",
            "invalid expiry",
        )
    })?;
    if expires_at < chrono::Utc::now() {
        return Err(v1_error(StatusCode::GONE, "EXPIRED", "QR code has expired"));
    }

    // Mark claimed atomically in transaction (guarantees no concurrent double claim)
    let update_res = sqlx::query(
        "UPDATE pairing_qr_codes SET claimed = 1, claimed_at = ? WHERE id = ? AND claimed = 0",
    )
    .bind(chrono::Utc::now().to_rfc3339())
    .bind(&db_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DB_ERROR",
            &e.to_string(),
        )
    })?;

    if update_res.rows_affected() == 0 {
        return Err(v1_error(
            StatusCode::CONFLICT,
            "CONCURRENT_CLAIM",
            "QR code was claimed concurrently",
        ));
    }

    // Generate device token for the claimer
    let device_name = body
        .device_name
        .unwrap_or_else(|| "QR-Paired Device".into());
    let device_id = Uuid::new_v4();
    let token = generate_device_token();

    // Register device in canonical link_devices registry inside the same transaction
    let core_device = michi_core::LinkDevice {
        device_id,
        alias: device_name.clone(),
        device_type: body.device_type.unwrap_or_else(|| "mobile".into()),
        device_model: None,
        token_hash: hash_token(&token),
        permissions_json: r#"{"playback":true,"queue":true,"library_read":true,"settings":false}"#
            .into(),
        created_at: chrono::Utc::now(),
        last_seen: Some(chrono::Utc::now().to_rfc3339()),
        revoked: false,
    };

    let dev_id_str = core_device.device_id.to_string();
    let created_str = core_device.created_at.to_rfc3339();
    sqlx::query(
        "INSERT INTO link_devices (device_id, alias, device_type, device_model, token_hash, permissions, created_at, last_seen, revoked)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&dev_id_str)
    .bind(&core_device.alias)
    .bind(&core_device.device_type)
    .bind(&core_device.device_model)
    .bind(&core_device.token_hash)
    .bind(&core_device.permissions_json)
    .bind(&created_str)
    .bind(&core_device.last_seen)
    .bind(core_device.revoked as i64)
    .execute(&mut *tx)
    .await
    .map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DEVICE_PERSISTENCE_ERROR",
            &e.to_string(),
        )
    })?;

    tx.commit().await.map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "COMMIT_ERROR",
            &e.to_string(),
        )
    })?;

    state
        .token_store
        .store(&token, TokenType::Device, device_id)
        .await;

    // Broadcast device paired event
    let _ = state.tx.send(
        serde_json::json!({
            "type": "device_paired",
            "device_id": device_id,
            "alias": device_name,
        })
        .to_string(),
    );

    Ok(Json(serde_json::json!({
        "status": "claimed",
        "device_token": token,
        "device_id": device_id,
        "server_id": state.server_id(),
        "pairing_code": qr_str,
    })))
}
