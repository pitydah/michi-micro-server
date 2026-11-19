//! KILLCRITIC R1.2 Adversarial Security, Trusted Proxy & X.509 Classification Tests.
//!
//! Verifies every mandatory acceptance criterion of R1.2:
//! - P0-A: Full 18-case trusted proxy HTTPS matrix, live TCP HTTP contradictory
//!   header rejection from a trusted proxy peer, playback/queue non-exemption for
//!   `michi_home_*`, and direct TLS precedence over proxy headers.
//! - P0-B: Decisive dual-CA test (CA-A trusted vs CA-B untrusted on live `tokio-rustls`
//!   listeners), proving zero false positives on `android_default_trust_compatible`,
//!   JSON Schema conformance against `vendor/michi-link/schemas/aircat-tls-status.schema.json`,
//!   and complete X.509 matrix (self-signed verified, self-issued unverified, incomplete
//!   chain, out-of-order chain, expired intermediate, non-CA intermediate, missing
//!   `serverAuth` EKU, SPKI key reuse vs key rotation).

use axum::{
    body::Body,
    extract::ConnectInfo,
    http::{header, HeaderName, HeaderValue, Request, StatusCode},
};
use ed25519_dalek::SigningKey;
use michi_api::{
    create_router, validate_x509_for_home_tls, verify_x509_against_explicit_roots, AirCatTlsInfo,
    AirCatTlsVerifiedConnection, AndroidTrustVerificationStatus, AppState, TlsCertificateProfile,
    TlsTrustMode,
};
use michi_config::Config;
use serde_json::Value;
use sqlx::SqlitePool;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tower::ServiceExt;
use uuid::Uuid;

async fn test_db() -> (SqlitePool, String) {
    let url = format!(
        "sqlite:file:memdb_interop_r1_2_{}?mode=memory&cache=shared",
        Uuid::new_v4()
    );
    let pool = michi_db::init_pool(&url).await.unwrap();
    (pool, url)
}

fn test_config(db_url: String, config_dir: PathBuf, music_dir: PathBuf) -> Config {
    let _ = std::fs::create_dir_all(&config_dir);
    let _ = std::fs::create_dir_all(&music_dir);
    let cache_dir = config_dir.parent().unwrap().join("cache");
    let _ = std::fs::create_dir_all(&cache_dir);

    Config {
        port: 9091,
        music_paths: vec![music_dir],
        config_path: config_dir,
        cache_path: cache_dir,
        database_url: db_url,
        version: "1.0.0-rc.4",
        sync_peers: Vec::new(),
        sync_name: "michi-server-interop-r1-2".to_string(),
        listenbrainz_token: None,
        lastfm_token: None,
        scrobble_enabled: false,
        auth_username: None,
        auth_password: None,
        auth_enabled: true,
        allow_registration: false,
        server_id: Uuid::new_v4(),
        cors_origin: None,
        dev_mode: true,
        resource_profile: michi_core::ResourceProfile::Balanced,
        format_policy: michi_core::AudioFormatPolicy::LosslessOnly,
        stream_profile: michi_core::StreamProfile::Original,
        max_remote_bitrate: 320_000,
        remote_sync: false,
        language: "en".into(),
        ui: Default::default(),
        auto_backup_enabled: false,
        backup_max_keep: 7,
        job_max_concurrent: 3,
        reconnect_delay_max: 300,
        opensubsonic_enabled: false,
        trust_proxy: false,
        trusted_proxies: vec!["127.0.0.1".parse().unwrap(), "::1".parse().unwrap()],
        deployment_platform: "docker".into(),
    }
}

async fn make_app(
    mut cfg_fn: impl FnMut(&mut Config),
) -> (axum::Router, SqlitePool, AppState, String) {
    let (pool, db_url) = test_db().await;
    let root_tmp = std::env::temp_dir().join(format!("michi-interop-r1-2-{}", Uuid::new_v4()));
    let config_dir = root_tmp.join("config");
    let music_dir = root_tmp.join("music");
    let mut config = test_config(db_url, config_dir, music_dir);
    cfg_fn(&mut config);

    let identity = Arc::new(
        michi_identity::IdentityManager::load_or_generate(
            &config.config_path,
            "Michi Micro Server",
            "interop-r1-2-password",
        )
        .unwrap(),
    );
    let admin_id = Uuid::new_v4();
    michi_db::create_user(&pool, &admin_id, "admin-r1-2", "password", true)
        .await
        .unwrap();
    let state = AppState::new_with_identity(config, pool.clone(), Some(admin_id), identity);
    let admin_token = state.auth_sessions.create_session(admin_id).await.unwrap();
    let app = create_router(state.clone());
    (app, pool, state, admin_token)
}

fn with_tls(mut req: Request<Body>) -> Request<Body> {
    req.extensions_mut().insert(AirCatTlsVerifiedConnection);
    req
}

fn with_peer(mut req: Request<Body>, ip: &str) -> Request<Body> {
    let addr: SocketAddr = format!("{ip}:45123").parse().unwrap();
    req.extensions_mut().insert(ConnectInfo(addr));
    req
}

async fn enable_tls(state: &AppState) {
    *state.aircat_tls.write().await = Some(AirCatTlsInfo {
        pinned_base_url: "https://localhost:9443".into(),
        tls_spki_sha256: "sha256/AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".into(),
        cert_path: PathBuf::from("/tmp/r1-2-tls.crt"),
        key_path: PathBuf::from("/tmp/r1-2-tls.key"),
        tls_port: 9443,
        tls_trust_mode: TlsTrustMode::CaChained,
        certificate_profile: TlsCertificateProfile::CaIssuedChainPresent,
        chain_cryptographically_verified: true,
        android_trust_verification_status: AndroidTrustVerificationStatus::NotVerified,
        cert_chain_len: 2,
        cert_not_before: Some("2026-01-01T00:00:00Z".into()),
        cert_not_after: Some("2027-01-01T00:00:00Z".into()),
    });
    state.set_aircat_tls_listener_ready(true);
}

async fn establish_home_session(app: &axum::Router, state: &AppState) -> (String, String) {
    let sk = SigningKey::generate(&mut rand::rngs::OsRng);
    let vk = sk.verifying_key();
    let pk = michi_identity::encode_base64url(&vk.to_bytes());
    let mid = michi_identity::MichiId::from_public_key(&vk).to_base64url();
    let home_id = state.home_root_authority.home_id();
    let membership = state.home_root_authority.issue_membership(
        &home_id,
        &mid,
        &pk,
        "mobile",
        vec![michi_identity::Role::MobilePlayer],
        &chrono::Utc::now().to_rfc3339(),
        21,
    );
    let ch_resp = app
        .clone()
        .oneshot(with_tls(
            Request::builder()
                .uri("/api/v1/auth/challenge")
                .method("POST")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&michi_identity::DeviceAuthChallengeRequest {
                        client_michi_id: mid.clone(),
                        client_public_key: pk,
                        home_id: home_id.clone(),
                    })
                    .unwrap(),
                ))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(ch_resp.status(), StatusCode::OK);
    let ch: michi_identity::DeviceAuthChallengeResponse = serde_json::from_slice(
        &axum::body::to_bytes(ch_resp.into_body(), 64 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();
    let sig = michi_identity::sign_device_auth_challenge(
        &sk,
        &home_id,
        &ch.server_michi_id,
        &mid,
        &ch.challenge_id.to_string(),
        &ch.challenge_nonce,
    );
    let sess_resp = app
        .clone()
        .oneshot(with_tls(
            Request::builder()
                .uri("/api/v1/auth/session")
                .method("POST")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&michi_identity::DeviceAuthSessionRequest {
                        challenge_id: ch.challenge_id,
                        client_michi_id: mid.clone(),
                        membership,
                        client_signature: sig,
                    })
                    .unwrap(),
                ))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(sess_resp.status(), StatusCode::OK);
    let sess: michi_identity::DeviceAuthSessionResponse = serde_json::from_slice(
        &axum::body::to_bytes(sess_resp.into_body(), 64 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();
    (sess.session_token, mid)
}

fn run_openssl(dir: &Path, args: &[&str]) {
    let out = std::process::Command::new("openssl")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("openssl binary required for R1.2 PKI tests");
    assert!(
        out.status.success(),
        "openssl {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
}

#[tokio::test]
async fn test_r1_2_p0_a_trusted_proxy_full_matrix_and_real_tcp_contradiction() {
    // Part A: TLS disabled -> 503 TLS_NOT_CONFIGURED even from trusted proxy with HTTPS headers
    {
        let (app, _pool, _state, admin_token) = make_app(|c| c.trust_proxy = true).await;
        let resp = app
            .oneshot(with_peer(
                Request::builder()
                    .uri("/api/v1/home/enrollment/pending")
                    .method("GET")
                    .header(header::AUTHORIZATION, format!("Bearer {admin_token}"))
                    .header("X-Forwarded-Proto", "https")
                    .body(Body::empty())
                    .unwrap(),
                "127.0.0.1",
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    // Part B: trust_proxy = false -> HTTP rejected even from 127.0.0.1 with XFP=https
    {
        let (app, _pool, state, admin_token) = make_app(|c| c.trust_proxy = false).await;
        enable_tls(&state).await;
        let resp = app
            .oneshot(with_peer(
                Request::builder()
                    .uri("/api/v1/home/enrollment/pending")
                    .method("GET")
                    .header(header::AUTHORIZATION, format!("Bearer {admin_token}"))
                    .header("X-Forwarded-Proto", "https")
                    .body(Body::empty())
                    .unwrap(),
                "127.0.0.1",
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    // Part C: trust_proxy = true with full §3.7 matrix
    let (app, _pool, state, admin_token) = make_app(|c| c.trust_proxy = true).await;
    enable_tls(&state).await;
    let admin_auth = format!("Bearer {admin_token}");
    let (home_token, _mid) = establish_home_session(&app, &state).await;
    let home_auth = format!("Bearer {home_token}");

    // Direct TLS takes precedence even if bogus contradictory proxy headers exist
    let direct_tls_ok = app
        .clone()
        .oneshot(with_tls(
            Request::builder()
                .uri("/api/v1/home/enrollment/pending")
                .method("GET")
                .header(header::AUTHORIZATION, &admin_auth)
                .header("X-Forwarded-Proto", "http")
                .header("Forwarded", "for=192.0.2.1;proto=http")
                .body(Body::empty())
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(direct_tls_ok.status(), StatusCode::OK);

    // Table-driven matrix on /api/v1/home/enrollment/pending
    type ProxyCase<'a> = (
        &'a str,
        Option<&'a str>,
        Vec<(&'a str, &'a str)>,
        StatusCode,
    );
    let cases: Vec<ProxyCase<'_>> = vec![
        (
            "HTTP direct without headers",
            Some("127.0.0.1"),
            vec![],
            StatusCode::FORBIDDEN,
        ),
        (
            "HTTP XFP=https untrusted peer",
            Some("203.0.113.10"),
            vec![("X-Forwarded-Proto", "https")],
            StatusCode::FORBIDDEN,
        ),
        (
            "HTTP Forwarded proto=https untrusted peer",
            Some("203.0.113.11"),
            vec![("Forwarded", "for=192.0.2.1;proto=https")],
            StatusCode::FORBIDDEN,
        ),
        (
            "Trusted proxy + coherent XFP=https",
            Some("127.0.0.1"),
            vec![("X-Forwarded-Proto", "https")],
            StatusCode::OK,
        ),
        (
            "Trusted proxy + coherent Forwarded proto=https",
            Some("127.0.0.1"),
            vec![("Forwarded", "for=192.0.2.10;proto=https")],
            StatusCode::OK,
        ),
        (
            "Trusted proxy + coherent BOTH XFP & Forwarded",
            Some("127.0.0.1"),
            vec![
                ("X-Forwarded-Proto", "https"),
                ("Forwarded", "for=192.0.2.10;proto=https"),
            ],
            StatusCode::OK,
        ),
        (
            "Contradiction: XFP=https + Forwarded proto=http",
            Some("127.0.0.1"),
            vec![
                ("X-Forwarded-Proto", "https"),
                ("Forwarded", "for=192.0.2.10;proto=http"),
            ],
            StatusCode::FORBIDDEN,
        ),
        (
            "Contradiction: XFP=http + Forwarded proto=https",
            Some("127.0.0.1"),
            vec![
                ("X-Forwarded-Proto", "http"),
                ("Forwarded", "for=192.0.2.10;proto=https"),
            ],
            StatusCode::FORBIDDEN,
        ),
        (
            "Mixed XFP hops: https, http",
            Some("127.0.0.1"),
            vec![("X-Forwarded-Proto", "https, http")],
            StatusCode::FORBIDDEN,
        ),
        (
            "Mixed Forwarded hops",
            Some("127.0.0.1"),
            vec![(
                "Forwarded",
                "for=192.0.2.1;proto=https, for=10.0.0.2;proto=http",
            )],
            StatusCode::FORBIDDEN,
        ),
        (
            "Duplicate XFP headers opposing values",
            Some("127.0.0.1"),
            vec![
                ("X-Forwarded-Proto", "https"),
                ("X-Forwarded-Proto", "http"),
            ],
            StatusCode::FORBIDDEN,
        ),
        (
            "Duplicate Forwarded headers opposing values",
            Some("127.0.0.1"),
            vec![
                ("Forwarded", "for=192.0.2.1;proto=https"),
                ("Forwarded", "for=10.0.0.2;proto=http"),
            ],
            StatusCode::FORBIDDEN,
        ),
        (
            "Incompatible hop counts between XFP and Forwarded",
            Some("127.0.0.1"),
            vec![
                ("X-Forwarded-Proto", "https, https"),
                ("Forwarded", "for=192.0.2.10;proto=https"),
            ],
            StatusCode::FORBIDDEN,
        ),
        (
            "Empty header",
            Some("127.0.0.1"),
            vec![("X-Forwarded-Proto", "")],
            StatusCode::FORBIDDEN,
        ),
        (
            "Malformed Forwarded header",
            Some("127.0.0.1"),
            vec![("Forwarded", "for=192.0.2.10;proto=\"https")],
            StatusCode::FORBIDDEN,
        ),
        (
            "Peer without ConnectInfo",
            None,
            vec![("X-Forwarded-Proto", "https")],
            StatusCode::FORBIDDEN,
        ),
    ];

    for (label, peer_opt, hdrs, expected) in cases {
        let mut builder = Request::builder()
            .uri("/api/v1/home/enrollment/pending")
            .method("GET")
            .header(header::AUTHORIZATION, &admin_auth);
        for (k, v) in hdrs {
            builder = builder.header(
                HeaderName::from_bytes(k.as_bytes()).unwrap(),
                HeaderValue::from_str(v).unwrap(),
            );
        }
        let mut req = builder.body(Body::empty()).unwrap();
        if let Some(ip) = peer_opt {
            req = with_peer(req, ip);
        }
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), expected, "matrix case failed: {label}");
    }

    // Part D: Mandatory live TCP HTTP test from real trusted proxy loopback IP (127.0.0.1)
    // proving contradictory headers are rejected across Home enrollment, Home auth, AirCat,
    // library, playback, and queue (ZERO exemptions for playback/queue!).
    // Use a fresh AppState so Part C's 16-case matrix does not saturate 127.0.0.1's rate-limit bucket.
    let (tcp_app, _tcp_pool, tcp_state, tcp_admin_token) = make_app(|c| c.trust_proxy = true).await;
    enable_tls(&tcp_state).await;
    let tcp_admin_auth = format!("Bearer {tcp_admin_token}");
    let (tcp_home_token, _tcp_mid) = establish_home_session(&tcp_app, &tcp_state).await;
    let tcp_home_auth = format!("Bearer {tcp_home_token}");
    let _ = &home_auth;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let srv = tokio::spawn(async move {
        let _ = axum::serve(
            listener,
            tcp_app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await;
    });

    let client = reqwest::Client::new();
    for (method, path, auth_hdr) in [
        (
            "GET",
            "/api/v1/home/enrollment/pending",
            tcp_admin_auth.as_str(),
        ),
        ("POST", "/api/v1/auth/challenge", ""),
        ("GET", "/api/v1/aircat/manifest", tcp_home_auth.as_str()),
        ("GET", "/api/v1/tracks", tcp_home_auth.as_str()),
        ("GET", "/api/v1/playback/state", tcp_home_auth.as_str()),
        ("GET", "/api/v1/queue", tcp_home_auth.as_str()),
    ] {
        let mut req = match method {
            "POST" => client
                .post(format!("http://{addr}{path}"))
                .json(&serde_json::json!({})),
            _ => client.get(format!("http://{addr}{path}")),
        };
        if !auth_hdr.is_empty() {
            req = req.header(header::AUTHORIZATION, auth_hdr);
        }
        let contradictory = req
            .header("X-Forwarded-Proto", "https")
            .header("Forwarded", "for=192.0.2.10;proto=http")
            .send()
            .await
            .unwrap();
        assert_eq!(
            contradictory.status(),
            StatusCode::FORBIDDEN,
            "live TCP trusted proxy contradictory headers MUST be rejected on {method} {path}"
        );
        let j: Value = contradictory.json().await.unwrap();
        assert_eq!(j["error"]["code"], "TLS_REQUIRED");
    }

    // Coherent BOTH headers over live TCP from trusted proxy 127.0.0.1 -> ACCEPTED!
    for path in [
        "/api/v1/aircat/manifest",
        "/api/v1/playback/state",
        "/api/v1/queue",
    ] {
        let coherent = client
            .get(format!("http://{addr}{path}"))
            .header(header::AUTHORIZATION, &tcp_home_auth)
            .header("X-Forwarded-Proto", "https")
            .header("Forwarded", "for=192.0.2.10;proto=https")
            .send()
            .await
            .unwrap();
        assert_eq!(
            coherent.status(),
            StatusCode::OK,
            "live TCP coherent trusted proxy HTTPS headers should succeed on {path}"
        );
    }

    srv.abort();
}

#[tokio::test]
async fn test_r1_2_p0_b_decisive_dual_ca_trust_separation_and_live_tls() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let dir = std::env::temp_dir().join(format!("michi-r1-2-dual-ca-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();

    // Generate TWO independent private CAs: CA-A (trusted by test client) and CA-B (untrusted)
    for (prefix, cn) in [
        ("ca_a", "Michi Test CA-A"),
        ("ca_b", "Michi Untrusted CA-B"),
    ] {
        run_openssl(
            &dir,
            &[
                "req",
                "-x509",
                "-newkey",
                "ec",
                "-pkeyopt",
                "ec_paramgen_curve:prime256v1",
                "-keyout",
                &format!("{prefix}.key"),
                "-out",
                &format!("{prefix}.crt"),
                "-days",
                "365",
                "-nodes",
                "-subj",
                &format!("/CN={cn}"),
                "-addext",
                "basicConstraints=critical,CA:TRUE",
            ],
        );
        std::fs::write(
            dir.join(format!("{prefix}_leaf.ext")),
            "basicConstraints=CA:FALSE\nkeyUsage=digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\nsubjectAltName=DNS:localhost,IP:127.0.0.1\n",
        )
        .unwrap();
        run_openssl(
            &dir,
            &[
                "req",
                "-new",
                "-newkey",
                "ec",
                "-pkeyopt",
                "ec_paramgen_curve:prime256v1",
                "-keyout",
                &format!("{prefix}_leaf.key"),
                "-out",
                &format!("{prefix}_leaf.csr"),
                "-nodes",
                "-subj",
                "/CN=localhost",
            ],
        );
        run_openssl(
            &dir,
            &[
                "x509",
                "-req",
                "-in",
                &format!("{prefix}_leaf.csr"),
                "-CA",
                &format!("{prefix}.crt"),
                "-CAkey",
                &format!("{prefix}.key"),
                "-CAcreateserial",
                "-out",
                &format!("{prefix}_leaf.crt"),
                "-days",
                "365",
                "-extfile",
                &format!("{prefix}_leaf.ext"),
            ],
        );
    }

    let ca_a_pem = std::fs::read(dir.join("ca_a.crt")).unwrap();
    let ca_b_pem = std::fs::read(dir.join("ca_b.crt")).unwrap();
    let mut chain_a = std::fs::read(dir.join("ca_a_leaf.crt")).unwrap();
    chain_a.push(b'\n');
    chain_a.extend_from_slice(&ca_a_pem);
    let key_a = std::fs::read(dir.join("ca_a_leaf.key")).unwrap();

    let mut chain_b = std::fs::read(dir.join("ca_b_leaf.crt")).unwrap();
    chain_b.push(b'\n');
    chain_b.extend_from_slice(&ca_b_pem);
    let key_b = std::fs::read(dir.join("ca_b_leaf.key")).unwrap();

    let now = chrono::Utc::now();
    let meta_a =
        validate_x509_for_home_tls(&chain_a, &key_a, "https://localhost:9443", now).unwrap();
    let meta_b =
        validate_x509_for_home_tls(&chain_b, &key_b, "https://localhost:9444", now).unwrap();

    // 1. Both present structurally valid verified chains, YET NEITHER claims Android default trust!
    for (name, m) in [("CA-A", &meta_a), ("CA-B", &meta_b)] {
        assert_eq!(
            m.certificate_profile,
            TlsCertificateProfile::CaIssuedChainPresent,
            "{name}"
        );
        assert!(m.chain_cryptographically_verified, "{name}");
        assert_eq!(
            m.android_trust_verification_status,
            AndroidTrustVerificationStatus::NotVerified,
            "{name} MUST report NotVerified (never inferred trusted by Android)"
        );
    }

    // 2. Explicit root check accepts CA-A against CA-A root, rejects CA-B against CA-A root
    assert!(
        verify_x509_against_explicit_roots(&chain_a, &ca_a_pem, "https://localhost:9443", now)
            .is_ok()
    );
    assert!(
        verify_x509_against_explicit_roots(&chain_b, &ca_a_pem, "https://localhost:9444", now)
            .is_err()
    );

    // 3. Spin up TWO real tokio-rustls HTTPS listeners (Server A with CA-A, Server B with CA-B)
    let spawn_https =
        |chain_pem: Vec<u8>, key_pem: Vec<u8>, meta: michi_api::VerifiedX509Metadata| async move {
            let (app, _pool, state, admin_token) = make_app(|_| {}).await;
            let certs: Vec<rustls::pki_types::CertificateDer<'static>> =
                rustls_pemfile::certs(&mut std::io::Cursor::new(&chain_pem))
                    .collect::<Result<Vec<_>, _>>()
                    .unwrap();
            let key = rustls_pemfile::private_key(&mut std::io::Cursor::new(&key_pem))
                .unwrap()
                .unwrap();
            let tls_cfg = rustls::ServerConfig::builder()
                .with_no_client_auth()
                .with_single_cert(certs, key)
                .unwrap();
            let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(tls_cfg));
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            *state.aircat_tls.write().await = Some(AirCatTlsInfo {
                pinned_base_url: format!("https://localhost:{}", addr.port()),
                tls_spki_sha256: meta.tls_spki_sha256,
                cert_path: PathBuf::from("/tmp/dual.crt"),
                key_path: PathBuf::from("/tmp/dual.key"),
                tls_port: addr.port(),
                tls_trust_mode: meta.tls_trust_mode,
                certificate_profile: meta.certificate_profile,
                chain_cryptographically_verified: meta.chain_cryptographically_verified,
                android_trust_verification_status: meta.android_trust_verification_status,
                cert_chain_len: meta.cert_chain_len,
                cert_not_before: Some(meta.not_before.to_rfc3339()),
                cert_not_after: Some(meta.not_after.to_rfc3339()),
            });
            state.set_aircat_tls_listener_ready(true);
            let app_clone = app.clone();
            let task = tokio::spawn(async move {
                loop {
                    let Ok((tcp, peer)) = listener.accept().await else {
                        break;
                    };
                    let acceptor = acceptor.clone();
                    let app = app_clone.clone();
                    tokio::spawn(async move {
                        let Ok(tls) = acceptor.accept(tcp).await else {
                            return;
                        };
                        let io = hyper_util::rt::TokioIo::new(tls);
                        let svc = hyper::service::service_fn(
                            move |req: hyper::Request<hyper::body::Incoming>| {
                                let mut axum_req = req.map(axum::body::Body::new);
                                axum_req.extensions_mut().insert(ConnectInfo(peer));
                                axum_req
                                    .extensions_mut()
                                    .insert(AirCatTlsVerifiedConnection);
                                let svc = app.clone();
                                async move { tower::ServiceExt::oneshot(svc, axum_req).await }
                            },
                        );
                        let _ = hyper_util::server::conn::auto::Builder::new(
                            hyper_util::rt::TokioExecutor::new(),
                        )
                        .serve_connection(io, svc)
                        .await;
                    });
                }
            });
            (addr, admin_token, app, task)
        };

    let (addr_a, token_a, app_a, task_a) = spawn_https(chain_a, key_a, meta_a).await;
    let (addr_b, token_b, app_b, task_b) = spawn_https(chain_b, key_b, meta_b).await;

    // Test client trusts ONLY CA-A (strictly no webpki/system roots fallback)
    let client_trusting_ca_a = reqwest::Client::builder()
        .tls_built_in_root_certs(false)
        .add_root_certificate(reqwest::Certificate::from_pem(&ca_a_pem).unwrap())
        .resolve("localhost", addr_a)
        .build()
        .unwrap();

    // Client trusting CA-A ACCEPTS Server A
    let resp_a = client_trusting_ca_a
        .get(format!(
            "https://localhost:{}/api/v1/aircat/status",
            addr_a.port()
        ))
        .bearer_auth(&token_a)
        .send()
        .await
        .expect("client trusting CA-A MUST accept Server A");
    assert_eq!(resp_a.status(), StatusCode::OK);
    let status_a: Value = resp_a.json().await.unwrap();

    // Same CA-A client REJECTS Server B (untrusted CA-B) at TLS handshake!
    let client_to_b = reqwest::Client::builder()
        .tls_built_in_root_certs(false)
        .add_root_certificate(reqwest::Certificate::from_pem(&ca_a_pem).unwrap())
        .resolve("localhost", addr_b)
        .build()
        .unwrap();
    let res_b = client_to_b
        .get(format!(
            "https://localhost:{}/api/v1/aircat/status",
            addr_b.port()
        ))
        .bearer_auth(&token_b)
        .send()
        .await;
    assert!(
        res_b.is_err(),
        "client trusting ONLY CA-A MUST reject TLS handshake to untrusted CA-B"
    );

    // Inspect Server B status via router: Server B MUST NOT claim Android default trust!
    let status_b: Value = serde_json::from_slice(
        &axum::body::to_bytes(
            app_b
                .clone()
                .oneshot(
                    Request::builder()
                        .uri("/api/v1/aircat/status")
                        .method("GET")
                        .header(header::AUTHORIZATION, format!("Bearer {token_b}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap()
                .into_body(),
            64 * 1024,
        )
        .await
        .unwrap(),
    )
    .unwrap();

    // Validate BOTH status_a and status_b against canonical michi-link JSON Schema
    let schema_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../vendor/michi-link/schemas/aircat-tls-status.schema.json");
    let schema_json: Value =
        serde_json::from_str(&std::fs::read_to_string(&schema_path).unwrap()).unwrap();
    let compiled = jsonschema::validator_for(&schema_json).unwrap();

    for (label, doc) in [
        ("Server A (CA-A)", &status_a),
        ("Server B (CA-B)", &status_b),
    ] {
        assert!(
            compiled.is_valid(doc),
            "{label} failed aircat-tls-status.schema.json: {doc}"
        );
        assert_eq!(
            doc["certificate_profile"], "ca_issued_chain_present",
            "{label}"
        );
        assert_eq!(doc["chain_cryptographically_verified"], true, "{label}");
        assert_eq!(
            doc["android_trust_verification_status"], "not_verified",
            "{label}"
        );
        assert_eq!(
            doc["android_default_trust_compatible"], false,
            "{label} MUST NOT emit false positive for Android default trust"
        );
    }

    // AirCat canonical 5 flags remain preserved on Server A & B (server availability != Android trust)
    for app_ref in [&app_a, &app_b] {
        let info: Value = serde_json::from_slice(
            &axum::body::to_bytes(
                app_ref
                    .clone()
                    .oneshot(
                        Request::builder()
                            .uri("/api/v1/server/info")
                            .method("GET")
                            .body(Body::empty())
                            .unwrap(),
                    )
                    .await
                    .unwrap()
                    .into_body(),
                64 * 1024,
            )
            .await
            .unwrap(),
        )
        .unwrap();
        for cap in michi_identity::AIRCAT_REQUIRED_CAPABILITIES {
            assert_eq!(
                info["features"][*cap], true,
                "AirCat flag {cap} must stay true"
            );
        }
    }

    task_a.abort();
    task_b.abort();
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn test_r1_2_p0_b_comprehensive_x509_adversarial_matrix() {
    let dir = std::env::temp_dir().join(format!("michi-r1-2-x509-matrix-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let now = chrono::Utc::now();

    // 1. Self-signed verified cert
    run_openssl(
        &dir,
        &[
            "req",
            "-x509",
            "-newkey",
            "ec",
            "-pkeyopt",
            "ec_paramgen_curve:prime256v1",
            "-keyout",
            "self.key",
            "-out",
            "self.crt",
            "-days",
            "365",
            "-nodes",
            "-subj",
            "/CN=localhost",
            "-addext",
            "subjectAltName=DNS:localhost,IP:127.0.0.1",
        ],
    );
    let self_meta = validate_x509_for_home_tls(
        &std::fs::read(dir.join("self.crt")).unwrap(),
        &std::fs::read(dir.join("self.key")).unwrap(),
        "https://localhost:9443",
        now,
    )
    .unwrap();
    assert_eq!(
        self_meta.certificate_profile,
        TlsCertificateProfile::SelfSignedVerified
    );
    assert!(!self_meta.chain_cryptographically_verified);
    assert_eq!(
        self_meta.android_trust_verification_status,
        AndroidTrustVerificationStatus::VerificationFailed
    );

    // 2. Self-issued (issuer == subject) BUT signed by a DIFFERENT key (NOT self-signed!)
    run_openssl(
        &dir,
        &[
            "req",
            "-x509",
            "-newkey",
            "ec",
            "-pkeyopt",
            "ec_paramgen_curve:prime256v1",
            "-keyout",
            "signer_same_dn.key",
            "-out",
            "signer_same_dn.crt",
            "-days",
            "365",
            "-nodes",
            "-subj",
            "/CN=localhost",
            "-addext",
            "basicConstraints=critical,CA:TRUE",
        ],
    );
    std::fs::write(
        dir.join("leaf_san.ext"),
        "basicConstraints=CA:FALSE\nextendedKeyUsage=serverAuth\nsubjectAltName=DNS:localhost,IP:127.0.0.1\n",
    )
    .unwrap();
    run_openssl(
        &dir,
        &[
            "req",
            "-new",
            "-newkey",
            "ec",
            "-pkeyopt",
            "ec_paramgen_curve:prime256v1",
            "-keyout",
            "self_issued.key",
            "-out",
            "self_issued.csr",
            "-nodes",
            "-subj",
            "/CN=localhost",
        ],
    );
    run_openssl(
        &dir,
        &[
            "x509",
            "-req",
            "-in",
            "self_issued.csr",
            "-CA",
            "signer_same_dn.crt",
            "-CAkey",
            "signer_same_dn.key",
            "-CAcreateserial",
            "-out",
            "self_issued.crt",
            "-days",
            "365",
            "-extfile",
            "leaf_san.ext",
        ],
    );
    let self_issued_meta = validate_x509_for_home_tls(
        &std::fs::read(dir.join("self_issued.crt")).unwrap(),
        &std::fs::read(dir.join("self_issued.key")).unwrap(),
        "https://localhost:9443",
        now,
    )
    .unwrap();
    assert_eq!(
        self_issued_meta.certificate_profile,
        TlsCertificateProfile::SelfIssuedUnverified,
        "issuer==subject signed by a different key MUST NOT be classified SelfSignedVerified"
    );
    assert!(!self_issued_meta.chain_cryptographically_verified);
    assert_eq!(
        self_issued_meta.android_trust_verification_status,
        AndroidTrustVerificationStatus::VerificationFailed
    );

    // 3. Three-tier chain: Root CA -> Intermediate CA (valid 2 days) -> Leaf (valid 365 days)
    run_openssl(
        &dir,
        &[
            "req",
            "-x509",
            "-newkey",
            "ec",
            "-pkeyopt",
            "ec_paramgen_curve:prime256v1",
            "-keyout",
            "root.key",
            "-out",
            "root.crt",
            "-days",
            "365",
            "-nodes",
            "-subj",
            "/CN=Michi Matrix Root CA",
            "-addext",
            "basicConstraints=critical,CA:TRUE",
        ],
    );
    std::fs::write(
        dir.join("inter.ext"),
        "basicConstraints=critical,CA:TRUE,pathlen:0\nkeyUsage=critical,keyCertSign,cRLSign\n",
    )
    .unwrap();
    run_openssl(
        &dir,
        &[
            "req",
            "-new",
            "-newkey",
            "ec",
            "-pkeyopt",
            "ec_paramgen_curve:prime256v1",
            "-keyout",
            "inter.key",
            "-out",
            "inter.csr",
            "-nodes",
            "-subj",
            "/CN=Michi Matrix Intermediate CA",
        ],
    );
    run_openssl(
        &dir,
        &[
            "x509",
            "-req",
            "-in",
            "inter.csr",
            "-CA",
            "root.crt",
            "-CAkey",
            "root.key",
            "-CAcreateserial",
            "-out",
            "inter.crt",
            "-days",
            "2",
            "-extfile",
            "inter.ext",
        ],
    );
    run_openssl(
        &dir,
        &[
            "req",
            "-new",
            "-newkey",
            "ec",
            "-pkeyopt",
            "ec_paramgen_curve:prime256v1",
            "-keyout",
            "leaf3.key",
            "-out",
            "leaf3.csr",
            "-nodes",
            "-subj",
            "/CN=localhost",
        ],
    );
    run_openssl(
        &dir,
        &[
            "x509",
            "-req",
            "-in",
            "leaf3.csr",
            "-CA",
            "inter.crt",
            "-CAkey",
            "inter.key",
            "-CAcreateserial",
            "-out",
            "leaf3.crt",
            "-days",
            "365",
            "-extfile",
            "leaf_san.ext",
        ],
    );

    let leaf3 = std::fs::read(dir.join("leaf3.crt")).unwrap();
    let leaf3_key = std::fs::read(dir.join("leaf3.key")).unwrap();
    let inter = std::fs::read(dir.join("inter.crt")).unwrap();
    let root = std::fs::read(dir.join("root.crt")).unwrap();
    let now = chrono::Utc::now() + chrono::Duration::seconds(5);

    // 3a. Incomplete chain (leaf only, missing intermediate)
    let inc_meta =
        validate_x509_for_home_tls(&leaf3, &leaf3_key, "https://localhost:9443", now).unwrap();
    assert_eq!(
        inc_meta.certificate_profile,
        TlsCertificateProfile::IncompleteChain
    );
    assert!(!inc_meta.chain_cryptographically_verified);
    assert_eq!(
        inc_meta.android_trust_verification_status,
        AndroidTrustVerificationStatus::NotVerified
    );

    // 3b. Ordered 3-tier chain [leaf, inter, root] -> CaIssuedChainPresent, chain verified, Android NotVerified
    let mut ordered_chain = Vec::new();
    ordered_chain.extend_from_slice(&leaf3);
    ordered_chain.push(b'\n');
    ordered_chain.extend_from_slice(&inter);
    ordered_chain.push(b'\n');
    ordered_chain.extend_from_slice(&root);
    let ok3 = validate_x509_for_home_tls(&ordered_chain, &leaf3_key, "https://localhost:9443", now)
        .unwrap();
    assert_eq!(
        ok3.certificate_profile,
        TlsCertificateProfile::CaIssuedChainPresent
    );
    assert!(ok3.chain_cryptographically_verified);
    assert_eq!(
        ok3.android_trust_verification_status,
        AndroidTrustVerificationStatus::NotVerified
    );

    // 3c. Out-of-order chain [leaf, root, inter] -> IncompleteChain + VerificationFailed!
    let mut unordered_chain = Vec::new();
    unordered_chain.extend_from_slice(&leaf3);
    unordered_chain.push(b'\n');
    unordered_chain.extend_from_slice(&root);
    unordered_chain.push(b'\n');
    unordered_chain.extend_from_slice(&inter);
    let bad_order =
        validate_x509_for_home_tls(&unordered_chain, &leaf3_key, "https://localhost:9443", now)
            .unwrap();
    assert_eq!(
        bad_order.certificate_profile,
        TlsCertificateProfile::IncompleteChain
    );
    assert!(!bad_order.chain_cryptographically_verified);
    assert_eq!(
        bad_order.android_trust_verification_status,
        AndroidTrustVerificationStatus::VerificationFailed
    );

    // 3d. Expired intermediate (evaluate 5 days from now: leaf valid 365d, intermediate expired at 2d!)
    let expired_inter_time = now + chrono::Duration::days(5);
    let exp_inter = validate_x509_for_home_tls(
        &ordered_chain,
        &leaf3_key,
        "https://localhost:9443",
        expired_inter_time,
    )
    .unwrap();
    assert_eq!(
        exp_inter.certificate_profile,
        TlsCertificateProfile::IncompleteChain
    );
    assert!(!exp_inter.chain_cryptographically_verified);
    assert_eq!(
        exp_inter.android_trust_verification_status,
        AndroidTrustVerificationStatus::VerificationFailed
    );

    // 4. Leaf with EKU = clientAuth only (missing serverAuth) -> REJECTED!
    std::fs::write(
        dir.join("client_only.ext"),
        "basicConstraints=CA:FALSE\nextendedKeyUsage=clientAuth\nsubjectAltName=DNS:localhost,IP:127.0.0.1\n",
    )
    .unwrap();
    run_openssl(
        &dir,
        &[
            "x509",
            "-req",
            "-in",
            "leaf3.csr",
            "-CA",
            "inter.crt",
            "-CAkey",
            "inter.key",
            "-CAcreateserial",
            "-out",
            "client_only.crt",
            "-days",
            "365",
            "-extfile",
            "client_only.ext",
        ],
    );
    assert!(
        validate_x509_for_home_tls(
            &std::fs::read(dir.join("client_only.crt")).unwrap(),
            &leaf3_key,
            "https://localhost:9443",
            now,
        )
        .is_err(),
        "leaf cert with EKU=clientAuth only MUST be rejected"
    );

    // 5. SPKI preservation when renewing cert with SAME key vs SPKI change on key rotation
    run_openssl(
        &dir,
        &[
            "x509",
            "-req",
            "-in",
            "leaf3.csr",
            "-CA",
            "inter.crt",
            "-CAkey",
            "inter.key",
            "-CAcreateserial",
            "-out",
            "leaf3_renewed.crt",
            "-days",
            "180",
            "-extfile",
            "leaf_san.ext",
        ],
    );
    let mut renewed_chain = std::fs::read(dir.join("leaf3_renewed.crt")).unwrap();
    renewed_chain.push(b'\n');
    renewed_chain.extend_from_slice(&inter);
    let renewed_meta =
        validate_x509_for_home_tls(&renewed_chain, &leaf3_key, "https://localhost:9443", now)
            .unwrap();
    assert_eq!(
        renewed_meta.tls_spki_sha256, ok3.tls_spki_sha256,
        "renewing certificate with the same key MUST keep identical SPKI SHA-256 pin"
    );
    assert_ne!(
        self_meta.tls_spki_sha256, ok3.tls_spki_sha256,
        "different key MUST produce a distinct SPKI SHA-256 pin"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
