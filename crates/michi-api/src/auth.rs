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

const SESSION_DURATION: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);

/// Extract a Bearer token from the Authorization header
#[allow(dead_code)]
pub fn extract_bearer_token(request: &Request) -> Option<String> {
    let auth_header = request.headers().get("Authorization")?.to_str().ok()?;
    auth_header.strip_prefix("Bearer ").map(|s| s.to_string())
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
    pub sessions: Arc<RwLock<std::collections::HashMap<String, SessionData>>>,
}

impl AuthState {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(std::collections::HashMap::new())),
        }
    }

    pub async fn create_session(&self, user_id: Uuid) -> String {
        let token = uuid::Uuid::new_v4().to_string();
        let mut sessions = self.sessions.write().await;
        sessions.insert(
            token.clone(),
            SessionData {
                expiry: std::time::Instant::now() + SESSION_DURATION,
                user_id,
            },
        );
        token
    }

    pub async fn validate(&self, token: &str) -> bool {
        let sessions = self.sessions.read().await;
        matches!(sessions.get(token), Some(data) if data.expiry > std::time::Instant::now())
    }

    pub async fn extract_user_id(&self, token: &str) -> Option<Uuid> {
        let sessions = self.sessions.read().await;
        sessions.get(token).and_then(|data| {
            if data.expiry > std::time::Instant::now() {
                Some(data.user_id)
            } else {
                None
            }
        })
    }

    pub async fn invalidate(&self, token: &str) {
        let mut sessions = self.sessions.write().await;
        sessions.remove(token);
    }

    pub async fn cleanup(&self) {
        let mut sessions = self.sessions.write().await;
        sessions.retain(|_, data| data.expiry > std::time::Instant::now());
    }
}

fn extract_token(request: &Request) -> Option<String> {
    let auth_header = request.headers().get("Authorization")?.to_str().ok()?;
    auth_header.strip_prefix("Bearer ").map(|s| s.to_string())
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
        (status = 401, description = "Invalid credentials")
    )
)]
pub(crate) async fn login_handler(
    State(state): State<AppState>,
    Json(body): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, (StatusCode, Json<serde_json::Value>)> {
    if !state.auth_enabled {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"status": "error", "message": "auth not configured"})),
        ));
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

    let token = state.auth_sessions.create_session(id).await;
    Ok(Json(LoginResponse {
        token,
        id,
        username,
        is_admin,
    }))
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
    State(state): State<AppState>,
    Json(body): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, (StatusCode, Json<serde_json::Value>)> {
    if !state.auth_enabled {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"status": "error", "message": "auth not configured"})),
        ));
    }

    if !state.config.allow_registration {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({"status": "error", "message": "registration not allowed"})),
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

    let user_id = Uuid::new_v4();
    let password_hash = hash_password(&body.password).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"status": "error", "message": e.to_string()})),
        )
    })?;

    michi_db::create_user(&state.db, &user_id, &body.username, &password_hash, false)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"status": "error", "message": e.to_string()})),
            )
        })?;

    let token = state.auth_sessions.create_session(user_id).await;
    Ok(Json(LoginResponse {
        token,
        id: user_id,
        username: body.username,
        is_admin: false,
    }))
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
    State(state): State<AppState>,
    request: Request,
) -> impl IntoResponse {
    if let Some(token) = extract_token(&request) {
        state.auth_sessions.invalidate(&token).await;
    }
    StatusCode::OK
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
                    (true, None, None, None)
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
