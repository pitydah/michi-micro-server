use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{header, HeaderMap, Request, StatusCode};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use ed25519_dalek::SigningKey;
use michi_api::routes::v1::pair::{
    is_authorized_qr_host, resolve_canonical_qr_server_url, QrUrlPolicy,
};
use michi_api::{create_router, AirCatTlsVerifiedConnection, AppState};
use michi_config::Config;
use serde_json::{json, Value};
use tower::ServiceExt;
use uuid::Uuid;

async fn build_r14_app(
    tmp: &std::path::Path,
    trust_proxy: bool,
) -> (axum::Router, AppState, String, PathBuf) {
    let music_dir = tmp.join("music");
    let config_dir = tmp.join("config");
    let cache_dir = tmp.join("cache");
    std::fs::create_dir_all(&music_dir).unwrap();
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::create_dir_all(&cache_dir).unwrap();

    let db_path = tmp.join("r14.db");
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
        sync_name: "michi-r14-test".to_string(),
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
        michi_identity::IdentityManager::load_or_generate(&cfg.config_path, "R1.4 Server", "pwd")
            .unwrap(),
    );
    let admin_id = Uuid::new_v4();
    michi_db::create_user(&pool, &admin_id, "admin", "SuperSecretPass123!", true)
        .await
        .unwrap();

    let state = AppState::new_with_identity(cfg, pool, Some(admin_id), identity);
    *state.aircat_tls.write().await = Some(michi_api::AirCatTlsInfo {
        pinned_base_url: "https://localhost:9443".to_string(),
        tls_spki_sha256: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            .to_string(),
        cert_path: config_dir.join("leaf.crt"),
        key_path: config_dir.join("leaf.key"),
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
    let mut wav = Vec::with_capacity(44 + 1920);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36u32 + 1920).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&48000u32.to_le_bytes());
    wav.extend_from_slice(&(48000u32 * 4).to_le_bytes());
    wav.extend_from_slice(&4u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&1920u32.to_le_bytes());
    wav.resize(44 + 1920, 0u8);
    std::fs::write(&path, &wav).unwrap();

    let track = michi_core::Track {
        id: michi_core::track_id_from_library_path(music_dir, &path),
        title: Some(name.to_string()),
        artist: Some("Michi R1.4".to_string()),
        album: Some("Readiness".to_string()),
        album_artist: Some("Michi R1.4".to_string()),
        duration_ms: Some(10_000),
        file_path: path.to_string_lossy().to_string(),
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
    let bytes = axum::body::to_bytes(resp.into_body(), 2 * 1024 * 1024)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap_or(Value::Null)
}

fn with_client_ip(mut req: Request<Body>, octet: u8) -> Request<Body> {
    req.extensions_mut()
        .insert(ConnectInfo(SocketAddr::from(([127, 0, 2, octet], 41000))));
    req
}

fn with_tls(mut req: Request<Body>) -> Request<Body> {
    req.extensions_mut().insert(AirCatTlsVerifiedConnection);
    req
}

fn gen_client_keypair() -> (SigningKey, String, String) {
    let mut rng = rand::rngs::OsRng;
    let sk = SigningKey::generate(&mut rng);
    let vk = sk.verifying_key();
    let pk_b64 = URL_SAFE_NO_PAD.encode(vk.to_bytes());
    let mid = michi_identity::MichiId::from_public_key(&vk).to_base64url();
    (sk, pk_b64, mid)
}

fn repo_schemas_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../vendor/michi-link/schemas")
        .canonicalize()
        .expect("vendor/michi-link/schemas must exist")
}

fn compile_michi_link_schema(schema_filename: &str) -> jsonschema::Validator {
    let schema_dir = repo_schemas_dir();
    let target_path = schema_dir.join(schema_filename);
    let target_json: Value =
        serde_json::from_str(&std::fs::read_to_string(&target_path).unwrap()).unwrap();

    let mut options = jsonschema::ValidationOptions::default();
    for entry in std::fs::read_dir(&schema_dir)
        .unwrap()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("json") {
            let content = std::fs::read_to_string(&path).unwrap();
            let json: Value = serde_json::from_str(&content).unwrap();
            if let Some(id) = json.get("$id").and_then(|v| v.as_str()).map(str::to_string) {
                if let Ok(resource) = jsonschema::Resource::from_contents(json) {
                    options.with_resource(id, resource);
                }
            }
        }
    }
    options.build(&target_json).expect("valid schema")
}

fn assert_matches_schema(validator: &jsonschema::Validator, payload: &Value, context: &str) {
    let errors: Vec<String> = validator
        .iter_errors(payload)
        .map(|e| format!("{}: {}", e.instance_path, e))
        .collect();
    assert!(
        errors.is_empty(),
        "Schema validation failed for {context}:\n{}\nPayload:\n{}",
        errors.join("\n"),
        serde_json::to_string_pretty(payload).unwrap()
    );
}

async fn enroll_mobile_member(
    app: &axum::Router,
    state: &AppState,
    admin_auth: &str,
    device_name: &str,
    octet: u8,
) -> (
    SigningKey,
    String,
    String,
    michi_identity::DeviceMembershipDto,
    String,
) {
    let (sk, pk, mid) = gen_client_keypair();
    let home_id = state.home_root_authority.home_id();

    let ch_resp = app
        .clone()
        .oneshot(with_tls(with_client_ip(
            Request::builder()
                .uri("/api/v1/home/enrollment/challenge")
                .method("POST")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_string(&michi_identity::HomeEnrollmentChallengeRequest {
                        purpose: "home_enrollment".into(),
                        home_id: home_id.clone(),
                        client_michi_id: mid.clone(),
                        client_public_key: pk.clone(),
                    })
                    .unwrap(),
                ))
                .unwrap(),
            octet,
        )))
        .await
        .unwrap();
    assert_eq!(ch_resp.status(), StatusCode::OK);
    let ch: michi_identity::HomeEnrollmentChallengeResponse =
        serde_json::from_value(response_json(ch_resp).await).unwrap();

    let sig = michi_identity::sign_home_enrollment_pop(
        &sk,
        &home_id,
        &ch.server_michi_id,
        &mid,
        &pk,
        &ch.challenge_id.to_string(),
        &ch.challenge_nonce,
    );

    let req_resp = app
        .clone()
        .oneshot(with_tls(with_client_ip(
            Request::builder()
                .uri("/api/v1/home/enrollment/request")
                .method("POST")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_string(&michi_identity::HomeEnrollmentRequest {
                        home_id: home_id.clone(),
                        challenge_id: ch.challenge_id,
                        client_michi_id: mid.clone(),
                        client_public_key: pk.clone(),
                        device_name: device_name.to_string(),
                        device_type: "mobile".into(),
                        requested_roles: vec![
                            michi_identity::Role::MobilePlayer,
                            michi_identity::Role::RemoteController,
                        ],
                        client_signature: sig,
                    })
                    .unwrap(),
                ))
                .unwrap(),
            octet,
        )))
        .await
        .unwrap();
    assert_eq!(req_resp.status(), StatusCode::ACCEPTED);
    let enr_req: michi_identity::HomeEnrollmentRequestResponse =
        serde_json::from_value(response_json(req_resp).await).unwrap();

    let approve_resp = app
        .clone()
        .oneshot(with_tls(
            Request::builder()
                .uri(format!(
                    "/api/v1/home/enrollment/{}/approve",
                    enr_req.request_id
                ))
                .method("POST")
                .header(header::AUTHORIZATION, admin_auth)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(approve_resp.status(), StatusCode::OK);

    let status_resp = app
        .clone()
        .oneshot(with_tls(
            Request::builder()
                .uri(format!(
                    "/api/v1/home/enrollment/{}/status",
                    enr_req.request_id
                ))
                .header("X-Michi-Enrollment-Claim", &enr_req.claim_token)
                .body(Body::empty())
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(status_resp.status(), StatusCode::OK);
    let st: michi_identity::HomeEnrollmentStatusResponse =
        serde_json::from_value(response_json(status_resp).await).unwrap();

    (
        sk,
        pk,
        mid,
        st.membership.unwrap(),
        st.home_root_public_key.unwrap(),
    )
}

#[allow(clippy::too_many_arguments)]
async fn authenticate_home_member(
    app: &axum::Router,
    state: &AppState,
    sk: &SigningKey,
    pk: &str,
    mid: &str,
    membership: &michi_identity::DeviceMembershipDto,
    root_pk: &str,
    octet: u8,
) -> String {
    let home_id = state.home_root_authority.home_id();
    let ch_resp = app
        .clone()
        .oneshot(with_tls(with_client_ip(
            Request::builder()
                .uri("/api/v1/auth/challenge")
                .method("POST")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_string(&michi_identity::DeviceAuthChallengeRequest {
                        client_michi_id: mid.to_string(),
                        client_public_key: pk.to_string(),
                        home_id: home_id.clone(),
                    })
                    .unwrap(),
                ))
                .unwrap(),
            octet,
        )))
        .await
        .unwrap();
    assert_eq!(ch_resp.status(), StatusCode::OK);
    let ch: michi_identity::DeviceAuthChallengeResponse =
        serde_json::from_value(response_json(ch_resp).await).unwrap();

    let sig = michi_identity::sign_device_auth_challenge(
        sk,
        &home_id,
        &ch.server_michi_id,
        mid,
        &ch.challenge_id.to_string(),
        &ch.challenge_nonce,
    );

    let ver_resp = app
        .clone()
        .oneshot(with_tls(with_client_ip(
            Request::builder()
                .uri("/api/v1/auth/session")
                .method("POST")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_string(&michi_identity::DeviceAuthSessionRequest {
                        challenge_id: ch.challenge_id,
                        client_michi_id: mid.to_string(),
                        membership: membership.clone(),
                        client_signature: sig,
                    })
                    .unwrap(),
                ))
                .unwrap(),
            octet,
        )))
        .await
        .unwrap();
    assert_eq!(ver_resp.status(), StatusCode::OK);
    let sess: michi_identity::DeviceAuthSessionResponse =
        serde_json::from_value(response_json(ver_resp).await).unwrap();
    michi_identity::verify_server_auth_session(
        &sess,
        root_pk,
        &home_id,
        mid,
        &ch.challenge_id.to_string(),
        &[],
    )
    .unwrap();
    sess.session_token
}

/// R1.4 §3.1: Unified ActiveQueueCoordinator — concurrent mutations, shuffle preservation,
/// fail-closed QUEUE_DEGRADED blocking, and explicit reconciliation recovery.
#[tokio::test]
async fn test_r14_01_unified_queue_coordinator_concurrency_shuffle_and_degraded_recovery() {
    let tmp = std::env::temp_dir().join(format!("michi-r14-01-{}", Uuid::new_v4()));
    let (app, state, admin_auth, music_dir) = build_r14_app(&tmp, false).await;

    let mut tracks = Vec::new();
    for i in 0..8 {
        tracks.push(seed_real_wav_track(&state.db, &music_dir, &format!("r14_t{i}")).await);
    }

    // 1. Seed initial 4 tracks via POST /api/v1/queue/items
    let add_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/queue/items")
                .method("POST")
                .header(header::AUTHORIZATION, &admin_auth)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!({"track_ids": &tracks[0..4]}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(add_resp.status(), StatusCode::OK);
    let add_json = response_json(add_resp).await;
    let v1 = add_json["queue_version"].as_u64().unwrap();
    assert!(v1 >= 1);

    // 2. Concurrent delete + insert + reorder burst
    let q_before = response_json(
        app.clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/queue")
                    .header(header::AUTHORIZATION, &admin_auth)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
    )
    .await;
    let first_item_id = q_before["items"][0]["id"].as_str().unwrap().to_string();
    let second_item_id = q_before["items"][1]["id"].as_str().unwrap().to_string();

    let mut handles = Vec::new();
    // Two concurrent deletes of different items
    for del_id in [first_item_id.clone(), second_item_id.clone()] {
        let app_c = app.clone();
        let auth_c = admin_auth.clone();
        handles.push(tokio::spawn(async move {
            app_c
                .oneshot(
                    Request::builder()
                        .uri(format!("/api/v1/queue/items/{del_id}"))
                        .method("DELETE")
                        .header(header::AUTHORIZATION, &auth_c)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap()
                .status()
        }));
    }
    // Concurrent appends of tracks 4..8
    for tid in &tracks[4..8] {
        let app_c = app.clone();
        let auth_c = admin_auth.clone();
        let tid_c = *tid;
        handles.push(tokio::spawn(async move {
            app_c
                .oneshot(
                    Request::builder()
                        .uri("/api/v1/queue/items")
                        .method("POST")
                        .header(header::AUTHORIZATION, &auth_c)
                        .header(header::CONTENT_TYPE, "application/json")
                        .body(Body::from(json!({"track_ids": [tid_c]}).to_string()))
                        .unwrap(),
                )
                .await
                .unwrap()
                .status()
        }));
    }
    // Concurrent queue save
    {
        let app_c = app.clone();
        let auth_c = admin_auth.clone();
        let save_tracks = tracks[2..4].to_vec();
        handles.push(tokio::spawn(async move {
            app_c
                .oneshot(
                    Request::builder()
                        .uri("/api/v1/queue/save")
                        .method("POST")
                        .header(header::AUTHORIZATION, &auth_c)
                        .header(header::CONTENT_TYPE, "application/json")
                        .body(Body::from(
                            json!({"track_ids": save_tracks, "current_index": 0, "position_ms": 0})
                                .to_string(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap()
                .status()
        }));
    }

    for h in handles {
        let st = h.await.unwrap();
        assert_eq!(st, StatusCode::OK, "no concurrent queue op may 500/busy");
    }

    // Verify SQLite positions are strictly contiguous 0..N-1 (expected 4 - 2 + 4 = 6 items)
    let q_after = response_json(
        app.clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/queue")
                    .header(header::AUTHORIZATION, &admin_auth)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
    )
    .await;
    let items_after = q_after["items"].as_array().unwrap();
    assert_eq!(items_after.len(), 6);
    for (idx, it) in items_after.iter().enumerate() {
        assert_eq!(it["position"].as_u64().unwrap(), idx as u64);
    }
    let v_after = q_after["queue_version"].as_u64().unwrap();
    assert!(v_after > v1, "queue_version must advance monotonically");
    assert_eq!(q_after["queue_degraded"], false);

    // 3. Shuffle + delete preserves shuffle enabled & surviving tracks
    let shuf_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/queue/shuffle")
                .method("POST")
                .header(header::AUTHORIZATION, &admin_auth)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!({"enabled": true}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(shuf_resp.status(), StatusCode::OK);

    let drop_item_id = items_after[3]["id"].as_str().unwrap();
    let del_shuf = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/queue/items/{drop_item_id}"))
                .method("DELETE")
                .header(header::AUTHORIZATION, &admin_auth)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(del_shuf.status(), StatusCode::OK);
    let snap = state.playback_engine.snapshot().await.unwrap();
    assert!(snap.shuffle, "shuffle mode must stay active after deletion");

    // 4. Fail-closed QUEUE_DEGRADED blocks ALL mutators until explicit reconciliation
    state
        .queue_coordinator
        .mark_degraded("simulated rollback failure for R1.4 test");

    let q_deg = response_json(
        app.clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/queue")
                    .header(header::AUTHORIZATION, &admin_auth)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(q_deg["queue_degraded"], true);

    let remaining_item_id = q_deg["items"][0]["id"].as_str().unwrap().to_string();
    let blocked_reqs = vec![
        Request::builder()
            .uri("/api/v1/queue/items")
            .method("POST")
            .header(header::AUTHORIZATION, &admin_auth)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(json!({"track_ids": [tracks[0]]}).to_string()))
            .unwrap(),
        Request::builder()
            .uri(format!("/api/v1/queue/items/{remaining_item_id}"))
            .method("DELETE")
            .header(header::AUTHORIZATION, &admin_auth)
            .body(Body::empty())
            .unwrap(),
        Request::builder()
            .uri("/api/v1/queue/reorder")
            .method("PUT")
            .header(header::AUTHORIZATION, &admin_auth)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({"item_ids": [remaining_item_id]}).to_string(),
            ))
            .unwrap(),
        Request::builder()
            .uri("/api/v1/queue/jump")
            .method("POST")
            .header(header::AUTHORIZATION, &admin_auth)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(json!({"index": 0}).to_string()))
            .unwrap(),
        Request::builder()
            .uri("/api/v1/queues")
            .method("POST")
            .header(header::AUTHORIZATION, &admin_auth)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({"name": "blocked", "track_ids": []}).to_string(),
            ))
            .unwrap(),
    ];
    for req in blocked_reqs {
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
        let err_body = response_json(resp).await;
        assert_eq!(err_body["error"]["code"], "QUEUE_DEGRADED");
    }

    // Reconcile recovers degraded state cleanly
    let rec_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/queue/reconcile")
                .method("POST")
                .header(header::AUTHORIZATION, &admin_auth)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(rec_resp.status(), StatusCode::OK);
    let rec_json = response_json(rec_resp).await;
    assert_eq!(rec_json["status"], "reconciled");
    assert_eq!(rec_json["queue_degraded"], false);

    // Mutations succeed again after reconciliation
    let post_rec = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/queue/items")
                .method("POST")
                .header(header::AUTHORIZATION, &admin_auth)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!({"track_ids": [tracks[0]]}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(post_rec.status(), StatusCode::OK);

    let _ = std::fs::remove_dir_all(&tmp);
}

/// R1.4 §3.2: Validate LIVE `GET /api/v1/home/roster` JSON directly against the
/// authoritative `vendor/michi-link/schemas/home-device-roster.schema.json` (and
/// nested `michi-membership.schema.json`) across all Home member states.
#[tokio::test]
async fn test_r14_02_home_roster_runtime_json_schema_validation_all_states() {
    let validator = compile_michi_link_schema("home-device-roster.schema.json");
    let tmp = std::env::temp_dir().join(format!("michi-r14-02-{}", Uuid::new_v4()));
    let (app, state, admin_auth, _music_dir) = build_r14_app(&tmp, false).await;

    // 1. Unauthorized -> 401
    let unauth = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/home/roster")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unauth.status(), StatusCode::UNAUTHORIZED);

    // 2. Baseline server-only roster matches schema
    let r0 = response_json(
        app.clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/home/roster")
                    .header(header::AUTHORIZATION, &admin_auth)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
    )
    .await;
    assert_matches_schema(&validator, &r0, "server-only baseline roster");
    assert_eq!(r0["version"], 1);
    assert_eq!(r0["home_id"], state.home_root_authority.home_id());

    // 3. Enroll mobile device -> enrolled + offline (no active session)
    let (sk, pk, mid, membership, root_pk) =
        enroll_mobile_member(&app, &state, &admin_auth, "Pixel 9 Offline", 11).await;
    let r1 = response_json(
        app.clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/home/roster")
                    .header(header::AUTHORIZATION, &admin_auth)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
    )
    .await;
    assert_matches_schema(&validator, &r1, "enrolled-offline mobile roster");
    let d1 = r1["devices"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["device_michi_id"] == mid)
        .unwrap();
    assert_eq!(d1["membership_status"], "enrolled");
    assert_eq!(d1["authenticated"], false);
    assert_eq!(d1["online"], false);
    assert_eq!(d1["presence_state"], "offline");
    assert_eq!(d1["status"], "offline");
    assert!(d1["membership"].is_object());

    // 4. Authenticate -> authenticated + online
    let _session_tok =
        authenticate_home_member(&app, &state, &sk, &pk, &mid, &membership, &root_pk, 12).await;
    let r2 = response_json(
        app.clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/home/roster")
                    .header(header::AUTHORIZATION, &admin_auth)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
    )
    .await;
    assert_matches_schema(&validator, &r2, "authenticated-online mobile roster");
    let d2 = r2["devices"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["device_michi_id"] == mid)
        .unwrap();
    assert_eq!(d2["authenticated"], true);
    assert_eq!(d2["online"], true);
    assert_eq!(d2["presence_state"], "online");
    assert_eq!(d2["status"], "online");

    // 5. Revoke device -> revoked + not authenticated + offline
    let rev_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/home/revoke")
                .method("POST")
                .header(header::AUTHORIZATION, &admin_auth)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"device_michi_id": mid, "reason": "r14_schema_audit"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(rev_resp.status(), StatusCode::OK);

    let r3 = response_json(
        app.clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/home/roster")
                    .header(header::AUTHORIZATION, &admin_auth)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
    )
    .await;
    assert_matches_schema(&validator, &r3, "revoked mobile roster");
    let d3 = r3["devices"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["device_michi_id"] == mid)
        .unwrap();
    assert_eq!(d3["revoked"], true);
    assert_eq!(d3["authenticated"], false);
    assert_eq!(d3["online"], false);
    assert_eq!(d3["membership_status"], "revoked");
    assert_eq!(d3["presence_state"], "revoked");
    assert_eq!(d3["status"], "revoked");

    // 6. Fail-closed 503 when SQLite revocation store is unavailable
    sqlx::query("DROP TABLE home_revocations")
        .execute(&state.db)
        .await
        .unwrap();
    let r_fail = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/home/roster")
                .header(header::AUTHORIZATION, &admin_auth)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r_fail.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        response_json(r_fail).await["error"]["code"],
        "REVOCATION_STORE_UNAVAILABLE"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

/// R1.4 §3.3: Typed domain separation between `receiver_id` (registry key) and
/// `device_michi_id` (43-char cryptographic identity) when `receiver_id != device_michi_id`.
#[tokio::test]
async fn test_r14_03_typed_receiver_id_vs_device_michi_id_separation() {
    let validator = compile_michi_link_schema("home-device-roster.schema.json");
    let tmp = std::env::temp_dir().join(format!("michi-r14-03-{}", Uuid::new_v4()));
    let (app, state, admin_auth, _music_dir) = build_r14_app(&tmp, false).await;

    let (_sk, _pk, rx_michi_id) = gen_client_keypair();
    let rx_registry_id = "living-room-receiver-01".to_string();
    assert_ne!(rx_registry_id, rx_michi_id);

    // Register receiver in ReceiverRegistry with DISTINCT receiver_id and michi_id, initially offline
    {
        let reg_arc = state.receiver_manager.registry().await;
        let mut reg = reg_arc.write().await;
        reg.receivers.insert(
            rx_registry_id.clone(),
            michi_receivers::models::ReceiverRegistryEntry {
                receiver_id: rx_registry_id.clone(),
                michi_id: Some(rx_michi_id.clone()),
                name: "Living Room Stream".to_string(),
                device_type: "stream".to_string(),
                base_url: "http://127.0.0.1:8787".to_string(),
                paired: true,
                token: Some("tok".to_string()),
                presence: michi_receivers::models::ReceiverPresence::Offline,
                last_seen: None,
                capabilities: vec!["rtp_udp".to_string()],
                capabilities_verified_at: None,
                capabilities_stale: false,
                authority_supported: true,
                owner_michi_id: None,
                owner_name: None,
                active_session_id: None,
                max_sample_rate: 48000,
                max_bit_depth: 16,
                supported_transports: vec!["rtp_udp".to_string()],
                supported_codecs: vec!["pcm_s16le".to_string()],
                supported_sample_rates: vec![48000],
                supported_bit_depths: vec![16],
                supported_channels: vec![2],
                maximum_safe_volume: Some(100),
                qualification: michi_receivers::models::ReceiverQualification::Qualified,
                michi_home_id: Some(state.home_root_authority.home_id()),
                server_membership: None,
                authenticated: false,
                revoked: false,
            },
        );
    }

    // 1. Offline before session starts
    let r_off = response_json(
        app.clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/home/roster")
                    .header(header::AUTHORIZATION, &admin_auth)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
    )
    .await;
    assert_matches_schema(&validator, &r_off, "receiver offline roster");
    let dev_off = r_off["devices"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["device_michi_id"] == rx_michi_id)
        .unwrap();
    assert_eq!(dev_off["receiver_id"], rx_registry_id);
    assert_eq!(dev_off["online"], false);
    assert_eq!(dev_off["presence_state"], "offline");

    // 2. Active receiver session keyed ONLY by receiver_id (NOT device_michi_id!)
    {
        let sessions_arc = state.receiver_manager.active_sessions().await;
        let mut sessions = sessions_arc.write().await;
        sessions.insert(
            rx_registry_id.clone(),
            michi_receivers::models::ReceiverActiveSession {
                receiver_id: rx_registry_id.clone(),
                playback_session_id: Uuid::new_v4().to_string(),
                receiver_session_id: Uuid::new_v4().to_string(),
                session_token: None,
                device_token: None,
                stream_port: 5004,
                lease_seconds: 30,
                heartbeat_sequence: 1,
                negotiated_codec: "pcm_s16le".to_string(),
                negotiated_sample_rate: 48000,
                negotiated_bit_depth: 16,
                negotiated_channels: 2,
                payload_type: 96,
                ssrc: 12345,
                state: michi_receivers::models::ReceiverActiveSessionState::Active,
                created_at: chrono::Utc::now(),
                last_heartbeat: chrono::Utc::now(),
            },
        );
    }

    let r_on = response_json(
        app.clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/home/roster")
                    .header(header::AUTHORIZATION, &admin_auth)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
    )
    .await;
    assert_matches_schema(&validator, &r_on, "receiver active session roster");
    let dev_on = r_on["devices"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["device_michi_id"] == rx_michi_id)
        .unwrap();
    assert_eq!(dev_on["receiver_id"], rx_registry_id);
    assert_eq!(
        dev_on["online"], true,
        "active session keyed by receiver_id must project online even when receiver_id != device_michi_id"
    );
    assert_eq!(dev_on["authenticated"], true);
    assert_eq!(dev_on["presence_state"], "online");
    assert_eq!(dev_on["status"], "online");

    // 3. Stop session -> back to offline
    {
        let sessions_arc = state.receiver_manager.active_sessions().await;
        sessions_arc.write().await.remove(&rx_registry_id);
    }
    let r_stopped = response_json(
        app.clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/home/roster")
                    .header(header::AUTHORIZATION, &admin_auth)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
    )
    .await;
    assert_matches_schema(&validator, &r_stopped, "receiver session stopped roster");
    let dev_stopped = r_stopped["devices"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["device_michi_id"] == rx_michi_id)
        .unwrap();
    assert_eq!(dev_stopped["online"], false);
    assert_eq!(dev_stopped["presence_state"], "offline");

    let _ = std::fs::remove_dir_all(&tmp);
}

/// R1.4 §3.4: QR authorized origin strictly bound to THIS server's real NICs,
/// advertised mDNS hostnames, and operator allowlist — rejecting OTHER LAN/mDNS/Tailscale hosts.
#[tokio::test]
async fn test_r14_04_qr_origin_strictly_bound_to_server_interfaces_and_mdns() {
    let tmp = std::env::temp_dir().join(format!("michi-r14-04-{}", Uuid::new_v4()));
    let (app, state, admin_auth, _music_dir) = build_r14_app(&tmp, false).await;

    let policy = QrUrlPolicy {
        configured_public_url: None,
        allowed_hosts: vec!["my-michi.tail999.ts.net".to_string()],
        local_interface_ips: vec![
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            IpAddr::V4(Ipv4Addr::new(192, 168, 1, 20)),
        ],
        local_mdns_hosts: vec!["my-michi-server.local".to_string()],
        server_port: 9090,
    };

    // Server's own NIC, loopback, own mDNS, and configured Tailscale host -> ACCEPTED
    assert!(is_authorized_qr_host("192.168.1.20", &policy));
    assert!(is_authorized_qr_host("127.0.0.1", &policy));
    assert!(is_authorized_qr_host("localhost", &policy));
    assert!(is_authorized_qr_host("my-michi-server.local", &policy));
    assert!(is_authorized_qr_host("my-michi.tail999.ts.net", &policy));

    // OTHER LAN IP, OTHER RFC1918 IP, OTHER mDNS host, OTHER Tailscale host -> REJECTED!
    assert!(
        !is_authorized_qr_host("192.168.1.30", &policy),
        "another LAN host IP must be REJECTED"
    );
    assert!(
        !is_authorized_qr_host("10.0.0.44", &policy),
        "unrelated RFC1918 IP must be REJECTED"
    );
    assert!(
        !is_authorized_qr_host("other-host.local", &policy),
        "another machine's .local mDNS hostname must be REJECTED"
    );
    assert!(
        !is_authorized_qr_host("attacker.tail999.ts.net", &policy),
        "another Tailscale host must be REJECTED"
    );
    assert!(!is_authorized_qr_host("100.64.99.99", &policy));

    let peer = Some(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 55)));
    assert_eq!(
        resolve_canonical_qr_server_url(
            &state.config,
            &policy,
            peer,
            false,
            &HeaderMap::new(),
            Some("http://192.168.1.20:9090"),
        ),
        Ok("http://192.168.1.20:9090".to_string())
    );
    assert!(resolve_canonical_qr_server_url(
        &state.config,
        &policy,
        peer,
        false,
        &HeaderMap::new(),
        Some("http://192.168.1.30:9090"),
    )
    .is_err());

    // Live HTTP /api/v1/pair/qr rejects unrelated LAN/mDNS hosts with 400
    for spoof_host in ["198.51.100.20:9090", "not-this-server.local:9090"] {
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/pair/qr")
                    .method("POST")
                    .header(header::AUTHORIZATION, &admin_auth)
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::HOST, spoof_host)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "foreign host {spoof_host} must be rejected"
        );
    }

    let _ = std::fs::remove_dir_all(&tmp);
}

/// R1.4 §3.5 + §8 + §12: Exact leading-zero volume grammar parity (Rust <-> JSON Schema),
/// multi-client Michi Control concurrency, and discovery/capability parity.
#[tokio::test]
async fn test_r14_05_volume_leading_zero_grammar_parity_and_multiclient_control() {
    let playback_schema = compile_michi_link_schema("playback-control.schema.json");
    let tmp = std::env::temp_dir().join(format!("michi-r14-05-{}", Uuid::new_v4()));
    let (app, state, admin_auth, music_dir) = build_r14_app(&tmp, false).await;

    // Set baseline volume 42
    state.playback_engine.set_volume(42).await.unwrap();

    // Leading-zero volume strings MUST fail BOTH JSON Schema AND live Rust endpoint!
    for bad_vol in ["00", "01", "075", "0100", "000", "+50", "101", "50.5", ""] {
        let payload = json!({ "command": "set_volume", "value": bad_vol });
        assert!(
            !playback_schema.is_valid(&payload),
            "JSON Schema must reject leading-zero/invalid volume '{bad_vol}'"
        );
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/playback/control")
                    .method("POST")
                    .header(header::AUTHORIZATION, &admin_auth)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(payload.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "Rust endpoint must reject volume '{bad_vol}'"
        );
        assert_eq!(
            state.playback_engine.snapshot().await.unwrap().volume,
            42,
            "Engine volume must remain unchanged after invalid '{bad_vol}'"
        );
    }

    // Valid canonical volumes MUST pass BOTH JSON Schema AND live Rust endpoint
    for (val_json, expected) in [
        (json!("0"), 0u8),
        (json!("1"), 1u8),
        (json!("75"), 75u8),
        (json!("100"), 100u8),
        (json!(0), 0u8),
        (json!(55), 55u8),
        (json!(100), 100u8),
    ] {
        let payload = json!({ "command": "set_volume", "value": val_json });
        assert_matches_schema(&playback_schema, &payload, "valid set_volume");
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/playback/control")
                    .method("POST")
                    .header(header::AUTHORIZATION, &admin_auth)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(payload.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            state.playback_engine.snapshot().await.unwrap().volume,
            expected
        );
    }

    // Multi-client Michi Control concurrency (Client A & Client B)
    let t1 = seed_real_wav_track(&state.db, &music_dir, "mc_a").await;
    let t2 = seed_real_wav_track(&state.db, &music_dir, "mc_b").await;
    let (sk_a, pk_a, mid_a, mem_a, rpk_a) =
        enroll_mobile_member(&app, &state, &admin_auth, "Mobile A", 21).await;
    let (sk_b, pk_b, mid_b, mem_b, rpk_b) =
        enroll_mobile_member(&app, &state, &admin_auth, "Mobile B", 22).await;
    let tok_a =
        authenticate_home_member(&app, &state, &sk_a, &pk_a, &mid_a, &mem_a, &rpk_a, 21).await;
    let tok_b =
        authenticate_home_member(&app, &state, &sk_b, &pk_b, &mid_b, &mem_b, &rpk_b, 22).await;

    let (res_a, res_b, res_vol_a, res_vol_b) = tokio::join!(
        app.clone().oneshot(with_tls(with_client_ip(
            Request::builder()
                .uri("/api/v1/queue/items")
                .method("POST")
                .header(header::AUTHORIZATION, format!("Bearer {tok_a}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!({"track_ids": [t1]}).to_string()))
                .unwrap(),
            31,
        ))),
        app.clone().oneshot(with_tls(with_client_ip(
            Request::builder()
                .uri("/api/v1/queue/items")
                .method("POST")
                .header(header::AUTHORIZATION, format!("Bearer {tok_b}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!({"track_ids": [t2]}).to_string()))
                .unwrap(),
            32,
        ))),
        app.clone().oneshot(with_tls(with_client_ip(
            Request::builder()
                .uri("/api/v1/playback/control")
                .method("POST")
                .header(header::AUTHORIZATION, format!("Bearer {tok_a}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"command": "set_volume", "value": 33}).to_string()
                ))
                .unwrap(),
            33,
        ))),
        app.clone().oneshot(with_tls(with_client_ip(
            Request::builder()
                .uri("/api/v1/playback/control")
                .method("POST")
                .header(header::AUTHORIZATION, format!("Bearer {tok_b}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"command": "set_volume", "value": 66}).to_string()
                ))
                .unwrap(),
            34,
        ))),
    );
    assert_eq!(res_a.unwrap().status(), StatusCode::OK);
    assert_eq!(res_b.unwrap().status(), StatusCode::OK);
    assert_eq!(res_vol_a.unwrap().status(), StatusCode::OK);
    assert_eq!(res_vol_b.unwrap().status(), StatusCode::OK);

    let final_vol = state.playback_engine.snapshot().await.unwrap().volume;
    assert!(final_vol == 33 || final_vol == 66);

    // Discovery / Server Info / Capabilities identity parity
    let info = response_json(
        app.clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/server/info")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
    )
    .await;
    let caps = response_json(
        app.clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/capabilities")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
    )
    .await;
    let connect = michi_connect::MichiConnect::new(state.identity.clone(), 9090, None);
    let announce = connect.build_signed_announce(9090, "127.0.0.1").unwrap();

    assert_eq!(info["michi_id"].as_str(), announce.michi_id.as_deref());
    assert_eq!(info["public_key"].as_str(), announce.public_key.as_deref());
    assert_eq!(info["identity_scheme"], "ed25519-blake3-v1");
    assert_eq!(info["features"]["library"], true);
    assert_eq!(info["features"]["playback"], true);
    assert_eq!(info["features"]["aircat_original_v1"], true);
    assert_eq!(info["features"]["home_tls_spki_pinned"], true);
    let cap_features = caps["features"].as_array().unwrap();
    for required_feat in ["library", "playback", "stream"] {
        let feat = cap_features
            .iter()
            .find(|f| f["name"] == required_feat)
            .unwrap_or_else(|| panic!("capabilities missing feature {required_feat}"));
        assert_eq!(feat["enabled"], true);
    }

    let _ = std::fs::remove_dir_all(&tmp);
}
