use axum::{
    body::Body,
    http::{header, Request, StatusCode},
};
use michi_api::{create_router, init_admin_user, AppState};
use michi_config::Config;
use serde_json::Value;
use sqlx::SqlitePool;
use tower::ServiceExt;
use uuid::Uuid;

async fn test_db_file() -> (SqlitePool, std::path::PathBuf) {
    let tmp = std::env::temp_dir().join(format!("michi_test_rc2_{}.db", Uuid::new_v4()));
    let url = format!("sqlite://{}", tmp.display());
    let pool = michi_db::init_pool(&url).await.unwrap();
    (pool, tmp)
}

fn test_config_for_db(db_path: &std::path::Path, username: &str, password: &str) -> Config {
    let tmp = std::env::temp_dir().join(format!("michi_cfg_{}", Uuid::new_v4()));
    let url = format!("sqlite://{}", db_path.display());

    Config {
        port: 9090,
        music_paths: vec![std::env::temp_dir()],
        config_path: tmp.join("config"),
        cache_path: tmp.join("cache"),
        database_url: url,
        version: "1.0.0-rc.2",
        sync_peers: Vec::new(),
        sync_name: "test-rc2".to_string(),
        listenbrainz_token: None,
        lastfm_token: None,
        scrobble_enabled: false,
        auth_username: Some(username.to_string()),
        auth_password: Some(password.to_string()),
        auth_enabled: true,
        allow_registration: false,
        server_id: Uuid::new_v4(),
        cors_origin: None,
        dev_mode: false,
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
        trusted_proxies: vec!["127.0.0.1".parse().unwrap()],
        deployment_platform: "zimaos".into(),
    }
}

#[tokio::test]
async fn test_credential_authority_reconciliation_on_restart() {
    let (pool, db_path) = test_db_file().await;

    // 1. First boot: admin user created with initial password
    let cfg1 = test_config_for_db(&db_path, "admin", "initial_password_123");
    let admin_id1 = init_admin_user(&cfg1, &pool)
        .await
        .expect("admin user must be initialized");

    let state1 = AppState::new(cfg1.clone(), pool.clone(), Some(admin_id1));
    let app1 = create_router(state1);

    // Verify initial login works
    let login_req = Request::builder()
        .method("POST")
        .uri("/api/auth/login")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "username": "admin",
                "password": "initial_password_123"
            }))
            .unwrap(),
        ))
        .unwrap();
    let res = app1.oneshot(login_req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    // 2. Second boot: password in environment rotated to new password
    let cfg2 = test_config_for_db(&db_path, "admin", "new_rotated_password_456");
    let admin_id2 = init_admin_user(&cfg2, &pool)
        .await
        .expect("admin user must be reconciled");
    assert_eq!(admin_id1, admin_id2, "admin ID must remain identical");

    let state2 = AppState::new(cfg2.clone(), pool.clone(), Some(admin_id2));
    let app2 = create_router(state2);

    // Old password must now fail (fail-closed)
    let old_login_req = Request::builder()
        .method("POST")
        .uri("/api/auth/login")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "username": "admin",
                "password": "initial_password_123"
            }))
            .unwrap(),
        ))
        .unwrap();
    let old_res = app2.clone().oneshot(old_login_req).await.unwrap();
    assert_eq!(old_res.status(), StatusCode::UNAUTHORIZED);

    // New password must succeed
    let new_login_req = Request::builder()
        .method("POST")
        .uri("/api/auth/login")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "username": "admin",
                "password": "new_rotated_password_456"
            }))
            .unwrap(),
        ))
        .unwrap();
    let new_res = app2.oneshot(new_login_req).await.unwrap();
    assert_eq!(new_res.status(), StatusCode::OK);

    // Clean up temp file
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn test_session_persistence_across_restarts() {
    let (pool, db_path) = test_db_file().await;

    let cfg1 = test_config_for_db(&db_path, "admin", "supersecret123");
    let admin_id = init_admin_user(&cfg1, &pool)
        .await
        .expect("admin user must be initialized");

    let state1 = AppState::new(cfg1.clone(), pool.clone(), Some(admin_id));

    // Create session in instance 1
    let token = state1.auth_sessions.create_session(admin_id).await;
    assert!(state1.auth_sessions.validate(&token).await);

    // Simulate complete process crash and reboot: new AppState instance with empty in-memory cache
    let cfg2 = test_config_for_db(&db_path, "admin", "supersecret123");
    let state2 = AppState::new(cfg2, pool.clone(), Some(admin_id));

    // Verify session survives container reboot via SQLite persistence
    let user_id = state2.auth_sessions.extract_user_id(&token).await;
    assert_eq!(user_id, Some(admin_id));
    assert!(state2.auth_sessions.validate(&token).await);

    // Verify session works with authenticated endpoints in rebooted app
    let app2 = create_router(state2.clone());
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/settings")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let res = app2.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    // Now invalidate session and verify it is removed from SQLite
    state2.auth_sessions.invalidate(&token).await;
    assert!(!state2.auth_sessions.validate(&token).await);

    // Reboot once more and verify session remains gone
    let state3 = AppState::new(cfg1, pool.clone(), Some(admin_id));
    assert!(!state3.auth_sessions.validate(&token).await);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn test_update_status_and_check_contract() {
    let (pool, db_path) = test_db_file().await;
    let cfg = test_config_for_db(&db_path, "admin", "supersecret123");
    let admin_id = init_admin_user(&cfg, &pool)
        .await
        .expect("admin user must be initialized");

    let state = AppState::new(cfg, pool, Some(admin_id));
    let token = state.auth_sessions.create_session(admin_id).await;
    let app = create_router(state);

    // GET /api/v1/update/status
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/update/status?channel=preview")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let body_bytes = axum::body::to_bytes(res.into_body(), 1024 * 64)
        .await
        .unwrap();
    let json: Value = serde_json::from_slice(&body_bytes).unwrap();

    assert_eq!(json["current_version"], "1.0.0-rc.2");
    assert_eq!(json["channel"], "preview");
    assert_eq!(json["deployment_platform"], "zimaos");
    assert!(json["instructions"].as_str().unwrap().contains("ZimaOS"));

    // POST /api/v1/update/check
    let check_req = Request::builder()
        .method("POST")
        .uri("/api/v1/update/check")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "channel": "preview"
            }))
            .unwrap(),
        ))
        .unwrap();
    let check_res = app.oneshot(check_req).await.unwrap();
    assert_eq!(check_res.status(), StatusCode::OK);

    let check_bytes = axum::body::to_bytes(check_res.into_body(), 1024 * 64)
        .await
        .unwrap();
    let check_json: Value = serde_json::from_slice(&check_bytes).unwrap();
    assert_eq!(check_json["current_version"], "1.0.0-rc.2");
    assert_eq!(check_json["deployment_platform"], "zimaos");

    let _ = std::fs::remove_file(db_path);
}
