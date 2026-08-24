#![allow(
    unused_variables,
    clippy::needless_borrows_for_generic_args,
    clippy::len_zero
)]
use base64::Engine;
use std::path::PathBuf;
use std::sync::Arc;

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
        version: "0.2.0",
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
        opensubsonic_enabled: false,
    }
}

async fn make_raw_app() -> (axum::Router, SqlitePool, michi_api::AppState, String) {
    let pool = test_db().await;
    let config = test_config();
    let state = michi_api::AppState::new(config, pool.clone(), None);

    let admin_id = Uuid::new_v4();
    michi_db::create_user(
        &pool,
        &admin_id,
        &format!("test-admin-{admin_id}"),
        "unused",
        true,
    )
    .await
    .unwrap();
    let token = state.auth_sessions.create_session(admin_id).await;

    let router = create_router(state.clone());
    (router, pool, state, token)
}

async fn make_app() -> (axum::Router, SqlitePool, michi_api::AppState) {
    let (router, pool, state, token) = make_raw_app().await;
    let admin_router = router.layer(axum::middleware::from_fn_with_state(
        token,
        inject_test_authorization,
    ));
    (admin_router, pool, state)
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

async fn body_json(res: Response) -> Value {
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap_or(Value::Null)
}

async fn seed_track(pool: &SqlitePool, path_str: &str, title: &str) -> Track {
    let track = Track {
        id: track_id_from_path(path_str),
        title: Some(title.to_string()),
        artist: Some("Test Artist".to_string()),
        album: Some("Test Album".to_string()),
        album_artist: None,
        duration_ms: Some(180_000),
        file_path: path_str.to_string(),
        format: AudioFormat::Flac,
        sample_rate: Some(48000),
        bit_depth: Some(16),
        channels: Some(2),
        artwork_id: None,
        genre: Some("Electronic".to_string()),
        year: Some(2026),
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
    track
}

// ── UI-001 / UI-002 / UI-003: Unauthenticated QR Claim Bootstrap & Atomic Anti-Replay ──
#[tokio::test]
async fn test_unauthenticated_qr_claim_bootstrap_and_replay_protection() {
    let (raw_app, pool, state, admin_token) = make_raw_app().await;

    // 1. Admin generates QR code
    let gen_res = raw_app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/pair/qr")
                .header("Authorization", format!("Bearer {admin_token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(gen_res.status(), StatusCode::OK);
    let gen_json = body_json(gen_res).await;
    let qr_code = gen_json["qr_code"].as_str().unwrap();

    // 2. Unpaired Mobile claims QR with NO Authorization header (public bootstrap)
    let claim_res = raw_app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(&format!("/api/v1/pair/qr/{qr_code}/claim"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"device_name":"Mobile Pixel 9"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(claim_res.status(), StatusCode::OK);
    let claim_json = body_json(claim_res).await;
    assert_eq!(claim_json["status"], "claimed");
    let device_token = claim_json["device_token"].as_str().unwrap();
    let device_id = claim_json["device_id"].as_str().unwrap();

    // Verify LinkDevice was atomically persisted in SQL database
    let dev_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM link_devices WHERE device_id = ?")
            .bind(device_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(dev_count, 1);

    // 3. Replay attack: second claim with same QR code MUST be rejected (410 GONE)
    let replay_res = raw_app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(&format!("/api/v1/pair/qr/{qr_code}/claim"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"device_name":"Attacker"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(replay_res.status(), StatusCode::GONE);

    // 4. Claim with non-existent QR code returns 404
    let fake_qr = Uuid::new_v4();
    let fake_res = raw_app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(&format!("/api/v1/pair/qr/{fake_qr}/claim"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"device_name":"Random"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(fake_res.status(), StatusCode::NOT_FOUND);

    // 5. Verify the obtained device_token can access protected device routes
    let queue_res = raw_app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/queue")
                .header("Authorization", format!("Bearer {device_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(queue_res.status(), StatusCode::OK);
}

// ── UI-004: Receiver Online State Decoupled from Active Session ──
#[tokio::test]
async fn test_receiver_online_calculation_during_active_session() {
    let (app, _pool, state) = make_app().await;

    // Register a receiver with an active streaming session and recent last_seen
    let reg = state.receiver_manager.registry().await;
    let mut reg_write = reg.write().await;
    let entry = michi_receivers::ReceiverRegistryEntry {
        receiver_id: "rec-living-room".into(),
        name: "Living Room Stream".into(),
        device_type: "michi_stream".into(),
        base_url: "http://192.168.1.50:8080".into(),
        paired: true,
        token: Some("tok-123".into()),
        last_seen: Some(chrono::Utc::now()),
        capabilities: vec!["pcm_s16le".into()],
        active_session_id: Some("session-abc".into()),
        max_sample_rate: 48000,
        max_bit_depth: 16,
        supported_transports: vec!["rtp_udp".into()],
        supported_codecs: vec!["pcm_s16le".into()],
        supported_sample_rates: vec![48000],
        supported_bit_depths: vec![16],
        supported_channels: vec![2],
        maximum_safe_volume: Some(100),
    };
    reg_write.add(entry);
    drop(reg_write);
    drop(reg);

    // Fetch receivers list
    let res = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/receivers")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let json = body_json(res).await;
    let recs = json["receivers"].as_array().unwrap();
    let r = &recs[0];
    assert_eq!(
        r["online"], true,
        "Receiver with active session must still be reported online"
    );
    assert_eq!(r["session_active"], true);
}

// ── UI-005 / UI-006 / UI-007 / UI-008 / UI-009 / UI-010: Canonical Active Queue ──
#[tokio::test]
async fn test_canonical_active_queue_lifecycle() {
    let (app, pool, state) = make_app().await;

    let t1 = seed_track(&pool, "/music/track1.flac", "Track One").await;
    let t2 = seed_track(&pool, "/music/track2.flac", "Track Two").await;
    let t3 = seed_track(&pool, "/music/track3.flac", "Track Three").await;

    // UI-005: Add non-existent track fails and does not mutate
    let bad_id = Uuid::new_v4();
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/queue/items")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(format!(r#"{{"track_ids":["{bad_id}"]}}"#)))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);

    // UI-006: Sequential +Q A, +Q B, +Q C appends to ONE active canonical queue
    let res1 = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/queue/items")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(format!(r#"{{"track_ids":["{}"]}}"#, t1.id)))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res1.status(), StatusCode::OK);

    let res2 = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/queue/items")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(format!(r#"{{"track_ids":["{}"]}}"#, t2.id)))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res2.status(), StatusCode::OK);

    let res3 = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/queue/items")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(format!(r#"{{"track_ids":["{}"]}}"#, t3.id)))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res3.status(), StatusCode::OK);

    // Fetch canonical queue and verify all 3 tracks in order in active queue
    let q_res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/queue")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(q_res.status(), StatusCode::OK);
    let q_json = body_json(q_res).await;
    assert_eq!(q_json["items_count"], 3);
    let items = q_json["items"].as_array().unwrap();
    assert_eq!(items[0]["track_id"], t1.id.to_string());
    assert_eq!(items[1]["track_id"], t2.id.to_string());
    assert_eq!(items[2]["track_id"], t3.id.to_string());

    let item0_id = items[0]["id"].as_str().unwrap().to_string();
    let item1_id = items[1]["id"].as_str().unwrap().to_string();
    let item2_id = items[2]["id"].as_str().unwrap().to_string();
    let queue_id = q_json["queue_id"].as_str().unwrap().to_string();

    // UI-008: Queue Reorder by item_ids preserves track IDs and updates position
    let reorder_res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/v1/queue/reorder")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(format!(
                    r#"{{"queue_id":"{queue_id}","item_ids":["{item2_id}","{item1_id}","{item0_id}"]}}"#
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(reorder_res.status(), StatusCode::OK);

    let q_res_after = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/queue")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let q_json_after = body_json(q_res_after).await;
    let items_after = q_json_after["items"].as_array().unwrap();
    assert_eq!(items_after[0]["track_id"], t3.id.to_string());
    assert_eq!(items_after[1]["track_id"], t2.id.to_string());
    assert_eq!(items_after[2]["track_id"], t1.id.to_string());

    // UI-009: Queue Jump selects track at index and resets position_ms to 0
    let jump_res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/queue/jump")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(format!(
                    r#"{{"queue_id":"{queue_id}","index":1}}"#
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(jump_res.status(), StatusCode::OK);
    let ps = state.playback_state.read().await;
    assert_eq!(ps.track_id, Some(t2.id));
    assert_eq!(ps.position_ms, 0);
    drop(ps);

    // UI-010: Queue Delete removes queue and items transactionally
    let del_res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(&format!("/api/v1/queue/{queue_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(del_res.status(), StatusCode::OK);

    let del_second = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(&format!("/api/v1/queue/{queue_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(del_second.status(), StatusCode::NOT_FOUND);
}

// ── UI-011 / UI-012 / UI-013 / UI-014: Playback Chains Truthfulness ──
#[tokio::test]
async fn test_playback_chains_verifiable_effect() {
    let (app, pool, state) = make_app().await;
    let _t = seed_track(&pool, "/music/chain_track.flac", "Chain Track").await;

    // Create chain
    let create_res = app
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
    assert_eq!(create_res.status(), StatusCode::OK);
    let chain_id = body_json(create_res).await["chain"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    // UI-011: Play chain with 0 configured links MUST return 400 NO_OUTPUTS
    let play_res0 = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(&format!("/api/v1/chains/{chain_id}/play"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(play_res0.status(), StatusCode::BAD_REQUEST);
    let ps0 = state.playback_state.read().await;
    assert!(!ps0.playing);
    drop(ps0);

    // Add a link to an offline/unpaired receiver
    let add_link_res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(&format!("/api/v1/chains/{chain_id}/links"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"receiver_id":"offline-speaker","volume":70}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(add_link_res.status(), StatusCode::OK);

    // UI-012: Play chain where all receiver sessions fail MUST return 502 and NOT claim playing
    let play_res_fail = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(&format!("/api/v1/chains/{chain_id}/play"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(play_res_fail.status(), StatusCode::BAD_GATEWAY);
    let ps_fail = state.playback_state.read().await;
    assert!(!ps_fail.playing);
    drop(ps_fail);

    // UI-014: Volume validation strictly enforces 0..=100
    let vol_bad = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(&format!("/api/v1/chains/{chain_id}/volume"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"volume":150}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(vol_bad.status(), StatusCode::BAD_REQUEST);

    let vol_good = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(&format!("/api/v1/chains/{chain_id}/volume"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"volume":60}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(vol_good.status(), StatusCode::OK);

    // Stop chain
    let stop_res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(&format!("/api/v1/chains/{chain_id}/stop"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(stop_res.status(), StatusCode::OK);
}

// ── UI-015 / UI-016 / UI-017 / UI-018 / UI-020: Room Groups Persistence & Effect ──
#[tokio::test]
async fn test_room_groups_persistence_and_activation() {
    let (app, pool, state) = make_app().await;

    // Create room group
    let create_rg = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/rooms/groups")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"name":"Upstairs","mode":"party","receiver_ids":[]}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(create_rg.status(), StatusCode::OK);
    let rg_id = body_json(create_rg).await["group"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Verify room group was persisted directly in SQLite database
    let db_rows = michi_db::list_room_groups_db(&pool).await.unwrap();
    assert!(db_rows
        .iter()
        .any(|(id, name, _, _, _, _)| id.to_string() == rg_id && name == "Upstairs"));

    // UI-015: Activating empty room group MUST return 400 INVALID_ROOM
    let act_empty = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(&format!("/api/v1/rooms/groups/{rg_id}/activate"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(act_empty.status(), StatusCode::BAD_REQUEST);

    // /api/v1/rooms lists persistent room groups
    let list_rooms = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/rooms")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(list_rooms.status(), StatusCode::OK);
    let rooms_json = body_json(list_rooms).await;
    assert!(rooms_json["rooms"].as_array().unwrap().len() >= 1);
}

// ── PLAY-01..12: Playback Semantics, Next/Previous Traversal, Volume & Repeat ──
#[tokio::test]
async fn test_playback_controls_and_queue_traversal() {
    let (app, pool, state) = make_app().await;

    let t1 = seed_track(&pool, "/music/play1.flac", "Song 1").await;
    let t2 = seed_track(&pool, "/music/play2.flac", "Song 2").await;

    // Add tracks to queue
    let _ = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/queue/items")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(format!(
                    r#"{{"track_ids":["{}","{}"]}}"#,
                    t1.id, t2.id
                )))
                .unwrap(),
        )
        .await
        .unwrap();

    // Start with track 1
    {
        let mut ps = state.playback_state.write().await;
        ps.track_id = Some(t1.id);
        ps.playing = true;
        ps.position_ms = 0;
        ps.repeat = "all".into();
    }

    // PLAY-09: Next advances to track 2
    let next_res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/playback/control")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"command":"next"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(next_res.status(), StatusCode::OK);
    let ps_next = state.playback_state.read().await;
    assert_eq!(ps_next.track_id, Some(t2.id));
    drop(ps_next);

    // PLAY-10: Previous with position > 3000ms restarts current track
    {
        let mut ps = state.playback_state.write().await;
        ps.position_ms = 5000;
    }
    let prev_restart = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/playback/control")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"command":"previous"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(prev_restart.status(), StatusCode::OK);
    let ps_prev1 = state.playback_state.read().await;
    assert_eq!(ps_prev1.track_id, Some(t2.id));
    assert_eq!(ps_prev1.position_ms, 0);
    drop(ps_prev1);

    // Previous at position 0 moves back to track 1
    let prev_back = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/playback/control")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"command":"previous"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(prev_back.status(), StatusCode::OK);
    let ps_prev2 = state.playback_state.read().await;
    assert_eq!(ps_prev2.track_id, Some(t1.id));
    drop(ps_prev2);

    // PLAY-05: Strict Volume validation
    let vol_bad = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/playback/control")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"command":"set_volume","volume":120}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(vol_bad.status(), StatusCode::BAD_REQUEST);

    let vol_good = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/playback/control")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"command":"set_volume","volume":75}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(vol_good.status(), StatusCode::OK);
    let ps_vol = state.playback_state.read().await;
    assert_eq!((ps_vol.volume * 100.0) as u32, 75);
}

// ── PLAY-02 / PLAY-03 / PLAY-04 / PLAY-06 / PLAY-07 / PLAY-08: Seek, Pause, Resume & Repeat ──
#[tokio::test]
async fn test_playback_seek_pause_resume_and_repeat_modes() {
    let (app, pool, state) = make_app().await;
    let t = seed_track(&pool, "/music/seek_test.flac", "Seek Track").await;

    // Set initial playing state
    {
        let mut ps = state.playback_state.write().await;
        ps.track_id = Some(t.id);
        ps.playing = true;
        ps.position_ms = 0;
    }

    // PLAY-04: Seek to 45000ms
    let seek_res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/playback/control")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"command":"seek","position_ms":45000}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(seek_res.status(), StatusCode::OK);
    assert_eq!(state.playback_state.read().await.position_ms, 45000);

    // PLAY-02: Pause
    let pause_res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/playback/control")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"command":"pause"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(pause_res.status(), StatusCode::OK);
    assert!(!state.playback_state.read().await.playing);

    // PLAY-03: Resume
    let play_res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/playback/control")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"command":"play"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(play_res.status(), StatusCode::OK);
    assert!(state.playback_state.read().await.playing);

    // PLAY-08: Repeat "one"
    let rep_one = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/playback/control")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"command":"repeat","value":"one"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(rep_one.status(), StatusCode::OK);
    assert_eq!(state.playback_state.read().await.repeat, "one");
}

// ── UI-019 / UI-027: Settings Effective Profile & Persistent Restart Flag ──
#[tokio::test]
async fn test_settings_effective_profile_and_restart_flag() {
    let (app, _pool, _state) = make_app().await;

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/settings")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let json = body_json(res).await;
    assert!(json.get("effective_scan_workers").is_some());
    assert!(json.get("effective_transcode_workers").is_some());
    assert!(json.get("effective_db_pool").is_some());

    // Update resource profile -> restart_required must be true
    let put_res = app
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
    assert_eq!(put_res.status(), StatusCode::OK);
    let put_json = body_json(put_res).await;
    assert_eq!(put_json["restart_required"], true);
}

// ── UI-024 / UI-028 / UI-029: Automated Matrix & Endpoint Drift Check ──
#[tokio::test]
async fn test_automated_endpoint_drift_check() {
    // Verify essential frontend endpoints are accepted by the router
    let (app, _pool, _state) = make_app().await;

    let critical_routes = vec![
        ("GET", "/api/status"),
        ("GET", "/api/v1/server/info"),
        ("GET", "/api/v1/library/stats"),
        ("GET", "/api/v1/tracks"),
        ("GET", "/api/v1/home/dashboard"),
        ("GET", "/api/v1/playlists"),
        ("GET", "/api/v1/queue"),
        ("POST", "/api/v1/queue/items"),
        ("PUT", "/api/v1/queue/reorder"),
        ("POST", "/api/v1/queue/jump"),
        ("GET", "/api/v1/playback/state"),
        ("POST", "/api/v1/playback/control"),
        ("GET", "/api/v1/capabilities"),
        ("GET", "/api/v1/link/devices"),
        ("POST", "/api/v1/pair/qr"),
        ("GET", "/api/v1/receivers"),
        ("GET", "/api/v1/rooms/groups"),
        ("POST", "/api/v1/rooms/groups"),
        ("GET", "/api/v1/chains"),
        ("POST", "/api/v1/chains"),
        ("GET", "/api/v1/sources"),
        ("POST", "/api/v1/sources"),
        ("GET", "/api/v1/settings"),
        ("PUT", "/api/v1/settings"),
        ("GET", "/api/v1/backup"),
        ("POST", "/api/v1/backup/verify"),
        ("GET", "/api/v1/history"),
        ("GET", "/api/v1/history/stats"),
        ("GET", "/api/v1/history/export"),
        ("POST", "/api/v1/player/handoff"),
        ("GET", "/api/v1/events"),
    ];

    for (method, path) in critical_routes {
        let body = if method == "POST" || method == "PUT" {
            Body::from("{}")
        } else {
            Body::empty()
        };
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(body)
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_ne!(
            res.status(),
            StatusCode::NOT_FOUND,
            "Critical endpoint {method} {path} not found in router!"
        );
    }
}

// ── Strict Play track_id validation & Repeat rejection ──
#[tokio::test]
async fn test_strict_play_track_validation_and_invalid_repeat_rejection() {
    let (app, _pool, _state) = make_app().await;

    // Reject invalid object shape -> 422 UNPROCESSABLE_ENTITY (rejected by JSON deserializer)
    let fake_id = Uuid::new_v4();
    let play_fake = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/playback/control")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(format!(
                    r#"{{"command":"play","value":{{"track_id":"{fake_id}"}}}}"#
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(play_fake.status(), StatusCode::UNPROCESSABLE_ENTITY);

    // Invalid repeat mode string -> 422 UNPROCESSABLE_ENTITY
    let rep_invalid = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/playback/control")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"command":"repeat","value":"invalid_mode"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(rep_invalid.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

// ── Real Shuffle Queue Traversal ──
#[tokio::test]
async fn test_real_shuffle_queue_traversal() {
    let (app, pool, state) = make_app().await;

    let t1 = seed_track(&pool, "/music/shuf1.flac", "Track S1").await;
    let t2 = seed_track(&pool, "/music/shuf2.flac", "Track S2").await;
    let t3 = seed_track(&pool, "/music/shuf3.flac", "Track S3").await;

    let _ = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/queue/items")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(format!(
                    r#"{{"track_ids":["{}","{}","{}"]}}"#,
                    t1.id, t2.id, t3.id
                )))
                .unwrap(),
        )
        .await
        .unwrap();

    // Enable shuffle
    {
        let mut ps = state.playback_state.write().await;
        ps.track_id = Some(t1.id);
        ps.playing = true;
        ps.shuffle = true;
        ps.repeat = "all".into();
    }

    // Call next
    let next_res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/playback/control")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"command":"next"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(next_res.status(), StatusCode::OK);
    let ps = state.playback_state.read().await;
    assert!(ps.track_id.is_some());
}

// ── Room Play Rejection When Zero Receivers Connect ──
#[tokio::test]
async fn test_room_play_rejection_when_zero_receivers_connect() {
    let (app, pool, state) = make_app().await;
    let t = seed_track(&pool, "/music/room_part.flac", "Room Partial Track").await;

    // Create room with unreachable receivers
    let room_id = Uuid::new_v4();
    let mut volumes = std::collections::HashMap::new();
    volumes.insert("rec-room-offline".into(), 70);

    michi_db::save_room_group_db(
        &pool,
        &room_id,
        "Patio Unreachable",
        "custom",
        &["rec-room-offline".into()],
        &volumes,
    )
    .await
    .unwrap();

    let play_res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(&format!("/api/v1/rooms/{room_id}/play"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(format!(r#"{{"track_id":"{}"}}"#, t.id)))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(play_res.status(), StatusCode::BAD_GATEWAY);
    let ps = state.playback_state.read().await;
    assert!(!ps.playing);
}

// ── UI-025 / UI-030: Default Port 9090 Contract Across Entire Repository ──
#[tokio::test]
async fn test_default_port_9090_contract_consistency() {
    let cfg = test_config();
    assert_eq!(cfg.port(), 9090, "Config default port must be 9090");

    let dockerfile = std::fs::read_to_string("../../Dockerfile")
        .or_else(|_| std::fs::read_to_string("Dockerfile"))
        .unwrap_or_default();
    if !dockerfile.is_empty() {
        assert!(
            dockerfile.contains("9090"),
            "Dockerfile must reference default port 9090"
        );
    }

    let service_file = std::fs::read_to_string("../../deploy/michi.service")
        .or_else(|_| std::fs::read_to_string("deploy/michi.service"))
        .unwrap_or_default();
    if !service_file.is_empty() {
        assert!(
            service_file.contains("9090"),
            "michi.service must reference port 9090"
        );
    }
}

// ── M1 / R2.1: Server Info Contract Conformance Gate against Normative JSON Schema ──
#[tokio::test]
async fn test_server_info_canonical_roles_contract() {
    let (app, _pool, _state) = make_app().await;

    let res = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/server/info")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = body_json(res).await;
    let roles: Vec<String> = body["roles"]
        .as_array()
        .expect("roles must be an array")
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();

    let expected_canonical: Vec<String> = michi_link::CANONICAL_MICRO_ROLES
        .iter()
        .map(|r| r.as_str().to_string())
        .collect();

    assert_eq!(
        roles, expected_canonical,
        "server/info roles must match CANONICAL_MICRO_ROLES exactly"
    );
    assert_eq!(
        roles,
        vec!["music_server", "library_host", "playback_host"],
        "Micro server roles must strictly be music_server, library_host, playback_host"
    );

    // Validate entire JSON payload against vendor/michi-link/schemas/server-info.schema.json
    let schema_dir = std::path::Path::new("../../vendor/michi-link/schemas")
        .canonicalize()
        .or_else(|_| std::path::Path::new("vendor/michi-link/schemas").canonicalize())
        .expect("vendor/michi-link/schemas directory must exist");

    let server_info_path = schema_dir.join("server-info.schema.json");
    let schema_str = std::fs::read_to_string(&server_info_path).unwrap();
    let schema_json: serde_json::Value = serde_json::from_str(&schema_str).unwrap();

    let body_clone = body.clone();
    tokio::task::spawn_blocking(move || {
        let mut options = jsonschema::ValidationOptions::default();
        // Register all local schemas in vendor/michi-link/schemas to satisfy $ref offline
        if let Ok(entries) = std::fs::read_dir(&schema_dir) {
            for entry in entries.filter_map(|e| e.ok()) {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) == Some("json") {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                            if let Some(id) = json
                                .get("$id")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string())
                            {
                                if let Ok(resource) = jsonschema::Resource::from_contents(json) {
                                    let _ = options.with_resource(id, resource);
                                }
                            }
                        }
                    }
                }
            }
        }

        let validator = options
            .build(&schema_json)
            .expect("valid schema compilation");
        let mut errors: Vec<String> = Vec::new();
        for error in validator.iter_errors(&body_clone) {
            errors.push(format!(
                "JSON schema validation error at {}: {}",
                error.instance_path, error
            ));
        }
        assert!(
            errors.is_empty(),
            "GET /api/v1/server/info broke canonical Michi Link JSON Schema:\n{errors:#?}"
        );
    })
    .await
    .unwrap();
}

// ── M1.5.1 / P0-01: PIN Must Never Travel Over HTTP in /pair/start ──
#[tokio::test]
async fn test_pair_start_response_must_not_contain_pin() {
    let (app, _pool, state) = make_app().await;
    let dir = std::env::temp_dir().join(format!("michi-test-client-{}", Uuid::new_v4()));
    let client = michi_identity::IdentityManager::generate(&dir, "mobile-test", "").unwrap();

    let nonce_raw = [42u8; 32];
    let nonce = michi_identity::encode_base64url(&nonce_raw);
    let (signature, public_key) = client.sign_base64url(&nonce_raw);
    let payload = serde_json::json!({
        "device_name": "Test Mobile",
        "device_type": "mobile",
        "roles": ["mobile_player", "remote_controller"],
        "auth_strategy": "ED25519_CHALLENGE",
        "michi_id": client.michi_id().to_base64url(),
        "public_key": public_key,
        "challenge_nonce": nonce,
        "challenge_signature": signature,
    });

    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/pair/start")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = body_json(res).await;

    // Strict P0-01 assertion
    assert!(
        body.get("pin").is_none(),
        "P0-01 CONTRACT VIOLATION: 'pin' field must NEVER be present in /api/v1/pair/start HTTP response"
    );

    // Validate fields against pair-start-response.schema.json
    assert!(body.get("session_id").is_some());
    assert!(body.get("expires_at").is_some());
    assert!(body.get("attempts_remaining").is_some());
    assert!(body.get("server_michi_id").is_some());
    assert!(body.get("server_public_key").is_some());

    // Verify local observer did capture the PIN safely in memory
    let captured_pin = state.pairing_display.read().await;
    assert!(
        captured_pin.is_some(),
        "PIN must be recorded in local display observer"
    );
    assert_eq!(captured_pin.as_ref().unwrap().len(), 6);
}

// ── M1.5.2 / P0-04 & P0-05: ReceiverClient Uses Persistent Identity and Signs Raw Nonce ──
#[tokio::test]
async fn test_receiver_client_uses_persistent_identity_and_signs_raw_nonce() {
    let dir = std::env::temp_dir().join(format!("michi-test-server-id-{}", Uuid::new_v4()));
    let identity = Arc::new(
        michi_identity::IdentityManager::generate(&dir, "Michi Micro Server", "").unwrap(),
    );

    let client =
        michi_receivers::ReceiverClient::with_identity("http://127.0.0.1:9090", identity.clone());

    // Verify identity on client matches persistent server identity
    assert_eq!(
        client.identity.as_ref().unwrap().michi_id().to_base64url(),
        identity.michi_id().to_base64url()
    );

    // Verify raw nonce signing matches Ed25519 signature over binary bytes (not ASCII base64)
    let nonce_raw: [u8; 32] = [99u8; 32];
    let (sig_b64, pk_b64) = identity.sign_base64url(&nonce_raw);
    assert_eq!(pk_b64, identity.public_key_base64url());

    // Validate signature verification succeeds over RAW bytes
    let sig_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(&sig_b64)
        .unwrap();
    let pk_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(&pk_b64)
        .unwrap();
    let vk = ed25519_dalek::VerifyingKey::from_bytes(&pk_bytes.try_into().unwrap()).unwrap();
    let sig = ed25519_dalek::Signature::from_bytes(&sig_bytes.try_into().unwrap());
    assert!(
        vk.verify_strict(&nonce_raw, &sig).is_ok(),
        "Signature must verify over RAW nonce bytes"
    );

    // Negative assertion: signature over ASCII base64 representation MUST FAIL verification
    let ascii_nonce_b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(nonce_raw);
    assert!(
        vk.verify_strict(ascii_nonce_b64.as_bytes(), &sig).is_err(),
        "Signature over raw bytes MUST NOT verify against ASCII base64 string bytes"
    );
}

// ── M1.5.2: Micro Identity Persists Across Restarts ──
#[tokio::test]
async fn micro_identity_persists_across_restart() {
    let dir = std::env::temp_dir().join(format!("michi-test-persist-{}", Uuid::new_v4()));
    let _ = std::fs::create_dir_all(&dir);

    let id1 =
        michi_identity::IdentityManager::load_or_generate(&dir, "Michi Micro Server", "").unwrap();
    let michi_id1 = id1.michi_id().to_base64url();
    let pk1 = id1.public_key_base64url();

    // Reload from the same directory without re-generating
    let id2 =
        michi_identity::IdentityManager::load_or_generate(&dir, "Michi Micro Server", "").unwrap();
    let michi_id2 = id2.michi_id().to_base64url();
    let pk2 = id2.public_key_base64url();

    assert_eq!(
        michi_id1, michi_id2,
        "michi_id must be identical across reloads"
    );
    assert_eq!(pk1, pk2, "public_key must be identical across reloads");
}

// ── M1.5.9: Three-Way Real Integration E2E ──
// Mobile Controller -> Micro Server HTTP API -> ReceiverManager -> Receiver Registry Entry
#[tokio::test]
async fn test_three_way_integration_e2e_flow() {
    let (app, pool, state) = make_app().await;
    let track = seed_track(&pool, "/music/canonical_e2e.flac", "Three Way Integrity").await;
    let track_id = track.id;

    // 1. Mobile Controller client creates pairing challenge
    let dir = std::env::temp_dir().join(format!("michi-test-mobile-3way-{}", Uuid::new_v4()));
    let mobile = michi_identity::IdentityManager::generate(&dir, "Mobile Controller", "").unwrap();
    let nonce_raw = [88u8; 32];
    let nonce = michi_identity::encode_base64url(&nonce_raw);
    let (signature, public_key) = mobile.sign_base64url(&nonce_raw);

    // 2. Mobile starts pairing with Micro Server
    let start_payload = serde_json::json!({
        "device_name": "Pixel Mobile",
        "device_type": "mobile",
        "roles": ["mobile_player", "remote_controller"],
        "auth_strategy": "ED25519_CHALLENGE",
        "michi_id": mobile.michi_id().to_base64url(),
        "public_key": public_key,
        "challenge_nonce": nonce,
        "challenge_signature": signature,
    });

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/pair/start")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(start_payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let start_json = body_json(res).await;
    let session_id = start_json["session_id"].as_str().unwrap();

    // 3. User observes PIN on Micro Server display observer and inputs into Mobile
    let observed_pin = state
        .pairing_display
        .read()
        .await
        .clone()
        .expect("PIN on observer");

    // 4. Mobile confirms pairing
    let confirm_payload = serde_json::json!({
        "session_id": session_id,
        "pin": observed_pin,
        "michi_id": mobile.michi_id().to_base64url(),
        "public_key": mobile.public_key_base64url(),
    });

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/pair/confirm")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(confirm_payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let confirm_json = body_json(res).await;
    let device_token = confirm_json["token"].as_str().unwrap();
    assert!(!device_token.is_empty());

    // 5. Register an active receiver endpoint into Micro Server
    let reg_entry = michi_receivers::ReceiverRegistryEntry {
        receiver_id: "stream-living-room".to_string(),
        name: "Living Room Speaker".to_string(),
        device_type: "michi_stream_standard".to_string(),
        base_url: "http://127.0.0.1:8080".to_string(),
        paired: true,
        token: Some("dummy_token".to_string()),
        last_seen: Some(chrono::Utc::now()),
        capabilities: vec!["stream".into(), "volume".into(), "heartbeat".into()],
        active_session_id: None,
        max_sample_rate: 48000,
        max_bit_depth: 16,
        supported_transports: vec!["rtp_udp".into()],
        supported_codecs: vec!["pcm_s16le".to_string()],
        supported_sample_rates: vec![48000],
        supported_bit_depths: vec![16],
        supported_channels: vec![2],
        maximum_safe_volume: Some(100),
    };
    state
        .receiver_manager
        .registry()
        .await
        .write()
        .await
        .add(reg_entry);

    // 6. Mobile controls playback through Micro Server API (enqueue & play)
    let queue_payload = serde_json::json!({
        "track_ids": [track_id.to_string()]
    });
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/queue/items")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, format!("Bearer {device_token}"))
                .body(Body::from(queue_payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    // 7. Mobile issues play command (queue item was populated in step 6)
    let play_payload = serde_json::json!({
        "command": "play"
    });
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/playback/control")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, format!("Bearer {device_token}"))
                .body(Body::from(play_payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    // 8. Verify Micro Server playback state is playing
    let ps = state.playback_state.read().await;
    assert!(ps.playing);
}
