//! KILLCRITIC R1.1 Adversarial, Security, Concurrency, Persistence & Real-TLS Tests.
//!
//! Verifies every mandatory R1.1 blocker and invariant:
//! - P0-A: Centralized fail-closed `HomeTlsTransportPolicy` across ALL 13 Home auth,
//!   enrollment, admin approval, enrollment-QR, AirCat routes AND any route
//!   receiving `Authorization: Bearer michi_home_*` (plus anti-spoofing proxy tests).
//! - P0-B: Full X.509 certificate + private key pairing, validity window, SAN, and
//!   `TlsTrustMode::CaChained` vs `TlsTrustMode::SelfSigned` classification.
//! - P1-C: Single canonical capability projection parity across `/api/v1/server/info`,
//!   `/api/v1/capabilities`, `to_features_map()`, and `discovery_features()` under
//!   live module toggles.
//! - M4: Durable SQLite Home enrollment persistence across restarts, atomic serial
//!   allocation under concurrency, double-approval conflict (`409`), role escalation
//!   refusal (`422 INVALID_ROLES`), duplicate pending superseding, and revocation
//!   of pending enrollments.
//! - M5: Single Home session authority (`michi_home_*` NEVER stored in legacy `TokenStore`).
//! - P1-D: Fail-closed revocation (`HomeMemberAuthorizationState::Indeterminate` ->
//!   `503 REVOCATION_STORE_UNAVAILABLE`) on SQLite error.
//! - E2E: Real TCP HTTP listener vs real `tokio-rustls` HTTPS listener with CA-chained
//!   OpenSSL certificate running the complete mobile enrollment + mutual auth +
//!   library + AirCat manifest lifecycle.

use axum::{
    body::Body,
    extract::ConnectInfo,
    http::{header, Request, StatusCode},
};
use ed25519_dalek::SigningKey;
use michi_api::{
    create_router,
    routes::v1::aircat::{check_home_member_authorization, HomeMemberAuthorizationState},
    AirCatTlsInfo, AirCatTlsVerifiedConnection, AppState, TlsTrustMode,
};
use michi_config::Config;
use serde_json::Value;
use sqlx::SqlitePool;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tower::ServiceExt;
use uuid::Uuid;

async fn test_db() -> (SqlitePool, String) {
    let url = format!(
        "sqlite:file:memdb_interop_r1_1_{}?mode=memory&cache=shared",
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
        sync_name: "michi-server-interop-r1-1".to_string(),
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

async fn make_app_with_config(
    mut cfg_fn: impl FnMut(&mut Config),
) -> (axum::Router, SqlitePool, AppState, String, PathBuf, PathBuf) {
    let (pool, db_url) = test_db().await;
    let root_tmp = std::env::temp_dir().join(format!("michi-interop-r1-1-{}", Uuid::new_v4()));
    let config_dir = root_tmp.join("config");
    let music_dir = root_tmp.join("music");
    let mut config = test_config(db_url, config_dir.clone(), music_dir.clone());
    cfg_fn(&mut config);

    let identity = Arc::new(
        michi_identity::IdentityManager::load_or_generate(
            &config.config_path,
            "Michi Micro Server",
            "interop-r1-1-password",
        )
        .unwrap(),
    );
    let admin_id = Uuid::new_v4();
    michi_db::create_user(&pool, &admin_id, "admin-r1-1", "password", true)
        .await
        .unwrap();
    let state = AppState::new_with_identity(config, pool.clone(), Some(admin_id), identity);
    let admin_token = state.auth_sessions.create_session(admin_id).await.unwrap();
    let app = create_router(state.clone());
    (app, pool, state, admin_token, config_dir, music_dir)
}

fn with_tls(mut req: Request<Body>) -> Request<Body> {
    req.extensions_mut().insert(AirCatTlsVerifiedConnection);
    req
}

fn with_peer(mut req: Request<Body>, ip: &str) -> Request<Body> {
    let addr: SocketAddr = format!("{ip}:44321").parse().unwrap();
    req.extensions_mut().insert(ConnectInfo(addr));
    req
}

async fn enable_tls_ca_chained(state: &AppState) {
    *state.aircat_tls.write().await = Some(AirCatTlsInfo {
        pinned_base_url: "https://127.0.0.1:9443".into(),
        tls_spki_sha256: "sha256/AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".into(),
        cert_path: PathBuf::from("/tmp/r1-1-tls.crt"),
        key_path: PathBuf::from("/tmp/r1-1-tls.key"),
        tls_port: 9443,
        tls_trust_mode: TlsTrustMode::CaChained,
        cert_chain_len: 2,
        cert_not_before: Some("2026-01-01T00:00:00Z".into()),
        cert_not_after: Some("2027-01-01T00:00:00Z".into()),
    });
    state.set_aircat_tls_listener_ready(true);
}

fn gen_client_keypair() -> (SigningKey, String, String) {
    let sk = SigningKey::generate(&mut rand::rngs::OsRng);
    let vk = sk.verifying_key();
    let pk = michi_identity::encode_base64url(&vk.to_bytes());
    let mid = michi_identity::MichiId::from_public_key(&vk).to_base64url();
    (sk, pk, mid)
}

async fn body_json(resp: axum::response::Response) -> Value {
    let bytes = axum::body::to_bytes(resp.into_body(), 256 * 1024)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap_or_else(
        |_| serde_json::json!({ "raw": String::from_utf8_lossy(&bytes).to_string() }),
    )
}

async fn establish_home_session_over_tls(
    app: &axum::Router,
    state: &AppState,
    sk: &SigningKey,
    pk: &str,
    mid: &str,
    membership: michi_identity::DeviceMembershipDto,
) -> String {
    let home_id = state.home_root_authority.home_id();
    let ch_resp = app
        .clone()
        .oneshot(with_tls(
            Request::builder()
                .uri("/api/v1/auth/challenge")
                .method("POST")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&michi_identity::DeviceAuthChallengeRequest {
                        client_michi_id: mid.to_string(),
                        client_public_key: pk.to_string(),
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
        sk,
        &home_id,
        &ch.server_michi_id,
        mid,
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
                        client_michi_id: mid.to_string(),
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
    sess.session_token
}

/// Generates a real OpenSSL CA + CA-signed leaf fullchain (with SAN) plus a separate
/// self-signed leaf certificate and an unrelated private key for P0-B and real-TLS tests.
struct OpenSslFixtures {
    dir: PathBuf,
    ca_cert_pem: Vec<u8>,
    fullchain_pem: Vec<u8>,
    leaf_key_pem: Vec<u8>,
    self_signed_pem: Vec<u8>,
    self_signed_key_pem: Vec<u8>,
    wrong_key_pem: Vec<u8>,
}

impl Drop for OpenSslFixtures {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn generate_openssl_fixtures() -> OpenSslFixtures {
    let dir = std::env::temp_dir().join(format!("michi-r1-1-pki-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();

    let run = |args: &[&str]| {
        let out = std::process::Command::new("openssl")
            .args(args)
            .current_dir(&dir)
            .output()
            .expect("openssl binary required for PKI tests");
        assert!(
            out.status.success(),
            "openssl {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    };

    // 1. Create a real Home/Test CA
    run(&[
        "req",
        "-x509",
        "-newkey",
        "ec",
        "-pkeyopt",
        "ec_paramgen_curve:prime256v1",
        "-keyout",
        "ca.key",
        "-out",
        "ca.crt",
        "-days",
        "365",
        "-nodes",
        "-subj",
        "/CN=Michi Test Root CA",
        "-addext",
        "basicConstraints=critical,CA:TRUE",
    ]);

    // 2. Create leaf CSR + sign with CA including SAN DNS:localhost + IP:127.0.0.1
    std::fs::write(
        dir.join("leaf.ext"),
        "basicConstraints=CA:FALSE\nkeyUsage=digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\nsubjectAltName=DNS:localhost,DNS:michi.home.arpa,IP:127.0.0.1\n",
    )
    .unwrap();
    run(&[
        "req",
        "-new",
        "-newkey",
        "ec",
        "-pkeyopt",
        "ec_paramgen_curve:prime256v1",
        "-keyout",
        "leaf.key",
        "-out",
        "leaf.csr",
        "-nodes",
        "-subj",
        "/CN=localhost",
    ]);
    run(&[
        "x509",
        "-req",
        "-in",
        "leaf.csr",
        "-CA",
        "ca.crt",
        "-CAkey",
        "ca.key",
        "-CAcreateserial",
        "-out",
        "leaf.crt",
        "-days",
        "365",
        "-extfile",
        "leaf.ext",
    ]);

    let ca_cert_pem = std::fs::read(dir.join("ca.crt")).unwrap();
    let mut fullchain_pem = std::fs::read(dir.join("leaf.crt")).unwrap();
    fullchain_pem.push(b'\n');
    fullchain_pem.extend_from_slice(&ca_cert_pem);
    let leaf_key_pem = std::fs::read(dir.join("leaf.key")).unwrap();

    // 3. Self-signed leaf with matching SAN
    run(&[
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
    ]);
    let self_signed_pem = std::fs::read(dir.join("self.crt")).unwrap();
    let self_signed_key_pem = std::fs::read(dir.join("self.key")).unwrap();

    // 4. Unrelated key for mismatch test
    run(&[
        "ecparam",
        "-name",
        "prime256v1",
        "-genkey",
        "-noout",
        "-out",
        "wrong.key",
    ]);
    let wrong_key_pem = std::fs::read(dir.join("wrong.key")).unwrap();

    OpenSslFixtures {
        dir,
        ca_cert_pem,
        fullchain_pem,
        leaf_key_pem,
        self_signed_pem,
        self_signed_key_pem,
        wrong_key_pem,
    }
}

#[tokio::test]
async fn test_r1_1_p0_a_tls_required_on_all_home_routes_and_home_bearer_tokens() {
    let (app, _pool, state, admin_token, _cfg_dir, _music_dir) =
        make_app_with_config(|cfg| cfg.trust_proxy = true).await;
    let admin_auth = format!("Bearer {admin_token}");
    let req_id = Uuid::new_v4();
    let track_id = Uuid::new_v4();

    let all_home_tls_routes: Vec<(&str, String)> = vec![
        ("POST", "/api/v1/auth/challenge".into()),
        ("POST", "/api/v1/auth/session".into()),
        ("POST", "/api/v1/home/enrollment/challenge".into()),
        ("POST", "/api/v1/home/enrollment/request".into()),
        ("GET", format!("/api/v1/home/enrollment/{req_id}/status")),
        ("GET", "/api/v1/home/enrollment/pending".into()),
        ("POST", format!("/api/v1/home/enrollment/{req_id}/approve")),
        ("POST", format!("/api/v1/home/enrollment/{req_id}/reject")),
        ("POST", "/api/v1/home/members/enroll".into()),
        ("GET", "/api/v1/aircat/enrollment-qr".into()),
        ("GET", "/api/v1/aircat/manifest".into()),
        ("POST", "/api/v1/aircat/authorize".into()),
        ("GET", format!("/api/v1/aircat/download/{track_id}")),
    ];

    // 1. With TLS NOT configured -> EVERY single one of the 13 routes fails closed with 503 TLS_NOT_CONFIGURED
    for (idx, (method, uri)) in all_home_tls_routes.iter().enumerate() {
        let peer_ip = format!("198.51.100.{}", (idx % 200) + 1);
        let resp = app
            .clone()
            .oneshot(with_peer(
                Request::builder()
                    .uri(uri.as_str())
                    .method(*method)
                    .header(header::AUTHORIZATION, &admin_auth)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(b"{}".as_slice()))
                    .unwrap(),
                &peer_ip,
            ))
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::SERVICE_UNAVAILABLE,
            "expected 503 TLS_NOT_CONFIGURED on {method} {uri}"
        );
        let j = body_json(resp).await;
        assert_eq!(j["error"]["code"], "TLS_NOT_CONFIGURED", "{method} {uri}");
    }

    // 2. Enable TLS -> EVERY route still rejects cleartext HTTP with 403 TLS_REQUIRED
    enable_tls_ca_chained(&state).await;
    for (idx, (method, uri)) in all_home_tls_routes.iter().enumerate() {
        let peer_ip = format!("198.51.100.{}", (idx % 200) + 50);
        let resp = app
            .clone()
            .oneshot(with_peer(
                Request::builder()
                    .uri(uri.as_str())
                    .method(*method)
                    .header(header::AUTHORIZATION, &admin_auth)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(b"{}".as_slice()))
                    .unwrap(),
                &peer_ip,
            ))
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "expected 403 TLS_REQUIRED on cleartext {method} {uri}"
        );
        let j = body_json(resp).await;
        assert_eq!(j["error"]["code"], "TLS_REQUIRED", "{method} {uri}");
    }

    // 3. Spoofed X-Forwarded-Proto: https from untrusted IP -> 403 TLS_REQUIRED
    let spoofed_xfp = app
        .clone()
        .oneshot(with_peer(
            Request::builder()
                .uri("/api/v1/home/enrollment/pending")
                .method("GET")
                .header(header::AUTHORIZATION, &admin_auth)
                .header("X-Forwarded-Proto", "https")
                .body(Body::empty())
                .unwrap(),
            "203.0.113.55",
        ))
        .await
        .unwrap();
    assert_eq!(spoofed_xfp.status(), StatusCode::FORBIDDEN);

    // 4. Spoofed Forwarded: proto=https without ConnectInfo -> 403 TLS_REQUIRED
    let spoofed_fwd = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/home/enrollment/pending")
                .method("GET")
                .header(header::AUTHORIZATION, &admin_auth)
                .header("Forwarded", "for=192.0.2.1;proto=https")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(spoofed_fwd.status(), StatusCode::FORBIDDEN);

    // 5. Mixed hop X-Forwarded-Proto: http, https from trusted proxy -> 403 TLS_REQUIRED
    let mixed_hop = app
        .clone()
        .oneshot(with_peer(
            Request::builder()
                .uri("/api/v1/home/enrollment/pending")
                .method("GET")
                .header(header::AUTHORIZATION, &admin_auth)
                .header("X-Forwarded-Proto", "http, https")
                .body(Body::empty())
                .unwrap(),
            "127.0.0.1",
        ))
        .await
        .unwrap();
    assert_eq!(mixed_hop.status(), StatusCode::FORBIDDEN);

    // 6. Clean trusted proxy X-Forwarded-Proto: https from 127.0.0.1 -> 200 OK
    let trusted_ok = app
        .clone()
        .oneshot(with_peer(
            Request::builder()
                .uri("/api/v1/home/enrollment/pending")
                .method("GET")
                .header(header::AUTHORIZATION, &admin_auth)
                .header("X-Forwarded-Proto", "https")
                .body(Body::empty())
                .unwrap(),
            "127.0.0.1",
        ))
        .await
        .unwrap();
    assert_eq!(trusted_ok.status(), StatusCode::OK);

    // 7. michi_home_* bearer token on general /api/v1/tracks & /api/v1/playback/state
    //    MUST be rejected on cleartext HTTP (403 TLS_REQUIRED) and accepted on HTTPS (200 OK).
    let (sk, pk, mid) = gen_client_keypair();
    let home_id = state.home_root_authority.home_id();
    let membership = state.home_root_authority.issue_membership(
        &home_id,
        &mid,
        &pk,
        "mobile",
        vec![michi_identity::Role::MobilePlayer],
        &chrono::Utc::now().to_rfc3339(),
        2,
    );
    let home_token =
        establish_home_session_over_tls(&app, &state, &sk, &pk, &mid, membership).await;
    let home_auth = format!("Bearer {home_token}");

    for general_uri in ["/api/v1/tracks", "/api/v1/playback/state"] {
        let clear = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(general_uri)
                    .method("GET")
                    .header(header::AUTHORIZATION, &home_auth)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            clear.status(),
            StatusCode::FORBIDDEN,
            "michi_home_* MUST fail closed on cleartext {general_uri}"
        );
        assert_eq!(body_json(clear).await["error"]["code"], "TLS_REQUIRED");

        let https = app
            .clone()
            .oneshot(with_tls(
                Request::builder()
                    .uri(general_uri)
                    .method("GET")
                    .header(header::AUTHORIZATION, &home_auth)
                    .body(Body::empty())
                    .unwrap(),
            ))
            .await
            .unwrap();
        assert_eq!(https.status(), StatusCode::OK);
    }
}

#[tokio::test]
async fn test_r1_1_p0_b_x509_validation_ca_chained_vs_self_signed_expired_mismatch_san() {
    let fx = generate_openssl_fixtures();
    let now = chrono::Utc::now();

    // 1. CA-chained certificate with matching SAN + key -> CaChained (Android default compatible)
    let ok_ca = michi_api::validate_x509_for_home_tls(
        &fx.fullchain_pem,
        &fx.leaf_key_pem,
        "https://localhost:9443",
        now,
    )
    .expect("CA-chained cert must validate");
    assert_eq!(ok_ca.tls_trust_mode, TlsTrustMode::CaChained);
    assert_eq!(ok_ca.cert_chain_len, 2);
    assert!(ok_ca.tls_spki_sha256.starts_with("sha256/"));

    // 2. Self-signed certificate -> SelfSigned (NOT Android default compatible)
    let ok_self = michi_api::validate_x509_for_home_tls(
        &fx.self_signed_pem,
        &fx.self_signed_key_pem,
        "https://127.0.0.1:9443",
        now,
    )
    .expect("Self-signed cert parses cleanly but classified SelfSigned");
    assert_eq!(ok_self.tls_trust_mode, TlsTrustMode::SelfSigned);
    assert_eq!(ok_self.cert_chain_len, 1);

    // 3. Mismatched private key -> rejected
    let err_key = michi_api::validate_x509_for_home_tls(
        &fx.fullchain_pem,
        &fx.wrong_key_pem,
        "https://localhost:9443",
        now,
    );
    assert!(err_key.is_err(), "mismatched cert/key must be rejected");

    // 4. Wrong SAN hostname/IP -> rejected
    let err_san = michi_api::validate_x509_for_home_tls(
        &fx.fullchain_pem,
        &fx.leaf_key_pem,
        "https://wrong-host.example:9443",
        now,
    );
    assert!(err_san.is_err(), "wrong SAN must be rejected");

    // 5. Expired certificate -> rejected
    let err_exp = michi_api::validate_x509_for_home_tls(
        &fx.fullchain_pem,
        &fx.leaf_key_pem,
        "https://localhost:9443",
        ok_ca.not_after + chrono::Duration::seconds(60),
    );
    assert!(err_exp.is_err(), "expired cert must be rejected");

    // 6. Not-yet-valid certificate -> rejected
    let err_early = michi_api::validate_x509_for_home_tls(
        &fx.fullchain_pem,
        &fx.leaf_key_pem,
        "https://localhost:9443",
        ok_ca.not_before - chrono::Duration::seconds(60),
    );
    assert!(err_early.is_err(), "not-yet-valid cert must be rejected");

    // 7. /api/v1/aircat/status accurately reports tls_trust_mode & android_default_trust_compatible
    let (app, _pool, state, admin_token, _cfg_dir, _music_dir) = make_app_with_config(|_| {}).await;
    let admin_auth = format!("Bearer {admin_token}");
    *state.aircat_tls.write().await = Some(AirCatTlsInfo {
        pinned_base_url: "https://localhost:9443".into(),
        tls_spki_sha256: ok_self.tls_spki_sha256,
        cert_path: fx.dir.join("self.crt"),
        key_path: fx.dir.join("self.key"),
        tls_port: 9443,
        tls_trust_mode: TlsTrustMode::SelfSigned,
        cert_chain_len: 1,
        cert_not_before: Some(ok_self.not_before.to_rfc3339()),
        cert_not_after: Some(ok_self.not_after.to_rfc3339()),
    });
    state.set_aircat_tls_listener_ready(true);

    let st_self = body_json(
        app.clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/aircat/status")
                    .method("GET")
                    .header(header::AUTHORIZATION, &admin_auth)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(st_self["tls_trust_mode"], "self_signed");
    assert_eq!(st_self["android_default_trust_compatible"], false);

    enable_tls_ca_chained(&state).await;
    let st_ca = body_json(
        app.clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/aircat/status")
                    .method("GET")
                    .header(header::AUTHORIZATION, &admin_auth)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(st_ca["tls_trust_mode"], "ca_chained");
    assert_eq!(st_ca["android_default_trust_compatible"], true);
}

#[tokio::test]
async fn test_r1_1_p1_c_canonical_capability_parity_under_module_toggles() {
    let (app, _pool, state, _admin_token, _cfg_dir, _music_dir) =
        make_app_with_config(|_| {}).await;
    enable_tls_ca_chained(&state).await;

    let assert_parity =
        |info: &Value, caps: &Value, disc: &std::collections::BTreeMap<String, bool>| {
            let cap_enabled = |name: &str| -> bool {
                caps["features"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|f| f["name"].as_str() == Some(name))
                    .and_then(|f| f["enabled"].as_bool())
                    .unwrap_or_else(|| panic!("missing feature {name} in /api/v1/capabilities"))
            };
            for name in [
                "library",
                "playback",
                "queue",
                "sync",
                "sync_manifest",
                "multiroom",
                "transcoding",
                "home_tls_spki_pinned",
                "aircat_original_v1",
                "aircat_manifest_v1",
                "aircat_authorize_v1",
                "aircat_original_download_v1",
            ] {
                assert_eq!(
                    info["features"][name].as_bool().unwrap(),
                    cap_enabled(name),
                    "server/info vs /capabilities divergence on {name}"
                );
                if let Some(&disc_val) = disc.get(name) {
                    assert_eq!(
                        info["features"][name].as_bool().unwrap(),
                        disc_val,
                        "server/info vs Whisker discovery_features divergence on {name}"
                    );
                }
            }
        };

    // Disable playback, sync, rooms, and transcoding -> verify all surfaces drop coherently
    {
        let mut disabled = state.disabled_modules.write().await;
        disabled.insert("playback".into());
        disabled.insert("sync".into());
        disabled.insert("rooms".into());
        disabled.insert("transcoding".into());
    }

    let info = body_json(
        app.clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/server/info")
                    .method("GET")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
    )
    .await;
    let caps = body_json(
        app.clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/capabilities")
                    .method("GET")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
    )
    .await;
    let disc = michi_api::server_caps::ServerCapabilities::from_state(&state)
        .await
        .discovery_features();
    assert_parity(&info, &caps, &disc);
    assert_eq!(info["features"]["playback"], false);
    assert_eq!(info["features"]["queue"], false);
    assert_eq!(info["features"]["sync"], false);
    assert_eq!(info["features"]["sync_manifest"], false);
    assert_eq!(info["features"]["multiroom"], false);
    assert_eq!(info["features"]["transcoding"], false);
    assert_eq!(info["features"]["aircat_original_v1"], true);

    // Now disable library while TLS remains active -> home_tls_spki_pinned stays true,
    // but ALL 4 AirCat original-file producer flags MUST flip to false coherently!
    {
        let mut disabled = state.disabled_modules.write().await;
        disabled.insert("library".into());
    }
    let info2 = body_json(
        app.clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/server/info")
                    .method("GET")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
    )
    .await;
    let caps2 = body_json(
        app.clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/capabilities")
                    .method("GET")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
    )
    .await;
    let disc2 = michi_api::server_caps::ServerCapabilities::from_state(&state)
        .await
        .discovery_features();
    assert_parity(&info2, &caps2, &disc2);
    assert_eq!(info2["features"]["home_tls_spki_pinned"], true);
    assert_eq!(info2["features"]["aircat_original_v1"], false);
    assert_eq!(info2["features"]["aircat_manifest_v1"], false);
    assert_eq!(info2["features"]["aircat_authorize_v1"], false);
    assert_eq!(info2["features"]["aircat_original_download_v1"], false);
}

#[tokio::test]
async fn test_r1_1_m4_durable_enrollment_restart_atomic_serials_role_policy_and_revocation() {
    let (app, pool, state, admin_token, cfg_dir, music_dir) = make_app_with_config(|_| {}).await;
    enable_tls_ca_chained(&state).await;
    let admin_auth = format!("Bearer {admin_token}");
    let home_id = state.home_root_authority.home_id();

    // Helper to submit an enrollment request over TLS
    let submit_enrollment = |router: axum::Router,
                             sk: SigningKey,
                             pk: String,
                             mid: String,
                             name: String,
                             roles: Vec<michi_identity::Role>,
                             hid: String| async move {
        let ch_resp = router
            .clone()
            .oneshot(with_tls(
                Request::builder()
                    .uri("/api/v1/home/enrollment/challenge")
                    .method("POST")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&michi_identity::HomeEnrollmentChallengeRequest {
                            purpose: "home_enrollment".into(),
                            home_id: hid.clone(),
                            client_michi_id: mid.clone(),
                            client_public_key: pk.clone(),
                        })
                        .unwrap(),
                    ))
                    .unwrap(),
            ))
            .await
            .unwrap();
        assert_eq!(ch_resp.status(), StatusCode::OK);
        let ch: michi_identity::HomeEnrollmentChallengeResponse = serde_json::from_slice(
            &axum::body::to_bytes(ch_resp.into_body(), 64 * 1024)
                .await
                .unwrap(),
        )
        .unwrap();
        let pop = michi_identity::sign_home_enrollment_pop(
            &sk,
            &hid,
            &ch.server_michi_id,
            &mid,
            &pk,
            &ch.challenge_id.to_string(),
            &ch.challenge_nonce,
        );
        router
            .oneshot(with_tls(
                Request::builder()
                    .uri("/api/v1/home/enrollment/request")
                    .method("POST")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&michi_identity::HomeEnrollmentRequest {
                            home_id: hid,
                            challenge_id: ch.challenge_id,
                            client_michi_id: mid,
                            client_public_key: pk,
                            device_name: name,
                            device_type: "mobile".into(),
                            requested_roles: roles,
                            client_signature: pop,
                        })
                        .unwrap(),
                    ))
                    .unwrap(),
            ))
            .await
            .unwrap()
    };

    // 1. Role escalation: mobile requesting MusicServer -> 422 INVALID_ROLES
    let (esc_sk, esc_pk, esc_mid) = gen_client_keypair();
    let esc_resp = submit_enrollment(
        app.clone(),
        esc_sk,
        esc_pk,
        esc_mid,
        "Rogue Phone".into(),
        vec![michi_identity::Role::MusicServer],
        home_id.clone(),
    )
    .await;
    assert_eq!(esc_resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body_json(esc_resp).await["error"]["code"], "INVALID_ROLES");

    // 2. Valid mobile enrollment + duplicate request supersedes first pending request
    let (sk1, pk1, mid1) = gen_client_keypair();
    let r1_first = submit_enrollment(
        app.clone(),
        sk1.clone(),
        pk1.clone(),
        mid1.clone(),
        "Pixel 9".into(),
        vec![michi_identity::Role::MobilePlayer],
        home_id.clone(),
    )
    .await;
    assert_eq!(r1_first.status(), StatusCode::ACCEPTED);
    let r1_second = submit_enrollment(
        app.clone(),
        sk1.clone(),
        pk1.clone(),
        mid1.clone(),
        "Pixel 9".into(),
        vec![
            michi_identity::Role::MobilePlayer,
            michi_identity::Role::RemoteController,
        ],
        home_id.clone(),
    )
    .await;
    assert_eq!(r1_second.status(), StatusCode::ACCEPTED);
    let enr1: michi_identity::HomeEnrollmentRequestResponse = serde_json::from_slice(
        &axum::body::to_bytes(r1_second.into_body(), 64 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();

    // Verify claim_token plaintext is NEVER stored in SQLite
    let persisted_row = michi_db::get_home_enrollment_by_id_db(&pool, &enr1.request_id.to_string())
        .await
        .unwrap()
        .expect("enrollment row must be persisted in SQLite");
    assert_ne!(persisted_row.claim_token_hash, enr1.claim_token);
    assert_eq!(
        persisted_row.claim_token_hash,
        michi_api::auth::hash_token(&enr1.claim_token)
    );

    // 3. Simulate FULL server restart (fresh AppState, same SQLite DB + /config) -> pending survives!
    let identity2 = Arc::new(
        michi_identity::IdentityManager::load_or_generate(
            &cfg_dir,
            "Michi Micro Server",
            "interop-r1-1-password",
        )
        .unwrap(),
    );
    let state2 = AppState::new_with_identity(
        test_config(
            state.config.database_url.clone(),
            cfg_dir.clone(),
            music_dir.clone(),
        ),
        pool.clone(),
        None,
        identity2,
    );
    enable_tls_ca_chained(&state2).await;
    let app2 = create_router(state2.clone());

    let pend_after_restart = body_json(
        app2.clone()
            .oneshot(with_tls(
                Request::builder()
                    .uri("/api/v1/home/enrollment/pending")
                    .method("GET")
                    .header(header::AUTHORIZATION, &admin_auth)
                    .body(Body::empty())
                    .unwrap(),
            ))
            .await
            .unwrap(),
    )
    .await;
    // Duplicate superseded -> exactly 1 pending request survives restart
    assert_eq!(pend_after_restart["total"], 1);
    assert_eq!(
        pend_after_restart["requests"][0]["request_id"],
        enr1.request_id.to_string()
    );

    // 4. Enqueue second device and approve BOTH concurrently -> strictly distinct monotonic serials
    let (sk2, pk2, mid2) = gen_client_keypair();
    let r2_resp = submit_enrollment(
        app2.clone(),
        sk2,
        pk2,
        mid2,
        "Galaxy S25".into(),
        vec![michi_identity::Role::MobilePlayer],
        home_id.clone(),
    )
    .await;
    assert_eq!(r2_resp.status(), StatusCode::ACCEPTED);
    let enr2: michi_identity::HomeEnrollmentRequestResponse = serde_json::from_slice(
        &axum::body::to_bytes(r2_resp.into_body(), 64 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();

    let (ap1, ap2) = tokio::join!(
        app2.clone().oneshot(with_tls(
            Request::builder()
                .uri(format!(
                    "/api/v1/home/enrollment/{}/approve",
                    enr1.request_id
                ))
                .method("POST")
                .header(header::AUTHORIZATION, &admin_auth)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(b"{}".as_slice()))
                .unwrap(),
        )),
        app2.clone().oneshot(with_tls(
            Request::builder()
                .uri(format!(
                    "/api/v1/home/enrollment/{}/approve",
                    enr2.request_id
                ))
                .method("POST")
                .header(header::AUTHORIZATION, &admin_auth)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(b"{}".as_slice()))
                .unwrap(),
        )),
    );
    let ap1 = ap1.unwrap();
    let ap2 = ap2.unwrap();
    assert_eq!(ap1.status(), StatusCode::OK);
    let j1 = body_json(ap1).await;
    let s1 = j1["membership"]["serial"].as_u64().unwrap();
    assert_eq!(ap2.status(), StatusCode::OK);
    let j2 = body_json(ap2).await;
    let s2 = j2["membership"]["serial"].as_u64().unwrap();
    assert_ne!(s1, s2, "concurrent approvals MUST never collide on serial");
    assert!(s1 > 1 && s2 > 1);

    // 5. Double approval -> 409 ALREADY_RESOLVED
    let dup_approve = app2
        .clone()
        .oneshot(with_tls(
            Request::builder()
                .uri(format!(
                    "/api/v1/home/enrollment/{}/approve",
                    enr1.request_id
                ))
                .method("POST")
                .header(header::AUTHORIZATION, &admin_auth)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(b"{}".as_slice()))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(dup_approve.status(), StatusCode::CONFLICT);
    assert_eq!(
        body_json(dup_approve).await["error"]["code"],
        "ALREADY_RESOLVED"
    );

    // 6. Second simulated restart -> client claims approved membership via X-Michi-Enrollment-Claim!
    let identity3 = Arc::new(
        michi_identity::IdentityManager::load_or_generate(
            &cfg_dir,
            "Michi Micro Server",
            "interop-r1-1-password",
        )
        .unwrap(),
    );
    let state3 = AppState::new_with_identity(
        test_config(state.config.database_url.clone(), cfg_dir, music_dir),
        pool,
        None,
        identity3,
    );
    enable_tls_ca_chained(&state3).await;
    let app3 = create_router(state3.clone());

    let claimed = app3
        .clone()
        .oneshot(with_tls(
            Request::builder()
                .uri(format!(
                    "/api/v1/home/enrollment/{}/status",
                    enr1.request_id
                ))
                .method("GET")
                .header("X-Michi-Enrollment-Claim", &enr1.claim_token)
                .body(Body::empty())
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(claimed.status(), StatusCode::OK);
    let claimed_body: michi_identity::HomeEnrollmentStatusResponse = serde_json::from_slice(
        &axum::body::to_bytes(claimed.into_body(), 64 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        claimed_body.status,
        michi_identity::HomeEnrollmentStatus::Approved
    );
    assert!(claimed_body.membership.is_some());

    // 7. Revoking a device while enrollment is pending immediately rejects its enrollment
    let (sk3, pk3, mid3) = gen_client_keypair();
    let r3 = submit_enrollment(
        app3.clone(),
        sk3,
        pk3,
        mid3.clone(),
        "Lost Tablet".into(),
        vec![michi_identity::Role::MobilePlayer],
        home_id,
    )
    .await;
    assert_eq!(r3.status(), StatusCode::ACCEPTED);
    let enr3: michi_identity::HomeEnrollmentRequestResponse = serde_json::from_slice(
        &axum::body::to_bytes(r3.into_body(), 64 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();

    let revoke = app3
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/home/revoke")
                .method("POST")
                .header(header::AUTHORIZATION, &admin_auth)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({
                        "device_michi_id": mid3,
                        "reason": "revoked_while_pending"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(revoke.status(), StatusCode::OK);

    let approve_revoked = app3
        .clone()
        .oneshot(with_tls(
            Request::builder()
                .uri(format!(
                    "/api/v1/home/enrollment/{}/approve",
                    enr3.request_id
                ))
                .method("POST")
                .header(header::AUTHORIZATION, &admin_auth)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(b"{}".as_slice()))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert!(matches!(
        approve_revoked.status(),
        StatusCode::CONFLICT | StatusCode::FORBIDDEN
    ));
}

#[tokio::test]
async fn test_r1_1_m5_single_home_session_authority_not_in_legacy_token_store() {
    let (app, _pool, state, _admin_token, _cfg_dir, _music_dir) =
        make_app_with_config(|_| {}).await;
    enable_tls_ca_chained(&state).await;

    let (sk, pk, mid) = gen_client_keypair();
    let home_id = state.home_root_authority.home_id();
    let membership = state.home_root_authority.issue_membership(
        &home_id,
        &mid,
        &pk,
        "mobile",
        vec![michi_identity::Role::MobilePlayer],
        &chrono::Utc::now().to_rfc3339(),
        7,
    );
    let token = establish_home_session_over_tls(&app, &state, &sk, &pk, &mid, membership).await;
    assert!(token.starts_with("michi_home_"));

    // R1.1-M5 invariant: michi_home_* MUST NEVER be duplicated in legacy TokenStore!
    assert!(
        state
            .token_store
            .validate(&token, michi_link::TokenType::Device)
            .await
            .is_err(),
        "michi_home_* session token MUST NOT exist in legacy TokenStore"
    );

    // Must still authenticate /api/v1/tracks over HTTPS via aircat_security.home_sessions
    let ok = app
        .clone()
        .oneshot(with_tls(
            Request::builder()
                .uri("/api/v1/tracks")
                .method("GET")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(ok.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_r1_1_p1_d_revocation_fail_closed_on_sqlite_store_failure() {
    let (app, pool, state, admin_token, _cfg_dir, _music_dir) = make_app_with_config(|_| {}).await;
    enable_tls_ca_chained(&state).await;
    let admin_auth = format!("Bearer {admin_token}");

    let (sk, pk, mid) = gen_client_keypair();
    let home_id = state.home_root_authority.home_id();
    let membership = state.home_root_authority.issue_membership(
        &home_id,
        &mid,
        &pk,
        "mobile",
        vec![michi_identity::Role::MobilePlayer],
        &chrono::Utc::now().to_rfc3339(),
        8,
    );
    let token = establish_home_session_over_tls(&app, &state, &sk, &pk, &mid, membership).await;
    let home_auth = format!("Bearer {token}");

    // Baseline: Active -> 200 OK
    assert_eq!(
        check_home_member_authorization(&state, &home_id, &mid).await,
        HomeMemberAuthorizationState::Active
    );
    let ok = app
        .clone()
        .oneshot(with_tls(
            Request::builder()
                .uri("/api/v1/aircat/manifest")
                .method("GET")
                .header(header::AUTHORIZATION, &home_auth)
                .body(Body::empty())
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(ok.status(), StatusCode::OK);

    // Simulate SQLite revocation store failure by dropping home_revocations in this isolated DB
    sqlx::query("DROP TABLE home_revocations")
        .execute(&pool)
        .await
        .unwrap();

    // Must return Indeterminate, NEVER Active!
    assert_eq!(
        check_home_member_authorization(&state, &home_id, &mid).await,
        HomeMemberAuthorizationState::Indeterminate
    );

    // AirCat manifest, general /tracks with michi_home_*, /auth/challenge, /enrollment/challenge,
    // and /home/roster MUST ALL fail closed with 503 REVOCATION_STORE_UNAVAILABLE!
    let manifest_fail = app
        .clone()
        .oneshot(with_tls(
            Request::builder()
                .uri("/api/v1/aircat/manifest")
                .method("GET")
                .header(header::AUTHORIZATION, &home_auth)
                .body(Body::empty())
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(manifest_fail.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        body_json(manifest_fail).await["error"]["code"],
        "REVOCATION_STORE_UNAVAILABLE"
    );

    let tracks_fail = app
        .clone()
        .oneshot(with_tls(
            Request::builder()
                .uri("/api/v1/tracks")
                .method("GET")
                .header(header::AUTHORIZATION, &home_auth)
                .body(Body::empty())
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(tracks_fail.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        body_json(tracks_fail).await["error"]["code"],
        "REVOCATION_STORE_UNAVAILABLE"
    );

    let auth_ch_fail = app
        .clone()
        .oneshot(with_tls(
            Request::builder()
                .uri("/api/v1/auth/challenge")
                .method("POST")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&michi_identity::DeviceAuthChallengeRequest {
                        client_michi_id: mid.clone(),
                        client_public_key: pk.clone(),
                        home_id: home_id.clone(),
                    })
                    .unwrap(),
                ))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(auth_ch_fail.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        body_json(auth_ch_fail).await["error"]["code"],
        "REVOCATION_STORE_UNAVAILABLE"
    );

    let roster_fail = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/home/roster")
                .method("GET")
                .header(header::AUTHORIZATION, &admin_auth)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(roster_fail.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn test_r1_1_real_tcp_http_vs_real_rustls_https_full_mobile_lifecycle() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let fx = generate_openssl_fixtures();
    let verified = michi_api::validate_x509_for_home_tls(
        &fx.fullchain_pem,
        &fx.leaf_key_pem,
        "https://localhost:9443",
        chrono::Utc::now(),
    )
    .unwrap();
    assert_eq!(verified.tls_trust_mode, TlsTrustMode::CaChained);

    let certs: Vec<rustls::pki_types::CertificateDer<'static>> =
        rustls_pemfile::certs(&mut std::io::Cursor::new(&fx.fullchain_pem))
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
    let key = rustls_pemfile::private_key(&mut std::io::Cursor::new(&fx.leaf_key_pem))
        .unwrap()
        .unwrap();
    let server_tls_cfg = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .unwrap();
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(server_tls_cfg));

    let (app, _pool, state, admin_token, _cfg_dir, _music_dir) = make_app_with_config(|_| {}).await;

    // Bind BOTH a real plain TCP HTTP listener AND a real tokio-rustls HTTPS listener on the SAME router
    let http_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let http_addr = http_listener.local_addr().unwrap();
    let https_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let https_addr = https_listener.local_addr().unwrap();

    *state.aircat_tls.write().await = Some(AirCatTlsInfo {
        pinned_base_url: format!("https://localhost:{}", https_addr.port()),
        tls_spki_sha256: verified.tls_spki_sha256.clone(),
        cert_path: fx.dir.join("leaf.crt"),
        key_path: fx.dir.join("leaf.key"),
        tls_port: https_addr.port(),
        tls_trust_mode: TlsTrustMode::CaChained,
        cert_chain_len: 2,
        cert_not_before: Some(verified.not_before.to_rfc3339()),
        cert_not_after: Some(verified.not_after.to_rfc3339()),
    });
    state.set_aircat_tls_listener_ready(true);

    let http_app = app.clone();
    let http_task = tokio::spawn(async move {
        let _ = axum::serve(
            http_listener,
            http_app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await;
    });

    let https_app = app.clone();
    let https_task = tokio::spawn(async move {
        loop {
            let Ok((tcp, peer)) = https_listener.accept().await else {
                break;
            };
            let acceptor = acceptor.clone();
            let app = https_app.clone();
            tokio::spawn(async move {
                let Ok(tls_stream) = acceptor.accept(tcp).await else {
                    return;
                };
                let io = hyper_util::rt::TokioIo::new(tls_stream);
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

    // Build real reqwest HTTP client AND real reqwest HTTPS client trusting ONLY our test CA
    let ca_cert = reqwest::Certificate::from_pem(&fx.ca_cert_pem).unwrap();
    let https_client = reqwest::Client::builder()
        .add_root_certificate(ca_cert)
        .resolve("localhost", https_addr)
        .build()
        .unwrap();
    let http_client = reqwest::Client::new();

    let (sk, pk, mid) = gen_client_keypair();
    let home_id = state.home_root_authority.home_id();

    // 1. Over real TCP HTTP -> enrollment & auth MUST be rejected with 403 TLS_REQUIRED (even with spoofed XFP!)
    let clear_enr = http_client
        .post(format!(
            "http://{http_addr}/api/v1/home/enrollment/challenge"
        ))
        .header("X-Forwarded-Proto", "https")
        .json(&michi_identity::HomeEnrollmentChallengeRequest {
            purpose: "home_enrollment".into(),
            home_id: home_id.clone(),
            client_michi_id: mid.clone(),
            client_public_key: pk.clone(),
        })
        .send()
        .await
        .unwrap();
    assert_eq!(clear_enr.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        clear_enr.json::<Value>().await.unwrap()["error"]["code"],
        "TLS_REQUIRED"
    );

    let clear_auth = http_client
        .post(format!("http://{http_addr}/api/v1/auth/challenge"))
        .json(&michi_identity::DeviceAuthChallengeRequest {
            client_michi_id: mid.clone(),
            client_public_key: pk.clone(),
            home_id: home_id.clone(),
        })
        .send()
        .await
        .unwrap();
    assert_eq!(clear_auth.status(), StatusCode::FORBIDDEN);

    // 2. Over real tokio-rustls HTTPS (CA-verified!) -> complete Mobile lifecycle succeeds end-to-end!
    let base_https = format!("https://localhost:{}", https_addr.port());
    let ch: michi_identity::HomeEnrollmentChallengeResponse = https_client
        .post(format!("{base_https}/api/v1/home/enrollment/challenge"))
        .json(&michi_identity::HomeEnrollmentChallengeRequest {
            purpose: "home_enrollment".into(),
            home_id: home_id.clone(),
            client_michi_id: mid.clone(),
            client_public_key: pk.clone(),
        })
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();

    let pop = michi_identity::sign_home_enrollment_pop(
        &sk,
        &home_id,
        &ch.server_michi_id,
        &mid,
        &pk,
        &ch.challenge_id.to_string(),
        &ch.challenge_nonce,
    );
    let enr_req: michi_identity::HomeEnrollmentRequestResponse = https_client
        .post(format!("{base_https}/api/v1/home/enrollment/request"))
        .json(&michi_identity::HomeEnrollmentRequest {
            home_id: home_id.clone(),
            challenge_id: ch.challenge_id,
            client_michi_id: mid.clone(),
            client_public_key: pk.clone(),
            device_name: "Real TLS Pixel".into(),
            device_type: "mobile".into(),
            requested_roles: vec![michi_identity::Role::MobilePlayer],
            client_signature: pop,
        })
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();

    // Admin approves over real HTTPS
    https_client
        .post(format!(
            "{base_https}/api/v1/home/enrollment/{}/approve",
            enr_req.request_id
        ))
        .bearer_auth(&admin_token)
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();

    // Mobile claims approved membership over real HTTPS
    let status: michi_identity::HomeEnrollmentStatusResponse = https_client
        .get(format!(
            "{base_https}/api/v1/home/enrollment/{}/status",
            enr_req.request_id
        ))
        .header("X-Michi-Enrollment-Claim", &enr_req.claim_token)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let membership = status.membership.unwrap();
    let root_pk = status.home_root_public_key.unwrap();

    // Mobile performs mutual auth over real HTTPS
    let auth_ch: michi_identity::DeviceAuthChallengeResponse = https_client
        .post(format!("{base_https}/api/v1/auth/challenge"))
        .json(&michi_identity::DeviceAuthChallengeRequest {
            client_michi_id: mid.clone(),
            client_public_key: pk.clone(),
            home_id: home_id.clone(),
        })
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let sig = michi_identity::sign_device_auth_challenge(
        &sk,
        &home_id,
        &auth_ch.server_michi_id,
        &mid,
        &auth_ch.challenge_id.to_string(),
        &auth_ch.challenge_nonce,
    );
    let sess: michi_identity::DeviceAuthSessionResponse = https_client
        .post(format!("{base_https}/api/v1/auth/session"))
        .json(&michi_identity::DeviceAuthSessionRequest {
            challenge_id: auth_ch.challenge_id,
            client_michi_id: mid.clone(),
            membership,
            client_signature: sig,
        })
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    michi_identity::verify_server_auth_session(
        &sess,
        &root_pk,
        &home_id,
        &mid,
        &auth_ch.challenge_id.to_string(),
        &[],
    )
    .unwrap();

    // Mobile accesses /api/v1/tracks and /api/v1/aircat/manifest over real HTTPS (200 OK),
    // and is REJECTED on cleartext HTTP with the SAME michi_home_* token (403 TLS_REQUIRED)!
    let tracks_https = https_client
        .get(format!("{base_https}/api/v1/tracks"))
        .bearer_auth(&sess.session_token)
        .header("X-Michi-Device-Id", &mid)
        .send()
        .await
        .unwrap();
    assert_eq!(tracks_https.status(), StatusCode::OK);

    let manifest_https = https_client
        .get(format!("{base_https}/api/v1/aircat/manifest"))
        .bearer_auth(&sess.session_token)
        .header("X-Michi-Device-Id", &mid)
        .send()
        .await
        .unwrap();
    assert_eq!(manifest_https.status(), StatusCode::OK);

    let tracks_http = http_client
        .get(format!("http://{http_addr}/api/v1/tracks"))
        .bearer_auth(&sess.session_token)
        .send()
        .await
        .unwrap();
    assert_eq!(tracks_http.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        tracks_http.json::<Value>().await.unwrap()["error"]["code"],
        "TLS_REQUIRED"
    );

    http_task.abort();
    https_task.abort();
}
