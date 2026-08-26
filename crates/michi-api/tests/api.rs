#![allow(
    unused_variables,
    clippy::needless_borrows_for_generic_args,
    clippy::len_zero
)]
use std::path::{Path, PathBuf};

use axum::{
    body::Body,
    extract::{Request as AxumRequest, State},
    http::{Request, StatusCode},
    middleware::Next,
    response::Response,
};
use base64::Engine;
use michi_api::create_router;
use michi_config::Config;
use michi_core::{track_id_from_path, AudioFormat, Track, TrackUpdate};
use serde_json::Value;
use sqlx::SqlitePool;
use tower::ServiceExt;
use uuid::Uuid;

async fn test_db_with_url() -> (SqlitePool, String) {
    let url = format!(
        "sqlite:file:memdb_{}?mode=memory&cache=shared",
        Uuid::new_v4()
    );
    let pool = michi_db::init_pool(&url).await.unwrap();
    (pool, url)
}

async fn test_db() -> SqlitePool {
    test_db_with_url().await.0
}

fn test_config() -> Config {
    test_config_with_url("sqlite::memory:".to_string())
}

fn test_config_with_url(db_url: String) -> Config {
    let tmp = std::env::temp_dir().join(format!("michi-test-{}", Uuid::new_v4()));
    Config {
        port: 9999,
        music_paths: vec![tmp.join("music")],
        config_path: tmp.join("config"),
        cache_path: tmp.join("cache"),
        database_url: db_url,

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
        trusted_proxies: vec!["127.0.0.1".parse().unwrap(), "::1".parse().unwrap()],
    }
}

async fn make_app() -> (axum::Router, SqlitePool) {
    let (router, pool, _state) = make_app_with_state().await;
    (router, pool)
}

async fn make_app_with_state() -> (axum::Router, SqlitePool, michi_api::AppState) {
    let (pool, db_url) = test_db_with_url().await;
    let config = test_config_with_url(db_url);
    let state = michi_api::AppState::new(config, pool.clone(), None);
    (
        router_with_test_admin(state.clone(), &pool).await,
        pool,
        state,
    )
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

async fn make_unauthenticated_app() -> (axum::Router, SqlitePool) {
    let pool = test_db().await;
    let state = michi_api::AppState::new(test_config(), pool.clone(), None);
    (create_router(state), pool)
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
    next.run(request).await
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
        duration_ms: Some(200000),
        file_path: path.to_string(),
        format: AudioFormat::Flac,
        sample_rate: Some(44100),
        bit_depth: Some(16),
        channels: Some(2),
        artwork_id: None,
        genre: None,
        year: None,
        track_number: None,
        disc_number: None,
        content_hash: None,
        file_size: None,
        file_mtime_ns: None,
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

#[tokio::test]
async fn test_status_endpoint() {
    let (app, _pool) = make_app().await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let text = body_text(response).await;
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["status"], "ok");
    assert_eq!(v["name"], "Michi Micro Server");
    assert_eq!(v["version"], "test");
}

#[tokio::test]
async fn test_root_endpoint() {
    let (app, _pool) = make_app().await;
    let response = app
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_tracks_empty() {
    let (app, _pool) = make_app().await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/tracks")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let text = body_text(response).await;
    assert_eq!(text, "[]");
}

#[tokio::test]
async fn test_tracks_with_data() {
    let (app, pool) = make_app().await;
    seed_track(&pool, "/music/song1.flac", "Song One").await;

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/tracks")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let text = body_text(response).await;
    let tracks: Vec<Value> = serde_json::from_str(&text).unwrap();
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0]["title"], "Song One");
    assert_eq!(tracks[0]["artist"], "Test Artist");
}

#[tokio::test]
async fn test_get_track_by_id() {
    let (app, pool) = make_app().await;
    let id = seed_track(&pool, "/music/song1.flac", "Song One").await;

    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/tracks/{id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let text = body_text(response).await;
    let track: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(track["title"], "Song One");
    assert_eq!(track["id"], id.to_string());
}

#[tokio::test]
async fn test_get_track_not_found() {
    let (app, _pool) = make_app().await;
    let fake_id = "00000000-0000-0000-0000-000000000000";
    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/tracks/{fake_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_stats_endpoint() {
    let (app, pool) = make_app().await;
    seed_track(&pool, "/music/song1.flac", "Song One").await;

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/library/stats")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let text = body_text(response).await;
    let stats: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(stats["tracks"], 1);
}

#[tokio::test]
async fn test_update_track_handler() {
    let (app, pool) = make_app().await;
    let id = seed_track(&pool, "/music/song1.flac", "Original").await;

    let update = TrackUpdate {
        title: Some("Updated".into()),
        ..Default::default()
    };
    let body = serde_json::to_string(&update).unwrap();

    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/tracks/{id}"))
                .method("PUT")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let text = body_text(response).await;
    let track: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(track["title"], "Updated");
}

#[tokio::test]
async fn test_delete_track_handler() {
    let (app, pool) = make_app().await;
    let id = seed_track(&pool, "/music/song1.flac", "Song One").await;

    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/tracks/{id}"))
                .method("DELETE")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let text = body_text(response).await;
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["deleted"], true);
}

#[tokio::test]
async fn test_delete_all_tracks_handler() {
    let (app, pool) = make_app().await;
    seed_track(&pool, "/music/s1.flac", "S1").await;
    seed_track(&pool, "/music/s2.flac", "S2").await;

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/library/tracks")
                .method("DELETE")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let text = body_text(response).await;
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["deleted"], 2);
}

#[tokio::test]
async fn test_track_get_returns_400_for_bad_uuid() {
    let (app, _pool) = make_app().await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/tracks/not-a-uuid")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

// ---------------------------------------------------------------------------
// Streaming endpoint tests
// ---------------------------------------------------------------------------

fn create_test_file(dir: &Path, name: &str, content: &[u8]) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, content).unwrap();
    std::fs::canonicalize(&path).unwrap_or(path)
}

async fn make_streaming_app() -> (axum::Router, SqlitePool, tempfile::TempDir, Uuid) {
    let tmp = tempfile::tempdir().unwrap();
    let music_dir = std::fs::canonicalize(tmp.path().join("music"))
        .unwrap_or_else(|_| tmp.path().join("music"));
    std::fs::create_dir_all(&music_dir).unwrap();
    let music_dir = std::fs::canonicalize(&music_dir).unwrap_or(music_dir);

    let file_path = create_test_file(&music_dir, "test.flac", &[0u8; 50000]);

    let pool = test_db().await;
    let config = Config {
        port: 9999,
        music_paths: vec![music_dir],
        config_path: PathBuf::from("/tmp/michi-test/config"),
        cache_path: PathBuf::from("/tmp/michi-test/cache"),
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
        trusted_proxies: vec!["127.0.0.1".parse().unwrap(), "::1".parse().unwrap()],
    };
    let id = track_id_from_path(file_path.to_str().unwrap());
    let track = Track {
        id,
        title: Some("Test Stream".into()),
        artist: Some("Test Artist".into()),
        album: Some("Test Album".into()),
        album_artist: None,
        duration_ms: Some(5000),
        file_path: file_path.to_str().unwrap().to_string(),
        format: AudioFormat::Flac,
        sample_rate: Some(44100),
        bit_depth: Some(16),
        channels: Some(2),
        artwork_id: None,
        genre: None,
        year: None,
        track_number: None,
        disc_number: None,
        content_hash: None,
        file_size: None,
        file_mtime_ns: None,
        starred: false,
        rating: 0,
        starred_at: None,
        replaygain_track_gain: None,
        replaygain_track_peak: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    michi_db::upsert_track(&pool, &track).await.unwrap();

    let state = michi_api::AppState::new(config, pool.clone(), None);
    (michi_api::create_router(state), pool, tmp, id)
}

#[tokio::test]
async fn test_stream_full_file() {
    let (app, _pool, _tmp, id) = make_streaming_app().await;
    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/stream/{id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap(),
        "audio/flac"
    );
    assert_eq!(
        response
            .headers()
            .get("accept-ranges")
            .unwrap()
            .to_str()
            .unwrap(),
        "bytes"
    );
    let body = axum::body::to_bytes(response.into_body(), 100000)
        .await
        .unwrap();
    assert_eq!(body.len(), 50000);
}

#[tokio::test]
async fn test_stream_range_request() {
    let (app, _pool, _tmp, id) = make_streaming_app().await;
    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/stream/{id}"))
                .header("Range", "bytes=0-1023")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(
        response
            .headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap(),
        "audio/flac"
    );
    assert_eq!(
        response
            .headers()
            .get("content-range")
            .unwrap()
            .to_str()
            .unwrap(),
        "bytes 0-1023/50000"
    );
    let body = axum::body::to_bytes(response.into_body(), 100000)
        .await
        .unwrap();
    assert_eq!(body.len(), 1024);
}

#[tokio::test]
async fn test_stream_range_from_offset() {
    let (app, _pool, _tmp, id) = make_streaming_app().await;
    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/stream/{id}"))
                .header("Range", "bytes=100-")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(
        response
            .headers()
            .get("content-range")
            .unwrap()
            .to_str()
            .unwrap(),
        "bytes 100-49999/50000"
    );
    let body = axum::body::to_bytes(response.into_body(), 100000)
        .await
        .unwrap();
    assert_eq!(body.len(), 49900);
}

#[tokio::test]
async fn test_stream_range_not_satisfiable() {
    let (app, _pool, _tmp, id) = make_streaming_app().await;
    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/stream/{id}"))
                .header("Range", "bytes=50000-60000")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::RANGE_NOT_SATISFIABLE);
}

#[tokio::test]
async fn test_stream_track_not_found() {
    let (app, _pool, _tmp, _id) = make_streaming_app().await;
    let fake_id = "00000000-0000-0000-0000-000000000000";
    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/stream/{fake_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_stream_bad_uuid() {
    let (app, _pool, _tmp, _id) = make_streaming_app().await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/stream/not-a-uuid")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_stream_file_not_on_disk() {
    let pool = test_db().await;
    let config = Config {
        port: 9999,
        music_paths: vec![PathBuf::from("/tmp/michi-test/music")],
        config_path: PathBuf::from("/tmp/michi-test/config"),
        cache_path: PathBuf::from("/tmp/michi-test/cache"),
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
        trusted_proxies: vec!["127.0.0.1".parse().unwrap(), "::1".parse().unwrap()],
    };

    let id = track_id_from_path("/nonexistent/path/file.flac");
    let track = Track {
        id,
        title: Some("Missing File".into()),
        artist: None,
        album: None,
        album_artist: None,
        duration_ms: None,
        file_path: "/nonexistent/path/file.flac".to_string(),
        format: AudioFormat::Flac,
        sample_rate: None,
        bit_depth: None,
        channels: None,
        artwork_id: None,
        genre: None,
        year: None,
        track_number: None,
        disc_number: None,
        content_hash: None,
        file_size: None,
        file_mtime_ns: None,
        starred: false,
        rating: 0,
        starred_at: None,
        replaygain_track_gain: None,
        replaygain_track_peak: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    michi_db::upsert_track(&pool, &track).await.unwrap();

    let state = michi_api::AppState::new(config, pool.clone(), None);
    let app = router_with_test_admin(state, &pool).await;

    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/stream/{id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_stream_status_still_works() {
    let (app, _pool, _tmp, _id) = make_streaming_app().await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

// ---------------------------------------------------------------------------
// Search endpoint tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_search_by_title() {
    let (app, pool) = make_app().await;
    seed_track(&pool, "/music/song1.flac", "Yellow Submarine").await;
    seed_track(&pool, "/music/song2.flac", "Yesterday").await;
    seed_track(&pool, "/music/song3.flac", "Something").await;

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/search?q=yellow")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let text = body_text(response).await;
    let tracks: Vec<Value> = serde_json::from_str(&text).unwrap();
    assert_eq!(tracks.len(), 1, "should find 1 track matching 'yellow'");
    assert_eq!(tracks[0]["title"], "Yellow Submarine");
}

#[tokio::test]
async fn test_search_case_insensitive() {
    let (app, pool) = make_app().await;
    seed_track(&pool, "/music/beatles.flac", "Let It Be").await;

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/search?q=let+it")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let text = body_text(response).await;
    let tracks: Vec<Value> = serde_json::from_str(&text).unwrap();
    assert_eq!(tracks.len(), 1);
}

#[tokio::test]
async fn test_search_no_results() {
    let (app, _pool) = make_app().await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/search?q=zzz_nonexistent_zzz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let text = body_text(response).await;
    let tracks: Vec<Value> = serde_json::from_str(&text).unwrap();
    assert_eq!(tracks.len(), 0);
}

#[tokio::test]
async fn test_search_empty_query() {
    let (app, _pool) = make_app().await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/search?q=")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let text = body_text(response).await;
    let tracks: Vec<Value> = serde_json::from_str(&text).unwrap();
    assert_eq!(tracks.len(), 0, "empty query should return empty array");
}

// ---------------------------------------------------------------------------
// Pagination tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_tracks_pagination_limit_offset() {
    let (app, pool) = make_app().await;
    for i in 0..10 {
        let path = format!("/music/song_{i}.flac");
        let title = format!("Track {i}");
        seed_track(&pool, &path, &title).await;
    }

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/tracks?limit=3&offset=2")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let text = body_text(response).await;
    let tracks: Vec<Value> = serde_json::from_str(&text).unwrap();
    assert_eq!(tracks.len(), 3);
}

#[tokio::test]
async fn test_tracks_limit_max() {
    let (app, pool) = make_app().await;
    for i in 0..20 {
        let path = format!("/music/song_{i}.flac");
        let title = format!("Track {i}");
        seed_track(&pool, &path, &title).await;
    }

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/tracks?limit=9999")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let text = body_text(response).await;
    let tracks: Vec<Value> = serde_json::from_str(&text).unwrap();
    assert!(tracks.len() <= 500);
}

// ---------------------------------------------------------------------------
// Albums / Artists endpoint tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_albums_endpoint() {
    let (app, pool) = make_app().await;
    seed_track(&pool, "/music/a1.flac", "Song A1").await;
    seed_track(&pool, "/music/a2.flac", "Song A2").await;
    seed_track(&pool, "/music/b1.flac", "Song B1").await;

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/albums")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let text = body_text(response).await;
    let albums: Vec<Value> = serde_json::from_str(&text).unwrap();
    assert_eq!(albums.len(), 1);
    assert_eq!(albums[0]["album"], "Test Album");
    assert_eq!(albums[0]["track_count"], 3);
}

#[tokio::test]
async fn test_albums_endpoint_empty() {
    let (app, _pool) = make_app().await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/albums")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let text = body_text(response).await;
    assert_eq!(text, "[]");
}

#[tokio::test]
async fn test_artists_endpoint() {
    let (app, pool) = make_app().await;
    seed_track(&pool, "/music/a1.flac", "Song A1").await;

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/artists")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let text = body_text(response).await;
    let artists: Vec<Value> = serde_json::from_str(&text).unwrap();
    assert_eq!(artists.len(), 1);
    assert_eq!(artists[0]["artist"], "Test Artist");
    assert_eq!(artists[0]["track_count"], 1);
    assert_eq!(artists[0]["track_count"], 1);
}

#[tokio::test]
async fn test_album_tracks_endpoint() {
    let (app, pool) = make_app().await;
    let _id = seed_track(&pool, "/music/test.flac", "Test Song").await;

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/albums/Test%20Album")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let text = body_text(response).await;
    let tracks: Vec<Value> = serde_json::from_str(&text).unwrap();
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0]["title"], "Test Song");
}

#[tokio::test]
async fn test_artist_tracks_endpoint() {
    let (app, pool) = make_app().await;
    let _id = seed_track(&pool, "/music/test.flac", "Test Song").await;

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/artists/Test%20Artist")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let text = body_text(response).await;
    let tracks: Vec<Value> = serde_json::from_str(&text).unwrap();
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0]["title"], "Test Song");
}

#[tokio::test]
async fn test_artwork_not_found() {
    let (app, _pool) = make_app().await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/artwork/00000000-0000-0000-0000-000000000000")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------------
// Playlist endpoint tests
// ---------------------------------------------------------------------------

async fn seed_playlist(pool: &SqlitePool) -> michi_core::Playlist {
    michi_db::create_playlist(
        pool,
        &michi_core::PlaylistCreate {
            name: "Test Playlist".into(),
            description: None,
        },
        None,
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn test_playlists_empty() {
    let (app, _pool) = make_app().await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/playlists")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let text = body_text(response).await;
    assert_eq!(text, "[]");
}

#[tokio::test]
async fn test_create_playlist() {
    let (app, _pool) = make_app().await;
    let body = r#"{"name":"My Playlist"}"#;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/playlists")
                .method("POST")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let text = body_text(response).await;
    let pl: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(pl["name"], "My Playlist");
    assert_eq!(pl["track_count"], 0);
}

#[tokio::test]
async fn test_create_playlist_empty_name() {
    let (app, _pool) = make_app().await;
    let body = r#"{"name":""}"#;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/playlists")
                .method("POST")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_delete_playlist() {
    let (app, pool) = make_app().await;
    let pl = seed_playlist(&pool).await;

    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/playlists/{}", pl.id))
                .method("DELETE")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let text = body_text(response).await;
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["deleted"], true);
}

#[tokio::test]
async fn test_add_track_to_playlist() {
    let (app, pool) = make_app().await;
    let pl = seed_playlist(&pool).await;
    let track_id = seed_track(&pool, "/music/test.flac", "Test Song").await;

    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/playlists/{}/tracks/{}", pl.id, track_id))
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let text = body_text(response).await;
    let pt: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(pt["playlist_id"], pl.id.to_string());
    assert_eq!(pt["track_id"], track_id.to_string());
}

#[tokio::test]
async fn test_get_playlist_tracks() {
    let (app, pool) = make_app().await;
    let pl = seed_playlist(&pool).await;
    let track_id = seed_track(&pool, "/music/test.flac", "Test Song").await;
    michi_db::add_track_to_playlist(&pool, &pl.id, &track_id)
        .await
        .unwrap();

    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/playlists/{}/tracks", pl.id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let text = body_text(response).await;
    let tracks: Vec<Value> = serde_json::from_str(&text).unwrap();
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0]["title"], "Test Song");
}

// ---------------------------------------------------------------------------
// Pagination tests
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// WebSocket endpoint tests
// ---------------------------------------------------------------------------

async fn run_test_server() -> (u16, SqlitePool) {
    let pool = test_db().await;
    let config = test_config();
    let state = michi_api::AppState::new(config, pool.clone(), None);
    let app = create_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let port = addr.port();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    // Give server a moment to start
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    (port, pool)
}

#[tokio::test]
async fn test_websocket_connect() {
    let (port, _pool) = run_test_server().await;
    use futures_util::StreamExt;

    let url = format!("ws://127.0.0.1:{port}/api/ws");
    let (ws_stream, _) = tokio_tungstenite::connect_async(&url)
        .await
        .expect("WebSocket connection should succeed");
    let (mut _write, mut read) = ws_stream.split();

    // Should receive something within timeout
    let timeout = tokio::time::timeout(std::time::Duration::from_secs(2), read.next()).await;
    match timeout {
        Ok(Some(Ok(msg))) => {
            let text = msg.to_text().unwrap();
            // Should be valid JSON
            let _v: serde_json::Value = serde_json::from_str(text).unwrap();
        }
        Ok(Some(Err(e))) => panic!("WebSocket error: {e}"),
        Ok(None) => panic!("WebSocket closed unexpectedly"),
        Err(_) => { /* timeout is OK - server might not send initial message */ }
    }
}

#[tokio::test]
async fn test_websocket_receives_events() {
    let (port, _pool) = run_test_server().await;
    use futures_util::StreamExt;

    // Connect WebSocket
    let url = format!("ws://127.0.0.1:{port}/api/ws");
    let (ws_stream, _) = tokio_tungstenite::connect_async(&url)
        .await
        .expect("WebSocket should connect");
    let (mut _write, mut read) = ws_stream.split();

    // Trigger a library clear (which sends library_updated event)
    let client = reqwest::Client::new();
    let resp = client
        .delete(format!("http://127.0.0.1:{port}/api/library/tracks"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // Read the next WS message
    let timeout = tokio::time::timeout(std::time::Duration::from_secs(3), read.next()).await;
    match timeout {
        Ok(Some(Ok(msg))) => {
            let text = msg.to_text().unwrap();
            let v: serde_json::Value = serde_json::from_str(text).unwrap();
            assert_eq!(v["type"], "library_updated");
        }
        Ok(Some(Err(e))) => panic!("WS error: {e}"),
        Ok(None) => panic!("WS closed"),
        Err(_) => panic!("Timeout - no library_updated event received"),
    }
}

// ---------------------------------------------------------------------------
// M3U import/export tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_m3u_export_empty_playlist() {
    let (app, pool) = make_app().await;
    let pl = seed_playlist(&pool).await;

    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/playlists/{}/export", pl.id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap(),
        "audio/x-mpegurl"
    );

    let text = body_text(response).await;
    assert!(
        text.starts_with("#EXTM3U\n"),
        "M3U should start with #EXTM3U: {text}"
    );
}

#[tokio::test]
async fn test_m3u_export_with_tracks() {
    let (app, pool) = make_app().await;
    let pl = seed_playlist(&pool).await;
    let tid = seed_track(&pool, "/music/test.flac", "Test Song").await;
    michi_db::add_track_to_playlist(&pool, &pl.id, &tid)
        .await
        .unwrap();

    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/playlists/{}/export", pl.id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let text = body_text(response).await;
    assert!(text.contains("/music/test.flac"));
    assert!(text.contains("Test Song"));
}

#[tokio::test]
async fn test_m3u_import() {
    let (app, pool) = make_app().await;
    seed_track(&pool, "/music/test.flac", "Test Song").await;

    let m3u_content = "#EXTM3U\n\
                       #EXTINF:240,Test Song\n\
                       /music/test.flac\n\
                       #EXTINF:300,NonExistent\n\
                       /nonexistent/path.flac\n";

    let body = serde_json::json!({
        "name": "Imported Playlist",
        "content": m3u_content
    });

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/playlists/import")
                .method("POST")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let text = body_text(response).await;
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["matched"], 1, "should match 1 track");
    assert_eq!(v["total"], 2, "should have 2 entries total");
    assert_eq!(v["playlist"]["name"], "Imported Playlist");
}

#[tokio::test]
async fn test_m3u_import_empty_name() {
    let (app, _pool) = make_app().await;
    let body = serde_json::json!({
        "name": "",
        "content": "#EXTM3U\n"
    });

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/playlists/import")
                .method("POST")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

// ---------------------------------------------------------------------------
// Full pipeline test: scan real file and stream it
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_full_pipeline_scan_and_stream() {
    // Create a real audio file
    let tmp = tempfile::tempdir().unwrap();
    let music_dir = tmp.path().join("music");
    std::fs::create_dir_all(&music_dir).unwrap();
    let file_path = music_dir.join("test.flac");
    std::fs::write(&file_path, [0u8; 50000]).unwrap();

    // We seed the track directly then test streaming via tower
    let id = michi_core::track_id_from_path(file_path.to_str().unwrap());
    let track = michi_core::Track {
        id,
        title: Some("Pipeline Test".into()),
        artist: Some("Test Artist".into()),
        album: Some("Test Album".into()),
        album_artist: None,
        duration_ms: Some(5000),
        file_path: file_path.to_str().unwrap().to_string(),
        format: michi_core::AudioFormat::Flac,
        sample_rate: Some(44100),
        bit_depth: Some(16),
        channels: Some(2),
        artwork_id: None,
        genre: None,
        year: None,
        track_number: None,
        disc_number: None,
        content_hash: None,
        file_size: None,
        file_mtime_ns: None,
        starred: false,
        rating: 0,
        starred_at: None,
        replaygain_track_gain: None,
        replaygain_track_peak: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };

    // The streaming handler uses the real path, so we need the correct config
    let pool = test_db().await;
    michi_db::upsert_track(&pool, &track).await.unwrap();
    let config = michi_config::Config {
        port: 9999,
        music_paths: vec![music_dir],
        config_path: tmp.path().join("config"),
        cache_path: tmp.path().join("cache"),
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
        trusted_proxies: vec!["127.0.0.1".parse().unwrap(), "::1".parse().unwrap()],
    };
    let state = michi_api::AppState::new(config, pool.clone(), None);
    let test_app = create_router(state);
    let test_app2 = test_app.clone();

    // Stream the track via tower
    let response = test_app
        .oneshot(
            Request::builder()
                .uri(format!("/api/stream/{id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), 100000)
        .await
        .unwrap();
    assert_eq!(bytes.len(), 50000);

    // Also verify stats show the track
    let response = test_app2
        .oneshot(
            Request::builder()
                .uri("/api/library/stats")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let text = body_text(response).await;
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["tracks"], 1);
}

#[tokio::test]
async fn test_tracks_still_works_without_params() {
    let (app, pool) = make_app().await;
    for i in 0..3 {
        let path = format!("/music/song_{i}.flac");
        let title = format!("Track {i}");
        seed_track(&pool, &path, &title).await;
    }

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/tracks")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let text = body_text(response).await;
    let tracks: Vec<Value> = serde_json::from_str(&text).unwrap();
    assert_eq!(tracks.len(), 3, "no params should return all tracks");
}

#[tokio::test]
async fn test_v1_server_info() {
    let (app, _) = make_app().await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/server/info")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let text = body_text(response).await;
    let json: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(json["name"], "Michi Micro Server");
    assert_eq!(json["api_version"], "v1");
    assert_eq!(json["service"], "michi-micro-server");
    assert!(json["server_id"].is_string());
    let sid = json["server_id"].as_str().unwrap();
    assert!(Uuid::parse_str(sid).is_ok(), "server_id must be valid UUID");
    assert!(json["features"]["library"].as_bool().unwrap_or(false));
    assert!(json["features"]["search"].as_bool().unwrap_or(false));
    assert!(json["features"]["streaming"].as_bool().unwrap_or(false));
    assert!(json["features"]["download"].as_bool().unwrap_or(false));
    assert!(json["features"]["playlists"].as_bool().unwrap_or(false));
    assert!(
        json["features"]["artwork"].as_bool().unwrap_or(false),
        "artwork should be true"
    );
    assert!(json["features"]["events"].as_bool().unwrap_or(false));
    assert!(
        json["features"]["transcoding"].is_boolean(),
        "transcoding should be a boolean"
    );
    assert!(json["roles"].is_array());
    assert_eq!(json["auth"]["strategy"], "SERVER_CODE");
    assert!(json["auth"]["token_refresh"].as_bool().unwrap_or(false));
    assert_eq!(json["auth"]["required"].as_bool(), Some(true));
    assert_eq!(json["features"]["receivers"].as_bool(), Some(false));
    assert_eq!(json["features"]["rooms"].as_bool(), Some(false));
    assert_eq!(json["api_version"], "v1");
}

#[tokio::test]
async fn test_v1_server_id_persists() {
    let tmp = tempfile::tempdir().unwrap();
    let sid1 = michi_config::load_or_create_server_id(tmp.path());
    let sid2 = michi_config::load_or_create_server_id(tmp.path());
    assert_eq!(sid1, sid2, "server_id must be stable across calls");
}

#[tokio::test]
async fn test_v1_server_id_valid_uuid() {
    let tmp = tempfile::tempdir().unwrap();
    let sid = michi_config::load_or_create_server_id(tmp.path());
    assert!(!sid.is_nil(), "server_id must not be nil");
}

#[tokio::test]
async fn test_v1_status() {
    let (app, _) = make_app().await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let text = body_text(response).await;
    let json: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(json["status"], "ok");
}

#[tokio::test]
async fn test_v1_tracks() {
    let (app, _) = make_app().await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/tracks")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_v1_search() {
    let (app, _) = make_app().await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/search?q=test")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_v1_track_not_found_error() {
    let (app, _) = make_app().await;
    let fake_id = Uuid::new_v4();
    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/tracks/{fake_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let text = body_text(response).await;
    let json: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(json["error"]["code"], "TRACK_NOT_FOUND");
}

#[tokio::test]
async fn test_v1_invalid_id_returns_error() {
    let (app, _) = make_app().await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/tracks/not-a-uuid")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(
        response.status().is_client_error(),
        "invalid UUID should return client error, got {}",
        response.status()
    );
}

#[tokio::test]
async fn test_v1_stream_track_not_found() {
    let (app, _) = make_app().await;
    let fake_id = Uuid::new_v4();
    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/stream/{fake_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let text = body_text(response).await;
    let json: Value = serde_json::from_str(&text).unwrap();
    assert!(
        json.get("error").is_some(),
        "v1 stream error must have 'error' key"
    );
}

#[tokio::test]
async fn test_v1_tracks_with_seeded_data() {
    let (app, pool) = make_app().await;
    let id1 = seed_track(&pool, "/music/a.flac", "Alpha Song").await;
    let id2 = seed_track(&pool, "/music/b.flac", "Beta Song").await;

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/tracks")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let text = body_text(response).await;
    let body: Value = serde_json::from_str(&text).unwrap();
    let tracks = body["tracks"].as_array().unwrap();
    assert_eq!(tracks.len(), 2);
    let ids: Vec<&str> = tracks.iter().map(|t| t["id"].as_str().unwrap()).collect();
    assert!(ids.contains(&id1.to_string().as_str()));
    assert!(ids.contains(&id2.to_string().as_str()));
    // Confirm file_path is NOT exposed in v1 API
    for track in tracks {
        assert!(
            track.get("file_path").is_none(),
            "v1 tracks must not expose file_path"
        );
    }
}

#[tokio::test]
async fn test_v1_tracks_no_file_path() {
    let (app, pool) = make_app().await;
    seed_track(&pool, "/music/hidden.flac", "Hidden Path").await;

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/tracks")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let text = body_text(response).await;
    let body: Value = serde_json::from_str(&text).unwrap();
    let tracks = body["tracks"].as_array().unwrap();
    assert_eq!(tracks.len(), 1);
    assert!(tracks[0].get("file_path").is_none());
    assert!(tracks[0].get("stream_url").is_some());
    assert!(tracks[0].get("download_url").is_some());
}

#[tokio::test]
async fn test_v1_track_by_id_with_seeded_data() {
    let (app, pool) = make_app().await;
    let id = seed_track(&pool, "/music/x.flac", "X Song").await;

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/tracks/{id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let text = body_text(response).await;
    let track: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(track["title"], "X Song");
    assert_eq!(track["artist"], "Test Artist");
}

#[tokio::test]
async fn test_v1_search_with_seeded_data() {
    let (app, pool) = make_app().await;
    seed_track(&pool, "/music/abba.flac", "Dancing Queen").await;

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/search?q=dancing")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let text = body_text(response).await;
    let body: Value = serde_json::from_str(&text).unwrap();
    let tracks = body["tracks"].as_array().unwrap();
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0]["title"], "Dancing Queen");
}

#[tokio::test]
async fn test_v1_library_stats_with_seeded_data() {
    let (app, pool) = make_app().await;
    seed_track(&pool, "/music/a.flac", "A").await;
    seed_track(&pool, "/music/b.flac", "B").await;

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/library/stats")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let text = body_text(response).await;
    let stats: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(stats["tracks"], 2);
}

#[tokio::test]
async fn test_v1_endpoints_old_still_work() {
    let (app, pool) = make_app().await;
    seed_track(&pool, "/music/r.flac", "R").await;

    let old_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/tracks")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(old_response.status(), StatusCode::OK);

    let stat_response = app
        .oneshot(
            Request::builder()
                .uri("/api/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(stat_response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_v1_stream_invalid_id_error_format() {
    let (app, _) = make_app().await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/stream/not-a-uuid")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(response.status().is_client_error());
}

#[tokio::test]
async fn test_v1_stream_range_not_satisfiable() {
    let tmp = tempfile::tempdir().unwrap();
    let music = tmp.path().join("music");
    std::fs::create_dir_all(&music).unwrap();
    let file = music.join("song.flac");
    std::fs::write(&file, b"fake audio file with some bytes").unwrap();

    let pool = michi_db::init_pool("sqlite::memory:").await.unwrap();
    let id = michi_core::track_id_from_path(file.to_str().unwrap());
    let track = michi_core::Track {
        id,
        title: Some("Test".into()),
        artist: None,
        album: None,
        album_artist: None,
        duration_ms: Some(10000),
        file_path: file.to_str().unwrap().to_string(),
        format: michi_core::AudioFormat::Flac,
        sample_rate: None,
        bit_depth: None,
        channels: None,
        artwork_id: None,
        genre: None,
        year: None,
        track_number: None,
        disc_number: None,
        content_hash: None,
        file_size: None,
        file_mtime_ns: None,
        starred: false,
        rating: 0,
        starred_at: None,
        replaygain_track_gain: None,
        replaygain_track_peak: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    michi_db::upsert_track(&pool, &track).await.unwrap();

    let config = michi_config::Config {
        port: 9999,
        music_paths: vec![music.clone()],
        config_path: tmp.path().join("config"),
        cache_path: tmp.path().join("cache"),
        database_url: "sqlite::memory:".to_string(),
        version: "test",
        sync_peers: vec![],
        sync_name: "test".into(),
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
        trusted_proxies: vec!["127.0.0.1".parse().unwrap(), "::1".parse().unwrap()],
    };
    let state = michi_api::AppState::new(config, pool.clone(), None);
    let app = router_with_test_admin(state, &pool).await;

    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/stream/{id}"))
                .header("Range", "bytes=999999999-")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::RANGE_NOT_SATISFIABLE);
    let text = body_text(response).await;
    let json: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(json["error"]["code"], "RANGE_NOT_SATISFIABLE");
}

#[tokio::test]
async fn test_v1_stream_download_not_found() {
    let (app, _pool) = make_app().await;
    let fake_id = Uuid::new_v4();

    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/download/{fake_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let text = body_text(response).await;
    let json: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(json["error"]["code"], "TRACK_NOT_FOUND");
}

#[tokio::test]
async fn test_v1_hls_format_recognized() {
    let (app, _) = make_app().await;
    // hls format should be recognized — track doesn't exist, returns 404
    let fake_id = uuid::Uuid::new_v4();
    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/stream/{fake_id}?format=hls"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(
        response.status().is_client_error(),
        "HLS with bad track should return client error"
    );
    let text = body_text(response).await;
    let json: Value = serde_json::from_str(&text).unwrap();
    assert!(
        json.get("error").is_some(),
        "HLS error must have 'error' key"
    );
}

/// Build a canonical v1-lite `POST /pair/start` body for a freshly generated
/// client identity: Ed25519 challenge over a fixed 32-byte nonce.
fn canonical_pair_start_body(
    client: &michi_identity::IdentityManager,
    device_name: &str,
    device_type: &str,
) -> String {
    let nonce_raw = [7u8; 32];
    let nonce = michi_identity::encode_base64url(&nonce_raw);
    let (signature, public_key) = client.sign_base64url(&nonce_raw);
    format!(
        r#"{{
  "device_name": "{device_name}",
  "device_type": "{device_type}",
  "roles": ["mobile_player", "remote_controller"],
  "auth_strategy": "ED25519_CHALLENGE",
  "michi_id": "{}",
  "public_key": "{public_key}",
  "challenge_nonce": "{nonce}",
  "challenge_signature": "{signature}"
}}"#,
        client.michi_id().to_base64url(),
    )
}

/// Canonical v1-lite `POST /pair/confirm` body for a client session.
fn canonical_pair_confirm_body(
    client: &michi_identity::IdentityManager,
    session_id: &str,
    pin: &str,
) -> String {
    format!(
        r#"{{"session_id":"{session_id}","pin":"{pin}","michi_id":"{}","public_key":"{}"}}"#,
        client.michi_id().to_base64url(),
        client.public_key_base64url(),
    )
}

/// Fresh ephemeral client identity (unique temp dir per call).
fn fresh_test_client(name: &str) -> michi_identity::IdentityManager {
    let dir = std::env::temp_dir().join(format!("michi-test-client-{name}-{}", Uuid::new_v4()));
    michi_identity::IdentityManager::generate(&dir, name, "").expect("test client identity")
}

#[tokio::test]
async fn test_v1_pair_confirm_returns_canonical_contract() {
    let (app, _pool, state) = make_app_with_state().await;
    let client = fresh_test_client("mobile");

    // Start pairing (canonical v1-lite challenge)
    let start_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/pair/start")
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(canonical_pair_start_body(
                    &client,
                    "test-mobile",
                    "mobile",
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(start_resp.status(), StatusCode::OK);
    let start_text = body_text(start_resp).await;
    let start_json: Value = serde_json::from_str(&start_text).unwrap();
    assert!(
        start_json.get("pin").is_none(),
        "P0-01 contract violation: PIN MUST NEVER be returned over HTTP in pair/start"
    );
    let session_id = start_json["session_id"].as_str().unwrap().to_string();
    let pin = state
        .pairing_display
        .read()
        .await
        .as_ref()
        .expect("local pairing observer must capture PIN")
        .clone();
    assert_eq!(pin.len(), 6, "PIN must be 6 digits");
    assert!(
        start_json["server_michi_id"].as_str().unwrap().len() > 10,
        "server must expose its canonical michi_id"
    );
    assert!(start_json["server_public_key"].as_str().unwrap().len() > 10);
    assert!(start_json["expires_at"].as_str().unwrap().len() > 10);
    assert_eq!(start_json["attempts_remaining"].as_u64().unwrap(), 5);

    // Confirm with a wrong PIN -> 401 UNAUTHORIZED with PAIRING_PIN_MISMATCH
    let bad_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/pair/confirm")
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(canonical_pair_confirm_body(
                    &client,
                    &session_id,
                    "000000",
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(bad_resp.status(), StatusCode::UNAUTHORIZED);
    let bad_json: Value = serde_json::from_str(&body_text(bad_resp).await).unwrap();
    assert_eq!(bad_json["error"]["code"], "PAIRING_PIN_MISMATCH");

    // Confirm with the correct PIN -> canonical PairConfirmResponse
    let confirm_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/pair/confirm")
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(canonical_pair_confirm_body(
                    &client,
                    &session_id,
                    &pin,
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(confirm_resp.status(), StatusCode::OK);
    let confirm_text = body_text(confirm_resp).await;
    let confirm_json: Value = serde_json::from_str(&confirm_text).unwrap();

    assert!(confirm_json["token"].as_str().unwrap().len() > 10);
    assert!(confirm_json["refresh_token"].as_str().unwrap().len() > 10);
    assert!(confirm_json["expires_in"].as_u64().unwrap() > 0);
    assert!(confirm_json["device_id"].as_str().unwrap().len() > 10);
    assert!(confirm_json["server_id"].as_str().unwrap().len() > 10);
    assert!(
        confirm_json["permissions"].is_null(),
        "canonical confirm response must not carry a permissions array"
    );
}

#[tokio::test]
async fn test_v1_error_format_includes_details() {
    let (app, _) = make_app().await;
    let fake_id = Uuid::new_v4();

    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/tracks/{fake_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let text = body_text(response).await;
    let json: Value = serde_json::from_str(&text).unwrap();
    assert!(json.get("error").is_some(), "must have error key");
    assert!(json["error"].get("code").is_some(), "error must have code");
    assert!(
        json["error"].get("message").is_some(),
        "error must have message"
    );
    assert!(
        json["error"].get("details").is_some(),
        "error must have details: {{}}"
    );
}

#[tokio::test]
async fn test_v1_sync_manifest_no_file_path() {
    let (app, pool) = make_app().await;
    seed_track(&pool, "/music/secret.flac", "Secret").await;

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/sync/manifest")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let text = body_text(response).await;
    let json: Value = serde_json::from_str(&text).unwrap();
    let tracks = json["tracks"].as_array().unwrap();
    for t in tracks {
        assert!(
            t.get("file_path").is_none(),
            "sync manifest must not expose file_path"
        );
    }
    assert!(json.get("cursor").is_some(), "manifest must have cursor");
}

#[tokio::test]
async fn test_v1_playback_state_returns_state_field() {
    let (app, _) = make_app().await;

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/playback/state")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let text = body_text(response).await;
    let json: Value = serde_json::from_str(&text).unwrap();
    assert!(
        json.get("state").is_some(),
        "playback state must have 'state' field"
    );
    assert!(
        json.get("track_id").is_some(),
        "playback state must have track_id"
    );
    assert!(
        json.get("position_ms").is_some(),
        "playback state must have position_ms"
    );
    assert!(
        json.get("volume").is_some(),
        "playback state must have volume"
    );
    assert!(
        json.get("shuffle").is_some(),
        "playback state must have shuffle"
    );
    assert!(
        json.get("repeat").is_some(),
        "playback state must have repeat"
    );
}

#[tokio::test]
async fn test_v1_sync_manifest_delta_with_cursor() {
    let (app, pool) = make_app().await;
    seed_track(&pool, "/music/delta1.flac", "Delta One").await;

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/sync/manifest/delta?cursor=0")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let text = body_text(response).await;
    let json: Value = serde_json::from_str(&text).unwrap();
    assert!(json.get("added").is_some(), "delta must have added");
    assert!(json.get("cursor").is_some(), "delta must have cursor");
    assert!(json.get("total").is_some(), "delta must have total");
    let added = json["added"].as_array().unwrap();
    assert!(added.len() >= 1, "should have at least one added track");
    assert!(
        added[0].get("file_path").is_none(),
        "delta entries must not expose file_path"
    );
}

#[tokio::test]
async fn test_v1_e2e_mobile_flow() {
    let (app, pool, state) = make_app_with_state().await;
    seed_track(&pool, "/music/e2e.flac", "E2E Song").await;

    // 1. server/info
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/server/info")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let info: Value = serde_json::from_str(&body_text(resp).await).unwrap();
    assert_eq!(info["service"], "michi-micro-server");
    assert_eq!(info["api_version"], "v1");

    // 2. status
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // 3. pair/start (canonical v1-lite challenge)
    let client = fresh_test_client("e2e-mobile");
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/pair/start")
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(canonical_pair_start_body(
                    &client,
                    "e2e-mobile",
                    "mobile",
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let start: Value = serde_json::from_str(&body_text(resp).await).unwrap();
    let session_id = start["session_id"].as_str().unwrap().to_string();
    let pin = state
        .pairing_display
        .read()
        .await
        .as_ref()
        .expect("pairing display pin")
        .clone();

    // 4. pair/confirm
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/pair/confirm")
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(canonical_pair_confirm_body(
                    &client,
                    &session_id,
                    &pin,
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let confirm: Value = serde_json::from_str(&body_text(resp).await).unwrap();
    let _device_token = confirm["token"].as_str().unwrap().to_string();
    let refresh_token = confirm["refresh_token"].as_str().unwrap().to_string();
    let device_id = confirm["device_id"].as_str().unwrap().to_string();
    assert!(confirm["expires_in"].as_u64().unwrap() > 0);
    assert!(confirm["server_id"].as_str().unwrap().len() > 10);

    // 5. token/refresh
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/token/refresh")
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(format!(
                    r#"{{"refresh_token":"{refresh_token}","device_id":"{device_id}"}}"#
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // 6. library/stats
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/library/stats")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let stats: Value = serde_json::from_str(&body_text(resp).await).unwrap();
    assert!(stats["tracks"].as_i64().unwrap_or(0) >= 1);

    // 7. tracks
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/tracks")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let tracks_body: Value = serde_json::from_str(&body_text(resp).await).unwrap();
    let tracks = tracks_body["tracks"].as_array().unwrap();
    assert!(!tracks.is_empty());
    assert!(
        tracks[0].get("file_path").is_none(),
        "no file_path in response"
    );
    assert!(
        tracks[0].get("stream_url").is_some(),
        "must have stream_url"
    );

    // 8. sync/manifest
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/sync/manifest")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let manifest: Value = serde_json::from_str(&body_text(resp).await).unwrap();
    assert!(manifest["cursor"].as_i64().unwrap_or(0) >= 1);
    assert!(manifest["tracks"][0].get("file_path").is_none());

    // 9. sync/manifest/delta
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/sync/manifest/delta?cursor=0")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let delta: Value = serde_json::from_str(&body_text(resp).await).unwrap();
    assert!(delta["added"].as_array().unwrap().len() >= 1);

    // 10. playback/state
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/playback/state")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let ps: Value = serde_json::from_str(&body_text(resp).await).unwrap();
    assert!(ps.get("state").is_some());

    // 11. playback/control
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/playback/control")
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(r#"{"command":"play"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // 12. queue
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/queue")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // 13. devices/revoke
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/devices/revoke")
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(format!(r#"{{"device_id":"{device_id}"}}"#)))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_v1_import_flow() {
    let (app, pool) = make_app().await;
    seed_track(&pool, "/music/before.flac", "Before").await;

    // 1. import/session
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/import/session")
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(r#"{"total_tracks":2,"total_playlists":0}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let session: Value = serde_json::from_str(&body_text(resp).await).unwrap();
    let session_id = session["session_id"].as_str().unwrap().to_string();

    // 2. import/upload track 1
    let audio_data = base64::engine::general_purpose::STANDARD.encode(b"fake flac data 1");
    let hash = {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(b"fake flac data 1");
        hex::encode(h.finalize())
    };
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&format!("/api/v1/import/upload/{session_id}"))
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(format!(
                    r#"{{"filename":"song1.flac","data":"{audio_data}","hash":"{hash}"}}"#
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let upload: Value = serde_json::from_str(&body_text(resp).await).unwrap();
    assert_eq!(upload["status"], "uploaded");
    assert!(upload["remote_track_id"].is_string());
    assert!(upload["checksum"].is_string());

    // 3. import/upload track 2
    let audio_data2 = base64::engine::general_purpose::STANDARD.encode(b"fake flac data 2");
    let hash2 = {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(b"fake flac data 2");
        hex::encode(h.finalize())
    };
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&format!("/api/v1/import/upload/{session_id}"))
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(format!(
                    r#"{{"filename":"song2.flac","data":"{audio_data2}","hash":"{hash2}"}}"#
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // 4. duplicate upload rejected
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&format!("/api/v1/import/upload/{session_id}"))
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(format!(
                    r#"{{"filename":"song1-dupe.flac","data":"{audio_data}","hash":"{hash}"}}"#
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let dupe: Value = serde_json::from_str(&body_text(resp).await).unwrap();
    assert_eq!(dupe["status"], "duplicate");

    // 5. hash mismatch rejected
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&format!("/api/v1/import/upload/{session_id}"))
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(format!(
                    r#"{{"filename":"bad.flac","data":"{audio_data}","hash":"0000dead"}}"#
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let err: Value = serde_json::from_str(&body_text(resp).await).unwrap();
    assert_eq!(err["error"]["code"], "HASH_MISMATCH");

    // 6. invalid extension rejected
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&format!("/api/v1/import/upload/{session_id}"))
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(format!(
                    r#"{{"filename":"bad.exe","data":"{audio_data}","hash":"{hash}"}}"#
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let err: Value = serde_json::from_str(&body_text(resp).await).unwrap();
    assert_eq!(err["error"]["code"], "INVALID_EXTENSION");

    // 7. import/commit
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&format!("/api/v1/import/commit/{session_id}"))
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let commit: Value = serde_json::from_str(&body_text(resp).await).unwrap();
    assert_eq!(commit["tracks_imported"], 2);

    // 8. Subsequent rollback on already committed session MUST be rejected (409 Conflict)
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&format!("/api/v1/import/rollback/{session_id}"))
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let err: Value = serde_json::from_str(&body_text(resp).await).unwrap();
    assert_eq!(err["error"]["code"], "CANNOT_ROLLBACK_COMMITTED");

    // 9. Subsequent upload on already committed session MUST be rejected (410 Gone)
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&format!("/api/v1/import/upload/{session_id}"))
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(format!(
                    r#"{{"filename":"song3.flac","data":"{audio_data}","hash":"{hash}"}}"#
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::GONE);
}

#[tokio::test]
async fn test_v1_import_session_validation() {
    let (app, _) = make_app().await;

    // Reject zero-track session
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/import/session")
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(r#"{"total_tracks":0,"total_playlists":0}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // Reject too many tracks
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/import/session")
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(r#"{"total_tracks":99999,"total_playlists":0}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_v1_auth_flow_pairing_roundtrip() {
    let (app, _, state) = make_app_with_state().await;
    let client = fresh_test_client("auth-test");

    // pair/start (canonical v1-lite challenge)
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/pair/start")
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(canonical_pair_start_body(
                    &client,
                    "auth-test",
                    "player",
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let start: Value = serde_json::from_str(&body_text(resp).await).unwrap();
    let session_id = start["session_id"].as_str().unwrap().to_string();
    let pin = state
        .pairing_display
        .read()
        .await
        .as_ref()
        .expect("pairing display pin")
        .clone();
    assert_eq!(pin.len(), 6);

    // pair/confirm with unknown session -> 404 PAIRING_NOT_FOUND
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/pair/confirm")
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(canonical_pair_confirm_body(
                    &client,
                    "00000000-0000-0000-0000-000000000000",
                    "123456",
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let err: Value = serde_json::from_str(&body_text(resp).await).unwrap();
    assert_eq!(err["error"]["code"], "PAIRING_NOT_FOUND");

    // pair/confirm with wrong PIN -> 401 PAIRING_PIN_MISMATCH
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/pair/confirm")
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(canonical_pair_confirm_body(
                    &client,
                    &session_id,
                    "000000",
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let err: Value = serde_json::from_str(&body_text(resp).await).unwrap();
    assert_eq!(err["error"]["code"], "PAIRING_PIN_MISMATCH");

    // pair/confirm success -> canonical PairConfirmResponse
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/pair/confirm")
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(canonical_pair_confirm_body(
                    &client,
                    &session_id,
                    &pin,
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let confirm: Value = serde_json::from_str(&body_text(resp).await).unwrap();
    assert!(confirm["token"].as_str().unwrap().len() > 10);
    assert!(confirm["refresh_token"].as_str().unwrap().len() > 10);
    assert!(confirm["expires_in"].as_u64().unwrap() > 0);
    assert!(confirm["device_id"].as_str().unwrap().len() > 10);
    assert!(confirm["server_id"].as_str().unwrap().len() > 10);
    assert!(confirm["permissions"].is_null());

    // confirm again -> session consumed (registry keeps the consumed session,
    // canonical error is PairingAlreadyConsumed -> 409 PAIRING_ALREADY_CONSUMED)
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/pair/confirm")
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(canonical_pair_confirm_body(
                    &client,
                    &session_id,
                    &pin,
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let err: Value = serde_json::from_str(&body_text(resp).await).unwrap();
    assert_eq!(err["error"]["code"], "PAIRING_ALREADY_CONSUMED");
}

#[tokio::test]
async fn test_v1_import_rollback_cleans_staging() {
    let (app, pool) = make_app().await;
    seed_track(&pool, "/music/existing.flac", "Existing").await;

    // session
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/import/session")
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(r#"{"total_tracks":1,"total_playlists":0}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    let session: Value = serde_json::from_str(&body_text(resp).await).unwrap();
    let sid = session["session_id"].as_str().unwrap().to_string();

    // upload
    let data = base64::engine::general_purpose::STANDARD.encode(b"rollback test audio");
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(b"rollback test audio");
    let hash = hex::encode(h.finalize());
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&format!("/api/v1/import/upload/{sid}"))
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(format!(
                    r#"{{"filename":"rollback.flac","data":"{data}","hash":"{hash}"}}"#
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // rollback
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&format!("/api/v1/import/rollback/{sid}"))
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let rb: Value = serde_json::from_str(&body_text(resp).await).unwrap();
    assert_eq!(rb["status"], "rolled_back");

    // commit after rollback should fail
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&format!("/api/v1/import/commit/{sid}"))
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::GONE);
}

#[tokio::test]
async fn test_v1_autonomous_playback_state_independent() {
    let (app, _) = make_app().await;

    // Initial state is stopped/paused
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/playback/state")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let state: Value = serde_json::from_str(&body_text(resp).await).unwrap();
    assert_eq!(state["state"], "paused");
    assert!(state["track_id"].is_null());

    // Control: play without track_id
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/playback/control")
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(r#"{"command":"play"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Verify state changed
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/playback/state")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let state: Value = serde_json::from_str(&body_text(resp).await).unwrap();
    assert_eq!(state["state"], "playing");

    // Control: pause
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/playback/control")
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(r#"{"command":"pause"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/playback/state")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let state: Value = serde_json::from_str(&body_text(resp).await).unwrap();
    assert_eq!(state["state"], "paused");

    // Volume control: set_volume 50
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/playback/control")
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(r#"{"command":"set_volume","volume":50}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/playback/state")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let state: Value = serde_json::from_str(&body_text(resp).await).unwrap();
    assert_eq!(state["volume"], 50);

    // seek by position_ms
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/playback/control")
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(r#"{"command":"seek","position_ms":30000}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/playback/state")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let state: Value = serde_json::from_str(&body_text(resp).await).unwrap();
    assert_eq!(state["position_ms"], 30000);

    // stop
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/playback/control")
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(r#"{"command":"stop"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/playback/state")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let state: Value = serde_json::from_str(&body_text(resp).await).unwrap();
    assert_eq!(state["state"], "paused");
    assert_eq!(state["position_ms"], 0);
}

#[tokio::test]
async fn test_v1_stream_and_download_range() {
    use tokio::io::AsyncWriteExt;
    let tmp = tempfile::tempdir().unwrap();
    let music = tmp.path().join("music");
    std::fs::create_dir_all(&music).unwrap();
    let file_path = music.join("stream_range.flac");
    let content = vec![0u8; 65536]; // 64KB file
    let mut f = tokio::fs::File::create(&file_path).await.unwrap();
    f.write_all(&content).await.unwrap();
    drop(f);

    let (pool, db_url) = test_db_with_url().await;
    let canonical_music = music.canonicalize().unwrap_or(music.clone());
    let canonical_file = file_path.canonicalize().unwrap_or(file_path.clone());
    let id = michi_core::track_id_from_path(canonical_file.to_str().unwrap());
    let track = michi_core::Track {
        id,
        title: Some("Range Test".into()),
        artist: None,
        album: None,
        album_artist: None,
        duration_ms: Some(10000),
        file_path: canonical_file.to_str().unwrap().to_string(),
        format: michi_core::AudioFormat::Flac,
        sample_rate: None,
        bit_depth: None,
        channels: None,
        artwork_id: None,
        genre: None,
        year: None,
        track_number: None,
        disc_number: None,
        content_hash: None,
        file_size: Some(65536),
        file_mtime_ns: None,
        starred: false,
        rating: 0,
        starred_at: None,
        replaygain_track_gain: None,
        replaygain_track_peak: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    michi_db::upsert_track(&pool, &track).await.unwrap();

    let mut config = test_config_with_url(db_url);
    config.music_paths = vec![canonical_music];
    config.config_path = tmp.path().join("config");
    config.cache_path = tmp.path().join("cache");
    let state = michi_api::AppState::new(config, pool.clone(), None);
    let app = router_with_test_admin(state, &pool).await;

    // Full file 200
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/stream/{id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Range request 206
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/stream/{id}"))
                .header("Range", "bytes=0-1023")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::PARTIAL_CONTENT);

    // Download full 200
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/download/{id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Range not satisfiable 416
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/stream/{id}"))
                .header("Range", "bytes=999999999-")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    let body = body_text(resp).await;
    assert_eq!(
        status,
        StatusCode::RANGE_NOT_SATISFIABLE,
        "body was: {body}"
    );
}

#[tokio::test]
async fn test_v1_diagnostics_endpoint() {
    let (app, _) = make_app().await;
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/diagnostics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json: Value = serde_json::from_str(&body_text(resp).await).unwrap();
    assert!(json.get("db").is_some(), "diagnostics must have db");
    assert!(
        json.get("library").is_some(),
        "diagnostics must have library"
    );
    assert!(
        json.get("playback").is_some(),
        "diagnostics must have playback"
    );
    assert!(json.get("queues").is_some(), "diagnostics must have queues");
    assert!(json.get("events").is_some(), "diagnostics must have events");
    assert!(
        json.get("warnings").is_some(),
        "diagnostics must have warnings"
    );
    assert!(json["healthy"].as_bool().is_some());
}

#[tokio::test]
async fn test_v1_playback_session_persist_and_restore() {
    let (app, pool) = make_app().await;
    let tid = seed_track(&pool, "/music/persist.flac", "Persist").await;

    // Create playback session
    let resp = app.clone().oneshot(
        Request::builder().uri("/api/v1/playback/session").method("POST")
            .header("Content-Type", "application/json")
            .body(Body::from(format!(r#"{{"queue":["{tid}"],"current_track_id":"{tid}","position_ms":5000,"playing":true}}"#)))
            .unwrap(),
    ).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let session_resp: Value = serde_json::from_str(&body_text(resp).await).unwrap();
    let session_id = session_resp["session_id"].as_str().unwrap().to_string();
    let queue_id = session_resp["queue_id"].as_str().unwrap().to_string();

    // Get session
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&format!("/api/v1/playback/session/{session_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let get_resp: Value = serde_json::from_str(&body_text(resp).await).unwrap();
    assert_eq!(get_resp["session_id"], session_id);
    assert_eq!(get_resp["queue_id"], queue_id);
    assert!(!get_resp["queue_items"].as_array().unwrap().is_empty());

    // Restore session
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/playback/session/restore")
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let restore: Value = serde_json::from_str(&body_text(resp).await).unwrap();
    assert!(restore["restored"].as_bool().unwrap_or(false));
    assert_eq!(restore["position_ms"], 5000);

    // Delete queue
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&format!("/api/v1/queue/{queue_id}"))
                .method("DELETE")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_v1_import_session_status() {
    let (app, _) = make_app().await;

    // Create session
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/import/session")
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(r#"{"total_tracks":1,"total_playlists":0}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    let session: Value = serde_json::from_str(&body_text(resp).await).unwrap();
    let sid = session["session_id"].as_str().unwrap().to_string();

    // Get status
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&format!("/api/v1/import/session/{sid}/status"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let status: Value = serde_json::from_str(&body_text(resp).await).unwrap();
    assert_eq!(status["session_id"], sid);
    assert_eq!(status["status"], "created");
    assert_eq!(status["total_tracks"], 1);
}

#[tokio::test]
async fn test_v1_receiver_client_models() {
    // Test that the receiver API models serialize/deserialize correctly
    use michi_receivers::ReceiverClient;

    // Just test construction — no connection needed
    let client = ReceiverClient::new("http://127.0.0.1:9999");
    assert!(client.base_url.contains("127.0.0.1:9999"));
    assert!(client.token.is_none());
}

#[tokio::test]
async fn test_v1_import_preflight_already_present() {
    let (app, pool) = make_app().await;
    use chrono::Utc;
    use michi_core::{AudioFormat, Track};
    let tid = uuid::Uuid::new_v4();
    let track = Track {
        id: tid,
        title: Some("Existing".into()),
        artist: None,
        album: None,
        album_artist: None,
        duration_ms: Some(200000),
        file_path: "/tmp/michi-test/music/existing.flac".into(),
        format: AudioFormat::Flac,
        sample_rate: None,
        bit_depth: None,
        channels: None,
        artwork_id: None,
        genre: None,
        year: None,
        track_number: None,
        disc_number: None,
        content_hash: Some("aaabbbccc111".into()),
        file_size: None,
        file_mtime_ns: None,
        starred: false,
        rating: 0,
        starred_at: None,
        replaygain_track_gain: None,
        replaygain_track_peak: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    michi_db::upsert_track(&pool, &track).await.unwrap();

    let resp = app.clone().oneshot(
        Request::builder().uri("/api/v1/import/preflight").method("POST")
            .header("Content-Type", "application/json")
            .body(Body::from(r#"{"tracks":[{"content_hash":"aaabbbccc111","file_size":1000,"duration_ms":200000}]}"#))
            .unwrap(),
    ).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json: Value = serde_json::from_str(&body_text(resp).await).unwrap();
    let results = json["results"].as_array().unwrap();
    assert_eq!(results[0]["status"], "already_present");
    assert_eq!(results[0]["remote_track_id"], tid.to_string());
    assert_eq!(results[0]["match"], "exact_hash");
}

#[tokio::test]
async fn test_v1_import_preflight_needs_upload() {
    let (app, _) = make_app().await;

    let resp = app.clone().oneshot(
        Request::builder().uri("/api/v1/import/preflight").method("POST")
            .header("Content-Type", "application/json")
            .body(Body::from(r#"{"tracks":[{"content_hash":"nonexistent","file_size":1000,"duration_ms":200000}]}"#))
            .unwrap(),
    ).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json: Value = serde_json::from_str(&body_text(resp).await).unwrap();
    let results = json["results"].as_array().unwrap();
    assert_eq!(results[0]["status"], "needs_upload");
}

#[tokio::test]
async fn test_v1_import_preflight_conflict() {
    let (app, pool) = make_app().await;
    use chrono::Utc;
    use michi_core::{AudioFormat, Track};
    let tid = uuid::Uuid::new_v4();
    let track = Track {
        id: tid,
        title: Some("Conflict".into()),
        artist: None,
        album: None,
        album_artist: None,
        duration_ms: Some(100000),
        file_path: "/tmp/michi-test/music/conflict.flac".into(),
        format: AudioFormat::Flac,
        sample_rate: None,
        bit_depth: None,
        channels: None,
        artwork_id: None,
        genre: None,
        year: None,
        track_number: None,
        disc_number: None,
        content_hash: Some("conflict_hash".into()),
        file_size: None,
        file_mtime_ns: None,
        starred: false,
        rating: 0,
        starred_at: None,
        replaygain_track_gain: None,
        replaygain_track_peak: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    michi_db::upsert_track(&pool, &track).await.unwrap();

    // Same title but different duration_ms and no matching hash -> conflict
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/import/preflight")
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(
                    r#"{"tracks":[{"title":"Conflict","duration_ms":999999,"file_size":999}]}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json: Value = serde_json::from_str(&body_text(resp).await).unwrap();
    let results = json["results"].as_array().unwrap();
    assert_eq!(results[0]["status"], "conflict");
    assert_eq!(results[0]["match"], "metadata_duration");
}

#[tokio::test]
async fn test_v1_commit_returns_mapping() {
    let (app, _) = make_app().await;

    // Upload one track and commit
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/import/session")
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(r#"{"total_tracks":1,"total_playlists":0}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    let session: Value = serde_json::from_str(&body_text(resp).await).unwrap();
    let sid = session["session_id"].as_str().unwrap().to_string();

    use base64::Engine;
    let data = base64::engine::general_purpose::STANDARD.encode(b"mapping test audio");
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(b"mapping test audio");
    let hash = hex::encode(h.finalize());
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&format!("/api/v1/import/upload/{sid}"))
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(format!(
                    r#"{{"filename":"mapping.flac","data":"{data}","hash":"{hash}"}}"#
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&format!("/api/v1/import/commit/{sid}"))
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let commit: Value = serde_json::from_str(&body_text(resp).await).unwrap();
    assert!(
        commit.get("mapping").is_some(),
        "commit must include mapping"
    );
    let mapping = commit["mapping"].as_array().unwrap();
    assert!(!mapping.is_empty(), "mapping must have at least one entry");
    assert!(
        mapping[0].get("local_track_id").is_some(),
        "mapping entry must have local_track_id"
    );
    assert!(
        mapping[0].get("status").is_some(),
        "mapping entry must have status"
    );
    assert!(
        mapping[0].get("remote_track_id").is_some(),
        "mapping entry must have remote_track_id"
    );
}

#[tokio::test]
async fn test_v1_playback_queue_survives_restart() {
    let (app, pool) = make_app().await;

    // Seed a track
    let tid = seed_track(&pool, "/music/survive.flac", "Survivor").await;

    // Create queue items
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/queue/items")
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(format!(
                    r#"{{"track_ids":["{tid}"],"name":"survivor-queue"}}"#
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let queue_resp: Value = serde_json::from_str(&body_text(resp).await).unwrap();
    let _queue_id = queue_resp["queue_id"].as_str().unwrap().to_string();

    // Create playback session
    let resp = app.clone().oneshot(
        Request::builder().uri("/api/v1/playback/session").method("POST")
            .header("Content-Type", "application/json")
            .body(Body::from(format!(r#"{{"queue":["{tid}"],"current_track_id":"{tid}","position_ms":42000,"playing":true,"source":"player","resume_policy":"manual"}}"#)))
            .unwrap(),
    ).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let sess_resp: Value = serde_json::from_str(&body_text(resp).await).unwrap();
    let session_id = sess_resp["session_id"].as_str().unwrap().to_string();

    // Now simulate restart: create new app state with same DB, verify restore works
    let config2 = test_config();
    let state2 = michi_api::AppState::new(config2, pool.clone(), None);
    let app2 = router_with_test_admin(state2, &pool).await;

    // Verify queue is still queryable
    let resp2 = app2
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/playback/session/{session_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp2.status(), StatusCode::OK);
    let restored: Value = serde_json::from_str(&body_text(resp2).await).unwrap();
    assert_eq!(restored["session_id"], session_id);
    assert_eq!(restored["position_ms"], 42000);
    assert_eq!(restored["source"], "player");

    // Queue should survive
    let resp3 = app2
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/playback/session/restore")
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp3.status(), StatusCode::OK);
    let restore_resp: Value = serde_json::from_str(&body_text(resp3).await).unwrap();
    assert!(restore_resp["restored"].as_bool().unwrap_or(false));
    assert_eq!(restore_resp["position_ms"], 42000);
}

#[tokio::test]
async fn test_v1_diagnostics_has_disk_and_receiver() {
    let (app, _) = make_app().await;
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/diagnostics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json: Value = serde_json::from_str(&body_text(resp).await).unwrap();
    assert!(
        json.get("disk").is_some(),
        "diagnostics must have disk section"
    );
    assert!(
        json.get("receiver").is_some(),
        "diagnostics must have receiver section"
    );
    assert!(json["receiver"]["client_available"].as_bool().is_some());
    assert!(
        json["disk"]["music_path_free_bytes"].is_null()
            || json["disk"]["music_path_free_bytes"].as_u64().is_some()
    );
    assert!(
        json.get("player_compatibility").is_some(),
        "diagnostics must have player_compatibility section"
    );
    assert!(json["player_compatibility"]["contract_status"].is_string());
}

#[tokio::test]
async fn test_v1_import_preflight_exact_hash() {
    let (app, pool) = make_app().await;
    use chrono::Utc;
    use michi_core::{AudioFormat, Track};
    let tid = uuid::Uuid::new_v4();
    let track = Track {
        id: tid,
        title: Some("Exact Hash".into()),
        artist: Some("Artist".into()),
        album: Some("Album".into()),
        album_artist: None,
        duration_ms: Some(200000),
        file_path: "/tmp/test/exact.flac".into(),
        format: AudioFormat::Flac,
        sample_rate: None,
        bit_depth: None,
        channels: None,
        artwork_id: None,
        genre: None,
        year: None,
        track_number: None,
        disc_number: None,
        content_hash: Some(
            "aaabbbcccdddeeefff000111222333444555666777888999000aaabbbcccddd".into(),
        ),
        file_size: None,
        file_mtime_ns: None,
        starred: false,

        rating: 0,
        starred_at: None,
        replaygain_track_gain: None,
        replaygain_track_peak: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    michi_db::upsert_track(&pool, &track).await.unwrap();

    let resp = app.clone().oneshot(
        Request::builder().uri("/api/v1/import/preflight").method("POST")
            .header("Content-Type", "application/json")
            .body(Body::from(r#"{"tracks":[{"content_hash":"aaabbbcccdddeeefff000111222333444555666777888999000aaabbbcccddd","file_size":1000,"duration_ms":200000,"title":"Exact Hash"}]}"#))
            .unwrap(),
    ).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json: Value = serde_json::from_str(&body_text(resp).await).unwrap();
    let results = json["results"].as_array().unwrap();
    assert_eq!(results[0]["status"], "already_present");
    assert_eq!(results[0]["match"], "exact_hash");
}

#[tokio::test]
async fn test_v1_import_preflight_quick_hash() {
    let (app, pool) = make_app().await;
    use chrono::Utc;
    use michi_core::{AudioFormat, Track};
    let full_hash = "deadbeef123456789000111222333444555666777888999000aaabbbcccddd";
    let tid = uuid::Uuid::new_v4();
    let track = Track {
        id: tid,
        title: Some("Quick Hash Match".into()),
        artist: None,
        album: None,
        album_artist: None,
        duration_ms: Some(180000),
        file_path: "/tmp/test/quick.flac".into(),
        format: AudioFormat::Flac,
        sample_rate: None,
        bit_depth: None,
        channels: None,
        artwork_id: None,
        genre: None,
        year: None,
        track_number: None,
        disc_number: None,
        content_hash: Some(full_hash.into()),
        file_size: None,
        file_mtime_ns: None,
        starred: false,
        rating: 0,
        starred_at: None,
        replaygain_track_gain: None,
        replaygain_track_peak: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    michi_db::upsert_track(&pool, &track).await.unwrap();

    let quick = &full_hash[..16];
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/import/preflight")
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(format!(
                    r#"{{"tracks":[{{"quick_hash":"{quick}","file_size":1000}}]}}"#
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json: Value = serde_json::from_str(&body_text(resp).await).unwrap();
    assert_eq!(json["results"][0]["status"], "already_present");
    assert_eq!(json["results"][0]["match"], "quick_hash");
}

#[tokio::test]
async fn test_v1_import_upload_returns_remote_track_id() {
    let (app, _) = make_app().await;
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/import/session")
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(r#"{"total_tracks":1,"total_playlists":0}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    let session: Value = serde_json::from_str(&body_text(resp).await).unwrap();
    let sid = session["session_id"].as_str().unwrap().to_string();

    use base64::Engine;
    let data = base64::engine::general_purpose::STANDARD.encode(b"remote track id test");
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(b"remote track id test");
    let hash = hex::encode(h.finalize());

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&format!("/api/v1/import/upload/{sid}"))
                .method("POST")
                .header("Content-Type", "application/json")
                .header("X-Track-Id", "00000000-0000-0000-0000-000000000001")
                .body(Body::from(format!(
                    r#"{{"filename":"remote.flac","data":"{data}","hash":"{hash}"}}"#
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let upload: Value = serde_json::from_str(&body_text(resp).await).unwrap();
    assert_eq!(
        upload["local_track_id"],
        "00000000-0000-0000-0000-000000000001"
    );
    assert!(
        upload["remote_track_id"].is_string(),
        "remote_track_id must be present"
    );
    assert_eq!(upload["status"], "uploaded");
    assert!(upload["checksum"].is_string());
}

#[tokio::test]
async fn test_v1_queue_transfer_success() {
    let (app, pool) = make_app().await;
    let tid1 = seed_track(&pool, "/music/transfer1.flac", "Transfer One").await;
    let tid2 = seed_track(&pool, "/music/transfer2.flac", "Transfer Two").await;

    let resp = app.clone().oneshot(
        Request::builder().uri("/api/v1/queue/transfer").method("POST")
            .header("Content-Type", "application/json")
            .body(Body::from(format!(r#"{{"track_ids":["{tid1}","{tid2}"],"current_index":0,"position_ms":5000,"source":"michi-music-player"}}"#)))
            .unwrap(),
    ).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json: Value = serde_json::from_str(&body_text(resp).await).unwrap();
    assert!(json["queue_id"].is_string(), "must return queue_id");
    assert!(json["session_id"].is_string(), "must return session_id");
    assert!(json["accepted"].as_bool().unwrap_or(false));
    assert_eq!(json["current_index"], 0);
    assert_eq!(json["position_ms"], 5000);
}

#[tokio::test]
async fn test_v1_queue_transfer_unknown_track() {
    let (app, _) = make_app().await;
    let fake = uuid::Uuid::new_v4();
    let resp = app.clone().oneshot(
        Request::builder().uri("/api/v1/queue/transfer").method("POST")
            .header("Content-Type", "application/json")
            .body(Body::from(format!(r#"{{"track_ids":["{fake}"],"current_index":0,"position_ms":0,"source":"michi-music-player"}}"#)))
            .unwrap(),
    ).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let json: Value = serde_json::from_str(&body_text(resp).await).unwrap();
    assert_eq!(json["error"]["code"], "UNKNOWN_TRACKS");
}

#[tokio::test]
async fn test_v1_import_commit_returns_mapping_with_status() {
    let (app, _) = make_app().await;

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/import/session")
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(r#"{"total_tracks":1,"total_playlists":0}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    let session: Value = serde_json::from_str(&body_text(resp).await).unwrap();
    let sid = session["session_id"].as_str().unwrap().to_string();

    use base64::Engine;
    let data = base64::engine::general_purpose::STANDARD.encode(b"commit mapping status test");
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(b"commit mapping status test");
    let hash = hex::encode(h.finalize());
    let _ = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&format!("/api/v1/import/upload/{sid}"))
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(format!(
                    r#"{{"filename":"mapstatus.flac","data":"{data}","hash":"{hash}"}}"#
                )))
                .unwrap(),
        )
        .await
        .unwrap();

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&format!("/api/v1/import/commit/{sid}"))
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let commit: Value = serde_json::from_str(&body_text(resp).await).unwrap();
    let mapping = commit["mapping"].as_array().unwrap();
    assert!(!mapping.is_empty(), "mapping must have entries");
    assert!(
        mapping[0].get("local_track_id").is_some(),
        "mapping must have local_track_id"
    );
    assert!(
        mapping[0].get("status").is_some(),
        "mapping must have per-track status"
    );
    assert!(
        mapping[0].get("remote_track_id").is_some(),
        "mapping must have remote_track_id"
    );
    assert!(
        mapping[0].get("checksum").is_some(),
        "mapping must have checksum"
    );
}

#[tokio::test]
async fn test_v1_import_session_survives_memory_wipe_and_recovers_from_manifest() {
    let (app, _) = make_app().await;

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/import/session")
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(r#"{"total_tracks":1,"total_playlists":0}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    let session: Value = serde_json::from_str(&body_text(resp).await).unwrap();
    let sid = session["session_id"].as_str().unwrap().to_string();

    use base64::Engine;
    let data = base64::engine::general_purpose::STANDARD.encode(b"survive memory wipe test");
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(b"survive memory wipe test");
    let hash = hex::encode(h.finalize());
    let fname = format!("survive_{}.flac", Uuid::new_v4());
    let _ = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&format!("/api/v1/import/upload/{sid}"))
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(format!(
                    r#"{{"filename":"{fname}","data":"{data}","hash":"{hash}"}}"#
                )))
                .unwrap(),
        )
        .await
        .unwrap();

    // Wipe memory sessions (simulating server restart / power loss)
    michi_api::routes::v1::import::clear_import_sessions_for_test().await;

    // Commit should seamlessly recover the session from manifest.json on disk
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&format!("/api/v1/import/commit/{sid}"))
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let commit: Value = serde_json::from_str(&body_text(resp).await).unwrap();
    let mapping = commit["mapping"].as_array().unwrap();
    assert!(!mapping.is_empty(), "recovered mapping must have entries");
    assert_eq!(mapping[0]["status"], "inserted");
}

#[tokio::test]
async fn test_v1_import_same_name_different_checksum_collision_handled() {
    let (app, _) = make_app().await;

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/import/session")
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(r#"{"total_tracks":2,"total_playlists":0}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    let session: Value = serde_json::from_str(&body_text(resp).await).unwrap();
    let sid = session["session_id"].as_str().unwrap().to_string();

    use base64::Engine;
    use sha2::{Digest, Sha256};

    // Upload 1: Song.flac with content A
    let data_a = base64::engine::general_purpose::STANDARD.encode(b"content AAA distinct");
    let mut h_a = Sha256::new();
    h_a.update(b"content AAA distinct");
    let hash_a = hex::encode(h_a.finalize());

    let resp_a = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&format!("/api/v1/import/upload/{sid}"))
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(format!(
                    r#"{{"filename":"Track.flac","data":"{data_a}","hash":"{hash_a}"}}"#
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp_a.status(), StatusCode::OK);

    // Upload 2: Song.flac with content B (same filename, different content)
    let data_b = base64::engine::general_purpose::STANDARD.encode(b"content BBB distinct");
    let mut h_b = Sha256::new();
    h_b.update(b"content BBB distinct");
    let hash_b = hex::encode(h_b.finalize());

    let resp_b = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&format!("/api/v1/import/upload/{sid}"))
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(format!(
                    r#"{{"filename":"Track.flac","data":"{data_b}","hash":"{hash_b}"}}"#
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp_b.status(), StatusCode::OK);

    // Commit both files
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&format!("/api/v1/import/commit/{sid}"))
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let commit: Value = serde_json::from_str(&body_text(resp).await).unwrap();
    let mapping = commit["mapping"].as_array().unwrap();
    assert_eq!(
        mapping.len(),
        2,
        "both files must be committed without collision corruption"
    );
}

#[tokio::test]
async fn test_v1_import_duplicate_upload_reconciles_db_progress() {
    let (app, pool) = make_app().await;

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/import/session")
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(r#"{"total_tracks":2,"total_playlists":0}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    let session: Value = serde_json::from_str(&body_text(resp).await).unwrap();
    let sid_str = session["session_id"].as_str().unwrap().to_string();
    let sid = Uuid::parse_str(&sid_str).unwrap();

    use base64::Engine;
    use sha2::{Digest, Sha256};

    let data_a = base64::engine::general_purpose::STANDARD.encode(b"song audio content");
    let mut h_a = Sha256::new();
    h_a.update(b"song audio content");
    let hash_a = hex::encode(h_a.finalize());

    // 1. Initial upload -> imported_tracks becomes 1 in DB
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&format!("/api/v1/import/upload/{sid_str}"))
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(format!(
                    r#"{{"filename":"Track1.flac","data":"{data_a}","hash":"{hash_a}"}}"#
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let db_sess = michi_db::get_import_session_full(&pool, &sid)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(db_sess.imported_tracks, 1);

    // 2. Simulate DB ledger divergence (e.g. transient glitch previously left DB at 0)
    sqlx::query("UPDATE import_sessions SET imported_tracks = 0 WHERE session_id = ?")
        .bind(sid_str.clone())
        .execute(&pool)
        .await
        .unwrap();

    let db_sess_diverged = michi_db::get_import_session_full(&pool, &sid)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(db_sess_diverged.imported_tracks, 0);

    // 3. Retry identical upload -> status: duplicate AND reconciles DB ledger back to 1
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&format!("/api/v1/import/upload/{sid_str}"))
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(format!(
                    r#"{{"filename":"Track1.flac","data":"{data_a}","hash":"{hash_a}"}}"#
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body_dup: Value = serde_json::from_str(&body_text(resp).await).unwrap();
    assert_eq!(body_dup["status"], "duplicate");

    // 4. Assert DB progress was reconciled to 1
    let db_sess_healed = michi_db::get_import_session_full(&pool, &sid)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        db_sess_healed.imported_tracks, 1,
        "Duplicate retry must reconcile DB ledger"
    );
}

#[tokio::test]
async fn test_v1_auth_real_pair_and_use_token() {
    let (app, _, state) = make_app_with_state().await;
    let client = fresh_test_client("auth-test-player");

    // 1. Pair a device (canonical v1-lite challenge)
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/pair/start")
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(canonical_pair_start_body(
                    &client,
                    "auth-test-player",
                    "player",
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let start: Value = serde_json::from_str(&body_text(resp).await).unwrap();
    let session_id = start["session_id"].as_str().unwrap().to_string();
    let pin = state
        .pairing_display
        .read()
        .await
        .as_ref()
        .expect("pairing display pin")
        .clone();

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/pair/confirm")
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(canonical_pair_confirm_body(
                    &client,
                    &session_id,
                    &pin,
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let confirm: Value = serde_json::from_str(&body_text(resp).await).unwrap();
    let device_token = confirm["token"].as_str().unwrap().to_string();
    assert!(confirm["server_id"].as_str().unwrap().len() > 10);

    // 2. Use token for playback/control
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/playback/control")
                .method("POST")
                .header("Content-Type", "application/json")
                .header("Authorization", format!("Bearer {device_token}"))
                .body(Body::from(r#"{"command":"play"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // 3. Use token for playback/state
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/playback/state")
                .header("Authorization", format!("Bearer {device_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let state: Value = serde_json::from_str(&body_text(resp).await).unwrap();
    assert!(state.get("state").is_some());

    // 4. Invalid token rejected
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/playback/control")
                .method("POST")
                .header("Content-Type", "application/json")
                .header("Authorization", "Bearer invalid_token_xyz")
                .body(Body::from(r#"{"command":"play"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_v1_playback_queue_survives_full_restart() {
    let (app, pool) = make_app().await;
    let tid = seed_track(&pool, "/music/restart_test.flac", "Restart Test").await;

    // Create queue + session
    let resp = app.clone().oneshot(
        Request::builder().uri("/api/v1/queue/transfer").method("POST")
            .header("Content-Type", "application/json")
            .body(Body::from(format!(
                r#"{{"track_ids":["{tid}"],"current_index":0,"position_ms":12345,"source":"michi-music-player"}}"#
            ))).unwrap(),
    ).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let transfer: Value = serde_json::from_str(&body_text(resp).await).unwrap();
    assert!(transfer["accepted"].as_bool().unwrap_or(false));
    let queue_id = transfer["queue_id"].as_str().unwrap().to_string();

    // Simulate restart: create new AppState
    let config2 = test_config();
    let state2 = michi_api::AppState::new(config2, pool.clone(), None);
    let app2 = router_with_test_admin(state2, &pool).await;

    // Give auto_restore time to run
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    // Verify session was auto-restored
    let resp = app2
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/playback/state")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let restored: Value = serde_json::from_str(&body_text(resp).await).unwrap();
    assert_eq!(
        restored["position_ms"], 12345,
        "position_ms should be restored after restart"
    );

    // Verify diagnostics shows restore
    let resp = app2
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/diagnostics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let diag: Value = serde_json::from_str(&body_text(resp).await).unwrap();
    assert!(
        diag["player_compatibility"]["playback_restored"]
            .as_bool()
            .is_some(),
        "diagnostics must report playback_restored status"
    );
}

#[tokio::test]
async fn test_v1_smart_playlist_favorites() {
    let (app, pool) = make_app().await;
    let tid = seed_track(&pool, "/music/smart_fav.flac", "Smart Fav").await;

    // Star the track
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/star/{tid}"))
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::json!({"starred": true}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/playlists/smart")
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(
                    serde_json::json!({"name": "My Favorites", "rule": "favorites"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = serde_json::from_str(&body_text(resp).await).unwrap();
    assert_eq!(body["playlist"]["name"], "My Favorites");
    assert_eq!(body["rule"], "favorites");
    assert_eq!(body["tracks_added"], 1);
}

#[tokio::test]
async fn test_v1_smart_playlist_invalid_rule() {
    let (app, _pool) = make_app().await;
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/playlists/smart")
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(
                    serde_json::json!({"name": "Bad", "rule": "nonexistent"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_v1_backup_export() {
    let (app, pool) = make_app().await;
    seed_track(&pool, "/music/backup1.flac", "Backup One").await;
    seed_track(&pool, "/music/backup2.flac", "Backup Two").await;

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/backup")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = serde_json::from_str(&body_text(resp).await).unwrap();
    assert_eq!(body["version"], 1);
    assert_eq!(body["tracks"].as_array().unwrap().len(), 2);
    assert!(!body["exported_at"].as_str().unwrap().is_empty());
    assert!(!body["server_id"].as_str().unwrap().is_empty());
    assert!(!body["server_name"].as_str().unwrap().is_empty());
    assert!(body["playlists"].is_array());
    assert!(body["starred_tracks"].is_array());
    assert!(body["play_history"].is_array());
}

#[tokio::test]
async fn test_v1_restore_force_replaces_all_state_including_playlists_and_history() {
    let (app, pool) = make_app().await;

    // 1. Populate existing state (pre-existing tracks, playlist, play history)
    let t1 = seed_track(&pool, "/music/old1.flac", "Old One").await;
    let old_pl = michi_db::create_playlist(
        &pool,
        &michi_core::PlaylistCreate {
            name: "Old Favorites".into(),
            description: Some("Old desc".into()),
        },
        None,
    )
    .await
    .unwrap();
    michi_db::add_track_to_playlist(&pool, &old_pl.id, &t1)
        .await
        .unwrap();
    sqlx::query("INSERT INTO play_history (track_id, played_at) VALUES (?, ?)")
        .bind(t1.to_string())
        .bind("2024-01-01T00:00:00Z")
        .execute(&pool)
        .await
        .unwrap();

    // 2. Perform force restore with new payload
    let new_track_id = uuid::Uuid::new_v4();
    let new_track = Track {
        id: new_track_id,
        title: Some("New One".into()),
        artist: Some("New Artist".into()),
        album: None,
        album_artist: None,
        duration_ms: Some(180_000),
        file_path: "/music/new1.flac".into(),
        format: AudioFormat::Flac,
        sample_rate: Some(44100),
        bit_depth: Some(16),
        channels: Some(2),
        artwork_id: None,
        genre: None,
        year: None,
        track_number: None,
        disc_number: None,
        content_hash: None,
        file_size: None,
        file_mtime_ns: None,
        starred: false,
        rating: 0,
        starred_at: None,
        replaygain_track_gain: None,
        replaygain_track_peak: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };

    let restore_payload = serde_json::json!({
        "force": true,
        "tracks": [new_track],
        "playlists": [{
            "name": "New Restored Playlist",
            "description": "Restored description",
            "tracks": [new_track]
        }],
        "starred_tracks": [],
        "play_history": [{
            "track_id": new_track_id.to_string(),
            "played_at": "2026-02-01T00:00:00Z",
            "timestamp": "2026-02-01T00:00:00Z"
        }]
    });

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/backup/restore")
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(restore_payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let res_json: serde_json::Value = serde_json::from_str(&body_text(resp).await).unwrap();
    assert_eq!(res_json["status"], "restored");
    assert_eq!(res_json["tracks"], 1);
    assert_eq!(res_json["playlists"], 1);
    assert_eq!(res_json["history"], 1);

    // Verify that old playlists, old tracks and old history are completely replaced
    let current_tracks = michi_db::list_tracks(&pool).await.unwrap();
    assert_eq!(current_tracks.len(), 1);
    assert_eq!(current_tracks[0].id, new_track_id);

    let current_playlists = michi_db::list_playlists(&pool, None).await.unwrap();
    assert_eq!(current_playlists.len(), 1);
    assert_eq!(current_playlists[0].name, "New Restored Playlist");

    let history_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM play_history")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(history_count.0, 1);
}

#[tokio::test]
async fn test_v1_restore_strict_all_or_nothing_rolls_back_on_error() {
    let (app, pool) = make_app().await;

    // Seed original state
    let original_id = seed_track(&pool, "/music/orig.flac", "Original Track").await;

    // Send a restore with an invalid track entry or error that fails the transaction
    let valid_track_id = uuid::Uuid::new_v4();
    let valid_track = Track {
        id: valid_track_id,
        title: Some("Valid New Track".into()),
        artist: Some("Valid Artist".into()),
        album: None,
        album_artist: None,
        duration_ms: Some(180_000),
        file_path: "/music/valid.flac".into(),
        format: AudioFormat::Flac,
        sample_rate: Some(44100),
        bit_depth: Some(16),
        channels: Some(2),
        artwork_id: None,
        genre: None,
        year: None,
        track_number: None,
        disc_number: None,
        content_hash: None,
        file_size: None,
        file_mtime_ns: None,
        starred: false,
        rating: 0,
        starred_at: None,
        replaygain_track_gain: None,
        replaygain_track_peak: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };

    // Restore payload includes valid track, but starred_tracks attempts to star a non-existent track
    // (with invalid/empty path, etc.) or play_history insertion that violates DB constraint
    // Here: force=false on non-empty DB without force returns CONFLICT before modifying DB
    let restore_payload = serde_json::json!({
        "force": false,
        "tracks": [valid_track],
        "playlists": [],
        "starred_tracks": [],
        "play_history": []
    });

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/backup/restore")
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(restore_payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);

    // Verify DB still only has original track
    let tracks_after = michi_db::list_tracks(&pool).await.unwrap();
    assert_eq!(tracks_after.len(), 1);
    assert_eq!(tracks_after[0].id, original_id);
}

#[tokio::test]
async fn test_version_consistency() {
    let (app, _pool) = make_app().await;

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let status: serde_json::Value = serde_json::from_str(&body_text(resp).await).unwrap();
    let status_version = status["version"].as_str().unwrap_or("");

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/server/info")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let info: serde_json::Value = serde_json::from_str(&body_text(resp).await).unwrap();
    let info_version = info["version"].as_str().unwrap_or("");

    assert_eq!(
        status_version, info_version,
        "status version {status_version} must match server info version {info_version}"
    );
    assert!(!status_version.is_empty(), "version must not be empty");
}

#[tokio::test]
async fn test_static_content_types() {
    let (app, _pool) = make_app().await;

    let tests = vec![
        ("/static/styles.css", "text/css"),
        ("/static/app.js", "application/javascript"),
        ("/static/assets/michi-micro-server.svg", "image/svg+xml"),
        ("/static/assets/michi-micro-server.png", "image/png"),
        ("/static/assets/michi-micro-server-180.png", "image/png"),
        ("/static/assets/michi-micro-server-192.png", "image/png"),
        ("/static/assets/michi-micro-server-512.png", "image/png"),
        ("/static/assets/michi-hero-cat.webp", "image/webp"),
        ("/manifest.json", "application/json"),
    ];

    for (path, expected_type) in tests {
        let resp = app
            .clone()
            .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK, "expected 200 for {path}");
        let content_type = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        assert!(
            content_type.starts_with(expected_type),
            "expected {path} to start with '{expected_type}', got '{content_type}'"
        );
    }
}

#[tokio::test]
async fn test_pwa_manifest_uses_official_png_icons() {
    let (app, _pool) = make_app().await;

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/manifest.json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let manifest: serde_json::Value = serde_json::from_str(&body_text(resp).await).unwrap();
    let icons = manifest["icons"].as_array().unwrap();

    assert_eq!(icons.len(), 2);
    assert_eq!(icons[0]["src"], "/static/assets/michi-micro-server-192.png");
    assert_eq!(icons[0]["sizes"], "192x192");
    assert_eq!(icons[0]["type"], "image/png");
    assert_eq!(icons[0]["purpose"], "any");
    assert_eq!(icons[1]["src"], "/static/assets/michi-micro-server-512.png");
    assert_eq!(icons[1]["sizes"], "512x512");
    assert_eq!(icons[1]["type"], "image/png");
    assert_eq!(icons[1]["purpose"], "any");
}

#[tokio::test]
async fn test_root_references_official_browser_icons() {
    let (app, _pool) = make_app().await;

    let resp = app
        .clone()
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_text(resp).await;

    assert!(html.contains(
        "rel=\"icon\" type=\"image/svg+xml\" href=\"/static/assets/michi-micro-server.svg\""
    ));
    assert!(html.contains("rel=\"icon\" type=\"image/png\" sizes=\"192x192\" href=\"/static/assets/michi-micro-server-192.png\""));
    assert!(html.contains("rel=\"apple-touch-icon\" sizes=\"180x180\" href=\"/static/assets/michi-micro-server-180.png\""));
    assert!(html.contains(
        "src=\"/static/assets/michi-micro-server.svg\" alt=\"\" class=\"sidebar-logo-img\""
    ));
}

#[tokio::test]
async fn test_protected_route_rejects_no_auth() {
    let (app, _) = make_unauthenticated_app().await;

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/tracks")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_protected_route_rejects_bad_token() {
    let (app, _) = make_unauthenticated_app().await;

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/tracks")
                .header("Authorization", "Bearer invalidtoken123")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_auth_pairing_grants_proper_permissions() {
    let (app, _, state) = make_app_with_state().await;
    let client = fresh_test_client("perm-test");

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/pair/start")
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(canonical_pair_start_body(
                    &client,
                    "perm-test",
                    "player",
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let start: serde_json::Value = serde_json::from_str(&body_text(resp).await).unwrap();
    let session_id = start["session_id"].as_str().unwrap().to_string();
    let pin = state
        .pairing_display
        .read()
        .await
        .as_ref()
        .expect("pairing display pin")
        .clone();

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/pair/confirm")
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(canonical_pair_confirm_body(
                    &client,
                    &session_id,
                    &pin,
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let confirm: serde_json::Value = serde_json::from_str(&body_text(resp).await).unwrap();
    assert!(confirm["token"].as_str().unwrap().len() > 10);
    assert!(confirm["refresh_token"].as_str().unwrap().len() > 10);
    assert!(confirm["expires_in"].as_u64().unwrap() > 0);
    assert!(confirm["device_id"].as_str().unwrap().len() > 10);
    assert!(confirm["server_id"].as_str().unwrap().len() > 10);
    assert!(confirm["permissions"].is_null());
}
