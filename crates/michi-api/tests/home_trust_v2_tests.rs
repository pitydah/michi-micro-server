use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use michi_api::create_router;
use michi_config::Config;
use serde_json::Value;
use sqlx::SqlitePool;
use tower::ServiceExt;
use uuid::Uuid;

async fn test_db_with_url() -> (SqlitePool, String) {
    let url = format!(
        "sqlite:file:memdb_trust_v2_{}?mode=memory&cache=shared",
        Uuid::new_v4()
    );
    let pool = michi_db::init_pool(&url).await.unwrap();
    (pool, url)
}

fn test_config_with_url(db_url: String) -> Config {
    let tmp = std::env::temp_dir().join(format!("michi-trust-v2-test-{}", Uuid::new_v4()));
    let _ = std::fs::create_dir_all(&tmp);

    Config {
        port: 9090,
        music_paths: vec![std::env::temp_dir()],
        config_path: tmp.join("config"),
        cache_path: tmp.join("cache"),
        database_url: db_url,

        version: "1.0.0-rc.4",
        sync_peers: Vec::new(),
        sync_name: "michi-server-test".to_string(),
        listenbrainz_token: None,
        lastfm_token: None,
        scrobble_enabled: false,
        auth_username: None,
        auth_password: None,
        auth_enabled: false,
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
        deployment_platform: "unknown".into(),
    }
}

async fn make_app() -> (axum::Router, SqlitePool, michi_api::AppState, String) {
    let (pool, db_url) = test_db_with_url().await;
    let config = test_config_with_url(db_url);
    let identity = std::sync::Arc::new(
        michi_identity::IdentityManager::generate(
            &config.config_path,
            "Trust V2 Test Server",
            "password123",
        )
        .unwrap(),
    );
    let admin_id = Uuid::new_v4();
    michi_db::create_user(
        &pool,
        &admin_id,
        "admin-trust-v2",
        "password",
        true,
    )
    .await
    .unwrap();
    let state = michi_api::AppState::new_with_identity(config, pool.clone(), Some(admin_id), identity);
    let token = state.auth_sessions.create_session(admin_id).await.unwrap();
    let app = create_router(state.clone());
    (app, pool, state, token)
}

#[tokio::test]
async fn test_home_trust_v2_info_roster_and_revocation() {
    let (app, _pool, state, token) = make_app().await;
    let auth_header = format!("Bearer {token}");

    // 1. GET /api/v1/home/info
    let req = Request::builder()
        .uri("/api/v1/home/info")
        .method("GET")
        .header("Authorization", &auth_header)
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let info: Value = serde_json::from_slice(&bytes).unwrap();

    let home_id = info["home_id"].as_str().expect("home_id present");
    let root_pk = info["root_authority_public_key"]
        .as_str()
        .expect("root_authority_public_key present");
    let server_michi_id = info["server_michi_id"]
        .as_str()
        .expect("server_michi_id present");

    assert!(!home_id.is_empty());
    assert!(!root_pk.is_empty());
    assert!(!server_michi_id.is_empty());

    // Verify server_membership structure
    let membership: michi_identity::types::DeviceMembershipDto =
        serde_json::from_value(info["server_membership"].clone()).unwrap();
    assert_eq!(membership.home_id, home_id);
    assert_eq!(membership.device_michi_id, server_michi_id);

    // Cryptographically verify server membership against root authority public key
    michi_identity::home::verify_membership(&membership, root_pk, home_id, &[])
        .expect("server membership must verify under home root authority");

    // 2. Register a mock receiver in registry
    let device_michi_id = "test-device-michi-id-xyz";
    let receiver_id = "rec-123";
    {
        let reg_arc = state.receiver_manager.registry().await;
        let mut reg = reg_arc.write().await;
        reg.add(michi_receivers::ReceiverRegistryEntry {
            receiver_id: receiver_id.to_string(),
            michi_id: Some(device_michi_id.to_string()),
            name: "Living Room Speaker".to_string(),
            device_type: "standard".to_string(),
            base_url: "http://192.168.1.100:8080".to_string(),
            michi_home_id: Some(home_id.to_string()),
            authenticated: true,
            revoked: false,
            presence: michi_receivers::ReceiverPresence::VerifiedOnline,
            ..Default::default()
        });
    }

    // 3. GET /api/v1/home/roster
    let req = Request::builder()
        .uri("/api/v1/home/roster")
        .method("GET")
        .header("Authorization", &auth_header)
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let roster: Value = serde_json::from_slice(&bytes).unwrap();
    let devices = roster["devices"].as_array().expect("devices array");
    assert_eq!(devices.len(), 2);
    assert!(devices.iter().any(|d| d["device_type"] == "server" && d["device_michi_id"] == server_michi_id));
    let rec = devices.iter().find(|d| d["device_michi_id"] == device_michi_id).unwrap();
    assert_eq!(rec["authenticated"], true);
    assert_eq!(rec["revoked"], false);

    // 4. POST /api/v1/home/revoke
    let req = Request::builder()
        .uri("/api/v1/home/revoke")
        .method("POST")
        .header("Authorization", &auth_header)
        .header("Content-Type", "application/json")
        .body(Body::from(
            serde_json::json!({
                "device_michi_id": device_michi_id,
                "reason": "Test security revocation"
            })
            .to_string(),
        ))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let revoke_resp: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(revoke_resp["status"], "revoked");

    let revocation: michi_identity::types::HomeDeviceRevocationDto =
        serde_json::from_value(revoke_resp["revocation"].clone()).unwrap();
    assert_eq!(revocation.home_id, home_id);
    assert_eq!(revocation.revoked_device_michi_id, device_michi_id);
    assert_eq!(revocation.reason, "Test security revocation");

    // Cryptographically verify the revocation signature
    michi_identity::home::verify_revocation(&revocation, root_pk, home_id)
        .expect("revocation record must verify under home root authority");

    // 5. GET /api/v1/home/revocations
    let req = Request::builder()
        .uri("/api/v1/home/revocations")
        .method("GET")
        .header("Authorization", &auth_header)
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let revs_val: Value = serde_json::from_slice(&bytes).unwrap();
    let revs = revs_val["revocations"].as_array().expect("revocations");
    assert_eq!(revs.len(), 1);
    assert_eq!(revs[0]["revoked_device_michi_id"], device_michi_id);

    // 6. Verify receiver in registry is marked revoked and unauthenticated
    let req = Request::builder()
        .uri("/api/v1/receivers")
        .method("GET")
        .header("Authorization", &auth_header)
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let recs_val: Value = serde_json::from_slice(&bytes).unwrap();
    let receivers = recs_val["receivers"].as_array().expect("receivers");
    assert_eq!(receivers.len(), 1);
    assert_eq!(receivers[0]["revoked"], true);
    assert_eq!(receivers[0]["authenticated"], false);
}
