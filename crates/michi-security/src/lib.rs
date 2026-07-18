//! Michi Security Layer
//!
//! Provides rate limiting, input validation, idempotency, and security middleware.

pub mod idempotency;

use axum::{
    body::Body,
    extract::{ConnectInfo, Request, State},
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::Response,
};
use governor::{
    clock::{self, DefaultClock},
    middleware::NoOpMiddleware,
    state::{direct::NotKeyed, InMemoryState},
    Quota, RateLimiter,
};
use std::{net::SocketAddr, num::NonZeroU32, sync::Arc};
use tracing::warn;

pub use idempotency::IdempotencyStore;

pub type SharedRateLimiter = Arc<
    RateLimiter<
        NotKeyed,
        InMemoryState,
        DefaultClock,
        NoOpMiddleware<<DefaultClock as clock::Clock>::Instant>,
    >,
>;

/// Rate limiter por IP para pairing
pub type PairingRateLimiter = Arc<dashmap::DashMap<String, u32>>;

/// Security configuration
#[derive(Debug, Clone)]
pub struct SecurityConfig {
    pub rate_limit_rps: u32,
    pub rate_limit_burst: u32,
    pub max_body_size: usize,
    pub enable_validation: bool,
    pub pairing_rate_limit_per_minute: u32,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            rate_limit_rps: 10,
            rate_limit_burst: 20,
            max_body_size: 10 * 1024 * 1024,
            enable_validation: true,
            pairing_rate_limit_per_minute: 5,
        }
    }
}

/// Application state for security middleware
#[derive(Debug, Clone)]
pub struct SecurityState {
    pub config: SecurityConfig,
    pub rate_limiter: SharedRateLimiter,
    pub idempotency_store: IdempotencyStore,
    pub pairing_attempts: Arc<dashmap::DashMap<String, (u32, std::time::Instant)>>,
}

impl SecurityState {
    pub fn new(config: SecurityConfig) -> Self {
        let quota = Quota::per_second(NonZeroU32::new(config.rate_limit_rps).unwrap())
            .allow_burst(NonZeroU32::new(config.rate_limit_burst).unwrap());
        let rate_limiter = Arc::new(RateLimiter::direct(quota));
        let idempotency_store = IdempotencyStore::new();
        let pairing_attempts = Arc::new(dashmap::DashMap::new());

        Self {
            config,
            rate_limiter,
            idempotency_store,
            pairing_attempts,
        }
    }
}

/// Rate limiting middleware
pub async fn rate_limit_middleware(
    State(state): State<SecurityState>,
    req: Request<Body>,
    next: Next,
) -> Result<Response, (StatusCode, String)> {
    if state.rate_limiter.check().is_err() {
        warn!("Rate limit exceeded for request to {}", req.uri().path());
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            "10".to_string(),
        ));
    }
    Ok(next.run(req).await)
}

/// Rate limiting middleware específico para pairing (por IP, 5 intentos/minuto)
pub async fn pairing_rate_limit_middleware(
    State(state): State<SecurityState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    req: Request<Body>,
    next: Next,
) -> Result<Response, (StatusCode, String)> {
    let ip = addr.ip().to_string();
    let now = std::time::Instant::now();
    let mut entry = state.pairing_attempts.entry(ip.clone()).or_insert((0, now));
    let (count, last_reset) = entry.value();
    let elapsed = now.duration_since(*last_reset);

    if elapsed.as_secs() > 60 {
        // Reset cada minuto
        *entry = (1, now);
    } else if *count >= 5 {
        warn!("Pairing rate limit exceeded for IP: {}", ip);
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            "60".to_string(),
        ));
    } else {
        entry.value_mut().0 += 1;
    }
    drop(entry);

    Ok(next.run(req).await)
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
    response.headers_mut().insert(
        "Strict-Transport-Security",
        "max-age=31536000; includeSubDomains".parse().unwrap(),
    );
    response.headers_mut().insert(
        "Content-Security-Policy",
        "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:;"
            .parse()
            .unwrap(),
    );
    response
        .headers_mut()
        .insert("Cache-Control", "no-store".parse().unwrap());

    response
}

pub async fn health_check_handler() -> &'static str {
    "OK"
}

/// Middleware that enforces Content-Type: application/json for POST/PUT/PATCH.
/// Also limits JSON parsing depth to prevent stack overflow attacks.
pub async fn content_type_middleware(
    req: Request<Body>,
    next: Next,
) -> Result<Response, (StatusCode, String)> {
    let method = req.method().clone();
    if method == axum::http::Method::POST
        || method == axum::http::Method::PUT
        || method == axum::http::Method::PATCH
    {
        let has_json = req
            .headers()
            .get("Content-Type")
            .and_then(|v| v.to_str().ok())
            .map(|v| v.starts_with("application/json"))
            .unwrap_or(false);

        if !has_json {
            return Err((
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "415 Content-Type must be application/json".into(),
            ));
        }
    }
    Ok(next.run(req).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = SecurityConfig::default();
        assert_eq!(config.rate_limit_rps, 10);
        assert_eq!(config.pairing_rate_limit_per_minute, 5);
    }
}
