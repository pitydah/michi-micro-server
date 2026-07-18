//! Michi Security Layer
//!
//! Provides rate limiting, input validation, idempotency, and security middleware.

pub mod idempotency;

use axum::{
    body::Body,
    extract::State,
    http::{HeaderMap, Request, StatusCode},
    middleware::Next,
    response::Response,
};
use governor::{
    clock::{self, DefaultClock},
    middleware::NoOpMiddleware,
    state::{direct::NotKeyed, InMemoryState, RateLimiter},
    Quota,
};
use std::{num::NonZeroU32, sync::Arc};
use tower_http::limit::RequestBodyLimitLayer;
use tracing::warn;

pub use idempotency::IdempotencyStore;

/// Rate limiter state shared across requests
pub type SharedRateLimiter = Arc<
    RateLimiter<
        NotKeyed,
        InMemoryState,
        DefaultClock,
        NoOpMiddleware<<DefaultClock as clock::Clock>::Instant>,
    >,
>;

/// Security configuration
#[derive(Debug, Clone)]
pub struct SecurityConfig {
    pub rate_limit_rps: u32,
    pub rate_limit_burst: u32,
    pub max_body_size: usize,
    pub enable_validation: bool,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            rate_limit_rps: 10,
            rate_limit_burst: 20,
            max_body_size: 10 * 1024 * 1024,
            enable_validation: true,
        }
    }
}

/// Application state for security middleware
#[derive(Debug, Clone)]
pub struct SecurityState {
    pub config: SecurityConfig,
    pub rate_limiter: SharedRateLimiter,
    pub idempotency_store: IdempotencyStore,
}

impl SecurityState {
    pub fn new(config: SecurityConfig) -> Self {
        let quota = Quota::per_second(NonZeroU32::new(config.rate_limit_rps).unwrap())
            .allow_burst(NonZeroU32::new(config.rate_limit_burst).unwrap());
        let rate_limiter = Arc::new(RateLimiter::direct(quota));
        let idempotency_store = IdempotencyStore::new();

        Self {
            config,
            rate_limiter,
            idempotency_store,
        }
    }
}

/// Rate limiting middleware con Retry-After header.
pub async fn rate_limit_middleware(
    State(state): State<SecurityState>,
    req: Request<Body>,
    next: Next,
) -> Result<Response, (StatusCode, String)> {
    if state.rate_limiter.check().is_err() {
        warn!("Rate limit exceeded for request to {}", req.uri().path());
        let mut response = Response::new(Body::from(
            r#"{"error":{"code":"RATE_LIMITED","message":"Too many requests. Please wait.","details":{}}}"#,
        ));
        *response.status_mut() = StatusCode::TOO_MANY_REQUESTS;
        response.headers_mut().insert(
            "Retry-After",
            "10".parse().unwrap(),
        );
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            "10".to_string(),
        ));
    }

    Ok(next.run(req).await)
}

/// Request body size limit layer
pub fn body_size_limit_layer(size: usize) -> RequestBodyLimitLayer {
    RequestBodyLimitLayer::new(size)
}

/// Security headers middleware
pub async fn security_headers_middleware(req: Request<Body>, next: Next) -> Response {
    let mut response = next.run(req).await;

    response
        .headers_mut()
        .insert("X-Content-Type-Options", "nosniff".parse().unwrap());
    response
        .headers_mut()
        .insert("X-Frame-Options", "DENY".parse().unwrap());
    response
        .headers_mut()
        .insert("X-XSS-Protection", "1; mode=block".parse().unwrap());
    response.headers_mut().insert(
        "Referrer-Policy",
        "strict-origin-when-cross-origin".parse().unwrap(),
    );
    response.headers_mut().insert(
        "Permissions-Policy",
        "geolocation=(), microphone=(), camera=()".parse().unwrap(),
    );

    response
}

/// Health check endpoint
pub async fn health_check_handler() -> &'static str {
    "OK"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = SecurityConfig::default();
        assert_eq!(config.rate_limit_rps, 10);
        assert_eq!(config.rate_limit_burst, 20);
        assert_eq!(config.max_body_size, 10 * 1024 * 1024);
    }

    #[test]
    fn test_security_state_creation() {
        let config = SecurityConfig::default();
        let state = SecurityState::new(config);
        assert!(Arc::strong_count(&state.rate_limiter) >= 1);
    }

    #[test]
    fn test_idempotency_store() {
        let store = IdempotencyStore::new();
        let key = "test-key-1";
        let response = serde_json::json!({"status": "ok"});

        // Primera vez: no hay cache
        assert!(store.get(key).is_none());

        // Almacenar
        store.set(key, &response, "POST", "/api/v1/test");
        assert!(store.get(key).is_some());

        // Key diferente -> None
        assert!(store.get("other-key").is_none());
    }
}
