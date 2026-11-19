//! Centralized, fail-closed HTTPS/TLS transport policy for ALL Home authority
//! and AirCat Strict endpoints (`P0-A`).
//!
//! Guarantees:
//! 1. Verified TLS is recognized ONLY when:
//!    - `AirCatTlsVerifiedConnection` is present in request extensions (inserted
//!      exclusively by the `tokio-rustls` HTTPS accept loop; never constructible
//!      from HTTP headers), OR
//!    - `config.trust_proxy` is explicitly enabled AND the TCP peer IP is in
//!      `config.trusted_proxies` AND the trusted proxy attests HTTPS termination
//!      unambiguously (single `X-Forwarded-Proto: https` or `Forwarded: proto=https`
//!      without any conflicting `http` hop).
//! 2. If the HTTPS listener is unconfigured or not bound -> `503 TLS_NOT_CONFIGURED`.
//! 3. If the request arrived over cleartext HTTP (or with spoofed proxy headers)
//!    -> `403 TLS_REQUIRED`.

use axum::{
    extract::{ConnectInfo, Request, State},
    http::{Extensions, HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use std::net::SocketAddr;

use crate::{AirCatTlsVerifiedConnection, AppState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TlsTransportVerdict {
    VerifiedDirectTls,
    VerifiedTrustedProxyHttps,
    TlsNotConfigured,
    CleartextRejected,
}

pub struct HomeTlsTransportPolicy;

impl HomeTlsTransportPolicy {
    /// Checks whether a trusted reverse proxy unambiguously terminated HTTPS.
    /// Spoofed `X-Forwarded-Proto` / `Forwarded` headers from untrusted peers
    /// or without `ConnectInfo<SocketAddr>` are ALWAYS rejected.
    pub fn is_trusted_proxy_https(
        state: &AppState,
        headers: &HeaderMap,
        extensions: &Extensions,
    ) -> bool {
        if !state.config.trust_proxy {
            return false;
        }
        let Some(peer_ip) = extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|c| c.0.ip())
        else {
            return false;
        };
        if !state.config.is_trusted_proxy(&peer_ip) {
            return false;
        }

        // Check X-Forwarded-Proto: all header values and comma-separated hops
        // must be strictly "https" (never accept mixed/spoofed "http, https").
        let xfp_vals: Vec<&str> = headers
            .get_all("x-forwarded-proto")
            .iter()
            .filter_map(|v| v.to_str().ok())
            .collect();
        if !xfp_vals.is_empty() {
            let mut saw_any = false;
            for raw in xfp_vals {
                for hop in raw.split(',') {
                    let h = hop.trim();
                    if h.is_empty() {
                        return false;
                    }
                    if !h.eq_ignore_ascii_case("https") {
                        return false;
                    }
                    saw_any = true;
                }
            }
            return saw_any;
        }

        // Fallback: RFC 7239 Forwarded header (all entries must carry proto=https)
        let fwd_vals: Vec<&str> = headers
            .get_all("forwarded")
            .iter()
            .filter_map(|v| v.to_str().ok())
            .collect();
        if !fwd_vals.is_empty() {
            let mut saw_proto = false;
            for raw in fwd_vals {
                for elem in raw.split(',') {
                    let mut elem_https = false;
                    for pair in elem.split(';') {
                        if let Some((k, v)) = pair.trim().split_once('=') {
                            if k.trim().eq_ignore_ascii_case("proto") {
                                let proto = v.trim().trim_matches('"');
                                if !proto.eq_ignore_ascii_case("https") {
                                    return false;
                                }
                                elem_https = true;
                            }
                        }
                    }
                    if !elem_https {
                        return false;
                    }
                    saw_proto = true;
                }
            }
            return saw_proto;
        }

        false
    }

    pub async fn evaluate(
        state: &AppState,
        headers: &HeaderMap,
        extensions: &Extensions,
    ) -> TlsTransportVerdict {
        if !state.is_aircat_tls_ready().await {
            return TlsTransportVerdict::TlsNotConfigured;
        }
        if extensions.get::<AirCatTlsVerifiedConnection>().is_some() {
            return TlsTransportVerdict::VerifiedDirectTls;
        }
        if Self::is_trusted_proxy_https(state, headers, extensions) {
            return TlsTransportVerdict::VerifiedTrustedProxyHttps;
        }
        TlsTransportVerdict::CleartextRejected
    }

    pub async fn enforce(
        state: &AppState,
        headers: &HeaderMap,
        extensions: &Extensions,
    ) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
        match Self::evaluate(state, headers, extensions).await {
            TlsTransportVerdict::VerifiedDirectTls
            | TlsTransportVerdict::VerifiedTrustedProxyHttps => Ok(()),
            TlsTransportVerdict::TlsNotConfigured => {
                state.aircat_security.record_tls_rejection();
                Err(crate::routes::v1::aircat::aircat_error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "TLS_NOT_CONFIGURED",
                    "Home authentication and AirCat endpoints require an active HTTPS/TLS listener with verified certificate and SPKI pin",
                ))
            }
            TlsTransportVerdict::CleartextRejected => {
                state.aircat_security.record_tls_rejection();
                Err(crate::routes::v1::aircat::aircat_error(
                    StatusCode::FORBIDDEN,
                    "TLS_REQUIRED",
                    "This endpoint requires verified HTTPS transport; cleartext HTTP is forbidden",
                ))
            }
        }
    }
}

/// Axum middleware enforcing `HomeTlsTransportPolicy` fail-closed on every
/// Home enrollment, Home auth, Home admin enrollment, and AirCat endpoint.
pub async fn home_tls_required_middleware(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    if let Err(err) =
        HomeTlsTransportPolicy::enforce(&state, request.headers(), request.extensions()).await
    {
        return err.into_response();
    }
    next.run(request).await
}
