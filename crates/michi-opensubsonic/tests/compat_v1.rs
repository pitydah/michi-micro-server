use axum::{
    body::Body,
    http::{header, Request, StatusCode},
};
use michi_core::{AudioFormat, Track};
use michi_opensubsonic::routes::{router, OsAppState, ScanStatusProvider};
use tower::ServiceExt;
use uuid::Uuid;

async fn setup_test_env() -> (axum::Router, sqlx::SqlitePool, ScanStatusProvider) {
    let tmp = std::env::temp_dir().join(format!("michi-os-test-{}", Uuid::new_v4()));
    let music_dir = tmp.join("music");
    let cache_dir = tmp.join("cache");
    let _ = std::fs::create_dir_all(&music_dir);
    let _ = std::fs::create_dir_all(&cache_dir);

    // Create a 1000-byte test file
    let track_file = music_dir.join("test_song.wav");
    std::fs::write(&track_file, vec![0u8; 1000]).unwrap();

    let pool = michi_db::init_pool("sqlite::memory:").await.unwrap();

    let track = Track {
        id: Uuid::new_v4(),
        title: Some("Subsonic Range Test".into()),
        artist: Some("OpenSubsonic".into()),
        album: Some("Compat v1".into()),
        album_artist: None,
        duration_ms: Some(180000),
        file_path: track_file.to_string_lossy().to_string(),
        format: AudioFormat::Wav,
        sample_rate: Some(48000),
        bit_depth: Some(16),
        channels: Some(2),
        artwork_id: None,
        genre: None,
        year: Some(2026),
        track_number: Some(1),
        disc_number: None,
        content_hash: None,
        file_size: Some(1000),
        file_mtime_ns: None,
        starred: false,
        rating: 0,
        starred_at: None,
        replaygain_track_gain: None,
        replaygain_track_peak: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    michi_db::upsert_tracks(&pool, &[track.clone()])
        .await
        .unwrap();

    let scan_status = ScanStatusProvider::default();
    let state = OsAppState {
        db: pool.clone(),
        music_paths: vec![music_dir],
        cache_path: cache_dir,
        auth_username: Some("admin".to_string()),
        auth_password: Some("secret123".to_string()),
        auth_enabled: true,
        scan_status: scan_status.clone(),
    };

    let app = router(state);
    (app, pool, scan_status)
}

#[tokio::test]
async fn test_opensubsonic_stream_http_range_support() {
    let (app, pool, _) = setup_test_env().await;
    let track = michi_db::list_tracks(&pool).await.unwrap().remove(0);

    // 1. Unauthenticated request should return Subsonic error envelope (HTTP 200 with status: failed)
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/rest/stream?id={}&f=json", track.id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    let val: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(val["subsonic-response"]["status"], "failed");

    // 2. Full stream request without range -> 200 OK
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!(
                    "/rest/stream?id={}&u=admin&p=secret123&f=json",
                    track.id
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers().get(header::CONTENT_LENGTH).unwrap(), "1000");
    assert_eq!(res.headers().get(header::ACCEPT_RANGES).unwrap(), "bytes");

    // 3. Partial content request with valid Range -> 206 Partial Content
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!(
                    "/rest/stream?id={}&u=admin&p=secret123&f=json",
                    track.id
                ))
                .header(header::RANGE, "bytes=0-499")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(
        res.headers().get(header::CONTENT_RANGE).unwrap(),
        "bytes 0-499/1000"
    );
    assert_eq!(res.headers().get(header::CONTENT_LENGTH).unwrap(), "500");

    // 4. Out of bounds range request -> 416 Range Not Satisfiable
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!(
                    "/rest/stream?id={}&u=admin&p=secret123&f=json",
                    track.id
                ))
                .header(header::RANGE, "bytes=2000-3000")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::RANGE_NOT_SATISFIABLE);
}

#[tokio::test]
async fn test_opensubsonic_scan_lifecycle_truth() {
    let (app, _pool, _) = setup_test_env().await;

    // 1. Initial scan status -> not scanning, count is 1
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/rest/getScanStatus?u=admin&p=secret123&f=json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    let val: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(val["subsonic-response"]["scanStatus"]["scanning"], false);
    assert_eq!(val["subsonic-response"]["scanStatus"]["count"], 1);

    // 2. Start scan -> responds scanning: true
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/rest/startScan?u=admin&p=secret123&f=json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    let val: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(val["subsonic-response"]["scanStatus"]["scanning"], true);
}

#[tokio::test]
async fn test_artist_album_song_navigation_contract() {
    let (app, pool, _) = setup_test_env().await;

    // Seed 2 tracks for same artist and same album
    let track1 = Track {
        id: Uuid::new_v4(),
        title: Some("Song One".into()),
        artist: Some("The Beatles".into()),
        album: Some("AbbeyRoad".into()),
        album_artist: None,
        duration_ms: Some(180000),
        file_path: "/music/song1.wav".into(),
        format: AudioFormat::Wav,
        sample_rate: Some(44100),
        bit_depth: Some(16),
        channels: Some(2),
        artwork_id: None,
        genre: Some("Rock".into()),
        year: Some(1969),
        track_number: Some(1),
        disc_number: None,
        content_hash: None,
        file_size: Some(5000),
        file_mtime_ns: None,
        starred: false,
        rating: 0,
        starred_at: None,
        replaygain_track_gain: None,
        replaygain_track_peak: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    let track2 = Track {
        id: Uuid::new_v4(),
        title: Some("Song Two".into()),
        artist: Some("The Beatles".into()),
        album: Some("AbbeyRoad".into()),
        album_artist: None,
        duration_ms: Some(200000),
        file_path: "/music/song2.wav".into(),
        format: AudioFormat::Wav,
        sample_rate: Some(44100),
        bit_depth: Some(16),
        channels: Some(2),
        artwork_id: None,
        genre: Some("Rock".into()),
        year: Some(1969),
        track_number: Some(2),
        disc_number: None,
        content_hash: None,
        file_size: Some(6000),
        file_mtime_ns: None,
        starred: false,
        rating: 0,
        starred_at: None,
        replaygain_track_gain: None,
        replaygain_track_peak: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    michi_db::upsert_tracks(&pool, &[track1.clone(), track2.clone()])
        .await
        .unwrap();

    // 1. getArtist -> exactly 1 album with songCount == 2
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/rest/getArtist?id=The+Beatles&u=admin&p=secret123&f=json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    let val: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let albums = val["subsonic-response"]["artist"]["album"]
        .as_array()
        .unwrap();
    assert_eq!(albums.len(), 1);
    assert_eq!(albums[0]["name"], "AbbeyRoad");
    assert_eq!(albums[0]["songCount"], 2);

    let album_id = albums[0]["id"].as_str().unwrap();

    // 2. getAlbum using returned album_id -> 2 songs
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!(
                    "/rest/getAlbum?id={album_id}&u=admin&p=secret123&f=json"
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    let val: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let songs = val["subsonic-response"]["album"]["song"]
        .as_array()
        .unwrap();
    assert_eq!(songs.len(), 2);
}

#[tokio::test]
async fn test_start_scan_is_single_flight() {
    let (app, _, scan_status) = setup_test_env().await;

    // Simulate an ongoing scan by setting the atomic flag
    scan_status
        .scanning
        .store(true, std::sync::atomic::Ordering::SeqCst);

    // Calling startScan should return alreadyRunning: true
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/rest/startScan?u=admin&p=secret123&f=json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    let val: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(val["subsonic-response"]["scanStatus"]["scanning"], true);
    assert_eq!(
        val["subsonic-response"]["scanStatus"]["alreadyRunning"],
        true
    );
}
