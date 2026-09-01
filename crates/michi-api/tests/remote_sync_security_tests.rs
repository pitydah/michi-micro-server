use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
use michi_api::create_router;
use michi_config::Config;
use sqlx::sqlite::SqlitePoolOptions;
use std::net::SocketAddr;
use tower::ServiceExt;

async fn setup_app(
    remote_sync: bool,
    trust_proxy: bool,
    trusted_proxies: Vec<&str>,
) -> axum::Router {
    let pool = SqlitePoolOptions::new()
        .connect("sqlite::memory:")
        .await
        .unwrap();

    michi_db::run_migrations(&pool).await.unwrap();

    let tmp = std::env::temp_dir().join(format!("michi_sync_test_{}", uuid::Uuid::new_v4()));
    let music_dir = tmp.join("music");
    let _ = std::fs::create_dir_all(&music_dir);

    let config = Config {
        port: 9090,
        music_paths: vec![music_dir],
        config_path: tmp.join("config"),
        cache_path: tmp.join("cache"),
        database_url: "sqlite::memory:".to_string(),
        version: "test",
        sync_peers: Vec::new(),
        sync_name: "test".to_string(),
        listenbrainz_token: None,
        lastfm_token: None,
        scrobble_enabled: false,
        auth_username: None,
        auth_password: None,
        auth_enabled: false,
        allow_registration: false,
        server_id: uuid::Uuid::new_v4(),
        cors_origin: None,
        dev_mode: true,
        resource_profile: michi_core::ResourceProfile::Balanced,
        format_policy: michi_core::AudioFormatPolicy::LosslessOnly,
        stream_profile: michi_core::StreamProfile::Original,
        max_remote_bitrate: 320_000,
        remote_sync,
        language: "en".into(),
        ui: Default::default(),
        auto_backup_enabled: false,
        backup_max_keep: 7,
        job_max_concurrent: 3,
        reconnect_delay_max: 300,
        opensubsonic_enabled: false,
        trust_proxy,
        trusted_proxies: trusted_proxies
            .into_iter()
            .map(|s| s.parse().unwrap())
            .collect(),
    };

    let state = michi_api::AppState::new(config, pool, None);
    create_router(state)
}

fn make_ws_request(
    peer_addr: Option<SocketAddr>,
    xff: Option<&str>,
    x_real_ip: Option<&str>,
) -> Request<Body> {
    let mut req_builder = Request::builder()
        .uri("/api/sync")
        .method("GET")
        .version(axum::http::Version::HTTP_11)
        .header("Upgrade", "websocket")
        .header("Connection", "Upgrade")
        .header("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ==")
        .header("Sec-WebSocket-Version", "13");

    if let Some(x) = xff {
        req_builder = req_builder.header("X-Forwarded-For", x);
    }
    if let Some(r) = x_real_ip {
        req_builder = req_builder.header("X-Real-IP", r);
    }

    let mut req = req_builder.body(Body::empty()).unwrap();
    if let Some(addr) = peer_addr {
        req.extensions_mut().insert(ConnectInfo(addr));
    }
    req
}

#[tokio::test]
async fn test_remote_sync_disabled_direct_public_peer_rejected_even_with_spoofed_xff() {
    let app = setup_app(false, false, vec![]).await;
    let peer: SocketAddr = "203.0.113.50:41234".parse().unwrap();
    let req = make_ws_request(Some(peer), Some("127.0.0.1"), Some("192.168.1.1"));

    let res = app.oneshot(req).await.unwrap();
    assert_eq!(
        res.status(),
        StatusCode::FORBIDDEN,
        "Public peer with spoofed XFF must be rejected with 403"
    );
}

#[tokio::test]
async fn test_remote_sync_disabled_direct_loopback_peer_allowed() {
    let app = setup_app(false, false, vec![]).await;
    let peer: SocketAddr = "127.0.0.1:41234".parse().unwrap();
    let req = make_ws_request(Some(peer), None, None);

    let res = app.oneshot(req).await.unwrap();
    assert_ne!(
        res.status(),
        StatusCode::FORBIDDEN,
        "Local loopback peer must not be rejected with 403"
    );
    assert_eq!(
        res.status(),
        StatusCode::UPGRADE_REQUIRED,
        "In oneshot mock test, allowed peer proceeds past IP check to upgrade handler"
    );
}

#[tokio::test]
async fn test_remote_sync_disabled_direct_lan_peer_allowed() {
    let app = setup_app(false, false, vec![]).await;
    let peer: SocketAddr = "192.168.1.55:41234".parse().unwrap();
    let req = make_ws_request(Some(peer), None, None);

    let res = app.oneshot(req).await.unwrap();
    assert_ne!(
        res.status(),
        StatusCode::FORBIDDEN,
        "LAN private peer must not be rejected with 403"
    );
    assert_eq!(
        res.status(),
        StatusCode::UPGRADE_REQUIRED,
        "In oneshot mock test, allowed LAN peer proceeds past IP check to upgrade handler"
    );
}

#[tokio::test]
async fn test_remote_sync_disabled_trusted_proxy_with_public_xff_rejected() {
    let app = setup_app(false, true, vec!["10.0.0.1"]).await;
    let peer: SocketAddr = "10.0.0.1:41234".parse().unwrap();
    let req = make_ws_request(Some(peer), Some("203.0.113.99, 10.0.0.1"), None);

    let res = app.oneshot(req).await.unwrap();
    assert_eq!(
        res.status(),
        StatusCode::FORBIDDEN,
        "Public client forwarded by trusted proxy must be rejected when remote_sync is false"
    );
}

#[tokio::test]
async fn test_remote_sync_disabled_trusted_proxy_with_private_xff_allowed() {
    let app = setup_app(false, true, vec!["10.0.0.1"]).await;
    let peer: SocketAddr = "10.0.0.1:41234".parse().unwrap();
    let req = make_ws_request(Some(peer), Some("192.168.1.150, 10.0.0.1"), None);

    let res = app.oneshot(req).await.unwrap();
    assert_ne!(
        res.status(),
        StatusCode::FORBIDDEN,
        "Private client forwarded by trusted proxy must not be rejected with 403"
    );
    assert_eq!(
        res.status(),
        StatusCode::UPGRADE_REQUIRED,
        "In oneshot mock test, allowed trusted proxy peer proceeds to upgrade handler"
    );
}

#[tokio::test]
async fn test_remote_sync_disabled_untrusted_proxy_cannot_spoof_private_xff() {
    let app = setup_app(false, true, vec!["10.0.0.1"]).await;
    // Peer is an untrusted public proxy/attacker
    let peer: SocketAddr = "203.0.113.10:41234".parse().unwrap();
    let req = make_ws_request(Some(peer), Some("192.168.1.150"), None);

    let res = app.oneshot(req).await.unwrap();
    assert_eq!(
        res.status(),
        StatusCode::FORBIDDEN,
        "Untrusted peer cannot spoof private XFF to bypass remote_sync"
    );
}

#[tokio::test]
async fn test_remote_sync_disabled_missing_connect_info_fails_closed() {
    let app = setup_app(false, false, vec![]).await;
    let req = make_ws_request(None, None, None);

    let res = app.oneshot(req).await.unwrap();
    assert_eq!(
        res.status(),
        StatusCode::FORBIDDEN,
        "Missing ConnectInfo must fail closed with 403 when remote_sync=false"
    );
}

#[tokio::test]
async fn test_remote_sync_disabled_trusted_proxy_handles_spoofed_prefix_right_to_left() {
    let app = setup_app(false, true, vec!["10.0.0.1"]).await;
    let peer: SocketAddr = "10.0.0.1:41234".parse().unwrap();
    // Attacker sends private XFF prefix, but trusted proxy appends actual remote client 203.0.113.99
    let req = make_ws_request(Some(peer), Some("127.0.0.1, 203.0.113.99"), None);

    let res = app.oneshot(req).await.unwrap();
    assert_eq!(
        res.status(),
        StatusCode::FORBIDDEN,
        "Spoofed private prefix in XFF behind trusted proxy must be ignored in favor of real public client IP"
    );
}

#[tokio::test]
async fn test_remote_sync_disabled_trusted_proxy_missing_forwarded_headers_fails_closed() {
    let app = setup_app(false, true, vec!["10.0.0.1"]).await;
    let peer: SocketAddr = "10.0.0.1:41234".parse().unwrap();
    // Trusted proxy forwards request without any XFF or X-Real-IP
    let req = make_ws_request(Some(peer), None, None);

    let res = app.oneshot(req).await.unwrap();
    assert_eq!(
        res.status(),
        StatusCode::FORBIDDEN,
        "Trusted proxy with missing client IP headers must fail closed (cannot substitute proxy IP for client)"
    );
}

#[tokio::test]
async fn test_remote_sync_disabled_trusted_proxy_invalid_xff_fails_closed() {
    let app = setup_app(false, true, vec!["10.0.0.1"]).await;
    let peer: SocketAddr = "10.0.0.1:41234".parse().unwrap();
    let req = make_ws_request(Some(peer), Some("garbage, not_an_ip"), None);

    let res = app.oneshot(req).await.unwrap();
    assert_eq!(
        res.status(),
        StatusCode::FORBIDDEN,
        "Trusted proxy with unparseable XFF must fail closed"
    );
}

#[tokio::test]
async fn test_remote_sync_disabled_trusted_proxy_mixed_malformed_token_fails_closed() {
    let app = setup_app(false, true, vec!["10.0.0.1"]).await;
    let peer: SocketAddr = "10.0.0.1:41234".parse().unwrap();
    // Attacker sends 127.0.0.1, and an invalid token sits at the rightmost position
    let req = make_ws_request(Some(peer), Some("127.0.0.1, malformed_payload"), None);

    let res = app.oneshot(req).await.unwrap();
    assert_eq!(
        res.status(),
        StatusCode::FORBIDDEN,
        "Trusted proxy with mixed malformed token must fail closed rather than skipping to 127.0.0.1"
    );
}

#[tokio::test]
async fn test_remote_sync_disabled_multihop_trusted_proxies_with_public_client() {
    let app = setup_app(false, true, vec!["10.0.0.1", "10.0.0.2"]).await;
    let peer: SocketAddr = "10.0.0.1:41234".parse().unwrap();
    // Intermediate trusted proxy 10.0.0.2 skipped, resolving true client 203.0.113.99
    let req = make_ws_request(Some(peer), Some("203.0.113.99, 10.0.0.2"), None);

    let res = app.oneshot(req).await.unwrap();
    assert_eq!(
        res.status(),
        StatusCode::FORBIDDEN,
        "Multi-hop trusted proxy chain must skip trusted hops and reject public client"
    );
}

#[tokio::test]
async fn test_remote_sync_disabled_multihop_trusted_proxies_with_private_client() {
    let app = setup_app(false, true, vec!["10.0.0.1", "10.0.0.2"]).await;
    let peer: SocketAddr = "10.0.0.1:41234".parse().unwrap();
    // Intermediate trusted proxy 10.0.0.2 skipped, resolving true private client 192.168.1.100
    let req = make_ws_request(Some(peer), Some("192.168.1.100, 10.0.0.2"), None);

    let res = app.oneshot(req).await.unwrap();
    assert_ne!(
        res.status(),
        StatusCode::FORBIDDEN,
        "Multi-hop trusted proxy chain must allow verified private client"
    );
    assert_eq!(
        res.status(),
        StatusCode::UPGRADE_REQUIRED,
        "In oneshot mock test, allowed peer proceeds to upgrade handler"
    );
}

#[tokio::test]
async fn test_remote_sync_enabled_public_peer_allowed() {
    let app = setup_app(true, false, vec![]).await;
    let peer: SocketAddr = "203.0.113.50:41234".parse().unwrap();
    let req = make_ws_request(Some(peer), None, None);

    let res = app.oneshot(req).await.unwrap();
    assert_ne!(
        res.status(),
        StatusCode::FORBIDDEN,
        "Public peer must not be rejected with 403 when remote_sync=true"
    );
    assert_eq!(
        res.status(),
        StatusCode::UPGRADE_REQUIRED,
        "In oneshot mock test, allowed peer proceeds to upgrade handler"
    );
}

#[tokio::test]
async fn test_remote_sync_real_tcp_websocket_connect() {
    let pool = SqlitePoolOptions::new()
        .connect("sqlite::memory:")
        .await
        .unwrap();
    michi_db::run_migrations(&pool).await.unwrap();

    let tmp = std::env::temp_dir().join(format!("michi_tcp_test_{}", uuid::Uuid::new_v4()));
    let music_dir = tmp.join("music");
    let _ = std::fs::create_dir_all(&music_dir);

    let config = Config {
        port: 0,
        music_paths: vec![music_dir],
        config_path: tmp.join("config"),
        cache_path: tmp.join("cache"),
        database_url: "sqlite::memory:".to_string(),
        version: "test",
        sync_peers: Vec::new(),
        sync_name: "test".to_string(),
        listenbrainz_token: None,
        lastfm_token: None,
        scrobble_enabled: false,
        auth_username: None,
        auth_password: None,
        auth_enabled: false,
        allow_registration: false,
        server_id: uuid::Uuid::new_v4(),
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
        trusted_proxies: vec![],
    };

    let state = michi_api::AppState::new(config, pool, None);
    let app = create_router(state).into_make_service_with_connect_info::<SocketAddr>();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let url = format!("ws://127.0.0.1:{port}/api/sync");
    let (ws_stream, _) = tokio_tungstenite::connect_async(&url)
        .await
        .expect("WebSocket connection over real local TCP must succeed when remote_sync=false");
    drop(ws_stream);
}
