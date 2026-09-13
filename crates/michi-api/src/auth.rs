use std::sync::Arc;

use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Json, Response},
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::RwLock;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::AppState;
use base64::Engine;

const SESSION_DURATION: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);

/// Extract a Bearer token from the Authorization header or michi_web_session cookie
#[allow(dead_code)]
pub fn extract_bearer_token(request: &Request) -> Option<String> {
    extract_token(request)
}

/// Helper to determine if the request is over HTTPS
fn is_secure_request(
    connect_info: Option<axum::extract::ConnectInfo<std::net::SocketAddr>>,
    headers: &axum::http::HeaderMap,
    config: &michi_config::Config,
) -> bool {
    let peer_addr = connect_info.map(|axum::extract::ConnectInfo(addr)| addr);
    let peer_ip = peer_addr.map(|a| a.ip());
    if let Some(ref ip) = peer_ip {
        if config.is_trusted_proxy(ip) {
            if let Some(proto) = headers
                .get("X-Forwarded-Proto")
                .and_then(|v| v.to_str().ok())
            {
                return proto.eq_ignore_ascii_case("https");
            }
        }
    }
    false
}

/// Helper to build Set-Cookie header for michi_web_session
fn make_session_cookie(token: &str, max_age_secs: u64, secure: bool) -> String {
    if max_age_secs == 0 {
        format!(
            "michi_web_session=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0; Expires=Thu, 01 Jan 1970 00:00:00 GMT{}",
            if secure { "; Secure" } else { "" }
        )
    } else {
        format!(
            "michi_web_session={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age={max_age_secs}{}",
            if secure { "; Secure" } else { "" }
        )
    }
}

fn extract_token(request: &Request) -> Option<String> {
    if let Some(auth_header) = request
        .headers()
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
    {
        if let Some(token) = auth_header.strip_prefix("Bearer ") {
            let t = token.trim();
            if !t.is_empty() {
                return Some(t.to_string());
            }
        }
    }
    if let Some(cookie_header) = request
        .headers()
        .get(axum::http::header::COOKIE)
        .and_then(|h| h.to_str().ok())
    {
        for cookie in cookie_header.split(';') {
            let cookie = cookie.trim();
            if let Some(val) = cookie.strip_prefix("michi_web_session=") {
                let t = val.trim();
                if !t.is_empty() {
                    return Some(t.to_string());
                }
            }
        }
    }
    None
}

/// Resolve the device_id from a request using either link tokens or auth sessions
#[allow(dead_code)]
pub async fn resolve_device_id(state: &AppState, request: &Request) -> Option<Uuid> {
    let token = extract_bearer_token(request)?;
    // Try link token (v1 pairing)
    if let Ok(device_id) = state
        .token_store
        .validate(&token, michi_link::TokenType::Device)
        .await
    {
        return Some(device_id);
    }
    // Try auth session login
    if state.auth_enabled {
        state.auth_sessions.extract_user_id(&token).await
    } else {
        None
    }
}

#[derive(Debug, Clone)]
pub struct SessionData {
    pub expiry: std::time::Instant,
    pub user_id: Uuid,
}

#[derive(Debug, Clone)]
pub struct AuthState {
    pub db: Option<sqlx::SqlitePool>,
    pub sessions: Arc<RwLock<std::collections::HashMap<String, SessionData>>>,
}

pub fn hash_token(token: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    format!("{:x}", hasher.finalize())
}

impl Default for AuthState {
    fn default() -> Self {
        Self::new()
    }
}

impl AuthState {
    pub fn new() -> Self {
        Self {
            db: None,
            sessions: Arc::new(RwLock::new(std::collections::HashMap::new())),
        }
    }

    pub fn new_with_db(db: sqlx::SqlitePool) -> Self {
        Self {
            db: Some(db),
            sessions: Arc::new(RwLock::new(std::collections::HashMap::new())),
        }
    }

    pub async fn create_session(&self, user_id: Uuid) -> Result<String, String> {
        let mut random_bytes = [0u8; 32];
        rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut random_bytes);
        let token = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(random_bytes);
        let token_hash = hash_token(&token);
        let now = chrono::Utc::now();
        let expires_at_dt = now
            + chrono::Duration::from_std(SESSION_DURATION)
                .unwrap_or_else(|_| chrono::Duration::days(1));
        let created_at = now.to_rfc3339();
        let expires_at = expires_at_dt.to_rfc3339();

        if let Some(ref db) = self.db {
            michi_db::create_auth_session(db, &token_hash, &user_id, &created_at, &expires_at)
                .await
                .map_err(|e| format!("failed to persist session to database: {e}"))?;
        }

        let mut sessions = self.sessions.write().await;
        sessions.insert(
            token.clone(),
            SessionData {
                expiry: std::time::Instant::now() + SESSION_DURATION,
                user_id,
            },
        );
        Ok(token)
    }

    pub async fn validate(&self, token: &str) -> bool {
        self.extract_user_id(token).await.is_some()
    }

    pub async fn extract_user_id(&self, token: &str) -> Option<Uuid> {
        // 1. Check in-memory session cache first
        let cached_user_id = {
            let sessions = self.sessions.read().await;
            if let Some(data) = sessions.get(token) {
                if data.expiry > std::time::Instant::now() {
                    Some(data.user_id)
                } else {
                    None
                }
            } else {
                None
            }
        };

        if let Some(user_id) = cached_user_id {
            // Verify user still exists in database if DB is configured
            if let Some(ref db) = self.db {
                if let Ok(Some(_)) = michi_db::get_user_by_id(db, &user_id).await {
                    return Some(user_id);
                } else {
                    // Orphaned session whose user no longer exists - invalidate immediately
                    let token_clone = token.to_string();
                    let auth_clone = self.clone();
                    tokio::spawn(async move {
                        auth_clone.invalidate(&token_clone).await;
                    });
                    return None;
                }
            } else {
                return Some(user_id);
            }
        }

        // 2. Check SQLite persistent store if configured
        if let Some(ref db) = self.db {
            let token_hash = hash_token(token);
            if let Ok(Some((user_id, expires_at_str))) =
                michi_db::get_auth_session(db, &token_hash).await
            {
                if let Ok(exp) = chrono::DateTime::parse_from_rfc3339(&expires_at_str) {
                    let now = chrono::Utc::now();
                    if exp > now {
                        // Check if user still exists in database
                        if let Ok(Some(_)) = michi_db::get_user_by_id(db, &user_id).await {
                            let remaining = exp
                                .signed_duration_since(now)
                                .to_std()
                                .unwrap_or(std::time::Duration::from_secs(60));
                            let now_str = now.to_rfc3339();
                            let db_clone = db.clone();
                            let th = token_hash.clone();
                            tokio::spawn(async move {
                                let _ =
                                    michi_db::touch_auth_session(&db_clone, &th, &now_str).await;
                            });

                            let mut sessions = self.sessions.write().await;
                            sessions.insert(
                                token.to_string(),
                                SessionData {
                                    expiry: std::time::Instant::now() + remaining,
                                    user_id,
                                },
                            );
                            return Some(user_id);
                        } else {
                            // User deleted, remove orphaned session record
                            let db_clone = db.clone();
                            let th = token_hash.clone();
                            tokio::spawn(async move {
                                let _ = michi_db::delete_auth_session(&db_clone, &th).await;
                            });
                            return None;
                        }
                    }
                }
            }
        }

        None
    }

    pub async fn invalidate(&self, token: &str) {
        {
            let mut sessions = self.sessions.write().await;
            sessions.remove(token);
        }
        if let Some(ref db) = self.db {
            let token_hash = hash_token(token);
            let _ = michi_db::delete_auth_session(db, &token_hash).await;
        }
    }

    pub async fn invalidate_user_sessions(&self, user_id: &Uuid) {
        {
            let mut sessions = self.sessions.write().await;
            sessions.retain(|_, data| data.user_id != *user_id);
        }
        if let Some(ref db) = self.db {
            let _ = michi_db::delete_auth_sessions_for_user(db, user_id).await;
        }
    }

    pub async fn cleanup(&self) {
        {
            let mut sessions = self.sessions.write().await;
            sessions.retain(|_, data| data.expiry > std::time::Instant::now());
        }
        if let Some(ref db) = self.db {
            let now = chrono::Utc::now().to_rfc3339();
            let _ = michi_db::cleanup_expired_auth_sessions(db, &now).await;
        }
    }
}

fn auth_error(status: StatusCode, message: &str) -> Response {
    (
        status,
        Json(json!({
            "status": "error",
            "message": message
        })),
    )
        .into_response()
}

fn is_admin_route(path: &str) -> bool {
    if path.starts_with("/api/v1/pair/qr/") && (path.ends_with("/claim") || path.ends_with("/svg"))
    {
        return false;
    }
    [
        "/api/v1/audit",
        "/api/v1/backup",
        "/api/v1/config",
        "/api/v1/devices",
        "/api/v1/diagnostics",
        "/api/v1/health/mounts",
        "/api/v1/health/storage",
        "/api/v1/health/verify",
        "/api/v1/import",
        "/api/v1/jobs",
        "/api/v1/library/scan",
        "/api/v1/link/devices",
        "/api/v1/modules",
        "/api/v1/pair/start",
        "/api/v1/pair/qr",
        "/api/v1/receivers",
        "/api/v1/rooms",
        "/api/v1/settings",
        "/api/v1/setup",
        "/api/v1/sources",
        "/api/v1/webhook",
        "/api/v1/transcode",
        "/api/v1/update",
    ]
    .iter()
    .any(|prefix| path.starts_with(prefix))
}

/// V1 authorization policy:
/// - admin paths require an active administrator session;
/// - other protected paths accept an active user session or paired device token;
/// - this never fails open when username/password authentication is disabled.
pub async fn v1_auth_middleware(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let token = match extract_token(&request) {
        Some(token) => token,
        None => return auth_error(StatusCode::UNAUTHORIZED, "missing authorization header"),
    };

    if is_admin_route(request.uri().path()) {
        let Some(user_id) = state.auth_sessions.extract_user_id(&token).await else {
            return auth_error(StatusCode::UNAUTHORIZED, "administrator session required");
        };
        let is_admin = michi_db::get_user_by_id(&state.db, &user_id)
            .await
            .ok()
            .flatten()
            .map(|(_, _, _, is_admin)| is_admin)
            .unwrap_or(false);
        return if is_admin {
            next.run(request).await
        } else {
            auth_error(StatusCode::FORBIDDEN, "administrator privileges required")
        };
    }

    if state.auth_sessions.validate(&token).await
        || state
            .token_store
            .validate(&token, michi_link::TokenType::Device)
            .await
            .is_ok()
    {
        next.run(request).await
    } else {
        auth_error(StatusCode::UNAUTHORIZED, "invalid or expired token")
    }
}

pub async fn auth_middleware(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    if !state.auth_enabled {
        return next.run(request).await;
    }

    let token = match extract_token(&request) {
        Some(t) => t,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({
                    "status": "error",
                    "message": "missing authorization header"
                })),
            )
                .into_response();
        }
    };

    if state.auth_sessions.validate(&token).await {
        next.run(request).await
    } else {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "status": "error",
                "message": "invalid or expired token"
            })),
        )
            .into_response()
    }
}

#[derive(Debug, Deserialize, ToSchema)]
pub(crate) struct LoginRequest {
    username: String,
    password: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct LoginResponse {
    token: String,
    id: Uuid,
    username: String,
    is_admin: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct AuthStatusResponse {
    enabled: bool,
    authenticated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    is_admin: Option<bool>,
    registration_allowed: bool,
}

pub(crate) fn hash_password(password: &str) -> Result<String, argon2::password_hash::Error> {
    let salt = SaltString::generate(&mut OsRng);
    Ok(Argon2::default()
        .hash_password(password.as_bytes(), &salt)?
        .to_string())
}

pub(crate) fn verify_password(
    password: &str,
    hash: &str,
) -> Result<bool, argon2::password_hash::Error> {
    let parsed = PasswordHash::new(hash)?;
    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok())
}

#[utoipa::path(
    post,
    path = "/api/auth/login",
    tag = "Auth",
    request_body = LoginRequest,
    responses(
        (status = 200, description = "Login successful", body = LoginResponse),
        (status = 400, description = "Auth not configured"),
        (status = 401, description = "Invalid credentials"),
        (status = 429, description = "Too many requests")
    )
)]
pub(crate) async fn login_handler(
    connect_info: Option<axum::extract::ConnectInfo<std::net::SocketAddr>>,
    headers: axum::http::HeaderMap,
    State(state): State<AppState>,
    Json(body): Json<LoginRequest>,
) -> Result<(axum::http::HeaderMap, Json<LoginResponse>), (StatusCode, Json<serde_json::Value>)> {
    if !state.auth_enabled {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"status": "error", "message": "auth not configured"})),
        ));
    }

    let client_ip = crate::extract_client_ip(connect_info, &headers, &state.config);
    {
        let now = std::time::Instant::now();
        let mut entry = state
            .security_state
            .pairing_attempts
            .entry(format!("login_{client_ip}"))
            .or_insert((0u32, now));
        let (count, last_reset) = entry.value();
        let elapsed = now.duration_since(*last_reset);

        let max_attempts = state
            .security_state
            .config
            .login_rate_limit_per_minute
            .max(1);
        if elapsed.as_secs() > 60 {
            *entry = (1, now);
        } else if *count >= max_attempts {
            tracing::warn!(
                "Login rate limit exceeded for IP: {} (limit={}/min)",
                client_ip,
                max_attempts
            );
            return Err((
                StatusCode::TOO_MANY_REQUESTS,
                Json(json!({
                    "status": "error",
                    "message": "too many login attempts, please wait 60 seconds"
                })),
            ));
        } else {
            *entry = (count + 1, *last_reset);
        }
    }

    let user = michi_db::get_user_by_username(&state.db, &body.username)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"status": "error", "message": e.to_string()})),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                Json(json!({"status": "error", "message": "invalid credentials"})),
            )
        })?;

    let (id, username, password_hash, is_admin) = user;

    let valid = verify_password(&body.password, &password_hash).unwrap_or(false);
    if !valid {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({"status": "error", "message": "invalid credentials"})),
        ));
    }

    let token = state.auth_sessions.create_session(id).await.map_err(|e| {
        tracing::error!("failed to create persistent session: {e}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"status": "error", "message": "failed to initialize session"})),
        )
    })?;
    let secure = is_secure_request(connect_info, &headers, &state.config);
    let cookie_val = make_session_cookie(&token, 86400, secure);

    let mut resp_headers = axum::http::HeaderMap::new();
    if let Ok(v) = axum::http::HeaderValue::from_str(&cookie_val) {
        resp_headers.insert(axum::http::header::SET_COOKIE, v);
    }
    resp_headers.insert(
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-store"),
    );

    Ok((
        resp_headers,
        Json(LoginResponse {
            token,
            id,
            username,
            is_admin,
        }),
    ))
}

#[utoipa::path(
    post,
    path = "/api/auth/register",
    tag = "Auth",
    request_body = LoginRequest,
    responses(
        (status = 200, description = "Registration successful", body = LoginResponse),
        (status = 400, description = "Auth not configured"),
        (status = 403, description = "Registration not allowed"),
        (status = 409, description = "Username already exists")
    )
)]
pub(crate) async fn register_handler(
    connect_info: Option<axum::extract::ConnectInfo<std::net::SocketAddr>>,
    headers: axum::http::HeaderMap,
    State(state): State<AppState>,
    Json(body): Json<LoginRequest>,
) -> Result<(axum::http::HeaderMap, Json<LoginResponse>), (StatusCode, Json<serde_json::Value>)> {
    if !state.auth_enabled {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"status": "error", "message": "auth not configured"})),
        ));
    }

    let client_ip = crate::extract_client_ip(connect_info, &headers, &state.config);
    {
        let now = std::time::Instant::now();
        let mut entry = state
            .security_state
            .pairing_attempts
            .entry(format!("register_{client_ip}"))
            .or_insert((0u32, now));
        let (count, last_reset) = entry.value();
        let elapsed = now.duration_since(*last_reset);

        if elapsed.as_secs() > 60 {
            *entry = (1, now);
        } else if *count >= 5 {
            tracing::warn!("Register rate limit exceeded for IP: {}", client_ip);
            return Err((
                StatusCode::TOO_MANY_REQUESTS,
                Json(json!({
                    "status": "error",
                    "message": "too many registration attempts, please wait 60 seconds"
                })),
            ));
        } else {
            *entry = (count + 1, *last_reset);
        }
    }

    if !state.config.allow_registration {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({"status": "error", "message": "registration not allowed"})),
        ));
    }

    if body.password.trim().len() < 8 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(
                json!({"status": "error", "message": "password must be at least 8 non-whitespace characters"}),
            ),
        ));
    }

    if michi_db::get_user_by_username(&state.db, &body.username)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"status": "error", "message": e.to_string()})),
            )
        })?
        .is_some()
    {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({"status": "error", "message": "username already exists"})),
        ));
    }

    let password_hash = hash_password(&body.password).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"status": "error", "message": e.to_string()})),
        )
    })?;

    let user_id = Uuid::new_v4();
    michi_db::create_user(&state.db, &user_id, &body.username, &password_hash, false)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"status": "error", "message": e.to_string()})),
            )
        })?;

    let token = state
        .auth_sessions
        .create_session(user_id)
        .await
        .map_err(|e| {
            tracing::error!("failed to create persistent session on register: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"status": "error", "message": "failed to initialize session"})),
            )
        })?;
    let secure = is_secure_request(connect_info, &headers, &state.config);
    let cookie_val = make_session_cookie(&token, 86400, secure);

    let mut resp_headers = axum::http::HeaderMap::new();
    if let Ok(v) = axum::http::HeaderValue::from_str(&cookie_val) {
        resp_headers.insert(axum::http::header::SET_COOKIE, v);
    }
    resp_headers.insert(
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-store"),
    );

    Ok((
        resp_headers,
        Json(LoginResponse {
            token,
            id: user_id,
            username: body.username,
            is_admin: false,
        }),
    ))
}

#[utoipa::path(
    post,
    path = "/api/auth/logout",
    tag = "Auth",
    responses(
        (status = 200, description = "Logged out successfully")
    )
)]
pub(crate) async fn logout_handler(
    connect_info: Option<axum::extract::ConnectInfo<std::net::SocketAddr>>,
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    request: Request,
) -> impl IntoResponse {
    if let Some(token) = extract_token(&request) {
        state.auth_sessions.invalidate(&token).await;
    }
    let secure = is_secure_request(connect_info, &headers, &state.config);
    let cookie_val = make_session_cookie("", 0, secure);
    let mut resp_headers = axum::http::HeaderMap::new();
    if let Ok(v) = axum::http::HeaderValue::from_str(&cookie_val) {
        resp_headers.insert(axum::http::header::SET_COOKIE, v);
    }
    resp_headers.insert(
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-store"),
    );
    (StatusCode::OK, resp_headers, Json(json!({"status": "ok"})))
}

#[utoipa::path(
    get,
    path = "/api/auth/check",
    tag = "Auth",
    responses(
        (status = 200, description = "Auth status", body = AuthStatusResponse)
    )
)]
pub(crate) async fn check_handler(
    State(state): State<AppState>,
    request: Request,
) -> Json<AuthStatusResponse> {
    if !state.auth_enabled {
        return Json(AuthStatusResponse {
            enabled: false,
            authenticated: true,
            id: None,
            username: None,
            is_admin: None,
            registration_allowed: state.config.allow_registration,
        });
    }

    let (authenticated, id, username, is_admin) = match extract_token(&request) {
        Some(t) => {
            if let Some(uid) = state.auth_sessions.extract_user_id(&t).await {
                if let Ok(Some((id, uname, _, admin))) =
                    michi_db::get_user_by_id(&state.db, &uid).await
                {
                    (true, Some(id), Some(uname), Some(admin))
                } else {
                    state.auth_sessions.invalidate(&t).await;
                    (false, None, None, None)
                }
            } else {
                (false, None, None, None)
            }
        }
        None => (false, None, None, None),
    };

    Json(AuthStatusResponse {
        enabled: true,
        authenticated,
        id,
        username,
        is_admin,
        registration_allowed: state.config.allow_registration,
    })
}

pub fn auth_router() -> Router<AppState> {
    Router::new()
        .route("/api/auth/login", post(login_handler))
        .route("/api/auth/register", post(register_handler))
        .route("/api/auth/logout", post(logout_handler))
        .route("/api/auth/check", get(check_handler))
}

pub fn spawn_session_cleanup(state: AuthState) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(3600));
        loop {
            interval.tick().await;
            state.cleanup().await;
        }
    });
}
