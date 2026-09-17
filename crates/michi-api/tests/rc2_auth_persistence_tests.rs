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
        .expect("admin user must be initialized")
        .unwrap();

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
        .expect("admin user must be reconciled")
        .unwrap();
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
async fn test_managed_admin_username_rotation_revokes_old_session() {
    let (pool, db_path) = test_db_file().await;

    // 1. First boot: admin user created as 'admin_alpha'
    let cfg1 = test_config_for_db(&db_path, "admin_alpha", "alpha_secret_pw");
    let admin_id1 = init_admin_user(&cfg1, &pool)
        .await
        .expect("admin user must be initialized")
        .unwrap();

    let state1 = AppState::new(cfg1.clone(), pool.clone(), Some(admin_id1));
    let token1 = state1
        .auth_sessions
        .create_session(admin_id1)
        .await
        .unwrap();
    assert!(state1.auth_sessions.validate(&token1).await);

    // Create a regular non-admin user to verify it is NOT overwritten or affected
    let normal_user_id = Uuid::new_v4();
    let normal_pw_hash = michi_api::auth::hash_password("normal_pass_123").unwrap();
    michi_db::create_user(
        &pool,
        &normal_user_id,
        "normal_user",
        &normal_pw_hash,
        false,
    )
    .await
    .unwrap();

    // 2. Second boot: MICHI_AUTH_USERNAME rotated to 'admin_beta' with new password
    let cfg2 = test_config_for_db(&db_path, "admin_beta", "beta_secret_pw");
    let admin_id2 = init_admin_user(&cfg2, &pool)
        .await
        .expect("admin user must be reconciled under new username")
        .unwrap();

    // Must reconcile the SAME admin record (same UUID)
    assert_eq!(
        admin_id1, admin_id2,
        "Admin ID must be preserved during username rotation"
    );

    // Total admin accounts in DB must be exactly 1 (no accumulation of stale admin accounts)
    let admins = michi_db::list_admin_users(&pool).await.unwrap();
    assert_eq!(admins.len(), 1, "Must have exactly 1 admin after rotation");
    assert_eq!(admins[0].username, "admin_beta");

    // Verify non-admin user still exists intact
    let normal_user = michi_db::get_user_by_username(&pool, "normal_user")
        .await
        .unwrap();
    assert!(normal_user.is_some(), "Non-admin users must be preserved");

    // 3. Old session token for the rotated admin must be completely revoked
    let state2 = AppState::new(cfg2.clone(), pool.clone(), Some(admin_id2));
    assert!(
        !state2.auth_sessions.validate(&token1).await,
        "Prior session token must be invalidated following username rotation"
    );

    // 4. Old username login must fail
    let app2 = create_router(state2.clone());
    let old_login_req = Request::builder()
        .method("POST")
        .uri("/api/auth/login")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "username": "admin_alpha",
                "password": "alpha_secret_pw"
            }))
            .unwrap(),
        ))
        .unwrap();
    let old_res = app2.clone().oneshot(old_login_req).await.unwrap();
    assert_eq!(old_res.status(), StatusCode::UNAUTHORIZED);

    // 5. New username + new password login must succeed
    let new_login_req = Request::builder()
        .method("POST")
        .uri("/api/auth/login")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "username": "admin_beta",
                "password": "beta_secret_pw"
            }))
            .unwrap(),
        ))
        .unwrap();
    let new_res = app2.oneshot(new_login_req).await.unwrap();
    assert_eq!(new_res.status(), StatusCode::OK);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn test_clear_all_sessions_db_first_ordering() {
    let (pool, db_path) = test_db_file().await;
    let cfg = test_config_for_db(&db_path, "admin", "adminpass123");
    let admin_id = init_admin_user(&cfg, &pool).await.unwrap().unwrap();
    let state = AppState::new(cfg, pool.clone(), Some(admin_id));

    let token = state.auth_sessions.create_session(admin_id).await.unwrap();
    assert!(state.auth_sessions.validate(&token).await);

    // Drop auth_sessions table to force DB error during clear_all_sessions
    sqlx::query("DROP TABLE auth_sessions")
        .execute(&pool)
        .await
        .unwrap();

    // clear_all_sessions must fail with Err and NOT clear the in-memory cache if DB deletion fails
    let res = state.auth_sessions.clear_all_sessions().await;
    assert!(
        res.is_err(),
        "clear_all_sessions must fail closed when DB fails"
    );

    // In-memory cache must still have the session (fail closed, no inconsistency where DB failed but memory cleared)
    assert!(
        state
            .auth_sessions
            .sessions
            .read()
            .await
            .contains_key(&token),
        "In-memory session must not be cleared if database deletion failed"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn test_session_persistence_across_restarts() {
    let (pool, db_path) = test_db_file().await;

    let cfg1 = test_config_for_db(&db_path, "admin", "supersecret123");
    let admin_id = init_admin_user(&cfg1, &pool)
        .await
        .expect("admin user must be initialized")
        .unwrap();

    let state1 = AppState::new(cfg1.clone(), pool.clone(), Some(admin_id));

    // Create session in instance 1
    let token = state1.auth_sessions.create_session(admin_id).await.unwrap();
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
    state2.auth_sessions.invalidate(&token).await.unwrap();
    assert!(!state2.auth_sessions.validate(&token).await);

    // Reboot once more and verify session remains gone
    let state3 = AppState::new(cfg1, pool.clone(), Some(admin_id));
    assert!(!state3.auth_sessions.validate(&token).await);

    let _ = std::fs::remove_file(db_path);
}
static UPDATE_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Clone)]
struct MockReleaseSource {
    releases: Result<Vec<michi_api::routes::v1::update::GitHubRelease>, String>,
}

#[async_trait::async_trait]
impl michi_api::routes::v1::update::ReleaseSource for MockReleaseSource {
    async fn fetch_releases(
        &self,
    ) -> Result<Vec<michi_api::routes::v1::update::GitHubRelease>, String> {
        self.releases.clone()
    }
}

fn mock_release(tag: &str, prerelease: bool) -> michi_api::routes::v1::update::GitHubRelease {
    michi_api::routes::v1::update::GitHubRelease {
        tag_name: tag.to_string(),
        name: Some(tag.to_string()),
        html_url: format!("https://github.com/pitydah/michi-micro-server/releases/tag/{tag}"),
        published_at: Some("2026-09-12T00:00:00Z".to_string()),
        prerelease,
        body: Some("Test release body".to_string()),
    }
}

#[tokio::test]
async fn test_update_status_and_check_contract() {
    let _lock = UPDATE_TEST_LOCK.lock().await;
    michi_api::routes::v1::update::clear_releases_cache().await;
    michi_api::routes::v1::update::set_test_release_source(Some(std::sync::Arc::new(
        MockReleaseSource {
            releases: Ok(vec![mock_release("v1.0.0-rc.3", true)]),
        },
    )))
    .await;

    let (pool, db_path) = test_db_file().await;
    let cfg = test_config_for_db(&db_path, "admin", "supersecret123");
    let admin_id = init_admin_user(&cfg, &pool)
        .await
        .expect("admin user must be initialized")
        .unwrap();

    let state = AppState::new(cfg, pool, Some(admin_id));
    let token = state.auth_sessions.create_session(admin_id).await.unwrap();
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

    michi_api::routes::v1::update::set_test_release_source(None).await;
    michi_api::routes::v1::update::clear_releases_cache().await;
    let _ = std::fs::remove_file(db_path);
}

fn extract_cookie_token(res: &axum::response::Response) -> Option<String> {
    for val in res.headers().get_all(header::SET_COOKIE) {
        if let Ok(s) = val.to_str() {
            for part in s.split(';') {
                let part = part.trim();
                if let Some(token) = part.strip_prefix("michi_web_session=") {
                    if !token.is_empty() {
                        return Some(token.to_string());
                    }
                }
            }
        }
    }
    None
}

#[tokio::test]
async fn test_managed_admin_password_rotation_revokes_old_session() {
    let (pool, db_path) = test_db_file().await;

    // 1. Initial boot with pass1
    let cfg1 = test_config_for_db(&db_path, "admin", "pass1_initial");
    let admin_id = init_admin_user(&cfg1, &pool)
        .await
        .expect("admin user must be initialized")
        .unwrap();

    let state1 = AppState::new(cfg1, pool.clone(), Some(admin_id));
    let app1 = create_router(state1);

    // Login with pass1
    let login_req = Request::builder()
        .method("POST")
        .uri("/api/auth/login")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "username": "admin",
                "password": "pass1_initial"
            }))
            .unwrap(),
        ))
        .unwrap();
    let res1 = app1.clone().oneshot(login_req).await.unwrap();
    assert_eq!(res1.status(), StatusCode::OK);
    let cookie_token_a =
        extract_cookie_token(&res1).expect("michi_web_session cookie should be set");

    let body_bytes = axum::body::to_bytes(res1.into_body(), 1024 * 64)
        .await
        .unwrap();
    let json: Value = serde_json::from_slice(&body_bytes).unwrap();
    let token_a = json["token"].as_str().unwrap().to_string();
    assert_eq!(token_a, cookie_token_a);

    // Verify token A works on /api/v1/settings with Bearer
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/settings")
        .header(header::AUTHORIZATION, format!("Bearer {token_a}"))
        .body(Body::empty())
        .unwrap();
    let res = app1.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let _ = axum::body::to_bytes(res.into_body(), 1024 * 64)
        .await
        .unwrap();

    // 2. Rotate password to pass2_rotated
    let cfg2 = test_config_for_db(&db_path, "admin", "pass2_rotated");
    let admin_id2 = init_admin_user(&cfg2, &pool)
        .await
        .expect("admin password must be reconciled")
        .unwrap();
    assert_eq!(admin_id, admin_id2);

    let state2 = AppState::new(cfg2, pool.clone(), Some(admin_id2));
    let app2 = create_router(state2.clone());

    // Old login must return 401
    let old_login_req = Request::builder()
        .method("POST")
        .uri("/api/auth/login")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "username": "admin",
                "password": "pass1_initial"
            }))
            .unwrap(),
        ))
        .unwrap();
    let old_res = app2.clone().oneshot(old_login_req).await.unwrap();
    assert_eq!(old_res.status(), StatusCode::UNAUTHORIZED);

    // Old token A must be rejected on /api/v1/settings via Bearer (401)
    let bad_req1 = Request::builder()
        .method("GET")
        .uri("/api/v1/settings")
        .header(header::AUTHORIZATION, format!("Bearer {token_a}"))
        .body(Body::empty())
        .unwrap();
    let bad_res1 = app2.clone().oneshot(bad_req1).await.unwrap();
    assert_eq!(bad_res1.status(), StatusCode::UNAUTHORIZED);

    // Old token A must also be rejected via Cookie (401)
    let bad_req2 = Request::builder()
        .method("GET")
        .uri("/api/v1/settings")
        .header(header::COOKIE, format!("michi_web_session={token_a}"))
        .body(Body::empty())
        .unwrap();
    let bad_res2 = app2.clone().oneshot(bad_req2).await.unwrap();
    assert_eq!(bad_res2.status(), StatusCode::UNAUTHORIZED);

    // New login with rotated password must succeed and yield token B
    let new_login_req = Request::builder()
        .method("POST")
        .uri("/api/auth/login")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "username": "admin",
                "password": "pass2_rotated"
            }))
            .unwrap(),
        ))
        .unwrap();
    let new_res = app2.clone().oneshot(new_login_req).await.unwrap();
    assert_eq!(new_res.status(), StatusCode::OK);
    let body_bytes = axum::body::to_bytes(new_res.into_body(), 1024 * 64)
        .await
        .unwrap();
    let json: Value = serde_json::from_slice(&body_bytes).unwrap();
    let token_b = json["token"].as_str().unwrap().to_string();

    // Token B must work on /api/v1/settings
    let good_req = Request::builder()
        .method("GET")
        .uri("/api/v1/settings")
        .header(header::AUTHORIZATION, format!("Bearer {token_b}"))
        .body(Body::empty())
        .unwrap();
    let good_res = app2.oneshot(good_req).await.unwrap();
    assert_eq!(good_res.status(), StatusCode::OK);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn test_cookie_persistence_and_logout_across_restart() {
    let (pool, db_path) = test_db_file().await;

    let cfg1 = test_config_for_db(&db_path, "admin", "supersecret123");
    let admin_id = init_admin_user(&cfg1, &pool)
        .await
        .expect("admin user must be initialized")
        .unwrap();

    let state1 = AppState::new(cfg1.clone(), pool.clone(), Some(admin_id));
    let app1 = create_router(state1);

    // Login via POST /api/auth/login
    let login_req = Request::builder()
        .method("POST")
        .uri("/api/auth/login")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "username": "admin",
                "password": "supersecret123"
            }))
            .unwrap(),
        ))
        .unwrap();
    let login_res = app1.oneshot(login_req).await.unwrap();
    assert_eq!(login_res.status(), StatusCode::OK);
    let cookie_token =
        extract_cookie_token(&login_res).expect("michi_web_session cookie should be set");

    // Authenticated request using Cookie header
    let state1_app = create_router(AppState::new(cfg1.clone(), pool.clone(), Some(admin_id)));
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/settings")
        .header(header::COOKIE, format!("michi_web_session={cookie_token}"))
        .body(Body::empty())
        .unwrap();
    let res = state1_app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    // Simulate container restart (fresh AppState with cold cache)
    let state2 = AppState::new(cfg1.clone(), pool.clone(), Some(admin_id));
    let app2 = create_router(state2);

    // Request on rebooted server using Cookie header must succeed
    let reboot_req = Request::builder()
        .method("GET")
        .uri("/api/v1/settings")
        .header(header::COOKIE, format!("michi_web_session={cookie_token}"))
        .body(Body::empty())
        .unwrap();
    let reboot_res = app2.clone().oneshot(reboot_req).await.unwrap();
    assert_eq!(reboot_res.status(), StatusCode::OK);

    // Logout using Cookie
    let logout_req = Request::builder()
        .method("POST")
        .uri("/api/auth/logout")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::COOKIE, format!("michi_web_session={cookie_token}"))
        .body(Body::empty())
        .unwrap();
    let logout_res = app2.clone().oneshot(logout_req).await.unwrap();
    assert_eq!(logout_res.status(), StatusCode::OK);

    // Verify Set-Cookie clears the session cookie (Max-Age=0 or michi_web_session=;)
    let has_clear_cookie = logout_res
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .any(|v| {
            let s = v.to_str().unwrap_or("");
            s.contains("michi_web_session=") && (s.contains("Max-Age=0") || s.contains("expires="))
        });
    assert!(
        has_clear_cookie,
        "logout must send clearing Set-Cookie header"
    );

    // After logout, request with that cookie must be rejected (401)
    let after_req = Request::builder()
        .method("GET")
        .uri("/api/v1/settings")
        .header(header::COOKIE, format!("michi_web_session={cookie_token}"))
        .body(Body::empty())
        .unwrap();
    let after_res = app2.oneshot(after_req).await.unwrap();
    assert_eq!(after_res.status(), StatusCode::UNAUTHORIZED);

    // Another reboot confirms session was deleted from DB (cold cache check)
    let state3 = AppState::new(cfg1, pool.clone(), Some(admin_id));
    let app3 = create_router(state3);
    let reboot2_req = Request::builder()
        .method("GET")
        .uri("/api/v1/settings")
        .header(header::COOKIE, format!("michi_web_session={cookie_token}"))
        .body(Body::empty())
        .unwrap();
    let reboot2_res = app3.oneshot(reboot2_req).await.unwrap();
    assert_eq!(reboot2_res.status(), StatusCode::UNAUTHORIZED);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn test_multiple_session_isolation_and_bulk_revocation() {
    let (pool, db_path) = test_db_file().await;

    let cfg1 = test_config_for_db(&db_path, "admin", "password123");
    let admin_id = init_admin_user(&cfg1, &pool)
        .await
        .expect("admin user must be initialized")
        .unwrap();

    let state = AppState::new(cfg1.clone(), pool.clone(), Some(admin_id));

    // Create session A and session B
    let token_a = state.auth_sessions.create_session(admin_id).await.unwrap();
    let token_b = state.auth_sessions.create_session(admin_id).await.unwrap();
    assert_ne!(token_a, token_b);

    let app = create_router(state.clone());

    // Both sessions can access /api/v1/settings
    let req_a = Request::builder()
        .method("GET")
        .uri("/api/v1/settings")
        .header(header::AUTHORIZATION, format!("Bearer {token_a}"))
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        app.clone().oneshot(req_a).await.unwrap().status(),
        StatusCode::OK
    );

    let req_b = Request::builder()
        .method("GET")
        .uri("/api/v1/settings")
        .header(header::AUTHORIZATION, format!("Bearer {token_b}"))
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        app.clone().oneshot(req_b).await.unwrap().status(),
        StatusCode::OK
    );

    // Logout session A only
    let logout_a = Request::builder()
        .method("POST")
        .uri("/api/auth/logout")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {token_a}"))
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        app.clone().oneshot(logout_a).await.unwrap().status(),
        StatusCode::OK
    );

    // Session A is now 401
    let check_a = Request::builder()
        .method("GET")
        .uri("/api/v1/settings")
        .header(header::AUTHORIZATION, format!("Bearer {token_a}"))
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        app.clone().oneshot(check_a).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );

    // Session B is STILL 200 (session isolation)
    let check_b = Request::builder()
        .method("GET")
        .uri("/api/v1/settings")
        .header(header::AUTHORIZATION, format!("Bearer {token_b}"))
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        app.clone().oneshot(check_b).await.unwrap().status(),
        StatusCode::OK
    );

    // Rotate password -> triggers bulk revocation of all sessions for user
    let cfg2 = test_config_for_db(&db_path, "admin", "new_password456");
    init_admin_user(&cfg2, &pool)
        .await
        .expect("password rotation")
        .unwrap();

    let state2 = AppState::new(cfg2, pool.clone(), Some(admin_id));
    let app2 = create_router(state2);

    // Session B is now 401 as well
    let check_b2 = Request::builder()
        .method("GET")
        .uri("/api/v1/settings")
        .header(header::AUTHORIZATION, format!("Bearer {token_b}"))
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        app2.oneshot(check_b2).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn test_session_fail_closed_orphan_user() {
    let (pool, db_path) = test_db_file().await;

    let cfg = test_config_for_db(&db_path, "admin", "password123");
    let admin_id = init_admin_user(&cfg, &pool)
        .await
        .expect("admin user must be initialized")
        .unwrap();

    let state = AppState::new(cfg, pool.clone(), Some(admin_id));
    let token = state.auth_sessions.create_session(admin_id).await.unwrap();

    // Session works initially
    let app = create_router(state.clone());
    let req = Request::builder()
        .method("GET")
        .uri("/api/auth/check")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(res.into_body(), 1024 * 64)
        .await
        .unwrap();
    let json: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(json["authenticated"], true);

    // Delete user from SQLite users table to simulate orphaned session
    sqlx::query("DELETE FROM users WHERE id = ?")
        .bind(admin_id.to_string())
        .execute(&pool)
        .await
        .unwrap();

    // Creating a fresh AppState with cold cache to test fail-closed behavior from DB
    let cfg2 = test_config_for_db(&db_path, "admin", "password123");
    let state_reboot = AppState::new(cfg2, pool.clone(), None);
    let app_reboot = create_router(state_reboot.clone());

    // extract_user_id must return None (fail closed)
    let extracted = state_reboot.auth_sessions.extract_user_id(&token).await;
    assert_eq!(extracted, None);

    // /api/auth/check must return authenticated: false
    let check_req = Request::builder()
        .method("GET")
        .uri("/api/auth/check")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let check_res = app_reboot.clone().oneshot(check_req).await.unwrap();
    assert_eq!(check_res.status(), StatusCode::OK);
    let check_bytes = axum::body::to_bytes(check_res.into_body(), 1024 * 64)
        .await
        .unwrap();
    let check_json: Value = serde_json::from_slice(&check_bytes).unwrap();
    assert_eq!(check_json["authenticated"], false);

    // Protected endpoint must reject with 401
    let prot_req = Request::builder()
        .method("GET")
        .uri("/api/v1/settings")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let prot_res = app_reboot.oneshot(prot_req).await.unwrap();
    assert_eq!(prot_res.status(), StatusCode::UNAUTHORIZED);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn test_update_status_matrix_with_mock_source() {
    let _lock = UPDATE_TEST_LOCK.lock().await;
    let (pool, db_path) = test_db_file().await;
    let cfg = test_config_for_db(&db_path, "admin", "password123");
    let admin_id = init_admin_user(&cfg, &pool).await.unwrap().unwrap();

    let state = AppState::new(cfg, pool, Some(admin_id));
    let token = state.auth_sessions.create_session(admin_id).await.unwrap();

    // 1. RC + newer RC on preview channel -> UpdateAvailable
    michi_api::routes::v1::update::clear_releases_cache().await;
    michi_api::routes::v1::update::set_test_release_source(Some(std::sync::Arc::new(
        MockReleaseSource {
            releases: Ok(vec![
                mock_release("v1.0.0-rc.3", true),
                mock_release("v1.0.0-rc.2", true),
            ]),
        },
    )))
    .await;

    let app = create_router(state.clone());
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/update/status?channel=preview")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let json: Value = serde_json::from_slice(
        &axum::body::to_bytes(res.into_body(), 1024 * 64)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(json["status"], "update_available");
    assert_eq!(json["update_available"], true);
    assert_eq!(json["latest_version"], "1.0.0-rc.3");

    // 2. Stable ignores RC -> UpToDate
    michi_api::routes::v1::update::clear_releases_cache().await;
    michi_api::routes::v1::update::set_test_release_source(Some(std::sync::Arc::new(
        MockReleaseSource {
            releases: Ok(vec![mock_release("v1.0.0-rc.3", true)]),
        },
    )))
    .await;

    let app = create_router(state.clone());
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/update/status?channel=stable")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    let json: Value = serde_json::from_slice(
        &axum::body::to_bytes(res.into_body(), 1024 * 64)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(json["status"], "up_to_date");
    assert_eq!(json["update_available"], false);
    assert!(json["latest_version"].is_null());

    // 3. Preview accepts RC -> UpdateAvailable
    michi_api::routes::v1::update::clear_releases_cache().await;
    michi_api::routes::v1::update::set_test_release_source(Some(std::sync::Arc::new(
        MockReleaseSource {
            releases: Ok(vec![mock_release("v1.0.0-rc.3", true)]),
        },
    )))
    .await;

    let app = create_router(state.clone());
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/update/status?channel=preview")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    let json: Value = serde_json::from_slice(
        &axum::body::to_bytes(res.into_body(), 1024 * 64)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(json["status"], "update_available");
    assert_eq!(json["update_available"], true);

    // 4. SemVer numeric ordering: rc.10 > rc.2
    michi_api::routes::v1::update::clear_releases_cache().await;
    michi_api::routes::v1::update::set_test_release_source(Some(std::sync::Arc::new(
        MockReleaseSource {
            releases: Ok(vec![
                mock_release("v1.0.0-rc.2", true),
                mock_release("v1.0.0-rc.10", true),
                mock_release("v1.0.0-rc.1", true),
            ]),
        },
    )))
    .await;

    let app = create_router(state.clone());
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/update/status?channel=preview")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    let json: Value = serde_json::from_slice(
        &axum::body::to_bytes(res.into_body(), 1024 * 64)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(json["status"], "update_available");
    assert_eq!(json["update_available"], true);
    assert_eq!(json["latest_version"], "1.0.0-rc.10");

    // 5. Network failure without cache -> CheckFailed (update_available is null, error upstream_unavailable)
    michi_api::routes::v1::update::clear_releases_cache().await;
    michi_api::routes::v1::update::set_test_release_source(Some(std::sync::Arc::new(
        MockReleaseSource {
            releases: Err("connection timeout".to_string()),
        },
    )))
    .await;

    let app = create_router(state.clone());
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/update/status?channel=preview")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    let json: Value = serde_json::from_slice(
        &axum::body::to_bytes(res.into_body(), 1024 * 64)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(json["status"], "check_failed");
    assert!(json["update_available"].is_null());
    assert_eq!(json["error"], "upstream_unavailable");

    // 6. Network failure with stale cache -> StaleCache
    michi_api::routes::v1::update::set_test_cached_releases(
        vec![mock_release("v1.0.0-rc.3", true)],
        std::time::Instant::now() - std::time::Duration::from_secs(7 * 3600),
        "2026-09-10T12:00:00Z".to_string(),
    )
    .await;
    michi_api::routes::v1::update::set_test_release_source(Some(std::sync::Arc::new(
        MockReleaseSource {
            releases: Err("connection refused".to_string()),
        },
    )))
    .await;

    let app = create_router(state.clone());
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/update/status?channel=preview")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    let json: Value = serde_json::from_slice(
        &axum::body::to_bytes(res.into_body(), 1024 * 64)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(json["status"], "stale_cache");
    assert_eq!(json["error"], "upstream_unavailable");
    assert_eq!(json["last_successful_check_at"], "2026-09-10T12:00:00Z");

    // Reset test release source and cache
    michi_api::routes::v1::update::set_test_release_source(None).await;
    michi_api::routes::v1::update::clear_releases_cache().await;

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn test_update_invalid_version_fail_closed() {
    let _lock = UPDATE_TEST_LOCK.lock().await;
    let (pool, db_path) = test_db_file().await;
    let mut cfg = test_config_for_db(&db_path, "admin", "adminpass123");
    cfg.version = "banana"; // invalid semver

    let admin_id = init_admin_user(&cfg, &pool).await.unwrap().unwrap();
    let state = AppState::new(cfg, pool.clone(), Some(admin_id));
    let token = state.auth_sessions.create_session(admin_id).await.unwrap();

    michi_api::routes::v1::update::clear_releases_cache().await;
    michi_api::routes::v1::update::set_test_release_source(Some(std::sync::Arc::new(
        MockReleaseSource {
            releases: Ok(vec![mock_release("v1.0.0-rc.3", true)]),
        },
    )))
    .await;

    let app = create_router(state.clone());
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/update/status?channel=preview")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let json: Value = serde_json::from_slice(
        &axum::body::to_bytes(res.into_body(), 1024 * 64)
            .await
            .unwrap(),
    )
    .unwrap();

    assert_eq!(json["status"], "check_failed");
    assert_eq!(json["error"], "invalid_current_version");
    assert!(json["update_available"].is_null());

    michi_api::routes::v1::update::set_test_release_source(None).await;
    michi_api::routes::v1::update::clear_releases_cache().await;
    let _ = std::fs::remove_file(db_path);
}

struct CountingReleaseSource {
    count: std::sync::atomic::AtomicUsize,
    releases: Vec<michi_api::routes::v1::update::GitHubRelease>,
}

#[async_trait::async_trait]
impl michi_api::routes::v1::update::ReleaseSource for CountingReleaseSource {
    async fn fetch_releases(
        &self,
    ) -> Result<Vec<michi_api::routes::v1::update::GitHubRelease>, String> {
        self.count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        Ok(self.releases.clone())
    }
}

#[tokio::test]
async fn test_update_concurrent_refresh_coalescing() {
    let _lock = UPDATE_TEST_LOCK.lock().await;
    let (pool, db_path) = test_db_file().await;
    let cfg = test_config_for_db(&db_path, "admin", "adminpass123");
    let admin_id = init_admin_user(&cfg, &pool).await.unwrap().unwrap();
    let state = AppState::new(cfg, pool.clone(), Some(admin_id));
    let token = state.auth_sessions.create_session(admin_id).await.unwrap();

    michi_api::routes::v1::update::clear_releases_cache().await;
    let counting_source = std::sync::Arc::new(CountingReleaseSource {
        count: std::sync::atomic::AtomicUsize::new(0),
        releases: vec![mock_release("v1.0.0-rc.3", true)],
    });
    michi_api::routes::v1::update::set_test_release_source(Some(counting_source.clone())).await;

    // Launch 5 concurrent status requests
    let mut handles = Vec::new();
    for _ in 0..5 {
        let app = create_router(state.clone());
        let tok = token.clone();
        handles.push(tokio::spawn(async move {
            let req = Request::builder()
                .method("GET")
                .uri("/api/v1/update/status?channel=preview")
                .header(header::AUTHORIZATION, format!("Bearer {tok}"))
                .body(Body::empty())
                .unwrap();
            let res = app.oneshot(req).await.unwrap();
            assert_eq!(res.status(), StatusCode::OK);
        }));
    }

    for h in handles {
        h.await.unwrap();
    }

    // Must coalesce to exactly 1 upstream fetch due to FETCH_LOCK
    assert_eq!(
        counting_source
            .count
            .load(std::sync::atomic::Ordering::SeqCst),
        1
    );

    michi_api::routes::v1::update::set_test_release_source(None).await;
    michi_api::routes::v1::update::clear_releases_cache().await;
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn test_logout_sanitized_500_on_persistence_failure() {
    let (pool, db_path) = test_db_file().await;
    let cfg = test_config_for_db(&db_path, "admin", "adminpass123");
    let admin_id = init_admin_user(&cfg, &pool).await.unwrap().unwrap();
    let state = AppState::new(cfg, pool.clone(), Some(admin_id));
    let token = state.auth_sessions.create_session(admin_id).await.unwrap();

    let app = create_router(state.clone());

    // Force database failure on session invalidation by dropping auth_sessions table
    sqlx::query("DROP TABLE auth_sessions")
        .execute(&pool)
        .await
        .unwrap();

    let logout_req = Request::builder()
        .method("POST")
        .uri("/api/auth/logout")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();

    let res = app.oneshot(logout_req).await.unwrap();
    assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);

    // Verify clearing cookie was still set
    let cookie_header = res
        .headers()
        .get(header::SET_COOKIE)
        .expect("must include Set-Cookie")
        .to_str()
        .unwrap();
    assert!(cookie_header.contains("Max-Age=0"));

    // Verify cache control
    assert_eq!(
        res.headers().get(header::CACHE_CONTROL).unwrap(),
        "no-store"
    );

    // Verify sanitized error JSON
    let body_bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(json["status"], "error");
    assert_eq!(json["code"], "SESSION_REVOCATION_FAILED");
    assert_eq!(json["message"], "Unable to revoke the session securely.");

    // Ensure raw sqlite / internal error details are NOT leaked in response
    let raw_body = String::from_utf8_lossy(&body_bytes);
    assert!(!raw_body.contains("sqlite"));
    assert!(!raw_body.contains("table"));
    assert!(!raw_body.contains("DROP"));

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn test_force_backup_restore_purges_active_auth_sessions() {
    let (pool, db_path) = test_db_file().await;
    let cfg = test_config_for_db(&db_path, "admin", "adminpass123");
    let admin_id = init_admin_user(&cfg, &pool).await.unwrap().unwrap();
    let state = AppState::new(cfg, pool.clone(), Some(admin_id));
    let token = state.auth_sessions.create_session(admin_id).await.unwrap();

    let app = create_router(state.clone());

    // 1. Verify session works initially
    let check_req = Request::builder()
        .method("GET")
        .uri("/api/auth/check")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let res = app.clone().oneshot(check_req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    // 2. Perform force backup restore
    let restore_body = serde_json::json!({
        "tracks": [],
        "playlists": [],
        "starred_tracks": [],
        "play_history": [],
        "force": true
    });

    let restore_req = Request::builder()
        .method("POST")
        .uri("/api/v1/backup/restore")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&restore_body).unwrap()))
        .unwrap();

    let restore_res = app.clone().oneshot(restore_req).await.unwrap();
    assert_eq!(restore_res.status(), StatusCode::OK);

    // 3. Verify session was purged from database
    let db_sessions_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM auth_sessions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        db_sessions_count, 0,
        "auth_sessions table must be cleared on force restore"
    );

    // 4. Verify in-memory session cache was cleared
    assert_eq!(state.auth_sessions.sessions.read().await.len(), 0);

    // 5. Subsequent request with the restored session token must fail (401 Unauthorized)
    let check_after = Request::builder()
        .method("GET")
        .uri("/api/auth/check")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let res_after = app.clone().oneshot(check_after).await.unwrap();
    assert_eq!(res_after.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(res_after.into_body(), 1024 * 64)
        .await
        .unwrap();
    let json: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(json["authenticated"], false);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn test_managed_env_admin_survives_username_rotation_with_other_admin() {
    let (pool, db_path) = test_db_file().await;

    // 1. Initial boot: creates managed admin 'env_admin'
    let cfg1 = test_config_for_db(&db_path, "env_admin", "env_pass_1");
    let env_admin_id = init_admin_user(&cfg1, &pool)
        .await
        .expect("env admin must be initialized")
        .expect("env admin id");

    // 2. Insert another admin manually (managed_by_environment = 0)
    let manual_admin_id = Uuid::new_v4();
    let manual_hash = michi_api::auth::hash_password("manual_pass_123").unwrap();
    michi_db::create_user(&pool, &manual_admin_id, "manual_admin", &manual_hash, true)
        .await
        .unwrap();

    let admins_before = michi_db::list_admin_users(&pool).await.unwrap();
    assert_eq!(admins_before.len(), 2);

    // 3. Rotate env admin username to 'env_admin_v2'
    let cfg2 = test_config_for_db(&db_path, "env_admin_v2", "env_pass_2");
    let reconciled_id = init_admin_user(&cfg2, &pool)
        .await
        .expect("env admin must reconcile")
        .expect("reconciled admin id");

    assert_eq!(
        reconciled_id, env_admin_id,
        "Managed admin ID must be preserved"
    );

    // 4. Verify both admins exist: manual_admin untouched, env_admin renamed
    let admins_after = michi_db::list_admin_users(&pool).await.unwrap();
    assert_eq!(admins_after.len(), 2, "Both admins must still exist");

    let env_user = michi_db::get_user_by_username(&pool, "env_admin_v2")
        .await
        .unwrap()
        .expect("renamed env admin");
    assert_eq!(env_user.0, env_admin_id);

    let env_managed: i64 =
        sqlx::query_scalar("SELECT managed_by_environment FROM users WHERE id = ?")
            .bind(env_admin_id.to_string())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(env_managed, 1);

    let manual_user = michi_db::get_user_by_username(&pool, "manual_admin")
        .await
        .unwrap()
        .expect("manual admin");
    assert_eq!(manual_user.0, manual_admin_id);

    let manual_managed: i64 =
        sqlx::query_scalar("SELECT managed_by_environment FROM users WHERE id = ?")
            .bind(manual_admin_id.to_string())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(manual_managed, 0);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn test_managed_env_admin_repeated_rotations_do_not_accumulate_accounts() {
    let (pool, db_path) = test_db_file().await;

    let names = ["admin_r1", "admin_r2", "admin_r3", "admin_r4"];
    let mut original_id = None;

    for name in names {
        let cfg = test_config_for_db(&db_path, name, "password123");
        let admin_id = init_admin_user(&cfg, &pool)
            .await
            .expect("rotation must succeed")
            .expect("admin id");

        if let Some(prev) = original_id {
            assert_eq!(prev, admin_id, "Admin ID must not change across rotations");
        } else {
            original_id = Some(admin_id);
        }

        let admins = michi_db::list_admin_users(&pool).await.unwrap();
        assert_eq!(
            admins.len(),
            1,
            "Must never accumulate duplicate admin rows"
        );
        assert_eq!(admins[0].username, name);
    }

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn test_ambiguous_legacy_multiple_admins_fail_closed() {
    let (pool, db_path) = test_db_file().await;

    // Insert 2 legacy admins (managed_by_environment = 0)
    let id1 = Uuid::new_v4();
    let id2 = Uuid::new_v4();
    let hash = michi_api::auth::hash_password("legacy_pass_123").unwrap();
    michi_db::create_user(&pool, &id1, "legacy_one", &hash, true)
        .await
        .unwrap();
    michi_db::create_user(&pool, &id2, "legacy_two", &hash, true)
        .await
        .unwrap();

    // Now attempt to boot with a different username
    let cfg = test_config_for_db(&db_path, "legacy_three", "password123");
    let res = init_admin_user(&cfg, &pool).await;
    assert!(
        res.is_err(),
        "Must fail closed on ambiguous multiple legacy admins"
    );
    let err = res.unwrap_err();
    assert!(
        err.contains("Ambiguous administrator state") || err.contains("admin users exist"),
        "Error must clearly explain ambiguous legacy admins: {err}"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn test_managed_admin_username_collision_is_atomic() {
    let (pool, db_path) = test_db_file().await;

    // 1. Create managed admin 'admin'
    let cfg1 = test_config_for_db(&db_path, "admin", "admin_pw_123");
    let admin_id = init_admin_user(&cfg1, &pool).await.unwrap().unwrap();

    // 2. Create normal user 'alice'
    let alice_id = Uuid::new_v4();
    let alice_hash = michi_api::auth::hash_password("alice_pw_123").unwrap();
    michi_db::create_user(&pool, &alice_id, "alice", &alice_hash, false)
        .await
        .unwrap();

    // 3. Try to rotate managed admin to username 'alice' (collision with existing non-admin user)
    let cfg2 = test_config_for_db(&db_path, "alice", "new_admin_pw_123");
    let res = init_admin_user(&cfg2, &pool).await;
    assert!(res.is_err(), "Must fail closed on username collision");
    let err = res.unwrap_err();
    assert!(
        err.contains("collision"),
        "Error must mention collision: {err}"
    );

    // 4. Verify atomic rollback: admin is still 'admin', alice is still 'alice'
    let admin_user = michi_db::get_user_by_username(&pool, "admin")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(admin_user.0, admin_id);

    let alice_user = michi_db::get_user_by_username(&pool, "alice")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(alice_user.0, alice_id);
    assert!(!alice_user.3);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn test_session_delete_db_failure_does_not_clear_ram() {
    let (pool, db_path) = test_db_file().await;
    let cfg = test_config_for_db(&db_path, "admin", "password123");
    let admin_id = init_admin_user(&cfg, &pool).await.unwrap().unwrap();
    let state = AppState::new(cfg, pool.clone(), Some(admin_id));

    let token = state.auth_sessions.create_session(admin_id).await.unwrap();
    assert!(state.auth_sessions.validate(&token).await);

    // Drop auth_sessions table to simulate DB failure on session deletion
    sqlx::query("DROP TABLE auth_sessions")
        .execute(&pool)
        .await
        .unwrap();

    let res = state.auth_sessions.invalidate(&token).await;
    assert!(res.is_err(), "invalidate must fail closed on DB error");

    // In-memory cache must retain session (fail closed, no RAM-DB inconsistency)
    assert!(
        state
            .auth_sessions
            .sessions
            .read()
            .await
            .contains_key(&token),
        "RAM session must NOT be cleared if database deletion failed"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn test_clear_all_sessions_db_failure_does_not_clear_ram() {
    let (pool, db_path) = test_db_file().await;
    let cfg = test_config_for_db(&db_path, "admin", "password123");
    let admin_id = init_admin_user(&cfg, &pool).await.unwrap().unwrap();
    let state = AppState::new(cfg, pool.clone(), Some(admin_id));

    let token1 = state.auth_sessions.create_session(admin_id).await.unwrap();
    let token2 = state.auth_sessions.create_session(admin_id).await.unwrap();
    assert!(state.auth_sessions.validate(&token1).await);
    assert!(state.auth_sessions.validate(&token2).await);

    // Drop auth_sessions table to simulate DB failure
    sqlx::query("DROP TABLE auth_sessions")
        .execute(&pool)
        .await
        .unwrap();

    let res = state.auth_sessions.clear_all_sessions().await;
    assert!(
        res.is_err(),
        "clear_all_sessions must fail closed when DB fails"
    );

    // In-memory cache must still retain both sessions
    let ram = state.auth_sessions.sessions.read().await;
    assert!(ram.contains_key(&token1));
    assert!(ram.contains_key(&token2));

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn test_deployment_platform_guidance_and_compose_manifests() {
    let workspace_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();

    // 1. Root docker-compose.yml must declare MICHI_DEPLOYMENT_PLATFORM=docker
    let root_compose_path = workspace_root.join("docker-compose.yml");
    let root_compose_content =
        std::fs::read_to_string(&root_compose_path).expect("root docker-compose.yml must exist");
    assert!(
        root_compose_content.contains("MICHI_DEPLOYMENT_PLATFORM=docker"),
        "root docker-compose.yml must explicitly declare MICHI_DEPLOYMENT_PLATFORM=docker"
    );

    // 2. CasaOS compose must declare MICHI_DEPLOYMENT_PLATFORM=casaos
    let casaos_compose_path = workspace_root.join("casaos/docker-compose.casaos.yml");
    let casaos_compose_content = std::fs::read_to_string(&casaos_compose_path)
        .expect("casaos/docker-compose.casaos.yml must exist");
    assert!(
        casaos_compose_content.contains("MICHI_DEPLOYMENT_PLATFORM=casaos"),
        "casaos compose must declare MICHI_DEPLOYMENT_PLATFORM=casaos"
    );

    // 3. ZimaOS compose must declare MICHI_DEPLOYMENT_PLATFORM=zimaos
    let zimaos_compose_path = workspace_root.join("casaos/docker-compose.zimaos.yml");
    let zimaos_compose_content = std::fs::read_to_string(&zimaos_compose_path)
        .expect("casaos/docker-compose.zimaos.yml must exist");
    assert!(
        zimaos_compose_content.contains("MICHI_DEPLOYMENT_PLATFORM=zimaos"),
        "zimaos compose must declare MICHI_DEPLOYMENT_PLATFORM=zimaos"
    );

    // 4. Update guidance mappings
    // Docker: must provide docker compose instructions, never fallback unknown instructions
    let info_docker = michi_api::routes::v1::update::compute_update_info(
        "1.0.0-rc.2",
        "docker",
        None,
        None,
        None,
        &[],
        false,
    );
    assert_eq!(info_docker.deployment_platform, "docker");
    assert!(
        info_docker
            .instructions
            .contains("docker compose pull && docker compose up -d"),
        "docker instructions must contain docker compose pull: got {:?}",
        info_docker.instructions
    );
    assert!(
        !info_docker
            .instructions
            .contains("Visit the GitHub release page"),
        "docker instructions must not fall back to generic release page instructions"
    );

    // ZimaOS: must provide ZimaOS instructions
    let info_zima = michi_api::routes::v1::update::compute_update_info(
        "1.0.0-rc.2",
        "zimaos",
        None,
        None,
        None,
        &[],
        false,
    );
    assert_eq!(info_zima.deployment_platform, "zimaos");
    assert!(
        info_zima.instructions.contains("ZimaOS"),
        "zimaos instructions must mention ZimaOS: got {:?}",
        info_zima.instructions
    );

    // CasaOS: must provide CasaOS instructions
    let info_casa = michi_api::routes::v1::update::compute_update_info(
        "1.0.0-rc.2",
        "casaos",
        None,
        None,
        None,
        &[],
        false,
    );
    assert_eq!(info_casa.deployment_platform, "casaos");
    assert!(
        info_casa.instructions.contains("CasaOS"),
        "casaos instructions must mention CasaOS: got {:?}",
        info_casa.instructions
    );

    // Unknown/unspecified: must provide safe generic instructions
    let info_unknown = michi_api::routes::v1::update::compute_update_info(
        "1.0.0-rc.2",
        "unknown",
        None,
        None,
        None,
        &[],
        false,
    );
    assert_eq!(info_unknown.deployment_platform, "unknown");
    assert!(
        info_unknown
            .instructions
            .contains("Visit the GitHub release page"),
        "unknown instructions must provide generic safe instructions: got {:?}",
        info_unknown.instructions
    );
}
