//! Centralized, fail-closed HTTPS/TLS transport policy for ALL Home authority
//! and AirCat Strict endpoints (`P0-A` / `R1.2-P0-A`).
//!
//! Strict authority precedence:
//! 1. **Direct verified TLS**: `AirCatTlsVerifiedConnection` in request extensions
//!    (inserted exclusively by the `tokio-rustls` HTTPS accept loop after a real
//!    TLS handshake; never constructible from HTTP headers). Direct TLS ignores
//!    HTTP proxy forwarding headers completely.
//! 2. **Explicitly trusted reverse proxy HTTPS termination**: requires ALL of:
//!    - `config.trust_proxy == true`,
//!    - `ConnectInfo<SocketAddr>` peer IP present and in `config.trusted_proxies`,
//!    - Complete, strict parsing of **both** `X-Forwarded-Proto` and RFC 7239
//!      `Forwarded` whenever present (zero silent preference for either header),
//!    - No non-UTF-8, empty, malformed, duplicate-`proto`, non-`https` (`http`,
//!      `ws`, `wss`, unknown), mixed-hop, or contradictory/mismatched-hop-count
//!      declarations.
//! 3. **Fail-closed refusal**:
//!    - If the HTTPS listener is unconfigured or not bound -> `503 TLS_NOT_CONFIGURED`.
//!    - Otherwise -> `403 TLS_REQUIRED`.

use axum::{
    extract::{ConnectInfo, Request, State},
    http::{Extensions, HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use std::net::{IpAddr, SocketAddr};

use crate::{AirCatTlsVerifiedConnection, AppState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TlsTransportVerdict {
    VerifiedDirectTls,
    VerifiedTrustedProxyHttps,
    TlsNotConfigured,
    CleartextRejected,
}

fn is_scheme_token_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')
}

fn is_http_token_char(c: char) -> bool {
    c.is_ascii_alphanumeric()
        || matches!(
            c,
            '!' | '#'
                | '$'
                | '%'
                | '&'
                | '\''
                | '*'
                | '+'
                | '-'
                | '.'
                | '^'
                | '_'
                | '`'
                | '|'
                | '~'
        )
}

/// Splits an RFC 7230/7239 list on `delimiter` while respecting double-quoted
/// strings and backslash-escaped characters inside quotes.
fn split_outside_quotes(input: &str, delimiter: char) -> Result<Vec<&str>, &'static str> {
    let mut parts = Vec::new();
    let mut in_quotes = false;
    let mut escaped = false;
    let mut start = 0usize;

    for (idx, ch) in input.char_indices() {
        if in_quotes {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_quotes = false;
            }
        } else if ch == '"' {
            in_quotes = true;
        } else if ch == delimiter {
            parts.push(&input[start..idx]);
            start = idx + ch.len_utf8();
        }
    }

    if in_quotes || escaped {
        return Err("unterminated quoted-string or escape in Forwarded header");
    }
    parts.push(&input[start..]);
    Ok(parts)
}

fn parse_rfc7239_value(raw: &str) -> Result<String, &'static str> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("empty parameter value in Forwarded header");
    }
    if let Some(rest) = trimmed.strip_prefix('"') {
        let mut out = String::with_capacity(rest.len());
        let mut escaped = false;
        let mut closed = false;
        let mut chars = rest.chars();
        for ch in chars.by_ref() {
            if escaped {
                if !ch.is_ascii() || ch.is_ascii_control() {
                    return Err("invalid escaped char in Forwarded quoted-string");
                }
                out.push(ch);
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                closed = true;
                break;
            } else if ch.is_ascii_control() {
                return Err("control char in Forwarded quoted-string");
            } else {
                out.push(ch);
            }
        }
        if !closed || escaped || chars.next().is_some() {
            return Err("malformed quoted-string in Forwarded header");
        }
        if out.is_empty() {
            return Err("empty quoted-string value in Forwarded header");
        }
        Ok(out)
    } else {
        if !trimmed.chars().all(is_http_token_char) {
            return Err("invalid unquoted token value in Forwarded header");
        }
        Ok(trimmed.to_string())
    }
}

/// Strictly parses all `X-Forwarded-Proto` header lines into a flat list of
/// lowercased protocol hops. Returns `Ok(None)` when absent, `Err` on any
/// non-UTF-8, empty, or syntactically invalid hop.
pub fn parse_x_forwarded_proto_hops(
    headers: &HeaderMap,
) -> Result<Option<Vec<String>>, &'static str> {
    let mut saw_header = false;
    let mut hops = Vec::new();

    for value in headers.get_all("x-forwarded-proto") {
        saw_header = true;
        let raw = value
            .to_str()
            .map_err(|_| "non-utf8 X-Forwarded-Proto header")?;
        if raw.trim().is_empty() {
            return Err("empty X-Forwarded-Proto header");
        }
        for segment in raw.split(',') {
            let hop = segment.trim();
            if hop.is_empty() || !hop.chars().all(is_scheme_token_char) {
                return Err("malformed X-Forwarded-Proto hop");
            }
            hops.push(hop.to_ascii_lowercase());
        }
    }

    if !saw_header {
        Ok(None)
    } else if hops.is_empty() {
        Err("empty X-Forwarded-Proto hop list")
    } else {
        Ok(Some(hops))
    }
}

/// Strictly parses all RFC 7239 `Forwarded` header lines into a flat list of
/// lowercased `proto` hops (one required `proto` per forwarded-element).
/// Rejects non-UTF-8 values, malformed parameters, unclosed quotes, missing
/// `proto`, and duplicate `proto` parameters within the same element.
pub fn parse_rfc7239_forwarded_hops(
    headers: &HeaderMap,
) -> Result<Option<Vec<String>>, &'static str> {
    let mut saw_header = false;
    let mut hops = Vec::new();

    for value in headers.get_all("forwarded") {
        saw_header = true;
        let raw = value.to_str().map_err(|_| "non-utf8 Forwarded header")?;
        if raw.trim().is_empty() {
            return Err("empty Forwarded header");
        }
        for elem in split_outside_quotes(raw, ',')? {
            let elem_trimmed = elem.trim();
            if elem_trimmed.is_empty() {
                return Err("empty element in Forwarded header");
            }
            let mut elem_proto: Option<String> = None;
            for param in split_outside_quotes(elem_trimmed, ';')? {
                let param_trimmed = param.trim();
                if param_trimmed.is_empty() {
                    return Err("empty parameter in Forwarded element");
                }
                let (raw_key, raw_val) = param_trimmed
                    .split_once('=')
                    .ok_or("missing '=' in Forwarded parameter")?;
                let key = raw_key.trim();
                if key.is_empty() || !key.chars().all(is_http_token_char) {
                    return Err("invalid parameter name in Forwarded header");
                }
                let parsed_val = parse_rfc7239_value(raw_val)?;
                if key.eq_ignore_ascii_case("proto") {
                    if elem_proto.is_some() {
                        return Err("duplicate proto parameter in Forwarded element");
                    }
                    if !parsed_val.chars().all(is_scheme_token_char) {
                        return Err("invalid proto scheme in Forwarded element");
                    }
                    elem_proto = Some(parsed_val.to_ascii_lowercase());
                }
            }
            let proto = elem_proto.ok_or("missing proto parameter in Forwarded element")?;
            hops.push(proto);
        }
    }

    if !saw_header {
        Ok(None)
    } else if hops.is_empty() {
        Err("empty Forwarded hop list")
    } else {
        Ok(Some(hops))
    }
}

/// Classification of reverse proxy protocol forwarding headers (`X-Forwarded-Proto`
/// and RFC 7239 `Forwarded`) when evaluating transport scheme from a trusted proxy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProxyProtoEvidence {
    /// Neither `X-Forwarded-Proto` nor `Forwarded` was present.
    Absent,
    /// Present headers parsed strictly, matched hop counts (if both present), and every hop is `https`.
    CoherentHttps,
    /// Present headers parsed strictly, matched hop counts (if both present), and every hop is `http`.
    CoherentHttp,
    /// Contradictory (`https` vs `http`), mixed hops, mismatched hop counts, unsupported scheme
    /// (`ws`, `wss`, etc.), non-UTF-8, or malformed syntax.
    ContradictoryOrMalformed,
}

/// Strictly classifies `X-Forwarded-Proto` and RFC 7239 `Forwarded` headers without ever
/// ignoring one header in favor of the other.
pub fn classify_proxy_proto_headers(headers: &HeaderMap) -> ProxyProtoEvidence {
    let Ok(xfp_opt) = parse_x_forwarded_proto_hops(headers) else {
        return ProxyProtoEvidence::ContradictoryOrMalformed;
    };
    let Ok(fwd_opt) = parse_rfc7239_forwarded_hops(headers) else {
        return ProxyProtoEvidence::ContradictoryOrMalformed;
    };

    match (xfp_opt, fwd_opt) {
        (None, None) => ProxyProtoEvidence::Absent,
        (Some(xfp), None) => {
            if !xfp.is_empty() && xfp.iter().all(|p| p == "https") {
                ProxyProtoEvidence::CoherentHttps
            } else if !xfp.is_empty() && xfp.iter().all(|p| p == "http") {
                ProxyProtoEvidence::CoherentHttp
            } else {
                ProxyProtoEvidence::ContradictoryOrMalformed
            }
        }
        (None, Some(fwd)) => {
            if !fwd.is_empty() && fwd.iter().all(|p| p == "https") {
                ProxyProtoEvidence::CoherentHttps
            } else if !fwd.is_empty() && fwd.iter().all(|p| p == "http") {
                ProxyProtoEvidence::CoherentHttp
            } else {
                ProxyProtoEvidence::ContradictoryOrMalformed
            }
        }
        (Some(xfp), Some(fwd)) => {
            if xfp.is_empty() || xfp.len() != fwd.len() {
                ProxyProtoEvidence::ContradictoryOrMalformed
            } else if xfp.iter().all(|p| p == "https") && fwd.iter().all(|p| p == "https") {
                ProxyProtoEvidence::CoherentHttps
            } else if xfp.iter().all(|p| p == "http") && fwd.iter().all(|p| p == "http") {
                ProxyProtoEvidence::CoherentHttp
            } else {
                ProxyProtoEvidence::ContradictoryOrMalformed
            }
        }
    }
}

/// Evaluates BOTH `X-Forwarded-Proto` and RFC 7239 `Forwarded` unconditionally
/// (never preferring one header silently over the other). Returns `true` ONLY
/// when at least one header is present, every present header parses strictly,
/// every hop equals `https`, and (if both headers exist) their hop counts match.
pub fn are_proxy_https_headers_consistent(headers: &HeaderMap) -> bool {
    classify_proxy_proto_headers(headers) == ProxyProtoEvidence::CoherentHttps
}

pub struct HomeTlsTransportPolicy;

impl HomeTlsTransportPolicy {
    /// Checks whether a trusted reverse proxy unambiguously terminated HTTPS
    /// using an explicit `Config` and optional socket peer IP.
    pub fn is_trusted_proxy_https_with_config(
        config: &michi_config::Config,
        peer_ip: Option<IpAddr>,
        headers: &HeaderMap,
    ) -> bool {
        if !config.trust_proxy {
            return false;
        }
        let Some(ip) = peer_ip else {
            return false;
        };
        if !config.is_trusted_proxy(&ip) {
            return false;
        }
        are_proxy_https_headers_consistent(headers)
    }

    /// Checks whether a trusted reverse proxy unambiguously terminated HTTPS.
    /// Spoofed, malformed, or contradictory `X-Forwarded-Proto` / `Forwarded`
    /// headers or missing `ConnectInfo<SocketAddr>` are ALWAYS rejected.
    pub fn is_trusted_proxy_https(
        state: &AppState,
        headers: &HeaderMap,
        extensions: &Extensions,
    ) -> bool {
        let peer_ip = extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|c| c.0.ip());
        Self::is_trusted_proxy_https_with_config(&state.config, peer_ip, headers)
    }

    /// Returns `true` if the connection arrived via direct verified TLS OR a
    /// strictly validated trusted reverse proxy HTTPS hop.
    pub fn is_verified_https_transport(
        config: &michi_config::Config,
        peer_ip: Option<IpAddr>,
        headers: &HeaderMap,
        extensions: &Extensions,
    ) -> bool {
        if extensions.get::<AirCatTlsVerifiedConnection>().is_some() {
            return true;
        }
        Self::is_trusted_proxy_https_with_config(config, peer_ip, headers)
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn headers_from(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.append(
                axum::http::header::HeaderName::from_bytes(k.as_bytes()).unwrap(),
                HeaderValue::from_str(v).unwrap(),
            );
        }
        h
    }

    #[test]
    fn accepts_clean_single_and_multi_hop_https() {
        assert!(are_proxy_https_headers_consistent(&headers_from(&[(
            "X-Forwarded-Proto",
            "https"
        )])));
        assert!(are_proxy_https_headers_consistent(&headers_from(&[(
            "X-Forwarded-Proto",
            "HTTPS, https"
        )])));
        assert!(are_proxy_https_headers_consistent(&headers_from(&[(
            "Forwarded",
            "for=192.0.2.10;proto=https;host=\"michi.home:9443\""
        )])));
        assert!(are_proxy_https_headers_consistent(&headers_from(&[(
            "Forwarded",
            "for=\"192.0.2.10;not=sep,still-quoted\";proto=\"HTTPS\", for=10.0.0.2;proto=https"
        )])));
        assert!(are_proxy_https_headers_consistent(&headers_from(&[
            ("X-Forwarded-Proto", "https, https"),
            (
                "Forwarded",
                "for=192.0.2.1;proto=https, for=10.0.0.2;proto=https"
            ),
        ])));
    }

    #[test]
    fn rejects_contradictory_and_ambiguous_header_combinations() {
        // R1.2-P0-A primary bypass: XFP=https + Forwarded proto=http
        assert!(!are_proxy_https_headers_consistent(&headers_from(&[
            ("X-Forwarded-Proto", "https"),
            ("Forwarded", "for=192.0.2.10;proto=http"),
        ])));
        // Reverse contradiction: XFP=http + Forwarded proto=https
        assert!(!are_proxy_https_headers_consistent(&headers_from(&[
            ("X-Forwarded-Proto", "http"),
            ("Forwarded", "for=192.0.2.10;proto=https"),
        ])));
        // Incompatible hop counts between XFP and Forwarded
        assert!(!are_proxy_https_headers_consistent(&headers_from(&[
            ("X-Forwarded-Proto", "https, https"),
            ("Forwarded", "for=192.0.2.10;proto=https"),
        ])));
        // Mixed hops in XFP
        assert!(!are_proxy_https_headers_consistent(&headers_from(&[(
            "X-Forwarded-Proto",
            "https, http"
        )])));
        // Mixed hops in Forwarded
        assert!(!are_proxy_https_headers_consistent(&headers_from(&[(
            "Forwarded",
            "for=192.0.2.1;proto=https, for=10.0.0.2;proto=http"
        )])));
        // Duplicate XFP headers with opposite values
        assert!(!are_proxy_https_headers_consistent(&headers_from(&[
            ("X-Forwarded-Proto", "https"),
            ("X-Forwarded-Proto", "http"),
        ])));
        // Duplicate Forwarded headers with opposite values
        assert!(!are_proxy_https_headers_consistent(&headers_from(&[
            ("Forwarded", "for=192.0.2.1;proto=https"),
            ("Forwarded", "for=10.0.0.2;proto=http"),
        ])));
        // Duplicate proto parameter inside single Forwarded element
        assert!(!are_proxy_https_headers_consistent(&headers_from(&[(
            "Forwarded",
            "proto=https;proto=http"
        )])));
        assert!(!are_proxy_https_headers_consistent(&headers_from(&[(
            "Forwarded",
            "proto=https;proto=https"
        )])));
    }

    #[test]
    fn rejects_malformed_empty_unknown_and_non_utf8_headers() {
        for bad in [
            vec![],
            vec![("X-Forwarded-Proto", "")],
            vec![("X-Forwarded-Proto", "https, ")],
            vec![("X-Forwarded-Proto", "wss")],
            vec![("X-Forwarded-Proto", "ws")],
            vec![("X-Forwarded-Proto", "https http")],
            vec![("Forwarded", "")],
            vec![("Forwarded", "for=192.0.2.1")],
            vec![("Forwarded", "for=192.0.2.1;proto=")],
            vec![("Forwarded", "for=192.0.2.1;proto=\"https")],
            vec![("Forwarded", "for=192.0.2.1;proto=wss")],
            vec![("Forwarded", "for=192.0.2.1;proto=https;")],
            vec![("Forwarded", "malformed_no_equals;proto=https")],
            vec![
                ("X-Forwarded-Proto", "https"),
                ("Forwarded", "for=192.0.2.1;proto=\"unclosed"),
            ],
        ] {
            assert!(
                !are_proxy_https_headers_consistent(&headers_from(&bad)),
                "expected reject for {bad:?}"
            );
        }

        // Non-UTF-8 header alongside valid header MUST fail closed
        let mut non_utf8 = HeaderMap::new();
        non_utf8.insert("x-forwarded-proto", HeaderValue::from_static("https"));
        non_utf8.insert(
            "forwarded",
            HeaderValue::from_bytes(b"for=192.0.2.1;proto=\xffhttps").unwrap(),
        );
        assert!(!are_proxy_https_headers_consistent(&non_utf8));
    }
}
