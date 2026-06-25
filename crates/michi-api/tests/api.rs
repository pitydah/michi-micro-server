use std::path::{Path, PathBuf};

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use michi_api::create_router;
use michi_config::Config;
use michi_core::{track_id_from_path, AudioFormat, Track, TrackUpdate};
use serde_json::Value;
use sqlx::SqlitePool;
use tower::ServiceExt;
use uuid::Uuid;

async fn test_db() -> SqlitePool {
    michi_db::init_pool("sqlite::memory:").await.unwrap()
}

fn test_config() -> Config {
    Config {
        port: 9999,
        music_path: PathBuf::from("/tmp/michi-test/music"),
        config_path: PathBuf::from("/tmp/michi-test/config"),
        cache_path: PathBuf::from("/tmp/michi-test/cache"),
        database_url: "sqlite::memory:".to_string(),
        version: "test",
    }
}

async fn make_app() -> (axum::Router, SqlitePool) {
    let pool = test_db().await;
    let config = test_config();
    let state = michi_api::AppState::new(config, pool.clone());
    (create_router(state), pool)
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
    assert_eq!(v["service"], "michi-micro-server");
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
                .uri(format!("/api/tracks/{}", id))
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
                .uri(format!("/api/tracks/{}", fake_id))
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
                .uri(format!("/api/tracks/{}", id))
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
                .uri(format!("/api/tracks/{}", id))
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
    path
}

async fn make_streaming_app() -> (axum::Router, SqlitePool, tempfile::TempDir, Uuid) {
    let tmp = tempfile::tempdir().unwrap();
    let music_dir = tmp.path().join("music");
    std::fs::create_dir_all(&music_dir).unwrap();

    let file_path = create_test_file(&music_dir, "test.flac", &[0u8; 50000]);

    let pool = test_db().await;
    let config = Config {
        port: 9999,
        music_path: music_dir,
        config_path: PathBuf::from("/tmp/michi-test/config"),
        cache_path: PathBuf::from("/tmp/michi-test/cache"),
        database_url: "sqlite::memory:".to_string(),
        version: "test",
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
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    michi_db::upsert_track(&pool, &track).await.unwrap();

    let state = michi_api::AppState::new(config, pool.clone());
    (michi_api::create_router(state), pool, tmp, id)
}

#[tokio::test]
async fn test_stream_full_file() {
    let (app, _pool, _tmp, id) = make_streaming_app().await;
    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/stream/{}", id))
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
                .uri(format!("/api/stream/{}", id))
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
                .uri(format!("/api/stream/{}", id))
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
                .uri(format!("/api/stream/{}", id))
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
                .uri(format!("/api/stream/{}", fake_id))
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
        music_path: PathBuf::from("/tmp/michi-test/music"),
        config_path: PathBuf::from("/tmp/michi-test/config"),
        cache_path: PathBuf::from("/tmp/michi-test/cache"),
        database_url: "sqlite::memory:".to_string(),
        version: "test",
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
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    michi_db::upsert_track(&pool, &track).await.unwrap();

    let state = michi_api::AppState::new(config, pool.clone());
    let app = michi_api::create_router(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/stream/{}", id))
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
