use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Instant;

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{header, HeaderMap, HeaderValue, Request, StatusCode};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use ed25519_dalek::SigningKey;
use michi_api::routes::v1::pair::{resolve_canonical_qr_server_url, QrUrlPolicy};
use michi_api::routes::v1::receivers::{
    evaluate_home_member_presence, HOME_MEMBER_PRESENCE_TTL_MS,
};
use michi_api::{create_router, AirCatTlsVerifiedConnection, AppState};
use michi_config::Config;
use michi_playback::PlaybackLifecycle;
use serde_json::{json, Value};
use tower::ServiceExt;
use uuid::Uuid;

async fn build_r13_app(
    tmp: &std::path::Path,
    trust_proxy: bool,
) -> (axum::Router, AppState, String, std::path::PathBuf) {
    let music_dir = tmp.join("music");
    let config_dir = tmp.join("config");
    let cache_dir = tmp.join("cache");
    std::fs::create_dir_all(&music_dir).unwrap();
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::create_dir_all(&cache_dir).unwrap();

    let db_path = tmp.join("r13.db");
    let db_url = format!("sqlite://{}", db_path.display());
    let pool = michi_db::init_pool(&db_url).await.unwrap();

    let cfg = Config {
        port: 9090,
        music_paths: vec![music_dir.clone()],
        config_path: config_dir.clone(),
        cache_path: cache_dir,
        database_url: db_url,
        version: "1.0.0-rc.4",
        sync_peers: Vec::new(),
        sync_name: "michi-r13-test".to_string(),
        listenbrainz_token: None,
        lastfm_token: None,
        scrobble_enabled: false,
        auth_username: Some("admin".to_string()),
        auth_password: Some("SuperSecretPass123!".to_string()),
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
        trust_proxy,
        trusted_proxies: vec![IpAddr::V4(Ipv4Addr::LOCALHOST)],
        deployment_platform: "unknown".into(),
    };

    let identity = Arc::new(
        michi_identity::IdentityManager::load_or_generate(&cfg.config_path, "R1.3 Server", "pwd")
            .unwrap(),
    );
    let admin_id = Uuid::new_v4();
    michi_db::create_user(&pool, &admin_id, "admin", "SuperSecretPass123!", true)
        .await
        .unwrap();

    let state = AppState::new_with_identity(cfg, pool, Some(admin_id), identity);
    let token = state.auth_sessions.create_session(admin_id).await.unwrap();
    let app = create_router(state.clone());
    (app, state, format!("Bearer {token}"), music_dir)
}

async fn seed_real_wav_track(
    pool: &sqlx::SqlitePool,
    music_dir: &std::path::Path,
    name: &str,
) -> Uuid {
    let path = music_dir.join(format!("{name}.wav"));
    // Minimal valid 44-byte WAV header + 1920 bytes of silent PCM
    let mut wav = Vec::with_capacity(44 + 1920);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36u32 + 1920).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&2u16.to_le_bytes()); // stereo
    wav.extend_from_slice(&48000u32.to_le_bytes());
    wav.extend_from_slice(&(48000u32 * 4).to_le_bytes());
    wav.extend_from_slice(&4u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&1920u32.to_le_bytes());
    wav.resize(44 + 1920, 0u8);
    std::fs::write(&path, &wav).unwrap();

    let path_str = path.to_string_lossy().to_string();
    let track = michi_core::Track {
        id: Uuid::new_v5(&Uuid::NAMESPACE_URL, path_str.as_bytes()),
        title: Some(name.to_string()),
        artist: Some("Michi R1.3".to_string()),
        album: Some("Certification".to_string()),
        album_artist: Some("Michi R1.3".to_string()),
        duration_ms: Some(10_000),
        file_path: path_str,
        format: michi_core::AudioFormat::Wav,
        sample_rate: Some(48000),
        bit_depth: Some(16),
        channels: Some(2),
        artwork_id: None,
        genre: None,
        year: Some(2026),
        track_number: Some(1),
        disc_number: Some(1),
        content_hash: None,
        file_size: Some(wav.len() as u64),
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
    track.id
}

async fn response_json(resp: axum::response::Response) -> Value {
    let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap_or(Value::Null)
}

fn with_client_ip(mut req: Request<Body>, octet: u8) -> Request<Body> {
    req.extensions_mut()
        .insert(ConnectInfo(SocketAddr::from(([127, 0, 1, octet], 40000))));
    req
}

fn with_tls(mut req: Request<Body>) -> Request<Body> {
    req.extensions_mut().insert(AirCatTlsVerifiedConnection);
    req
}

async fn enable_test_aircat_tls(state: &AppState) {
    *state.aircat_tls.write().await = Some(michi_api::AirCatTlsInfo {
        pinned_base_url: "https://127.0.0.1:9443".to_string(),
        tls_spki_sha256: URL_SAFE_NO_PAD.encode([0xABu8; 32]),
        cert_path: std::path::PathBuf::from("/tmp/test-r13.crt"),
        key_path: std::path::PathBuf::from("/tmp/test-r13.key"),
        tls_port: 9443,
        tls_trust_mode: michi_api::TlsTrustMode::CaChained,
        certificate_profile: michi_api::TlsCertificateProfile::CaIssuedChainPresent,
        chain_cryptographically_verified: true,
        android_trust_verification_status: michi_api::AndroidTrustVerificationStatus::NotVerified,
        cert_chain_len: 2,
        cert_not_before: None,
        cert_not_after: None,
    });
    state.set_aircat_tls_listener_ready(true);
}

// ═══════════════════════════════════════════════════════════════════════════
// R13-01: Definitive set_volume parser, bounds, conflict & state invariance
// ═══════════════════════════════════════════════════════════════════════════
#[tokio::test]
async fn test_r13_01_set_volume_exhaustive_matrix_and_engine_invariance() {
    let tmp = std::env::temp_dir().join(format!("michi-r13-01-{}", Uuid::new_v4()));
    let (app, state, admin_auth, _music_dir) = build_r13_app(&tmp, false).await;

    // 1. Valid cases (canonical integer + legacy integer + legacy strict decimal text + matching pair)
    let valid_cases: &[(&str, u8)] = &[
        (r#"{"command":"set_volume","value":0}"#, 0),
        (r#"{"command":"set_volume","value":"0"}"#, 0),
        (r#"{"command":"set_volume","value":100}"#, 100),
        (r#"{"command":"set_volume","value":"100"}"#, 100),
        (r#"{"command":"set_volume","volume":65}"#, 65),
        (r#"{"command":"set_volume","value":75}"#, 75),
        (r#"{"command":"set_volume","value":"75"}"#, 75),
        (r#"{"command":"set_volume","volume":20,"value":"20"}"#, 20),
        (r#"{"command":"set_volume","volume":20,"value":20}"#, 20),
        (r#"{"command":"set_volume","volume":45,"value":null}"#, 45),
    ];

    let t0 = Instant::now();
    for (idx, (payload, expected_vol)) in valid_cases.iter().enumerate() {
        let resp = app
            .clone()
            .oneshot(with_client_ip(
                Request::builder()
                    .uri("/api/v1/playback/control")
                    .method("POST")
                    .header(header::AUTHORIZATION, &admin_auth)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(*payload))
                    .unwrap(),
                (idx as u8) + 1,
            ))
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "expected 200 OK for valid payload {payload}"
        );
        let body = response_json(resp).await;
        assert_eq!(body["status"], "accepted");
        assert_eq!(body["volume"], *expected_vol as u64);

        // Verify effective PlaybackEngine volume (not just HTTP response!)
        let snap = state.playback_engine.snapshot().await.unwrap();
        assert_eq!(
            snap.volume, *expected_vol,
            "engine volume must match {expected_vol} for {payload}"
        );
    }
    let elapsed_valid = t0.elapsed();
    assert!(
        elapsed_valid.as_millis() < 2000,
        "set_volume latency budget exceeded: {elapsed_valid:?}"
    );

    // 2. Establish sentinel volume = 37; EVERY invalid request MUST fail with 400 INVALID_REQUEST
    //    AND leave effective engine volume strictly equal to 37 (NEVER 80!).
    state.playback_engine.set_volume(37).await.unwrap();
    assert_eq!(state.playback_engine.snapshot().await.unwrap().volume, 37);

    let invalid_cases: &[&str] = &[
        r#"{"command":"set_volume","value":101}"#,
        r#"{"command":"set_volume","volume":101}"#,
        r#"{"command":"set_volume","value":-1}"#,
        r#"{"command":"set_volume","volume":-1}"#,
        r#"{"command":"set_volume","value":true}"#,
        r#"{"command":"set_volume","value":false}"#,
        r#"{"command":"set_volume","value":"75.5"}"#,
        r#"{"command":"set_volume","value":"75%"}"#,
        r#"{"command":"set_volume","value":"-1"}"#,
        r#"{"command":"set_volume","value":"1000"}"#,
        r#"{"command":"set_volume","value":"eighty"}"#,
        r#"{"command":"set_volume","value":""}"#,
        r#"{"command":"set_volume","value":" 50 "}"#,
        r#"{"command":"set_volume","value":null}"#,
        r#"{"command":"set_volume","volume":null}"#,
        r#"{"command":"set_volume"}"#,
        r#"{"command":"set_volume","value":4294967296}"#,
        r#"{"command":"set_volume","volume":4294967296}"#,
        r#"{"command":"set_volume","value":18446744073709551615}"#,
        r#"{"command":"set_volume","value":75.5}"#,
        r#"{"command":"set_volume","volume":75.5}"#,
        r#"{"command":"set_volume","volume":"75"}"#,
        r#"{"command":"set_volume","volume":20,"value":80}"#,
        r#"{"command":"set_volume","volume":20,"value":"80"}"#,
        r#"{"command":"set_volume","volume":20,"value":"bad"}"#,
    ];

    for (idx, payload) in invalid_cases.iter().enumerate() {
        let resp = app
            .clone()
            .oneshot(with_client_ip(
                Request::builder()
                    .uri("/api/v1/playback/control")
                    .method("POST")
                    .header(header::AUTHORIZATION, &admin_auth)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(*payload))
                    .unwrap(),
                (idx as u8) + 50,
            ))
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "expected 400 BAD_REQUEST for invalid payload {payload}"
        );
        let body = response_json(resp).await;
        assert_eq!(
            body["error"]["code"], "INVALID_REQUEST",
            "expected canonical INVALID_REQUEST error code for {payload}, got {body}"
        );
        let snap = state.playback_engine.snapshot().await.unwrap();
        assert_eq!(
            snap.volume, 37,
            "invalid set_volume payload {payload} MUST NOT mutate engine volume (got {})",
            snap.volume
        );
    }

    let _ = std::fs::remove_dir_all(&tmp);
}

// ═══════════════════════════════════════════════════════════════════════════
// R13-02 + §9: Queue active deletion, duplicates, concurrency, rollback & Michi Control
// ═══════════════════════════════════════════════════════════════════════════
#[tokio::test]
async fn test_r13_02_queue_deletion_duplicates_concurrency_rollback_and_control_flow() {
    let tmp = std::env::temp_dir().join(format!("michi-r13-02-{}", Uuid::new_v4()));
    let (app, state, admin_auth, music_dir) = build_r13_app(&tmp, false).await;

    let track_a = seed_real_wav_track(&state.db, &music_dir, "A").await;
    let track_b = seed_real_wav_track(&state.db, &music_dir, "B").await;
    let track_c = seed_real_wav_track(&state.db, &music_dir, "C").await;
    let track_d = seed_real_wav_track(&state.db, &music_dir, "D").await;
    let track_e = seed_real_wav_track(&state.db, &music_dir, "E").await;

    // 1. Add [A, B, C, D, E] to active queue via POST /api/v1/queue/items
    let add_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/queue/items")
                .method("POST")
                .header(header::AUTHORIZATION, &admin_auth)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({ "track_ids": [track_a, track_b, track_c, track_d, track_e] })
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        add_resp.status(),
        StatusCode::OK,
        "add queue items failed: {:?}",
        response_json(add_resp).await
    );

    // Inspect GET /api/v1/queue to obtain exact queue_item_ids
    let q_json = response_json(
        app.clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/queue")
                    .method("GET")
                    .header(header::AUTHORIZATION, &admin_auth)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
    )
    .await;
    let items = q_json["items"].as_array().unwrap();
    assert_eq!(items.len(), 5);
    let item_a = items[0]["id"].as_str().unwrap().to_string();
    let item_c = items[2]["id"].as_str().unwrap().to_string();

    // 2. Jump to index 2 (C) via POST /api/v1/queue/jump and pause
    let jump_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/queue/jump")
                .method("POST")
                .header(header::AUTHORIZATION, &admin_auth)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!({ "index": 2 }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(jump_resp.status(), StatusCode::OK);
    state.playback_engine.pause().await.unwrap();
    assert_eq!(
        state.playback_engine.snapshot().await.unwrap().track_id,
        Some(track_c)
    );

    // 3. Delete active middle track C -> MUST advance to D (NOT A or B) AND stay Paused!
    let t_del = Instant::now();
    let del_c = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/queue/items/{item_c}"))
                .method("DELETE")
                .header(header::AUTHORIZATION, &admin_auth)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(del_c.status(), StatusCode::OK);
    assert!(
        t_del.elapsed().as_millis() < 500,
        "queue deletion latency exceeded budget"
    );
    let del_c_json = response_json(del_c).await;
    assert_eq!(del_c_json["matched_by"], "queue_item_id");
    assert_eq!(del_c_json["remaining_count"], 4);

    let snap_after_del_c = state.playback_engine.snapshot().await.unwrap();
    assert_eq!(
        snap_after_del_c.track_id,
        Some(track_d),
        "deleting active C in [A,B,C,D,E] must select D"
    );
    assert_eq!(
        snap_after_del_c.lifecycle,
        PlaybackLifecycle::Paused,
        "deleting active track while paused MUST NOT auto-start playback"
    );

    // 4. Delete non-active item A (before D) -> D stays active, position preserved, Next -> E!
    let del_a = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/queue/items/{item_a}"))
                .method("DELETE")
                .header(header::AUTHORIZATION, &admin_auth)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(del_a.status(), StatusCode::OK);
    let snap_after_del_a = state.playback_engine.snapshot().await.unwrap();
    assert_eq!(snap_after_del_a.track_id, Some(track_d));
    assert_eq!(snap_after_del_a.queue_index, Some(1)); // [B, D, E]

    // Next command via POST /api/v1/playback/control MUST go to E (never B!)
    let next_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/playback/control")
                .method("POST")
                .header(header::AUTHORIZATION, &admin_auth)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"command":"next"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(next_resp.status(), StatusCode::OK);
    let state_json = response_json(
        app.clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/playback/state")
                    .method("GET")
                    .header(header::AUTHORIZATION, &admin_auth)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(state_json["track_id"], track_e.to_string());

    // 5. Repeat & Shuffle controls via Michi Control queue endpoints
    let rep_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/queue/repeat")
                .method("POST")
                .header(header::AUTHORIZATION, &admin_auth)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"mode":"all"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(rep_resp.status(), StatusCode::OK);
    let shuf_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/queue/shuffle")
                .method("POST")
                .header(header::AUTHORIZATION, &admin_auth)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"enabled":true}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(shuf_resp.status(), StatusCode::OK);
    let ps = response_json(
        app.clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/playback/state")
                    .method("GET")
                    .header(header::AUTHORIZATION, &admin_auth)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(ps["repeat"], "all");
    assert_eq!(ps["shuffle"], true);

    // Reset shuffle=false, repeat=off for deterministic duplicate & reorder checks
    state.playback_engine.set_shuffle(false).await.unwrap();
    state
        .playback_engine
        .set_repeat(michi_playback::RepeatMode::Off)
        .await
        .unwrap();

    // 6. Duplicate track_id safety: add duplicate track_b so queue has [B1, D, E, B2]
    let add_dup = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/queue/items")
                .method("POST")
                .header(header::AUTHORIZATION, &admin_auth)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!({ "track_ids": [track_b] }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(add_dup.status(), StatusCode::OK);

    let q_dup = response_json(
        app.clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/queue")
                    .method("GET")
                    .header(header::AUTHORIZATION, &admin_auth)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
    )
    .await;
    let dup_items = q_dup["items"].as_array().unwrap();
    assert_eq!(dup_items.len(), 4);
    let b1_item_id = dup_items[0]["id"].as_str().unwrap().to_string();
    let d_item_id = dup_items[1]["id"].as_str().unwrap().to_string();
    let e_item_id = dup_items[2]["id"].as_str().unwrap().to_string();
    let b2_item_id = dup_items[3]["id"].as_str().unwrap().to_string();

    // Reorder via PUT /api/v1/queue/reorder -> [B1, E, D, B2]
    let reorder_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/queue/reorder")
                .method("PUT")
                .header(header::AUTHORIZATION, &admin_auth)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({ "item_ids": [b1_item_id, e_item_id, d_item_id, b2_item_id] })
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(reorder_resp.status(), StatusCode::OK);

    // Delete B2 by its queue_item_id -> MUST leave B1 intact (3 items left, not 2!)
    let del_b2 = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/queue/items/{b2_item_id}"))
                .method("DELETE")
                .header(header::AUTHORIZATION, &admin_auth)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(del_b2.status(), StatusCode::OK);
    let q_after_b2 = response_json(
        app.clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/queue")
                    .method("GET")
                    .header(header::AUTHORIZATION, &admin_auth)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(q_after_b2["items_count"], 3);
    assert_eq!(q_after_b2["items"][0]["id"], b1_item_id);

    // Add second copy of B again and delete using legacy track_id -> MUST delete AT MOST ONE row!
    let _ = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/queue/items")
                .method("POST")
                .header(header::AUTHORIZATION, &admin_auth)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!({ "track_ids": [track_b] }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let del_legacy = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/queue/items/{track_b}"))
                .method("DELETE")
                .header(header::AUTHORIZATION, &admin_auth)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(del_legacy.status(), StatusCode::OK);
    let del_legacy_json = response_json(del_legacy).await;
    assert_eq!(del_legacy_json["matched_by"], "legacy_track_id");
    assert_eq!(
        del_legacy_json["remaining_count"], 3,
        "legacy track_id deletion MUST delete at most ONE duplicate entry"
    );

    // 7. Concurrent deletions serialize cleanly with consistent SQLite + engine state
    let q_before_conc = response_json(
        app.clone()
            .oneshot(with_client_ip(
                Request::builder()
                    .uri("/api/v1/queue")
                    .method("GET")
                    .header(header::AUTHORIZATION, &admin_auth)
                    .body(Body::empty())
                    .unwrap(),
                201,
            ))
            .await
            .unwrap(),
    )
    .await;
    let id1 = q_before_conc["items"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let id2 = q_before_conc["items"][1]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let (r1, r2) = tokio::join!(
        app.clone().oneshot(with_client_ip(
            Request::builder()
                .uri(format!("/api/v1/queue/items/{id1}"))
                .method("DELETE")
                .header(header::AUTHORIZATION, &admin_auth)
                .body(Body::empty())
                .unwrap(),
            202,
        )),
        app.clone().oneshot(with_client_ip(
            Request::builder()
                .uri(format!("/api/v1/queue/items/{id2}"))
                .method("DELETE")
                .header(header::AUTHORIZATION, &admin_auth)
                .body(Body::empty())
                .unwrap(),
            203,
        ))
    );
    assert_eq!(r1.unwrap().status(), StatusCode::OK);
    assert_eq!(r2.unwrap().status(), StatusCode::OK);
    let q_after_conc = response_json(
        app.clone()
            .oneshot(with_client_ip(
                Request::builder()
                    .uri("/api/v1/queue")
                    .method("GET")
                    .header(header::AUTHORIZATION, &admin_auth)
                    .body(Body::empty())
                    .unwrap(),
                204,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(q_after_conc["items_count"], 1);
    let last_remaining_id = q_after_conc["items"][0]["id"].as_str().unwrap().to_string();

    // 8. PlaybackEngine failure during delete -> compensating SQLite rollback preserves item
    state.playback_engine.shutdown().await;
    let del_after_engine_down = app
        .clone()
        .oneshot(with_client_ip(
            Request::builder()
                .uri(format!("/api/v1/queue/items/{last_remaining_id}"))
                .method("DELETE")
                .header(header::AUTHORIZATION, &admin_auth)
                .body(Body::empty())
                .unwrap(),
            205,
        ))
        .await
        .unwrap();
    assert_eq!(
        del_after_engine_down.status(),
        StatusCode::INTERNAL_SERVER_ERROR
    );
    let surviving: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM queue_items WHERE id = ?")
        .bind(&last_remaining_id)
        .fetch_one(&state.db)
        .await
        .unwrap();
    assert_eq!(
        surviving, 1,
        "SQLite item must be rolled back / preserved if PlaybackEngine rejects sync"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

// ═══════════════════════════════════════════════════════════════════════════
// R13-04: Home Presence separation (Membership vs Auth vs Live Presence vs Revoked)
// ═══════════════════════════════════════════════════════════════════════════
#[tokio::test]
async fn test_r13_04_home_presence_separation_ttl_restart_and_fail_closed() {
    // 0. Pure projection TTL & state-separation checks
    let now_ms = 1_700_000_000_000i64;
    let enrolled_idle = evaluate_home_member_presence(false, false, None, false, now_ms);
    assert!(!enrolled_idle.online);
    assert!(!enrolled_idle.authenticated);
    assert_eq!(enrolled_idle.membership_status, "enrolled");
    assert_eq!(enrolled_idle.presence_state, "offline");

    let active_recent =
        evaluate_home_member_presence(false, true, Some(now_ms - 5_000), false, now_ms);
    assert!(active_recent.online);
    assert!(active_recent.authenticated);
    assert_eq!(active_recent.presence_state, "online");

    let expired_ttl = evaluate_home_member_presence(
        false,
        true,
        Some(now_ms - HOME_MEMBER_PRESENCE_TTL_MS - 1),
        false,
        now_ms,
    );
    assert!(!expired_ttl.online);
    assert!(expired_ttl.authenticated);
    assert_eq!(expired_ttl.presence_state, "offline");

    let revoked_even_with_signals =
        evaluate_home_member_presence(true, true, Some(now_ms), true, now_ms);
    assert!(!revoked_even_with_signals.online);
    assert!(!revoked_even_with_signals.authenticated);
    assert_eq!(revoked_even_with_signals.presence_state, "revoked");
    assert_eq!(revoked_even_with_signals.membership_status, "revoked");

    // 1. Live AppState integration test: enrolled mobile device vs pending device vs revoked device
    let tmp = std::env::temp_dir().join(format!("michi-r13-04-{}", Uuid::new_v4()));
    let (app, state, admin_auth, _music_dir) = build_r13_app(&tmp, false).await;
    enable_test_aircat_tls(&state).await;
    let home_id = state.home_root_authority.home_id();

    let enroll_device = |seed: u8| {
        let sk = SigningKey::from_bytes(&[seed; 32]);
        let pk_bytes = sk.verifying_key().to_bytes();
        let pk_b64 = URL_SAFE_NO_PAD.encode(pk_bytes);
        let michi_id = URL_SAFE_NO_PAD.encode(blake3::hash(&pk_bytes).as_bytes());
        (sk, pk_b64, michi_id)
    };

    let (sk_mobile, pk_mobile, mid_mobile) = enroll_device(0x31);
    let (_sk_pending, pk_pending, mid_pending) = enroll_device(0x32);
    let (_sk_revoked, pk_revoked, mid_revoked) = enroll_device(0x33);

    // Seed pending (unapproved) enrollment request — must NEVER appear as enrolled/online in roster
    let pending_req_id = Uuid::new_v4().to_string();
    let now_rfc = chrono::Utc::now().to_rfc3339();
    let exp_dt = chrono::Utc::now() + chrono::Duration::minutes(10);
    let exp_rfc = exp_dt.to_rfc3339();
    let now_ts_ms = chrono::Utc::now().timestamp_millis();
    michi_db::create_home_enrollment_request_db(
        &state.db,
        &michi_db::PersistedHomeEnrollment {
            request_id: pending_req_id,
            home_id: home_id.clone(),
            client_michi_id: mid_pending.clone(),
            client_public_key: pk_pending,
            device_name: "Pending Phone".to_string(),
            device_type: "mobile".to_string(),
            requested_roles_json: r#"["mobile_player"]"#.to_string(),
            claim_token_hash: "deadbeef".to_string(),
            status: "pending".to_string(),
            membership_json: None,
            created_at: now_rfc.clone(),
            expires_at: exp_rfc,
            expires_at_ms: exp_dt.timestamp_millis(),
            resolved_at: None,
        },
        now_ts_ms,
        32,
    )
    .await
    .unwrap();

    // Persist two approved Home members in SQLite (mobile + soon-to-be-revoked tablet)
    let mem_mobile = state.home_root_authority.issue_membership(
        &home_id,
        &mid_mobile,
        &pk_mobile,
        "mobile",
        vec![
            michi_identity::Role::MobilePlayer,
            michi_identity::Role::RemoteController,
        ],
        &now_rfc,
        10,
    );
    michi_db::upsert_home_member(
        &state.db,
        &michi_db::PersistedHomeMember {
            home_id: home_id.clone(),
            device_michi_id: mid_mobile.clone(),
            device_public_key: pk_mobile.clone(),
            device_name: "Pixel Mobile".to_string(),
            device_type: "mobile".to_string(),
            roles_json: serde_json::to_string(&mem_mobile.roles).unwrap(),
            membership_json: serde_json::to_string(&mem_mobile).unwrap(),
            serial: 10,
            issued_at: mem_mobile.issued_at.clone(),
            revoked: false,
            last_seen_at: now_rfc.clone(),
            created_at: now_rfc.clone(),
        },
    )
    .await
    .unwrap();

    let mem_revoked = state.home_root_authority.issue_membership(
        &home_id,
        &mid_revoked,
        &pk_revoked,
        "mobile",
        vec![michi_identity::Role::MobilePlayer],
        &now_rfc,
        11,
    );
    michi_db::upsert_home_member(
        &state.db,
        &michi_db::PersistedHomeMember {
            home_id: home_id.clone(),
            device_michi_id: mid_revoked.clone(),
            device_public_key: pk_revoked.clone(),
            device_name: "Lost Tablet".to_string(),
            device_type: "mobile".to_string(),
            roles_json: serde_json::to_string(&mem_revoked.roles).unwrap(),
            membership_json: serde_json::to_string(&mem_revoked).unwrap(),
            serial: 11,
            issued_at: mem_revoked.issued_at.clone(),
            revoked: false,
            last_seen_at: now_rfc.clone(),
            created_at: now_rfc.clone(),
        },
    )
    .await
    .unwrap();

    // 2. Enrolled-but-never-authenticated device MUST be OFFLINE (not online!)
    let roster_initial = response_json(
        app.clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/home/roster")
                    .method("GET")
                    .header(header::AUTHORIZATION, &admin_auth)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
    )
    .await;
    let devices = roster_initial["devices"].as_array().unwrap();
    assert!(
        !devices.iter().any(|d| d["device_michi_id"] == mid_pending),
        "pending enrollment must not appear as an approved/online member in roster"
    );
    let mobile_entry = devices
        .iter()
        .find(|d| d["device_michi_id"] == mid_mobile)
        .expect("enrolled mobile must appear in roster");
    assert_eq!(mobile_entry["membership_status"], "enrolled");
    assert_eq!(mobile_entry["revoked"], false);
    assert_eq!(
        mobile_entry["authenticated"], false,
        "enrolled-only device without active session must not be marked authenticated"
    );
    assert_eq!(
        mobile_entry["online"], false,
        "enrolled-only device without live evidence MUST be offline"
    );
    assert_eq!(mobile_entry["presence_state"], "offline");

    // 3. Complete real mutual Home auth handshake over HTTPS -> becomes authenticated & online!
    let ch_resp = app
        .clone()
        .oneshot(with_tls(
            Request::builder()
                .uri("/api/v1/auth/challenge")
                .method("POST")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&michi_identity::DeviceAuthChallengeRequest {
                        client_michi_id: mid_mobile.clone(),
                        client_public_key: pk_mobile.clone(),
                        home_id: home_id.clone(),
                    })
                    .unwrap(),
                ))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(ch_resp.status(), StatusCode::OK);
    let auth_ch: michi_identity::DeviceAuthChallengeResponse = serde_json::from_slice(
        &axum::body::to_bytes(ch_resp.into_body(), 64 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();
    let client_sig = michi_identity::sign_device_auth_challenge(
        &sk_mobile,
        &home_id,
        &auth_ch.server_michi_id,
        &mid_mobile,
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
                        client_michi_id: mid_mobile.clone(),
                        membership: mem_mobile.clone(),
                        client_signature: client_sig,
                    })
                    .unwrap(),
                ))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(sess_resp.status(), StatusCode::OK);

    let roster_online = response_json(
        app.clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/home/roster")
                    .method("GET")
                    .header(header::AUTHORIZATION, &admin_auth)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
    )
    .await;
    let mobile_online = roster_online["devices"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["device_michi_id"] == mid_mobile)
        .unwrap();
    assert_eq!(mobile_online["authenticated"], true);
    assert_eq!(mobile_online["online"], true);
    assert_eq!(mobile_online["presence_state"], "online");

    // 4. Advance activity age past HOME_MEMBER_PRESENCE_TTL -> transitions to offline!
    let stale_ts = chrono::Utc::now().timestamp_millis() - HOME_MEMBER_PRESENCE_TTL_MS - 5_000;
    state
        .aircat_security
        .override_member_last_activity_ms(&mid_mobile, stale_ts);
    let roster_ttl = response_json(
        app.clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/home/roster")
                    .method("GET")
                    .header(header::AUTHORIZATION, &admin_auth)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
    )
    .await;
    let mobile_ttl = roster_ttl["devices"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["device_michi_id"] == mid_mobile)
        .unwrap();
    assert_eq!(mobile_ttl["online"], false);
    assert_eq!(mobile_ttl["presence_state"], "offline");

    // 5. Revoke second device -> MUST show revoked=true, online=false, authenticated=false
    let rev_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/home/revoke")
                .method("POST")
                .header(header::AUTHORIZATION, &admin_auth)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({ "device_michi_id": mid_revoked, "reason": "lost" }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(rev_resp.status(), StatusCode::OK);

    let roster_after_rev = response_json(
        app.clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/home/roster")
                    .method("GET")
                    .header(header::AUTHORIZATION, &admin_auth)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
    )
    .await;
    let rev_entry = roster_after_rev["devices"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["device_michi_id"] == mid_revoked)
        .unwrap();
    assert_eq!(rev_entry["revoked"], true);
    assert_eq!(rev_entry["online"], false);
    assert_eq!(rev_entry["authenticated"], false);
    assert_eq!(rev_entry["presence_state"], "revoked");
    assert_eq!(rev_entry["membership_status"], "revoked");

    // 6. Simulate Micro Server restart (fresh AppState on the SAME SQLite DB):
    //    Persisted member MUST NOT be invented as online after restart!
    let pool_restarted = michi_db::init_pool(&format!("sqlite://{}", tmp.join("r13.db").display()))
        .await
        .unwrap();
    let restarted_cfg = state.config.clone();
    let restarted_identity = Arc::new(
        michi_identity::IdentityManager::load_or_generate(
            &restarted_cfg.config_path,
            "R1.3 Server",
            "pwd",
        )
        .unwrap(),
    );
    let restarted_state = AppState::new_with_identity(
        restarted_cfg,
        pool_restarted.clone(),
        state.admin_user_id,
        restarted_identity,
    );
    let restarted_token = restarted_state
        .auth_sessions
        .create_session(state.admin_user_id.unwrap())
        .await
        .unwrap();
    let restarted_app = create_router(restarted_state.clone());

    let roster_restarted = response_json(
        restarted_app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/home/roster")
                    .method("GET")
                    .header(header::AUTHORIZATION, format!("Bearer {restarted_token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
    )
    .await;
    let mobile_after_restart = roster_restarted["devices"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["device_michi_id"] == mid_mobile)
        .unwrap();
    assert_eq!(mobile_after_restart["membership_status"], "enrolled");
    assert_eq!(mobile_after_restart["authenticated"], false);
    assert_eq!(
        mobile_after_restart["online"], false,
        "server restart MUST NEVER invent online presence from persisted SQLite membership"
    );
    assert_eq!(mobile_after_restart["presence_state"], "offline");
    assert!(mobile_after_restart["last_seen"].is_string());

    // 7. Revocation store unavailable -> fail-closed 503 REVOCATION_STORE_UNAVAILABLE (never online=true)
    sqlx::query("DROP TABLE home_revocations")
        .execute(&pool_restarted)
        .await
        .unwrap();
    let fail_closed = restarted_app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/home/roster")
                .method("GET")
                .header(header::AUTHORIZATION, format!("Bearer {restarted_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(fail_closed.status(), StatusCode::SERVICE_UNAVAILABLE);
    let fc_json = response_json(fail_closed).await;
    assert_eq!(fc_json["error"]["code"], "REVOCATION_STORE_UNAVAILABLE");

    let _ = std::fs::remove_dir_all(&tmp);
}

// ═══════════════════════════════════════════════════════════════════════════
// R13-05: QR Canonical URL Security, Origin Policy & Legacy Protocol Separation
// ═══════════════════════════════════════════════════════════════════════════
#[tokio::test]
async fn test_r13_05_qr_canonical_url_security_and_protocol_separation() {
    let tmp = std::env::temp_dir().join(format!("michi-r13-05-{}", Uuid::new_v4()));
    let (app, state, admin_auth, _music_dir) = build_r13_app(&tmp, true).await;

    let trusted_peer = Some(IpAddr::V4(Ipv4Addr::LOCALHOST));
    let untrusted_peer = Some(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 77)));
    let base_policy = QrUrlPolicy {
        configured_public_url: None,
        allowed_hosts: vec!["music.home.example.com".to_string()],
        server_port: 9090,
    };

    // 1. Unit policy matrix covering every §7.5 case
    // Configured public URL takes precedence over untrusted Host / X-Forwarded-Host
    let pinned_policy = QrUrlPolicy {
        configured_public_url: Some("https://music.home.example.com".to_string()),
        allowed_hosts: vec!["music.home.example.com".to_string()],
        server_port: 9090,
    };
    let mut spoof_hdrs = HeaderMap::new();
    spoof_hdrs.insert("host", HeaderValue::from_static("evil.attacker.org"));
    spoof_hdrs.insert(
        "x-forwarded-host",
        HeaderValue::from_static("evil.attacker.org"),
    );
    assert_eq!(
        resolve_canonical_qr_server_url(
            &state.config,
            &pinned_policy,
            trusted_peer,
            false,
            &spoof_hdrs,
            None,
        )
        .unwrap(),
        "https://music.home.example.com"
    );

    // Legitimate LAN IP, mDNS .local, Tailscale .ts.net & CGNAT, IPv6 ULA
    for valid_url in [
        "http://192.168.1.42:9090",
        "http://10.0.0.5:9090",
        "http://michi-nas.local:9090",
        "https://michi.tail1234.ts.net",
        "http://100.88.12.34:9090",
        "http://[fd00::10]:9090",
        "https://music.home.example.com",
    ] {
        let resolved = resolve_canonical_qr_server_url(
            &state.config,
            &base_policy,
            untrusted_peer,
            false,
            &HeaderMap::new(),
            Some(valid_url),
        );
        assert_eq!(
            resolved.as_deref(),
            Ok(valid_url),
            "legitimate local/allowlisted URL must be accepted: {valid_url}"
        );
    }

    // Coherent trusted proxy HTTPS with authorized X-Forwarded-Host
    let mut coherent_proxy_hdrs = HeaderMap::new();
    coherent_proxy_hdrs.insert("x-forwarded-proto", HeaderValue::from_static("https"));
    coherent_proxy_hdrs.insert("forwarded", HeaderValue::from_static("proto=https"));
    coherent_proxy_hdrs.insert(
        "x-forwarded-host",
        HeaderValue::from_static("music.home.example.com"),
    );
    assert_eq!(
        resolve_canonical_qr_server_url(
            &state.config,
            &base_policy,
            trusted_peer,
            false,
            &coherent_proxy_hdrs,
            None,
        )
        .unwrap(),
        "https://music.home.example.com"
    );

    // Contradictory trusted proxy headers MUST fail closed!
    let mut contra_hdrs = HeaderMap::new();
    contra_hdrs.insert("x-forwarded-proto", HeaderValue::from_static("https"));
    contra_hdrs.insert("forwarded", HeaderValue::from_static("proto=http"));
    contra_hdrs.insert("x-forwarded-host", HeaderValue::from_static("michi.local"));
    assert!(resolve_canonical_qr_server_url(
        &state.config,
        &base_policy,
        trusted_peer,
        false,
        &contra_hdrs,
        None,
    )
    .is_err());

    // Spoofed X-Forwarded-Host from trusted proxy (unauthorized domain) MUST be rejected!
    let mut spoof_xfh = HeaderMap::new();
    spoof_xfh.insert("x-forwarded-proto", HeaderValue::from_static("https"));
    spoof_xfh.insert("x-forwarded-host", HeaderValue::from_static("evil.com"));
    assert!(resolve_canonical_qr_server_url(
        &state.config,
        &base_policy,
        trusted_peer,
        false,
        &spoof_xfh,
        None,
    )
    .is_err());

    // Spoofed X-Forwarded-Host + X-Forwarded-Proto from UNTRUSTED peer MUST be ignored!
    let mut untrusted_spoof = HeaderMap::new();
    untrusted_spoof.insert("host", HeaderValue::from_static("192.168.1.50:9090"));
    untrusted_spoof.insert("x-forwarded-proto", HeaderValue::from_static("https"));
    untrusted_spoof.insert("x-forwarded-host", HeaderValue::from_static("evil.com"));
    assert_eq!(
        resolve_canonical_qr_server_url(
            &state.config,
            &base_policy,
            untrusted_peer,
            false,
            &untrusted_spoof,
            None,
        )
        .unwrap(),
        "http://192.168.1.50:9090"
    );

    // Reject malicious / invalid URLs: external unauthorized host, userinfo, javascript:, file:, path/query, CRLF, 0.0.0.0
    for malicious_url in [
        "https://evil.attacker.org",
        "http://8.8.8.8:9090",
        "https://trusted@192.168.1.10:9090",
        "https://user:pass@michi.local:9090",
        "javascript:alert(1)",
        "file:///etc/passwd",
        "ftp://192.168.1.10:9090",
        "http://192.168.1.10:9090/phish?next=evil",
        "http://192.168.1.10:9090#frag",
        "http://192.168.1.10:9090\r\nX-Injected: 1",
        "http://0.0.0.0:9090",
        "http://bad host.local:9090",
    ] {
        assert!(
            resolve_canonical_qr_server_url(
                &state.config,
                &base_policy,
                untrusted_peer,
                false,
                &HeaderMap::new(),
                Some(malicious_url),
            )
            .is_err(),
            "malicious/unauthorized URL MUST be rejected: {malicious_url}"
        );
    }

    // Multi-host X-Forwarded-Host injection rejected
    let mut multi_host = HeaderMap::new();
    multi_host.insert(
        "x-forwarded-host",
        HeaderValue::from_static("michi.local, evil.com"),
    );
    assert!(resolve_canonical_qr_server_url(
        &state.config,
        &base_policy,
        trusted_peer,
        false,
        &multi_host,
        None,
    )
    .is_err());

    // 2. End-to-end HTTP checks on /api/v1/pair/qr, /status, /svg, /claim
    // Unauthorized external Host header rejected with 400
    let bad_host_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/pair/qr")
                .method("POST")
                .header(header::AUTHORIZATION, &admin_auth)
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::HOST, "phishing.evil.example")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(bad_host_resp.status(), StatusCode::BAD_REQUEST);

    // Contradictory proxy headers from trusted 127.0.0.1 rejected with 400
    let mut contra_req = Request::builder()
        .uri("/api/v1/pair/qr")
        .method("POST")
        .header(header::AUTHORIZATION, &admin_auth)
        .header(header::CONTENT_TYPE, "application/json")
        .header("x-forwarded-proto", "https")
        .header("forwarded", "proto=http")
        .header("x-forwarded-host", "michi.local")
        .body(Body::empty())
        .unwrap();
    contra_req
        .extensions_mut()
        .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 55555))));
    let contra_resp = app.clone().oneshot(contra_req).await.unwrap();
    assert_eq!(contra_resp.status(), StatusCode::BAD_REQUEST);

    // Legitimate LAN request succeeds and strictly separates legacy QR from Home cryptographic anchor
    let ok_qr = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/pair/qr")
                .method("POST")
                .header(header::AUTHORIZATION, &admin_auth)
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::HOST, "192.168.1.42:9090")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(ok_qr.status(), StatusCode::OK);
    let qr_json = response_json(ok_qr).await;
    assert_eq!(qr_json["server_url"], "http://192.168.1.42:9090");
    assert_eq!(qr_json["qr_protocol"], "legacy_server_code_v1");
    assert!(qr_json.get("root_authority_public_key").is_none());
    assert!(qr_json.get("tls_spki_sha256").is_none());
    assert!(qr_json.get("home_id").is_none());

    // Expired QR is rejected on both /svg and /claim (410 GONE) and reported expired on /status
    let qr_code = qr_json["qr_code"].as_str().unwrap().to_string();
    let past = (chrono::Utc::now() - chrono::Duration::minutes(10)).to_rfc3339();
    sqlx::query("UPDATE pairing_qr_codes SET expires_at = ? WHERE qr_code = ?")
        .bind(&past)
        .bind(&qr_code)
        .execute(&state.db)
        .await
        .unwrap();

    let st_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/pair/qr/{qr_code}/status"))
                .method("GET")
                .header(header::AUTHORIZATION, &admin_auth)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(st_resp.status(), StatusCode::OK);
    assert_eq!(response_json(st_resp).await["status"], "expired");

    let svg_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/pair/qr/{qr_code}/svg"))
                .method("GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(svg_resp.status(), StatusCode::GONE);

    let claim_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/pair/qr/{qr_code}/claim"))
                .method("POST")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"device_name":"Late Phone"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(claim_resp.status(), StatusCode::GONE);

    let _ = std::fs::remove_dir_all(&tmp);
}
