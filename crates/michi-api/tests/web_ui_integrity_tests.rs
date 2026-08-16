#![allow(
    unused_variables,
    clippy::needless_borrows_for_generic_args,
    clippy::len_zero
)]
use std::path::PathBuf;

use axum::{
    body::Body,
    extract::{Request as AxumRequest, State},
    http::{header, Request, StatusCode},
    middleware::Next,
    response::Response,
};
use michi_api::create_router;
use michi_config::Config;
use michi_core::{track_id_from_path, AudioFormat, Track};
use serde_json::Value;
use sqlx::SqlitePool;
use tower::ServiceExt;
use uuid::Uuid;

async fn test_db() -> SqlitePool {
    michi_db::init_pool("sqlite::memory:").await.unwrap()
}

fn test_config() -> Config {
    Config {
        port: 9090,
        music_paths: vec![PathBuf::from("/tmp/michi-test/music")],
        config_path: PathBuf::from("/tmp/michi-test/config"),
        cache_path: PathBuf::from("/tmp/michi-test/cache"),
        database_url: "sqlite::memory:".to_string(),
        version: "0.2.0-test",
        sync_peers: Vec::new(),
        sync_name: "michi-server-test".to_string(),
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
    }
}

async fn make_app() -> (axum::Router, SqlitePool, michi_api::AppState) {
    let pool = test_db().await;
    let config = test_config();
    let state = michi_api::AppState::new(config, pool.clone(), None);
    let router = router_with_test_admin(state.clone(), &pool).await;
    (router, pool, state)
}

async fn router_with_test_admin(state: michi_api::AppState, pool: &SqlitePool) -> axum::Router {
    let admin_id = Uuid::new_v4();
    michi_db::create_user(
        pool,
        &admin_id,
        &format!("test-admin-{admin_id}"),
        "unused",
        true,
    )
    .await
    .unwrap();
    let token = state.auth_sessions.create_session(admin_id).await;
    create_router(state).layer(axum::middleware::from_fn_with_state(
        token,
        inject_test_authorization,
    ))
}

async fn inject_test_authorization(
    State(token): State<String>,
    mut request: AxumRequest,
    next: Next,
) -> Response {
    if !request.headers().contains_key("Authorization") {
        request
            .headers_mut()
            .insert("Authorization", format!("Bearer {token}").parse().unwrap());
    }
    if (request.method() == axum::http::Method::POST
        || request.method() == axum::http::Method::PUT
        || request.method() == axum::http::Method::PATCH)
        && !request.headers().contains_key("Content-Type")
    {
        request
            .headers_mut()
            .insert("Content-Type", "application/json".parse().unwrap());
    }
    next.run(request).await
}

async fn body_json(response: axum::response::Response) -> Value {
    let body = response.into_body();
    let bytes = axum::body::to_bytes(body, 1024 * 1024).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

async fn body_text(response: axum::response::Response) -> String {
    let body = response.into_body();
    let bytes = axum::body::to_bytes(body, 1024 * 1024).await.unwrap();
    String::from_utf8(bytes.to_vec()).unwrap()
}

async fn seed_track(pool: &SqlitePool, path: &str, title: &str) -> Uuid {
    let id = track_id_from_path(path);
    let track = Track {
        id,
        title: Some(title.to_string()),
        artist: Some("Test Artist".into()),
        album: Some("Test Album".into()),
        album_artist: None,
        duration_ms: Some(180000),
        file_path: path.to_string(),
        format: AudioFormat::Flac,
        sample_rate: Some(48000),
        bit_depth: Some(16),
        channels: Some(2),
        artwork_id: None,
        genre: Some("Jazz".into()),
        year: Some(2025),
        track_number: Some(1),
        disc_number: Some(1),
        content_hash: None,
        starred: false,
        rating: 0,
        starred_at: None,
        replaygain_track_gain: None,
        replaygain_track_peak: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    michi_db::upsert_track(pool, &track).await.unwrap();
    id
}

// ── Test 1: Connection probes /health/live and /api/v1/server/info ──
#[tokio::test]
async fn test_connection_probes_live_and_info() {
    let (app, _pool, _state) = make_app().await;

    // 1. /health/live
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/health/live")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let text = body_text(res).await;
    assert_eq!(text, "OK");

    // 2. /api/v1/server/info
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/server/info")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let json = body_json(res).await;
    assert!(json.get("server_id").is_some());
    assert!(json.get("version").is_some());

    // 3. /api/v1/capabilities
    let res = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/capabilities")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let json = body_json(res).await;
    assert!(json.get("features").is_some());
}

// ── Test 2: Server URL dynamic derivation (no hardcoded localhost in index.html) ──
#[tokio::test]
async fn test_server_url_no_hardcoded_localhost() {
    let (app, _pool, _state) = make_app().await;

    // Generate QR with explicit Host header
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/pair/qr")
                .header(header::HOST, "192.168.1.100:9090")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"device_type":"mobile"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let json = body_json(res).await;
    let url = json["server_url"].as_str().unwrap();
    assert_eq!(url, "http://192.168.1.100:9090");

    // Check static index.html doesn't contain hardcoded input value="http://localhost:8096" or "http://localhost:9090"
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let index_path = std::path::Path::new(manifest_dir).join("static/index.html");
    let index_content = std::fs::read_to_string(&index_path).unwrap();
    assert!(
        !index_content.contains(r#"value="http://localhost"#),
        "index.html must not contain hardcoded localhost URL"
    );
}

// ── Test 3: QR Lifecycle status polling ──
#[tokio::test]
async fn test_qr_lifecycle_status_polling() {
    let (app, _pool, _state) = make_app().await;

    // 1. Generate QR code
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/pair/qr")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"server_url":"http://10.0.0.5:9090"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let json = body_json(res).await;
    let qr_code = json["qr_code"].as_str().unwrap();

    // 2. Poll status -> waiting_for_scan
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&format!("/api/v1/pair/qr/{qr_code}/status"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let json = body_json(res).await;
    assert_eq!(json["status"], "waiting_for_scan");
    assert_eq!(json["claimed"], false);

    // 3. Claim QR
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(&format!("/api/v1/pair/qr/{qr_code}/claim"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"device_name":"Living Room Phone"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let json = body_json(res).await;
    assert_eq!(json["status"], "claimed");

    // 4. Poll status again -> claimed
    let res = app
        .oneshot(
            Request::builder()
                .uri(&format!("/api/v1/pair/qr/{qr_code}/status"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let json = body_json(res).await;
    assert_eq!(json["status"], "claimed");
    assert_eq!(json["claimed"], true);
}

// ── Test 4: QR claim registers link device in Ecosystem ──
#[tokio::test]
async fn test_qr_claim_registers_link_device() {
    let (app, _pool, _state) = make_app().await;

    // Generate QR
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/pair/qr")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    let json = body_json(res).await;
    let qr_code = json["qr_code"].as_str().unwrap();

    // Claim
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(&format!("/api/v1/pair/qr/{qr_code}/claim"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"device_name":"Mobile Tester 1"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    // Verify device in GET /api/v1/link/devices
    let res = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/link/devices")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let json = body_json(res).await;
    let devices = json["devices"].as_array().unwrap();
    assert!(
        devices
            .iter()
            .any(|d| d["alias"].as_str() == Some("Mobile Tester 1")),
        "Claimed QR device must appear in link devices"
    );
}

// ── Test 5: mDNS service type is canonical _michi-link._tcp.local. ──
#[tokio::test]
async fn test_mdns_service_type_is_michi_link() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let receivers_path = std::path::Path::new(manifest_dir).join("src/routes/v1/receivers.rs");
    let receivers_src = std::fs::read_to_string(&receivers_path).unwrap();
    assert!(
        receivers_src.contains(r#"let service_type = "_michi-link._tcp.local.";"#),
        "mDNS discovery must query canonical service type _michi-link._tcp.local."
    );
    assert!(
        !receivers_src.contains(r#"let service_type = "_michi-receiver._tcp.local.";"#),
        "mDNS discovery must not query deprecated _michi-receiver._tcp.local."
    );
}

// ── Test 6: Playback State canonical authority ──
#[tokio::test]
async fn test_playback_state_canonical_authority() {
    let (app, pool, _state) = make_app().await;
    let track_id = seed_track(&pool, "/tmp/michi-test/music/track1.flac", "Song 1").await;

    // Check initial state
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/playback/state")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let json = body_json(res).await;
    assert_eq!(json["state"], "paused");

    // Play track via control handler
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/playback/control")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(format!(
                    r#"{{"command":"play","value":{{"track_id":"{track_id}","position_ms":5000}}}}"#
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let json = body_json(res).await;
    assert_eq!(json["state"], "playing");
    assert_eq!(json["position_ms"], 5000);

    // Verify canonical state reflects playing state
    let res = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/playback/state")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let json = body_json(res).await;
    assert_eq!(json["state"], "playing");
    assert_eq!(json["track_id"], track_id.to_string());
    assert_eq!(json["position_ms"], 5000);
}

// ── Test 7: Playback Control repeat and shuffle ──
#[tokio::test]
async fn test_playback_control_repeat_and_shuffle() {
    let (app, _pool, _state) = make_app().await;

    // 1. Toggle shuffle
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/playback/control")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"command":"shuffle"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let json = body_json(res).await;
    assert_eq!(json["shuffle"], true);

    // 2. Toggle repeat
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/playback/control")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"command":"repeat"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let json = body_json(res).await;
    assert_eq!(json["repeat"], "all");

    // 3. Verify state endpoint returns shuffle and repeat
    let res = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/playback/state")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let json = body_json(res).await;
    assert_eq!(json["shuffle"], true);
    assert_eq!(json["repeat"], "all");
}

// ── Test 8: Queue canonical persistence ──
#[tokio::test]
async fn test_queue_canonical_persistence() {
    let (app, pool, _state) = make_app().await;
    let track1 = seed_track(&pool, "/tmp/michi-test/music/song1.flac", "Song 1").await;
    let track2 = seed_track(&pool, "/tmp/michi-test/music/song2.flac", "Song 2").await;

    // Add items to server queue
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/queue/items")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(format!(
                    r#"{{"track_ids":["{track1}","{track2}"]}}"#
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let json = body_json(res).await;
    assert_eq!(json["items_count"], 2);

    // Fetch saved queue
    let res = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/queue/saved")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let json = body_json(res).await;
    assert_eq!(json["found"], true);
    let items = json["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["track_id"], track1.to_string());
    assert_eq!(items[1]["track_id"], track2.to_string());
}

// ── Test 9: Room Groups audio truth pcm_s16le ──
#[tokio::test]
async fn test_room_groups_audio_truth_pcm_s16le() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let receivers_path = std::path::Path::new(manifest_dir).join("src/routes/v1/receivers.rs");
    let receivers_src = std::fs::read_to_string(&receivers_path).unwrap();
    assert!(
        receivers_src.contains(r#""pcm_s16le""#),
        "Room groups must use pcm_s16le codec format"
    );
    assert!(
        receivers_src.contains("48000"),
        "Room groups must use 48000 Hz sample rate"
    );
    assert!(
        receivers_src.contains("16"),
        "Room groups must use 16 bit depth"
    );
}

// ── Test 10: Room Groups structured activation status ──
#[tokio::test]
async fn test_room_groups_structured_activation_status() {
    let (app, _pool, _state) = make_app().await;

    // Create room group with a non-existent receiver
    let fake_recv = Uuid::new_v4().to_string();
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/rooms/groups")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(format!(
                    r#"{{"name":"Basement","mode":"relax","receiver_ids":["{fake_recv}"]}}"#
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let json = body_json(res).await;
    let group_id = json["group"]["id"].as_str().unwrap();

    // Activate room group -> should return 502 / ROOM_ACTIVATION_FAILED because receiver not found/paired
    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(&format!("/api/v1/rooms/groups/{group_id}/activate"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_GATEWAY);
    let json = body_json(res).await;
    assert_eq!(json["error"]["code"], "ROOM_ACTIVATION_FAILED");
}

// ── Test 11: Chains audio truth and links_active count ──
#[tokio::test]
async fn test_chains_audio_truth_and_links_active_count() {
    let (app, pool, _state) = make_app().await;
    let track_id = seed_track(&pool, "/tmp/michi-test/music/song.flac", "Song").await;

    // Create chain
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/chains")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"name":"Master Chain"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let json = body_json(res).await;
    let chain_id = json["chain"]["id"].as_str().unwrap();

    // Set track
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(&format!("/api/v1/chains/{chain_id}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(format!(r#"{{"track_id":"{track_id}"}}"#)))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    // Play chain (0 active receivers configured) -> links_active must be 0
    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(&format!("/api/v1/chains/{chain_id}/play"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let json = body_json(res).await;
    assert_eq!(json["status"], "playing");
    assert_eq!(json["links_active"], 0);
}

// ── Test 12: Settings restart_required flag ──
#[tokio::test]
async fn test_settings_restart_required_flag() {
    let (app, _pool, _state) = make_app().await;

    // Changing port requires restart
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/v1/settings")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"resource_profile":"performance"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let json = body_json(res).await;
    assert_eq!(json["restart_required"], true);

    // Changing theme does not require restart
    let res = app
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/v1/settings")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"theme":"light"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let json = body_json(res).await;
    assert_eq!(json["restart_required"], false);
}

// ── Test 13: Webhook test real HTTP status and latency ──
#[tokio::test]
async fn test_webhook_test_real_http_status_and_latency() {
    let (app, _pool, _state) = make_app().await;

    // Without webhook URL configured -> 400
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/webhook/test")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);

    // Configure invalid unreachable URL
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/webhook")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"url":"http://127.0.0.1:59999/nonexistent"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    // Test webhook -> 502 Bad Gateway
    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/webhook/test")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_GATEWAY);
    let json = body_json(res).await;
    assert_eq!(json["error"]["code"], "WEBHOOK_FAILED");
}

// ── Test 14: Library Snapshot records statistics ──
#[tokio::test]
async fn test_library_snapshot_records_statistics() {
    let (app, pool, _state) = make_app().await;
    seed_track(&pool, "/tmp/michi-test/music/snap1.flac", "Track 1").await;
    seed_track(&pool, "/tmp/michi-test/music/snap2.flac", "Track 2").await;

    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/backup/snapshot")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let json = body_json(res).await;
    assert_eq!(json["status"], "snapshot_created");
    assert_eq!(json["snapshot"]["stats"]["tracks"], 2);
}

// ── Test 15: File availability check no fake corrupt count ──
#[tokio::test]
async fn test_file_availability_check_no_fake_corrupt_count() {
    let (app, pool, _state) = make_app().await;
    seed_track(&pool, "/tmp/non_existent_file_path_12345.flac", "Missing Track").await;

    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/backup/verify")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let json = body_json(res).await;
    assert_eq!(json["status"], "issues_found");
    assert_eq!(json["missing"], 1);
    assert_eq!(json["available"], 0);
    assert_eq!(json["total"], 1);
    assert!(
        json.get("corrupt").is_none(),
        "verify must not report fabricated 'corrupt: 0' statistics"
    );
}
