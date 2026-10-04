use std::net::TcpListener;
use std::process::{Child, Command};
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use michi_api::create_router;
use michi_config::Config;
use serde_json::Value;
use tower::ServiceExt;
use uuid::Uuid;

struct SimulatorGuard {
    child: Child,
    pub base_url: String,
}

impl Drop for SimulatorGuard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn pick_free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind free port");
    let port = listener.local_addr().expect("local addr").port();
    drop(listener);
    port
}

async fn spawn_stream_simulator() -> SimulatorGuard {
    let port = pick_free_port();
    let base_url = format!("http://127.0.0.1:{port}");

    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let sim_script = repo_root.join("vendor/michi-music-stream/simulator/receiver_sim.py");

    assert!(
        sim_script.exists(),
        "simulator script must exist at {:?}",
        sim_script
    );

    let child = Command::new("python3")
        .arg(&sim_script)
        .arg("--type")
        .arg("standard")
        .arg("--port")
        .arg(port.to_string())
        .spawn()
        .expect("spawn stream simulator");

    let guard = SimulatorGuard {
        child,
        base_url: base_url.clone(),
    };

    // Poll until ready
    let client = reqwest::Client::new();
    let info_url = format!("{base_url}/api/v1/server/info");
    let mut ready = false;
    for _ in 0..50 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        if let Ok(res) = client.get(&info_url).send().await {
            if res.status().is_success() {
                ready = true;
                break;
            }
        }
    }

    assert!(ready, "Stream simulator failed to start on port {port}");
    guard
}

fn root_authority_for_vector_home() -> (michi_identity::HomeRootAuthority, String) {
    // Vector home root seed from michi-link / michi-music-stream:
    // root_seed = blake3(b"michi-link contract vectors v1" + b"home-root")
    let seed_hex = "c66a870d78788b4028cf21153c6a2b38efb5b0c92bcc4cfae58cc87e1f1f0377";
    let seed_bytes = hex::decode(seed_hex).expect("valid hex");
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&seed_bytes);
    let sk = ed25519_dalek::SigningKey::from_bytes(&arr);
    let root = michi_identity::HomeRootAuthority::from_signing_key(sk);
    let home_id = root.home_id();
    (root, home_id)
}

#[tokio::test]
async fn test_true_cross_repo_certification_e2e() {
    // 1. Launch real Michi Music Stream simulator
    let sim = spawn_stream_simulator().await;

    // 2. Set up Micro Server configured with the exact contract Home Root Authority
    let (_root_auth, home_id) = root_authority_for_vector_home();
    let tmp = std::env::temp_dir().join(format!("michi-cert-test-{}", Uuid::new_v4()));
    let _ = std::fs::create_dir_all(&tmp);

    // Persist root key so AppState loads it
    let home_key_path = tmp.join("config").join("home_root_authority.key");
    let _ = std::fs::create_dir_all(tmp.join("config"));
    let seed_bytes = hex::decode("c66a870d78788b4028cf21153c6a2b38efb5b0c92bcc4cfae58cc87e1f1f0377").unwrap();
    std::fs::write(&home_key_path, &seed_bytes).unwrap();

    let db_path = tmp.join("cert_test.db");
    let db_url = format!("sqlite://{}", db_path.display());
    let pool = michi_db::init_pool(&db_url).await.unwrap();

    let config = Config {
        port: 9090,
        music_paths: vec![std::env::temp_dir()],
        config_path: tmp.join("config"),
        cache_path: tmp.join("cache"),
        database_url: db_url,

        version: "1.0.0-rc.4",
        sync_peers: Vec::new(),
        sync_name: "michi-server-cert".to_string(),
        listenbrainz_token: None,
        lastfm_token: None,
        scrobble_enabled: false,
        auth_username: None,
        auth_password: None,
        auth_enabled: false,
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
        trust_proxy: false,
        trusted_proxies: vec!["127.0.0.1".parse().unwrap(), "::1".parse().unwrap()],
        deployment_platform: "unknown".into(),
    };

    let identity = Arc::new(
        michi_identity::IdentityManager::generate(&config.config_path, "Certification Server", "pwd")
            .unwrap(),
    );

    let admin_id = Uuid::new_v4();
    michi_db::create_user(&pool, &admin_id, "admin-cert", "pass", true)
        .await
        .unwrap();
    let state = michi_api::AppState::new_with_identity(config, pool.clone(), Some(admin_id), identity);
    let token = state.auth_sessions.create_session(admin_id).await.unwrap();
    let auth_header = format!("Bearer {token}");
    let app = create_router(state.clone());

    // 3. Verify Home Root Authority initialized on Micro Server
    assert_eq!(state.home_root_authority.home_id(), home_id);

    // 4. Test Auto-Authentication against live Stream Simulator via /api/v1/receivers/pair/start
    let pair_req = Request::builder()
        .uri("/api/v1/receivers/pair/start")
        .method("POST")
        .header("Authorization", &auth_header)
        .header("Content-Type", "application/json")
        .body(Body::from(
            serde_json::json!({
                "base_url": sim.base_url,
                "initiator_id": "michi-cert"
            })
            .to_string(),
        ))
        .unwrap();

    let resp = app.clone().oneshot(pair_req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
    let auth_res: Value = serde_json::from_slice(&bytes).unwrap();

    assert_eq!(auth_res["status"], "authenticated");
    assert_eq!(auth_res["authenticated"], true);
    let rec_id = auth_res["receiver_id"].as_str().unwrap().to_string();
    let stream_michi_id = auth_res["michi_id"].as_str().unwrap().to_string();

    // 5. Verify the receiver is now registered as Authenticated Home Member
    let get_rec_req = Request::builder()
        .uri(format!("/api/v1/receivers/{rec_id}"))
        .method("GET")
        .header("Authorization", &auth_header)
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(get_rec_req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
    let rec_data: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(rec_data["authenticated"], true);
    assert_eq!(rec_data["michi_home_id"], home_id);
    assert_eq!(rec_data["revoked"], false);

    // 6. Start RTP playback session with authority to the Stream simulator
    let session_start_res = state
        .receiver_manager
        .start_session(
            &rec_id,
            "session-cert-1",
            "pcm_s16le",
            48000,
            16,
            2,
            0,
            200,
            100,
        )
        .await
        .expect("start session with authority must succeed");
    assert!(!session_start_res.session_id.is_empty());
    assert_eq!(session_start_res.transport, "rtp_udp");
    assert!(session_start_res.stream_port > 0);

    // 7. Stream real PCM audio packets over UDP to the simulator port
    // Send 10 packets of 1920 bytes (PCM silence / audio frame)
    let pcm_frame = vec![0u8; 1920];
    for _ in 0..10 {
        state
            .receiver_manager
            .write_pcm(&rec_id, &pcm_frame)
            .await
            .expect("send rtp frame");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    // 8. Verify simulator received real RTP packets with 0 rejected
    let device_token = {
        let reg = state.receiver_manager.registry().await;
        let r = reg.read().await;
        r.get(&rec_id).unwrap().token.clone().expect("device token")
    };

    let http_client = reqwest::Client::new();
    let session_url = format!("{}/api/v1/receiver-lite/session", sim.base_url);
    let mut verified_packets = false;
    for _ in 0..30 {
        tokio::time::sleep(Duration::from_millis(50)).await;
        if let Ok(res) = http_client
            .get(&session_url)
            .header("Authorization", format!("Bearer {device_token}"))
            .send()
            .await
        {
            if let Ok(m) = res.json::<Value>().await {
                let pkts = m["packets_received"].as_u64().unwrap_or(0);
                let rejected = m["packets_rejected"].as_u64().unwrap_or(0);
                if pkts >= 10 {
                    assert_eq!(rejected, 0, "must have zero rejected RTP packets");
                    verified_packets = true;
                    break;
                }
            }
        }
    }
    assert!(verified_packets, "Simulator must have received 10 valid RTP packets");

    // 9. Stop receiver session cleanly
    state
        .receiver_manager
        .stop_session(&rec_id)
        .await
        .expect("stop session");

    // 10. Test Cryptographic Revocation
    let revoke_req = Request::builder()
        .uri("/api/v1/home/revoke")
        .method("POST")
        .header("Authorization", &auth_header)
        .header("Content-Type", "application/json")
        .body(Body::from(
            serde_json::json!({
                "device_michi_id": stream_michi_id,
                "reason": "Security certification revocation test"
            })
            .to_string(),
        ))
        .unwrap();

    let resp = app.clone().oneshot(revoke_req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
    let rev_res: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(rev_res["status"], "revoked");

    // Verify subsequent session attempt fails closed because device is revoked
    let revoked_session_res = state
        .receiver_manager
        .start_session(
            &rec_id,
            "session-cert-2",
            "pcm_s16le",
            48000,
            16,
            2,
            0,
            200,
            100,
        )
        .await;
    assert!(
        revoked_session_res.is_err(),
        "Session attempt to revoked device must fail closed"
    );

    println!("CROSS-REPO CERTIFICATION: ALL CHECKS PASSED!");
}
