use std::sync::Arc;

use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Json, Response},
    routing::post,
    Router,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::RwLock;

use crate::AppState;

const SESSION_DURATION: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);

#[derive(Debug, Deserialize)]
struct LoginRequest {
    username: String,
    password: String,
}

#[derive(Debug, Serialize)]
struct LoginResponse {
    token: String,
}

#[derive(Debug, Serialize)]
struct AuthStatusResponse {
    enabled: bool,
    authenticated: bool,
}

#[derive(Debug, Clone)]
pub struct AuthState {
    pub sessions: Arc<RwLock<std::collections::HashMap<String, std::time::Instant>>>,
}

impl AuthState {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(std::collections::HashMap::new())),
        }
    }

    pub async fn create_session(&self) -> String {
        let token = uuid::Uuid::new_v4().to_string();
        let mut sessions = self.sessions.write().await;
        sessions.insert(token.clone(), std::time::Instant::now() + SESSION_DURATION);
        token
    }

    pub async fn validate(&self, token: &str) -> bool {
        let sessions = self.sessions.read().await;
        matches!(sessions.get(token), Some(expiry) if *expiry > std::time::Instant::now())
    }

    pub async fn invalidate(&self, token: &str) {
        let mut sessions = self.sessions.write().await;
        sessions.remove(token);
    }

    pub async fn cleanup(&self) {
        let mut sessions = self.sessions.write().await;
        sessions.retain(|_, expiry| *expiry > std::time::Instant::now());
    }
}

fn extract_token(request: &Request) -> Option<String> {
    let auth_header = request.headers().get("Authorization")?.to_str().ok()?;
    auth_header.strip_prefix("Bearer ").map(|s| s.to_string())
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

async fn login_handler(
    State(state): State<AppState>,
    Json(body): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, (StatusCode, Json<serde_json::Value>)> {
    if !state.auth_enabled {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"status": "error", "message": "auth not configured"})),
        ));
    }

    let valid = state.config.auth_username.as_deref() == Some(&body.username)
        && state.config.auth_password.as_deref() == Some(&body.password);

    if !valid {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({"status": "error", "message": "invalid credentials"})),
        ));
    }

    let token = state.auth_sessions.create_session().await;
    Ok(Json(LoginResponse { token }))
}

async fn logout_handler(State(state): State<AppState>, request: Request) -> impl IntoResponse {
    if let Some(token) = extract_token(&request) {
        state.auth_sessions.invalidate(&token).await;
    }
    StatusCode::OK
}

async fn check_handler(
    State(state): State<AppState>,
    request: Request,
) -> Json<AuthStatusResponse> {
    if !state.auth_enabled {
        return Json(AuthStatusResponse {
            enabled: false,
            authenticated: true,
        });
    }

    let authenticated = match extract_token(&request) {
        Some(t) => state.auth_sessions.validate(&t).await,
        None => false,
    };

    Json(AuthStatusResponse {
        enabled: true,
        authenticated,
    })
}

pub fn auth_router() -> Router<AppState> {
    Router::new()
        .route("/api/auth/login", post(login_handler))
        .route("/api/auth/logout", post(logout_handler))
        .route("/api/auth/check", axum::routing::get(check_handler))
}

// Background task to cleanup expired sessions periodically
pub fn spawn_session_cleanup(state: AuthState) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(3600));
        loop {
            interval.tick().await;
            state.cleanup().await;
        }
    });
}
