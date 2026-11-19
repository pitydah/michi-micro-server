use axum::{
    body::Body,
    http::{header, Request, StatusCode},
};
use ed25519_dalek::SigningKey;
use michi_api::create_router;
use michi_config::Config;
use michi_core::{AudioFormat, Track};
use serde_json::Value;
use sqlx::SqlitePool;
use std::io::Write;
use std::path::PathBuf;
use tower::ServiceExt;
use uuid::Uuid;

async fn r1_test_db() -> (SqlitePool, String) {
    let url = format!(
        "sqlite:file:memdb_interop_r1_{}?mode=memory&cache=shared",
        Uuid::new_v4()
    );
    let pool = michi_db::init_pool(&url).await.unwrap();
    (pool, url)
}

fn r1_test_config(db_url: String, config_dir: PathBuf, music_dir: PathBuf) -> Config {
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
        sync_name: "michi-server-interop-r1".to_string(),
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

async fn make_r1_app() -> (
    axum::Router,
    SqlitePool,
    michi_api::AppState,
    String,
    PathBuf,
    PathBuf,
) {
    let (pool, db_url) = r1_test_db().await;
    let root_tmp = std::env::temp_dir().join(format!("michi-interop-r1-{}", Uuid::new_v4()));
    let config_dir = root_tmp.join("config");
    let music_dir = root_tmp.join("music");
    let config = r1_test_config(db_url, config_dir.clone(), music_dir.clone());

    let identity = std::sync::Arc::new(
        michi_identity::IdentityManager::load_or_generate(
            &config.config_path,
            "Michi Micro Server",
            "interop-r1-password",
        )
        .unwrap(),
    );
    let admin_id = Uuid::new_v4();
    michi_db::create_user(&pool, &admin_id, "admin-r1", "password", true)
        .await
        .unwrap();
    let state =
        michi_api::AppState::new_with_identity(config, pool.clone(), Some(admin_id), identity);
    let admin_token = state.auth_sessions.create_session(admin_id).await.unwrap();
    let app = create_router(state.clone());
    (app, pool, state, admin_token, config_dir, music_dir)
}

fn with_tls(mut req: Request<Body>) -> Request<Body> {
    req.extensions_mut()
        .insert(michi_api::AirCatTlsVerifiedConnection);
    req
}

async fn enable_tls(state: &michi_api::AppState) {
    *state.aircat_tls.write().await = Some(michi_api::AirCatTlsInfo {
        pinned_base_url: "https://127.0.0.1:9443".into(),
        tls_spki_sha256: "sha256/AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".into(),
        cert_path: PathBuf::from("/tmp/r1-tls.crt"),
        key_path: PathBuf::from("/tmp/r1-tls.key"),
        tls_port: 9443,
        tls_trust_mode: michi_api::TlsTrustMode::CaChained,
        certificate_profile: michi_api::TlsCertificateProfile::CaIssuedChainPresent,
        chain_cryptographically_verified: true,
        android_trust_verification_status: michi_api::AndroidTrustVerificationStatus::NotVerified,
        cert_chain_len: 2,
        cert_not_before: Some("2026-01-01T00:00:00Z".into()),
        cert_not_after: Some("2027-01-01T00:00:00Z".into()),
    });
    state.set_aircat_tls_listener_ready(true);
}

#[tokio::test]
async fn test_r1_canonical_identity_coherence_across_http_whisker_and_restart() {
    let (app, pool, state, _admin_token, config_dir, music_dir) = make_r1_app().await;

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/server/info")
                .method("GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let raw_str = String::from_utf8_lossy(&bytes);
    assert!(
        !raw_str.contains("private_key") && !raw_str.contains("secret_key"),
        "server/info must never expose private keys"
    );

    let info: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(info["identity_scheme"], "ed25519-blake3-v1");
    let http_michi_id = info["michi_id"].as_str().unwrap();
    let http_public_key = info["public_key"].as_str().unwrap();
    let http_home_id = info["michi_home_id"].as_str().unwrap();
    let http_server_id = info["server_id"].as_str().unwrap();

    assert_eq!(http_michi_id.len(), 43);
    assert_eq!(http_public_key.len(), 43);
    assert_eq!(http_home_id.len(), 43);
    assert_ne!(http_michi_id, http_server_id);
    assert!(Uuid::parse_str(http_server_id).is_ok());

    // Verify BLAKE3(public_key) == michi_id
    let derived = michi_identity::IdentityManager::derive_michi_id(http_public_key).unwrap();
    assert_eq!(derived.to_base64url(), http_michi_id);

    // Verify Whisker signed announce coherence
    let connect = michi_connect::MichiConnect::new(state.identity.clone(), 9091, None);
    connect.set_home_id(state.home_root_authority.home_id());
    let announce = connect.build_signed_announce(9091, "192.168.1.50").unwrap();
    assert_eq!(announce.michi_id.as_deref(), Some(http_michi_id));
    assert_eq!(announce.public_key.as_deref(), Some(http_public_key));
    assert_eq!(announce.michi_home_id.as_deref(), Some(http_home_id));

    let verifier = michi_identity::DiscoveryEngine::new(state.identity.clone());
    assert!(verifier
        .verify_announce(&announce, Some("192.168.1.50:53318".parse().unwrap()))
        .is_ok());

    // Simulate server restart using the exact same /config directory -> identity MUST persist
    let reloaded_identity = std::sync::Arc::new(
        michi_identity::IdentityManager::load_or_generate(
            &config_dir,
            "Michi Micro Server",
            "interop-r1-password",
        )
        .unwrap(),
    );
    let restarted_cfg = r1_test_config(
        format!(
            "sqlite:file:memdb_interop_r1_restart_{}?mode=memory&cache=shared",
            Uuid::new_v4()
        ),
        config_dir,
        music_dir,
    );
    let restarted_state =
        michi_api::AppState::new_with_identity(restarted_cfg, pool, None, reloaded_identity);
    let restarted_app = create_router(restarted_state);
    let resp2 = restarted_app
        .oneshot(
            Request::builder()
                .uri("/api/v1/server/info")
                .method("GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let info2: Value = serde_json::from_slice(
        &axum::body::to_bytes(resp2.into_body(), 1024 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(info2["michi_id"], http_michi_id);
    assert_eq!(info2["public_key"], http_public_key);
    assert_eq!(info2["michi_home_id"], http_home_id);
}

#[tokio::test]
async fn test_r1_capability_normalization_across_server_info_capabilities_and_whisker() {
    let (app, _pool, state, _admin_token, _config_dir, _music_dir) = make_r1_app().await;

    // 1. Without TLS active -> all 5 AirCat flags MUST be false everywhere
    let info_off: Value = serde_json::from_slice(
        &axum::body::to_bytes(
            app.clone()
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
            1024 * 1024,
        )
        .await
        .unwrap(),
    )
    .unwrap();
    let caps_off: Value = serde_json::from_slice(
        &axum::body::to_bytes(
            app.clone()
                .oneshot(
                    Request::builder()
                        .uri("/api/v1/capabilities")
                        .method("GET")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap()
                .into_body(),
            1024 * 1024,
        )
        .await
        .unwrap(),
    )
    .unwrap();
    let disc_off = michi_api::server_caps::ServerCapabilities::from_state(&state)
        .await
        .discovery_features();

    let cap_enabled = |caps_doc: &Value, name: &str| -> Option<bool> {
        caps_doc["features"]
            .as_array()?
            .iter()
            .find(|f| f["name"].as_str() == Some(name))
            .and_then(|f| f["enabled"].as_bool())
    };

    assert_eq!(info_off["features"]["sync"], true);
    assert_eq!(info_off["features"]["sync_manifest"], true);
    for cap in michi_identity::AIRCAT_REQUIRED_CAPABILITIES {
        assert_eq!(
            info_off["features"][*cap], false,
            "server/info.{cap} must be false when TLS is inactive"
        );
        assert_eq!(
            cap_enabled(&caps_off, cap),
            Some(false),
            "capabilities.{cap} must be false when TLS is inactive"
        );
    }
    assert_eq!(disc_off.get("aircat_original_v1"), Some(&false));
    assert_eq!(disc_off.get("home_tls_spki_pinned"), Some(&false));

    // 2. TLS configured ONLY in config without listener ready -> STILL false!
    *state.aircat_tls.write().await = Some(michi_api::AirCatTlsInfo {
        pinned_base_url: "https://127.0.0.1:9443".into(),
        tls_spki_sha256: "sha256/AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".into(),
        cert_path: PathBuf::from("/tmp/c.crt"),
        key_path: PathBuf::from("/tmp/c.key"),
        tls_port: 9443,
        tls_trust_mode: michi_api::TlsTrustMode::CaChained,
        certificate_profile: michi_api::TlsCertificateProfile::CaIssuedChainPresent,
        chain_cryptographically_verified: true,
        android_trust_verification_status: michi_api::AndroidTrustVerificationStatus::NotVerified,
        cert_chain_len: 2,
        cert_not_before: None,
        cert_not_after: None,
    });
    let caps_unready = michi_api::server_caps::ServerCapabilities::from_state(&state).await;
    assert!(!caps_unready.feature_enabled("aircat_original_v1"));

    // 3. Mark TLS listener genuinely ready -> all 5 AirCat flags flip to true coherently
    state.set_aircat_tls_listener_ready(true);
    let info_on: Value = serde_json::from_slice(
        &axum::body::to_bytes(
            app.clone()
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
            1024 * 1024,
        )
        .await
        .unwrap(),
    )
    .unwrap();
    let caps_on: Value = serde_json::from_slice(
        &axum::body::to_bytes(
            app.clone()
                .oneshot(
                    Request::builder()
                        .uri("/api/v1/capabilities")
                        .method("GET")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap()
                .into_body(),
            1024 * 1024,
        )
        .await
        .unwrap(),
    )
    .unwrap();
    let disc_on = michi_api::server_caps::ServerCapabilities::from_state(&state)
        .await
        .discovery_features();

    for cap in michi_identity::AIRCAT_REQUIRED_CAPABILITIES {
        assert_eq!(info_on["features"][*cap], true);
        assert_eq!(cap_enabled(&caps_on, cap), Some(true));
    }
    assert_eq!(disc_on.get("aircat_original_v1"), Some(&true));
    assert_eq!(disc_on.get("home_tls_spki_pinned"), Some(&true));
}

#[tokio::test]
async fn test_r1_home_membership_bootstrap_approval_renewal_and_revocation() {
    let (app, pool, state, admin_token, _config_dir, _music_dir) = make_r1_app().await;
    enable_tls(&state).await;
    let admin_auth = format!("Bearer {admin_token}");

    let non_admin_id = Uuid::new_v4();
    michi_db::create_user(&pool, &non_admin_id, "member-user", "password", false)
        .await
        .unwrap();
    let non_admin_token = state
        .auth_sessions
        .create_session(non_admin_id)
        .await
        .unwrap();
    let non_admin_auth = format!("Bearer {non_admin_token}");

    // Client keypair (Michi Mobile)
    let client_sk = SigningKey::generate(&mut rand::rngs::OsRng);
    let client_vk = client_sk.verifying_key();
    let client_pk = michi_identity::encode_base64url(&client_vk.to_bytes());
    let client_michi_id = michi_identity::MichiId::from_public_key(&client_vk).to_base64url();
    let home_id = state.home_root_authority.home_id();

    // Step 1: Request enrollment challenge (HTTPS required by P0-A)
    let ch_resp = app
        .clone()
        .oneshot(with_tls(
            Request::builder()
                .uri("/api/v1/home/enrollment/challenge")
                .method("POST")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&michi_identity::HomeEnrollmentChallengeRequest {
                        purpose: "home_enrollment".into(),
                        home_id: home_id.clone(),
                        client_michi_id: client_michi_id.clone(),
                        client_public_key: client_pk.clone(),
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

    // Step 2: Tampered PoP signature must fail (and consumes challenge)
    let bad_req = app
        .clone()
        .oneshot(with_tls(
            Request::builder()
                .uri("/api/v1/home/enrollment/request")
                .method("POST")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&michi_identity::HomeEnrollmentRequest {
                        home_id: home_id.clone(),
                        challenge_id: ch.challenge_id,
                        client_michi_id: client_michi_id.clone(),
                        client_public_key: client_pk.clone(),
                        device_name: "Pixel 9".into(),
                        device_type: "mobile".into(),
                        requested_roles: vec![michi_identity::Role::MobilePlayer],
                        client_signature: "A".repeat(86),
                    })
                    .unwrap(),
                ))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(bad_req.status(), StatusCode::FORBIDDEN);

    // Request a fresh challenge and sign valid Ed25519 PoP
    let ch2_resp = app
        .clone()
        .oneshot(with_tls(
            Request::builder()
                .uri("/api/v1/home/enrollment/challenge")
                .method("POST")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&michi_identity::HomeEnrollmentChallengeRequest {
                        purpose: "home_enrollment".into(),
                        home_id: home_id.clone(),
                        client_michi_id: client_michi_id.clone(),
                        client_public_key: client_pk.clone(),
                    })
                    .unwrap(),
                ))
                .unwrap(),
        ))
        .await
        .unwrap();
    let ch2: michi_identity::HomeEnrollmentChallengeResponse = serde_json::from_slice(
        &axum::body::to_bytes(ch2_resp.into_body(), 64 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();

    let pop_sig = michi_identity::sign_home_enrollment_pop(
        &client_sk,
        &home_id,
        &ch2.server_michi_id,
        &client_michi_id,
        &client_pk,
        &ch2.challenge_id.to_string(),
        &ch2.challenge_nonce,
    );

    let req_resp = app
        .clone()
        .oneshot(with_tls(
            Request::builder()
                .uri("/api/v1/home/enrollment/request")
                .method("POST")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&michi_identity::HomeEnrollmentRequest {
                        home_id: home_id.clone(),
                        challenge_id: ch2.challenge_id,
                        client_michi_id: client_michi_id.clone(),
                        client_public_key: client_pk.clone(),
                        device_name: "Pixel 9".into(),
                        device_type: "mobile".into(),
                        requested_roles: vec![
                            michi_identity::Role::MobilePlayer,
                            michi_identity::Role::RemoteController,
                        ],
                        client_signature: pop_sig,
                    })
                    .unwrap(),
                ))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(req_resp.status(), StatusCode::ACCEPTED);
    let enrolled: michi_identity::HomeEnrollmentRequestResponse = serde_json::from_slice(
        &axum::body::to_bytes(req_resp.into_body(), 64 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        enrolled.status,
        michi_identity::HomeEnrollmentStatus::Pending
    );

    // Step 3: Non-admin user MUST NOT list pending or approve
    let non_admin_pending = app
        .clone()
        .oneshot(with_tls(
            Request::builder()
                .uri("/api/v1/home/enrollment/pending")
                .method("GET")
                .header(header::AUTHORIZATION, &non_admin_auth)
                .body(Body::empty())
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(non_admin_pending.status(), StatusCode::FORBIDDEN);

    let non_admin_approve = app
        .clone()
        .oneshot(with_tls(
            Request::builder()
                .uri(format!(
                    "/api/v1/home/enrollment/{}/approve",
                    enrolled.request_id
                ))
                .method("POST")
                .header(header::AUTHORIZATION, &non_admin_auth)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(b"{}".as_slice()))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(non_admin_approve.status(), StatusCode::FORBIDDEN);

    // Claim token alone cannot access protected routes
    let claim_as_bearer = app
        .clone()
        .oneshot(with_tls(
            Request::builder()
                .uri("/api/v1/aircat/manifest")
                .method("GET")
                .header(
                    header::AUTHORIZATION,
                    format!("Bearer {}", enrolled.claim_token),
                )
                .body(Body::empty())
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(claim_as_bearer.status(), StatusCode::UNAUTHORIZED);

    // Wrong claim token on /status is rejected
    let wrong_claim = app
        .clone()
        .oneshot(with_tls(
            Request::builder()
                .uri(format!(
                    "/api/v1/home/enrollment/{}/status",
                    enrolled.request_id
                ))
                .method("GET")
                .header("X-Michi-Enrollment-Claim", "wrong_claim_token")
                .body(Body::empty())
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(wrong_claim.status(), StatusCode::FORBIDDEN);

    // Valid claim token while pending -> pending & membership = None
    let pending_status_resp = app
        .clone()
        .oneshot(with_tls(
            Request::builder()
                .uri(format!(
                    "/api/v1/home/enrollment/{}/status",
                    enrolled.request_id
                ))
                .method("GET")
                .header("X-Michi-Enrollment-Claim", &enrolled.claim_token)
                .body(Body::empty())
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(pending_status_resp.status(), StatusCode::OK);
    let pending_status: michi_identity::HomeEnrollmentStatusResponse = serde_json::from_slice(
        &axum::body::to_bytes(pending_status_resp.into_body(), 64 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        pending_status.status,
        michi_identity::HomeEnrollmentStatus::Pending
    );
    assert!(pending_status.membership.is_none());

    // Step 4: Admin lists pending and explicitly approves
    let pending_list = app
        .clone()
        .oneshot(with_tls(
            Request::builder()
                .uri("/api/v1/home/enrollment/pending")
                .method("GET")
                .header(header::AUTHORIZATION, &admin_auth)
                .body(Body::empty())
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(pending_list.status(), StatusCode::OK);
    let pending_json: Value = serde_json::from_slice(
        &axum::body::to_bytes(pending_list.into_body(), 64 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(pending_json["total"], 1);

    let approve_resp = app
        .clone()
        .oneshot(with_tls(
            Request::builder()
                .uri(format!(
                    "/api/v1/home/enrollment/{}/approve",
                    enrolled.request_id
                ))
                .method("POST")
                .header(header::AUTHORIZATION, &admin_auth)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(b"{}".as_slice()))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(approve_resp.status(), StatusCode::OK);

    // Step 5: Client polls /status with claim token and receives signed MichiMembership
    let approved_status_resp = app
        .clone()
        .oneshot(with_tls(
            Request::builder()
                .uri(format!(
                    "/api/v1/home/enrollment/{}/status",
                    enrolled.request_id
                ))
                .method("GET")
                .header("X-Michi-Enrollment-Claim", &enrolled.claim_token)
                .body(Body::empty())
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(approved_status_resp.status(), StatusCode::OK);
    let approved: michi_identity::HomeEnrollmentStatusResponse = serde_json::from_slice(
        &axum::body::to_bytes(approved_status_resp.into_body(), 64 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        approved.status,
        michi_identity::HomeEnrollmentStatus::Approved
    );
    let membership = approved.membership.expect("approved membership missing");
    let root_pk = approved
        .home_root_public_key
        .expect("root public key missing");
    michi_identity::verify_membership(&membership, &root_pk, &home_id, &[]).unwrap();

    // Enrolled mobile device appears in /api/v1/home/roster
    let roster_resp = app
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
    let roster: Value = serde_json::from_slice(
        &axum::body::to_bytes(roster_resp.into_body(), 64 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();
    assert!(roster["devices"]
        .as_array()
        .unwrap()
        .iter()
        .any(|d| d["device_michi_id"] == client_michi_id && d["revoked"] == false));

    // Step 6: Mutual Auth + Renewal (2 consecutive challenge/session handshakes over HTTPS)
    let mut latest_session_token = String::new();
    for _ in 0..2 {
        let auth_ch_resp = app
            .clone()
            .oneshot(with_tls(
                Request::builder()
                    .uri("/api/v1/auth/challenge")
                    .method("POST")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&michi_identity::DeviceAuthChallengeRequest {
                            client_michi_id: client_michi_id.clone(),
                            client_public_key: client_pk.clone(),
                            home_id: home_id.clone(),
                        })
                        .unwrap(),
                    ))
                    .unwrap(),
            ))
            .await
            .unwrap();
        assert_eq!(auth_ch_resp.status(), StatusCode::OK);
        let auth_ch: michi_identity::DeviceAuthChallengeResponse = serde_json::from_slice(
            &axum::body::to_bytes(auth_ch_resp.into_body(), 64 * 1024)
                .await
                .unwrap(),
        )
        .unwrap();

        let client_sig = michi_identity::sign_device_auth_challenge(
            &client_sk,
            &home_id,
            &auth_ch.server_michi_id,
            &client_michi_id,
            &auth_ch.challenge_id.to_string(),
            &auth_ch.challenge_nonce,
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
                            challenge_id: auth_ch.challenge_id,
                            client_michi_id: client_michi_id.clone(),
                            membership: membership.clone(),
                            client_signature: client_sig,
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
        // Verify full mutual auth confirmation over session_token!
        michi_identity::verify_server_auth_session(
            &sess,
            &root_pk,
            &home_id,
            &client_michi_id,
            &auth_ch.challenge_id.to_string(),
            &[],
        )
        .unwrap();
        latest_session_token = sess.session_token;
    }

    // Active Home session accesses AirCat manifest over TLS cleanly
    let ok_manifest = app
        .clone()
        .oneshot(with_tls(
            Request::builder()
                .uri("/api/v1/aircat/manifest")
                .method("GET")
                .header(
                    header::AUTHORIZATION,
                    format!("Bearer {latest_session_token}"),
                )
                .header("X-Michi-Device-Id", &client_michi_id)
                .body(Body::empty())
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(ok_manifest.status(), StatusCode::OK);

    // Step 7: Immediate revocation invalidates active session & blocks renewal
    let revoke_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/home/revoke")
                .method("POST")
                .header(header::AUTHORIZATION, &admin_auth)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({
                        "device_michi_id": client_michi_id,
                        "reason": "lost_mobile"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(revoke_resp.status(), StatusCode::OK);

    let after_revoke_manifest = app
        .clone()
        .oneshot(with_tls(
            Request::builder()
                .uri("/api/v1/aircat/manifest")
                .method("GET")
                .header(
                    header::AUTHORIZATION,
                    format!("Bearer {latest_session_token}"),
                )
                .body(Body::empty())
                .unwrap(),
        ))
        .await
        .unwrap();
    assert!(matches!(
        after_revoke_manifest.status(),
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
    ));

    let after_revoke_challenge = app
        .clone()
        .oneshot(with_tls(
            Request::builder()
                .uri("/api/v1/auth/challenge")
                .method("POST")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&michi_identity::DeviceAuthChallengeRequest {
                        client_michi_id: client_michi_id.clone(),
                        client_public_key: client_pk,
                        home_id,
                    })
                    .unwrap(),
                ))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(after_revoke_challenge.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn test_r1_legacy_server_code_preserved_but_rejected_by_aircat_strict_and_device_id_policy() {
    let (app, _pool, state, admin_token, _config_dir, _music_dir) = make_r1_app().await;
    enable_tls(&state).await;
    let admin_auth = format!("Bearer {admin_token}");

    // 1. Legacy SERVER_CODE pairing flow works for general v1 APIs
    let client_sk = SigningKey::generate(&mut rand::rngs::OsRng);
    let client_vk = client_sk.verifying_key();
    let client_pk = michi_identity::encode_base64url(&client_vk.to_bytes());
    let client_michi_id = michi_identity::MichiId::from_public_key(&client_vk).to_base64url();
    let nonce_raw = [0x42u8; 32];
    let nonce = michi_identity::encode_base64url(&nonce_raw);
    use ed25519_dalek::Signer;
    let chal_sig = michi_identity::encode_base64url(&client_sk.sign(&nonce_raw).to_bytes());

    let pair_start = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/pair/start")
                .method("POST")
                .header(header::AUTHORIZATION, &admin_auth)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({
                        "device_name": "Legacy Player",
                        "device_type": "desktop",
                        "roles": ["desktop_player"],
                        "auth_strategy": "SERVER_CODE",
                        "michi_id": client_michi_id,
                        "public_key": client_pk,
                        "challenge_nonce": nonce,
                        "challenge_signature": chal_sig
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(pair_start.status(), StatusCode::OK);
    let start_json: Value = serde_json::from_slice(
        &axum::body::to_bytes(pair_start.into_body(), 64 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();
    let session_id = start_json["session_id"].as_str().unwrap().to_string();
    let pin = state
        .pairing_sessions_display
        .read()
        .await
        .get(&session_id)
        .cloned()
        .unwrap();

    let pair_confirm = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/pair/confirm")
                .method("POST")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({
                        "session_id": session_id,
                        "pin": pin,
                        "michi_id": client_michi_id,
                        "public_key": client_pk
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(pair_confirm.status(), StatusCode::OK);
    let confirm_json: Value = serde_json::from_slice(
        &axum::body::to_bytes(pair_confirm.into_body(), 64 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();
    let legacy_token = confirm_json["token"].as_str().unwrap().to_string();
    let paired_uuid = confirm_json["device_id"].as_str().unwrap().to_string();
    let legacy_auth = format!("Bearer {legacy_token}");

    // Legacy SERVER_CODE token works on general Link APIs (/tracks, /playback/state, /queue)
    for uri in ["/api/v1/tracks", "/api/v1/playback/state", "/api/v1/queue"] {
        let r = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(uri)
                    .method("GET")
                    .header(header::AUTHORIZATION, &legacy_auth)
                    .header("X-Michi-Device-Id", &paired_uuid)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            r.status(),
            StatusCode::OK,
            "legacy SERVER_CODE failed on {uri}"
        );
    }

    // 2. Legacy SERVER_CODE token MUST be rejected by AIRCAT_STRICT with 403 HOME_SESSION_REQUIRED
    let legacy_aircat = app
        .clone()
        .oneshot(with_tls(
            Request::builder()
                .uri("/api/v1/aircat/manifest")
                .method("GET")
                .header(header::AUTHORIZATION, &legacy_auth)
                .body(Body::empty())
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(legacy_aircat.status(), StatusCode::FORBIDDEN);
    let err_json: Value = serde_json::from_slice(
        &axum::body::to_bytes(legacy_aircat.into_body(), 64 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(err_json["error"]["code"], "HOME_SESSION_REQUIRED");

    // 3. WebUI admin token MUST also be rejected on AIRCAT_STRICT transfer endpoints
    let admin_aircat = app
        .clone()
        .oneshot(with_tls(
            Request::builder()
                .uri("/api/v1/aircat/manifest")
                .method("GET")
                .header(header::AUTHORIZATION, &admin_auth)
                .body(Body::empty())
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(admin_aircat.status(), StatusCode::FORBIDDEN);
    let err_admin: Value = serde_json::from_slice(
        &axum::body::to_bytes(admin_aircat.into_body(), 64 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(err_admin["error"]["code"], "HOME_SESSION_REQUIRED");

    // 4. Verified HOME_MEMBERSHIP session + X-Michi-Device-Id header policy
    let home_id = state.home_root_authority.home_id();
    let membership = state.home_root_authority.issue_membership(
        &home_id,
        &client_michi_id,
        &client_pk,
        "mobile",
        vec![michi_identity::Role::MobilePlayer],
        &chrono::Utc::now().to_rfc3339(),
        5,
    );
    let ch: michi_identity::DeviceAuthChallengeResponse = serde_json::from_slice(
        &axum::body::to_bytes(
            app.clone()
                .oneshot(with_tls(
                    Request::builder()
                        .uri("/api/v1/auth/challenge")
                        .method("POST")
                        .header(header::CONTENT_TYPE, "application/json")
                        .body(Body::from(
                            serde_json::to_vec(&michi_identity::DeviceAuthChallengeRequest {
                                client_michi_id: client_michi_id.clone(),
                                client_public_key: client_pk.clone(),
                                home_id: home_id.clone(),
                            })
                            .unwrap(),
                        ))
                        .unwrap(),
                ))
                .await
                .unwrap()
                .into_body(),
            64 * 1024,
        )
        .await
        .unwrap(),
    )
    .unwrap();
    let client_sig = michi_identity::sign_device_auth_challenge(
        &client_sk,
        &home_id,
        &ch.server_michi_id,
        &client_michi_id,
        &ch.challenge_id.to_string(),
        &ch.challenge_nonce,
    );
    let sess: michi_identity::DeviceAuthSessionResponse = serde_json::from_slice(
        &axum::body::to_bytes(
            app.clone()
                .oneshot(with_tls(
                    Request::builder()
                        .uri("/api/v1/auth/session")
                        .method("POST")
                        .header(header::CONTENT_TYPE, "application/json")
                        .body(Body::from(
                            serde_json::to_vec(&michi_identity::DeviceAuthSessionRequest {
                                challenge_id: ch.challenge_id,
                                client_michi_id: client_michi_id.clone(),
                                membership,
                                client_signature: client_sig,
                            })
                            .unwrap(),
                        ))
                        .unwrap(),
                ))
                .await
                .unwrap()
                .into_body(),
            64 * 1024,
        )
        .await
        .unwrap(),
    )
    .unwrap();
    let home_auth = format!("Bearer {}", sess.session_token);

    // Omitting X-Michi-Device-Id -> 200 OK
    let ok_omit = app
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
    assert_eq!(ok_omit.status(), StatusCode::OK);

    // Sending matching X-Michi-Device-Id: <client_michi_id> -> 200 OK (zero false IDENTITY_MISMATCH)
    let ok_match = app
        .clone()
        .oneshot(with_tls(
            Request::builder()
                .uri("/api/v1/aircat/manifest")
                .method("GET")
                .header(header::AUTHORIZATION, &home_auth)
                .header("X-Michi-Device-Id", &client_michi_id)
                .body(Body::empty())
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(ok_match.status(), StatusCode::OK);

    // Sending administrative UUID or spoofed foreign ID -> 403 IDENTITY_MISMATCH
    let bad_uuid_hdr = app
        .clone()
        .oneshot(with_tls(
            Request::builder()
                .uri("/api/v1/aircat/manifest")
                .method("GET")
                .header(header::AUTHORIZATION, &home_auth)
                .header("X-Michi-Device-Id", &paired_uuid)
                .body(Body::empty())
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(bad_uuid_hdr.status(), StatusCode::FORBIDDEN);
    let err_id: Value = serde_json::from_slice(
        &axum::body::to_bytes(bad_uuid_hdr.into_body(), 64 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(err_id["error"]["code"], "IDENTITY_MISMATCH");
}

#[tokio::test]
async fn test_r1_tls_fail_closed_unconfigured_and_x509_san_validation() {
    let (app, _pool, state, admin_token, _config_dir, _music_dir) = make_r1_app().await;
    let admin_auth = format!("Bearer {admin_token}");

    // 1. Even with NO TLS configured at all, AirCat transfer endpoints MUST reject cleartext with 503 TLS_NOT_CONFIGURED
    let resp_unconfigured = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/aircat/manifest")
                .method("GET")
                .header(header::AUTHORIZATION, &admin_auth)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp_unconfigured.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body_unconf: Value = serde_json::from_slice(
        &axum::body::to_bytes(resp_unconfigured.into_body(), 64 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(body_unconf["error"]["code"], "TLS_NOT_CONFIGURED");

    // 2. With TLS active, cleartext HTTP without TLS extension MUST be rejected with 403 TLS_REQUIRED
    enable_tls(&state).await;
    let resp_cleartext = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/aircat/manifest")
                .method("GET")
                .header(header::AUTHORIZATION, &admin_auth)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp_cleartext.status(), StatusCode::FORBIDDEN);
    let body_clear: Value = serde_json::from_slice(
        &axum::body::to_bytes(resp_cleartext.into_body(), 64 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(body_clear["error"]["code"], "TLS_REQUIRED");

    // 3. Real OpenSSL X.509 v3 certificate SAN check (Android hostname verifier compatibility)
    let tmp_dir = std::env::temp_dir().join(format!("michi-san-test-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&tmp_dir).unwrap();
    let cert_path = tmp_dir.join("tls.crt");
    let key_path = tmp_dir.join("tls.key");
    let openssl_out = std::process::Command::new("openssl")
        .args([
            "req",
            "-x509",
            "-newkey",
            "ec",
            "-pkeyopt",
            "ec_paramgen_curve:prime256v1",
            "-keyout",
            key_path.to_str().unwrap(),
            "-out",
            cert_path.to_str().unwrap(),
            "-days",
            "365",
            "-nodes",
            "-subj",
            "/CN=michi.local",
            "-addext",
            "subjectAltName=DNS:michi.local,IP:127.0.0.1",
        ])
        .output();
    if let Ok(out) = openssl_out {
        if out.status.success() {
            let cert_pem = std::fs::read(&cert_path).unwrap();
            assert!(michi_api::extract_spki_sha256_from_pem(&cert_pem).is_some());
            assert!(michi_api::validate_x509_san_for_url(
                &cert_pem,
                "https://127.0.0.1:9443"
            ));
            assert!(michi_api::validate_x509_san_for_url(
                &cert_pem,
                "https://michi.local:9443"
            ));
            assert!(!michi_api::validate_x509_san_for_url(
                &cert_pem,
                "https://192.168.99.99:9443"
            ));
            assert!(!michi_api::validate_x509_san_for_url(
                &cert_pem,
                "http://127.0.0.1:9090"
            ));
        }
    }
    let _ = std::fs::remove_dir_all(&tmp_dir);
}

#[tokio::test]
async fn test_r1_michi_control_playback_and_queue_mobile_contracts() {
    let (app, pool, _state, admin_token, _config_dir, music_dir) = make_r1_app().await;
    let auth = format!("Bearer {admin_token}");

    // Insert a real track so queue add + item delete work cleanly
    let file_path = music_dir.join("control-test.flac");
    {
        let mut f = std::fs::File::create(&file_path).unwrap();
        f.write_all(b"CONTROL_TEST_AUDIO_BYTES").unwrap();
    }
    let track_id = michi_core::track_id_from_library_path(&music_dir, &file_path);
    michi_db::upsert_track(
        &pool,
        &Track {
            id: track_id,
            file_path: file_path.to_string_lossy().to_string(),
            title: Some("Control Track".into()),
            artist: Some("Artist".into()),
            album: Some("Album".into()),
            album_artist: None,
            duration_ms: Some(120_000),
            format: AudioFormat::Flac,
            sample_rate: Some(48000),
            bit_depth: Some(16),
            channels: Some(2),
            artwork_id: None,
            genre: None,
            year: Some(2026),
            track_number: Some(1),
            disc_number: None,
            content_hash: None,
            file_size: Some(24),
            file_mtime_ns: None,
            starred: false,
            rating: 0,
            starred_at: None,
            replaygain_track_gain: None,
            replaygain_track_peak: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        },
    )
    .await
    .unwrap();

    // Mobile PlaybackControlRequestDto sends `value: ""` string on commands like pause
    let pause_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/playback/control")
                .method("POST")
                .header(header::AUTHORIZATION, &auth)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({
                        "command": "pause",
                        "value": ""
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(pause_resp.status(), StatusCode::OK);

    // Queue repeat & shuffle endpoints used by Mobile LinkClient.kt
    let rep_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/queue/repeat")
                .method("POST")
                .header(header::AUTHORIZATION, &auth)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({ "mode": "all" })).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(rep_resp.status(), StatusCode::OK);
    let rep_json: Value = serde_json::from_slice(
        &axum::body::to_bytes(rep_resp.into_body(), 16 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(rep_json["mode"], "all");

    let shuf_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/queue/shuffle")
                .method("POST")
                .header(header::AUTHORIZATION, &auth)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({ "enabled": true })).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(shuf_resp.status(), StatusCode::OK);
    let shuf_json: Value = serde_json::from_slice(
        &axum::body::to_bytes(shuf_resp.into_body(), 16 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(shuf_json["enabled"], true);

    // Add track to queue and delete single item via DELETE /api/v1/queue/items/:item_id
    let add_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/queue/items")
                .method("POST")
                .header(header::AUTHORIZATION, &auth)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({ "track_ids": [track_id] })).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(add_resp.status(), StatusCode::OK);

    let q_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/queue")
                .method("GET")
                .header(header::AUTHORIZATION, &auth)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let q_json: Value = serde_json::from_slice(
        &axum::body::to_bytes(q_resp.into_body(), 64 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();
    let item_id = q_json["items"][0]["id"].as_str().unwrap().to_string();

    let del_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/queue/items/{item_id}"))
                .method("DELETE")
                .header(header::AUTHORIZATION, &auth)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(del_resp.status(), StatusCode::OK);
}

#[test]
fn test_r1_cross_repo_interop_golden_vectors_parity() {
    let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../vendor/michi-link/tests/vectors/interop/micro-mobile-interop-vectors.json");
    let raw = std::fs::read_to_string(&fixture_path).expect("michi-link interop fixture missing");
    let v: Value = serde_json::from_str(&raw).unwrap();

    let home_id = v["home_root"]["home_id"].as_str().unwrap();
    let root_pk = v["home_root"]["public_key"].as_str().unwrap();
    let server_michi_id = v["server"]["michi_id"].as_str().unwrap();
    let server_pk = v["server"]["public_key"].as_str().unwrap();
    let client_michi_id = v["mobile_client"]["michi_id"].as_str().unwrap();
    let client_pk = v["mobile_client"]["public_key"].as_str().unwrap();

    // 1. BLAKE3 identity derivation parity
    assert_eq!(
        michi_identity::IdentityManager::derive_michi_id(root_pk)
            .unwrap()
            .to_base64url(),
        home_id
    );
    assert_eq!(
        michi_identity::IdentityManager::derive_michi_id(server_pk)
            .unwrap()
            .to_base64url(),
        server_michi_id
    );
    assert_eq!(
        michi_identity::IdentityManager::derive_michi_id(client_pk)
            .unwrap()
            .to_base64url(),
        client_michi_id
    );

    // 2. Home enrollment PoP parity
    assert!(michi_identity::verify_home_enrollment_pop(
        client_pk,
        v["home_enrollment_pop"]["client_signature"]
            .as_str()
            .unwrap(),
        home_id,
        server_michi_id,
        client_michi_id,
        v["home_enrollment_pop"]["challenge_id"].as_str().unwrap(),
        v["home_enrollment_pop"]["challenge_nonce"]
            .as_str()
            .unwrap(),
    )
    .unwrap());

    // 3. Home mutual auth (client challenge + server confirm over session_token) parity
    assert!(michi_identity::verify_device_auth_challenge(
        client_pk,
        v["home_mutual_auth"]["client_signature"].as_str().unwrap(),
        home_id,
        server_michi_id,
        client_michi_id,
        v["home_mutual_auth"]["challenge_id"].as_str().unwrap(),
        v["home_mutual_auth"]["challenge_nonce"].as_str().unwrap(),
    )
    .unwrap());

    let server_membership: michi_identity::DeviceMembershipDto =
        serde_json::from_value(v["server"]["membership"].clone()).unwrap();
    let session_resp = michi_identity::DeviceAuthSessionResponse {
        session_token: v["home_mutual_auth"]["session_token"]
            .as_str()
            .unwrap()
            .into(),
        token_type: "Bearer".into(),
        expires_in: 3600,
        server_michi_id: server_michi_id.into(),
        server_membership,
        server_signature: v["home_mutual_auth"]["server_signature"]
            .as_str()
            .unwrap()
            .into(),
    };
    michi_identity::verify_server_auth_session(
        &session_resp,
        root_pk,
        home_id,
        client_michi_id,
        v["home_mutual_auth"]["challenge_id"].as_str().unwrap(),
        &[],
    )
    .unwrap();

    // 4. AirCat Grant v1 parity
    let signed_grant: michi_api::aircat_grants::Grant =
        serde_json::from_value(v["aircat_grant_v1"]["signed_grant"].clone()).unwrap();
    let root_pk_bytes = michi_identity::decode_base64url_strict(root_pk).unwrap();
    let root_vk =
        ed25519_dalek::VerifyingKey::from_bytes(&root_pk_bytes.try_into().unwrap()).unwrap();
    michi_api::aircat_grants::verify_grant(
        &signed_grant,
        &root_vk,
        client_michi_id,
        1_763_467_300_000,
        false,
    )
    .unwrap();
}
