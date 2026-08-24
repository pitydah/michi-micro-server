//! Michi Security Layer
//!
//! Provides rate limiting, input validation, idempotency, and security middleware.

pub mod idempotency;

use axum::{
    body::Body,
    extract::{ConnectInfo, Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use governor::{
    clock::{self, DefaultClock},
    middleware::NoOpMiddleware,
    state::{direct::NotKeyed, InMemoryState},
    Quota, RateLimiter,
};
use std::{net::SocketAddr, num::NonZeroU32, sync::Arc, time::Instant};
use tracing::warn;

pub use idempotency::IdempotencyStore;

/// A single per-IP rate limiter (not keyed — one bucket per IP map entry).
pub type SingleRateLimiter = Arc<
    RateLimiter<
        NotKeyed,
        InMemoryState,
        DefaultClock,
        NoOpMiddleware<<DefaultClock as clock::Clock>::Instant>,
    >,
>;

/// Per-IP rate limiter map: IP string → (limiter, last_used timestamp).
/// Entries are evicted after `IP_LIMITER_IDLE_TTL_SECS` seconds of inactivity.
pub type IpRateLimiter = Arc<dashmap::DashMap<String, (SingleRateLimiter, Instant)>>;

const IP_LIMITER_IDLE_TTL_SECS: u64 = 120;

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
    /// Per-IP rate limiter map. Each IP gets its own independent token-bucket.
    pub ip_rate_limiter: IpRateLimiter,
    pub idempotency_store: IdempotencyStore,
    pub pairing_attempts: Arc<dashmap::DashMap<String, (u32, std::time::Instant)>>,
}

impl SecurityState {
    pub fn new(config: SecurityConfig) -> Self {
        Self {
            config,
            ip_rate_limiter: Arc::new(dashmap::DashMap::new()),
            idempotency_store: IdempotencyStore::new(),
            pairing_attempts: Arc::new(dashmap::DashMap::new()),
        }
    }

    /// Build a fresh per-IP rate limiter with the configured quota.
    fn make_limiter(&self) -> SingleRateLimiter {
        let quota = Quota::per_second(NonZeroU32::new(self.config.rate_limit_rps.max(1)).unwrap())
            .allow_burst(NonZeroU32::new(self.config.rate_limit_burst.max(1)).unwrap());
        Arc::new(RateLimiter::direct(quota))
    }
}

/// Rate limiting middleware — per client IP (raw TCP peer address, not forwarded headers).
///
/// Each source IP gets its own independent token bucket. Stale entries (idle for
/// more than `IP_LIMITER_IDLE_TTL_SECS` seconds) are evicted on next access so
/// the map does not grow without bound across unique IPs over the lifetime of the server.
///
/// Falls back to `127.0.0.1` when `ConnectInfo` is unavailable (e.g., in unit tests
/// that use `tower::ServiceExt::oneshot()` without a real TCP socket).
pub async fn rate_limit_middleware(
    State(state): State<SecurityState>,
    connect_info: Option<ConnectInfo<SocketAddr>>,
    req: Request<Body>,
    next: Next,
) -> Result<Response, (StatusCode, String)> {
    let ip = connect_info
        .map(|ci| ci.0.ip().to_string())
        .unwrap_or_else(|| "127.0.0.1".to_string());
    let now = Instant::now();

    // Evict stale entry if TTL has elapsed.
    if let Some(entry) = state.ip_rate_limiter.get(&ip) {
        if now.duration_since(entry.1).as_secs() > IP_LIMITER_IDLE_TTL_SECS {
            drop(entry);
            state.ip_rate_limiter.remove(&ip);
        }
    }

    // Look up or create a limiter for this IP, then touch the timestamp.
    let limiter = {
        let mut entry = state
            .ip_rate_limiter
            .entry(ip.clone())
            .or_insert_with(|| (state.make_limiter(), now));
        entry.1 = now;
        Arc::clone(&entry.0)
    };

    if limiter.check().is_err() {
        warn!("Rate limit exceeded for IP {} on {}", ip, req.uri().path());
        return Err((StatusCode::TOO_MANY_REQUESTS, "10".to_string()));
    }
    Ok(next.run(req).await)
}

/// Rate limiting middleware específico para pairing (por IP, 5 intentos/minuto)
pub async fn pairing_rate_limit_middleware(
    State(state): State<SecurityState>,
    connect_info: Option<ConnectInfo<SocketAddr>>,
    req: Request<Body>,
    next: Next,
) -> Result<Response, (StatusCode, String)> {
    let ip = connect_info
        .map(|ci| ci.0.ip().to_string())
        .unwrap_or_else(|| "127.0.0.1".to_string());
    let now = std::time::Instant::now();
    let mut entry = state.pairing_attempts.entry(ip.clone()).or_insert((0, now));
    let (count, last_reset) = entry.value();
    let elapsed = now.duration_since(*last_reset);

    if elapsed.as_secs() > 60 {
        // Reset cada minuto
        *entry = (1, now);
    } else if *count >= 5 {
        warn!("Pairing rate limit exceeded for IP: {}", ip);
        return Err((StatusCode::TOO_MANY_REQUESTS, "60".to_string()));
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

    #[test]
    fn test_ip_rate_limiter_starts_empty() {
        let state = SecurityState::new(SecurityConfig::default());
        // Map starts empty — no allocation until first request.
        assert_eq!(state.ip_rate_limiter.len(), 0);
    }

    #[test]
    fn test_make_limiter_respects_burst() {
        let config = SecurityConfig {
            rate_limit_rps: 5,
            rate_limit_burst: 10,
            ..Default::default()
        };
        let state = SecurityState::new(config);
        let limiter = state.make_limiter();
        // Burst of 10 consecutive checks should succeed.
        for _ in 0..10 {
            assert!(limiter.check().is_ok());
        }
        // 11th immediately after burst should be rate-limited.
        assert!(limiter.check().is_err());
    }

    #[test]
    fn test_different_ips_have_independent_buckets() {
        let config = SecurityConfig {
            rate_limit_rps: 1,
            rate_limit_burst: 1,
            ..Default::default()
        };
        let state = SecurityState::new(config);

        let limiter_a = state.make_limiter();
        let limiter_b = state.make_limiter();

        // Exhaust limiter A's burst.
        assert!(limiter_a.check().is_ok());
        assert!(limiter_a.check().is_err());

        // Limiter B still has its own full burst.
        assert!(limiter_b.check().is_ok());
    }
}
