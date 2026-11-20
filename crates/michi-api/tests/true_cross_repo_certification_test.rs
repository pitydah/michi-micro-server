use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use michi_api::create_router;
use michi_config::Config;
use serde::Serialize;
use serde_json::Value;
use tower::ServiceExt;
use uuid::Uuid;

struct SimulatorGuard {
    child: Child,
    pub base_url: String,
    stderr_log_path: PathBuf,
}

impl Drop for SimulatorGuard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.stderr_log_path);
    }
}

#[derive(Debug, Serialize)]
struct CrossRepoCertificationReceipt {
    test: &'static str,
    simulator_script: String,
    python_executable: String,
    simulator_started: bool,
    dependencies_verified: bool,
    discovery_verified: bool,
    canonical_identity_verified: bool,
    handshake_completed: bool,
    authentication_verified: bool,
    home_membership_verified: bool,
    receiver_registered: bool,
    rtp_packets_sent: u64,
    rtp_packets_received: u64,
    rtp_packets_rejected: u64,
    revocation_verified: bool,
    post_revocation_rejected: bool,
    result: &'static str,
}

fn repo_root_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates dir")
        .parent()
        .expect("repo root")
        .to_path_buf()
}

fn resolve_simulator_python(repo_root: &Path) -> PathBuf {
    if let Ok(explicit) = std::env::var("MICHI_SIMULATOR_PYTHON") {
        if !explicit.trim().is_empty() {
            return PathBuf::from(explicit);
        }
    }
    let venv_python = repo_root.join(".venv-sim/bin/python3");
    if venv_python.exists() {
        return venv_python;
    }
    PathBuf::from("python3")
}

fn verify_python_dependencies(python_bin: &Path) -> Result<(), String> {
    let output = Command::new(python_bin)
        .arg("-c")
        .arg("import blake3, flask, cryptography, jsonschema, referencing")
        .output()
        .map_err(|e| {
            format!(
                "failed to execute Python interpreter '{}': {e}",
                python_bin.display()
            )
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "missing required Python simulator dependencies under '{}' (install via `pip install -r vendor/michi-music-stream/requirements.txt`): {}",
            python_bin.display(),
            stderr.trim()
        ));
    }
    Ok(())
}

fn pick_free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind free port");
    let port = listener.local_addr().expect("local addr").port();
    drop(listener);
    port
}

async fn spawn_stream_simulator_with(
    python_bin: &Path,
    sim_script: &Path,
    startup_timeout: Duration,
) -> Result<SimulatorGuard, String> {
    if !sim_script.exists() {
        return Err(format!(
            "required Stream simulator script not found at '{}' (ensure submodules are initialized)",
            sim_script.display()
        ));
    }

    verify_python_dependencies(python_bin)?;

    let port = pick_free_port();
    let base_url = format!("http://127.0.0.1:{port}");
    let stderr_log_path =
        std::env::temp_dir().join(format!("michi-sim-stderr-{}.log", Uuid::new_v4()));
    let stderr_file = std::fs::File::create(&stderr_log_path)
        .map_err(|e| format!("failed to create simulator stderr log: {e}"))?;

    let child = Command::new(python_bin)
        .arg(sim_script)
        .arg("--type")
        .arg("standard")
        .arg("--port")
        .arg(port.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::from(stderr_file))
        .spawn()
        .map_err(|e| {
            let _ = std::fs::remove_file(&stderr_log_path);
            format!(
                "failed to spawn Stream simulator '{}' with '{}': {e}",
                sim_script.display(),
                python_bin.display()
            )
        })?;

    let mut guard = SimulatorGuard {
        child,
        base_url: base_url.clone(),
        stderr_log_path,
    };

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .map_err(|e| format!("failed to build HTTP client: {e}"))?;
    let info_url = format!("{base_url}/api/v1/server/info");
    let deadline = Instant::now() + startup_timeout;

    while Instant::now() < deadline {
        if let Ok(Some(exit_status)) = guard.child.try_wait() {
            let stderr = std::fs::read_to_string(&guard.stderr_log_path).unwrap_or_default();
            return Err(format!(
                "Stream simulator exited prematurely with status {exit_status} before becoming ready on port {port}; stderr: {}",
                stderr.trim()
            ));
        }

        if let Ok(res) = client.get(&info_url).send().await {
            if res.status().is_success() {
                return Ok(guard);
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let stderr = std::fs::read_to_string(&guard.stderr_log_path).unwrap_or_default();
    Err(format!(
        "Stream simulator did not become ready on port {port} within {:?}; stderr: {}",
        startup_timeout,
        stderr.trim()
    ))
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

fn verify_canonical_blake3_identity(michi_id: &str, public_key: &str) -> bool {
    let Ok(pk_bytes) = URL_SAFE_NO_PAD.decode(public_key) else {
        return false;
    };
    if pk_bytes.len() != 32 {
        return false;
    }
    let derived = URL_SAFE_NO_PAD.encode(blake3::hash(&pk_bytes).as_bytes());
    derived == michi_id
}

#[tokio::test]
async fn test_simulator_launcher_fails_explicitly_on_missing_script() {
    let repo_root = repo_root_dir();
    let python_bin = resolve_simulator_python(&repo_root);
    let missing_script = repo_root.join("vendor/michi-music-stream/simulator/does_not_exist.py");
    let err = spawn_stream_simulator_with(&python_bin, &missing_script, Duration::from_secs(2))
        .await
        .err()
        .expect("missing simulator script MUST fail explicitly, never silently pass");
    assert!(
        err.contains("not found"),
        "expected missing script diagnostic, got: {err}"
    );
}

#[tokio::test]
async fn test_simulator_launcher_fails_explicitly_on_missing_python_runtime() {
    let repo_root = repo_root_dir();
    let sim_script = repo_root.join("vendor/michi-music-stream/simulator/receiver_sim.py");
    let bogus_python = PathBuf::from("/nonexistent/michi-missing-python3");
    let err = spawn_stream_simulator_with(&bogus_python, &sim_script, Duration::from_secs(2))
        .await
        .err()
        .expect("missing Python runtime MUST fail explicitly, never silently pass");
    assert!(
        err.contains("failed to execute Python interpreter"),
        "expected python runtime diagnostic, got: {err}"
    );
}

#[tokio::test]
async fn test_true_cross_repo_certification_e2e() {
    let repo_root = repo_root_dir();
    let python_bin = resolve_simulator_python(&repo_root);
    let sim_script = repo_root.join("vendor/michi-music-stream/simulator/receiver_sim.py");

    // 1. Mandatory live Stream simulator launch — missing dependencies or startup error MUST FAIL.
    let sim = spawn_stream_simulator_with(&python_bin, &sim_script, Duration::from_secs(10))
        .await
        .expect(
            "TRUE CROSS-REPO CERTIFICATION FAILED: real Stream simulator is mandatory and cannot be skipped",
        );

    let http_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("build HTTP client");

    // 1b. Verify live discovery + canonical Ed25519/BLAKE3 identity on simulator /api/v1/server/info
    let sim_info: Value = http_client
        .get(format!("{}/api/v1/server/info", sim.base_url))
        .send()
        .await
        .expect("simulator /api/v1/server/info request")
        .error_for_status()
        .expect("simulator /api/v1/server/info 200 OK")
        .json()
        .await
        .expect("simulator server info JSON");
    let sim_michi_id = sim_info["michi_id"]
        .as_str()
        .expect("simulator must publish canonical michi_id");
    let sim_pub_key = sim_info["public_key"]
        .as_str()
        .expect("simulator must publish canonical public_key");
    let discovery_verified = !sim_michi_id.is_empty() && !sim_pub_key.is_empty();
    assert!(
        discovery_verified,
        "simulator discovery payload missing identity fields"
    );
    let sim_identity_ok = verify_canonical_blake3_identity(sim_michi_id, sim_pub_key);
    assert!(
        sim_identity_ok,
        "simulator michi_id does not match BLAKE3(public_key)"
    );

    // 2. Set up Micro Server configured with the exact contract Home Root Authority
    let (_root_auth, home_id) = root_authority_for_vector_home();
    let tmp = std::env::temp_dir().join(format!("michi-cert-test-{}", Uuid::new_v4()));
    std::fs::create_dir_all(tmp.join("config")).unwrap();

    // Persist root key so AppState loads it
    let home_key_path = tmp.join("config").join("home_root_authority.key");
    let seed_bytes =
        hex::decode("c66a870d78788b4028cf21153c6a2b38efb5b0c92bcc4cfae58cc87e1f1f0377").unwrap();
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
        michi_identity::IdentityManager::generate(
            &config.config_path,
            "Certification Server",
            "pwd",
        )
        .unwrap(),
    );

    let admin_id = Uuid::new_v4();
    michi_db::create_user(&pool, &admin_id, "admin-cert", "pass", true)
        .await
        .unwrap();
    let state =
        michi_api::AppState::new_with_identity(config, pool.clone(), Some(admin_id), identity);
    let token = state.auth_sessions.create_session(admin_id).await.unwrap();
    let auth_header = format!("Bearer {token}");
    let app = create_router(state.clone());

    // Verify canonical identity on Micro Server /api/v1/server/info too
    let srv_info_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/server/info")
                .method("GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(srv_info_resp.status(), StatusCode::OK);
    let srv_info: Value = serde_json::from_slice(
        &axum::body::to_bytes(srv_info_resp.into_body(), 1024 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();
    let srv_identity_ok = verify_canonical_blake3_identity(
        srv_info["michi_id"].as_str().unwrap_or_default(),
        srv_info["public_key"].as_str().unwrap_or_default(),
    );
    assert!(
        srv_identity_ok,
        "Micro Server michi_id must match BLAKE3(public_key)"
    );
    let canonical_identity_verified = sim_identity_ok && srv_identity_ok;

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
    let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let auth_res: Value = serde_json::from_slice(&bytes).unwrap();

    assert_eq!(auth_res["status"], "authenticated");
    assert_eq!(auth_res["authenticated"], true);
    let handshake_completed = auth_res["status"] == "authenticated";
    let authentication_verified = auth_res["authenticated"] == true;
    let rec_id = auth_res["receiver_id"].as_str().unwrap().to_string();
    let stream_michi_id = auth_res["michi_id"].as_str().unwrap().to_string();
    assert_eq!(stream_michi_id, sim_michi_id);

    // 5. Verify the receiver is now registered as Authenticated Home Member in both receiver detail & Home roster
    let get_rec_req = Request::builder()
        .uri(format!("/api/v1/receivers/{rec_id}"))
        .method("GET")
        .header("Authorization", &auth_header)
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(get_rec_req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let rec_data: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(rec_data["authenticated"], true);
    assert_eq!(rec_data["michi_home_id"], home_id);
    assert_eq!(rec_data["revoked"], false);
    let receiver_registered = rec_data["receiver_id"] == rec_id;

    let roster_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/home/roster")
                .method("GET")
                .header("Authorization", &auth_header)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(roster_resp.status(), StatusCode::OK);
    let roster_json: Value = serde_json::from_slice(
        &axum::body::to_bytes(roster_resp.into_body(), 1024 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();
    let home_membership_verified = roster_json["devices"]
        .as_array()
        .expect("roster devices array")
        .iter()
        .any(|d| {
            d["device_michi_id"] == stream_michi_id
                && d["authenticated"] == true
                && d["revoked"] == false
        });
    assert!(
        home_membership_verified,
        "authenticated receiver must appear in Home roster"
    );

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
    let rtp_packets_sent: u64 = 10;
    let pcm_frame = vec![0u8; 1920];
    for _ in 0..rtp_packets_sent {
        state
            .receiver_manager
            .write_pcm(&rec_id, &pcm_frame)
            .await
            .expect("send rtp frame");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    // 8. Verify simulator received real RTP packets with 0 rejected (observed directly from simulator)
    let device_token = {
        let reg = state.receiver_manager.registry().await;
        let r = reg.read().await;
        r.get(&rec_id).unwrap().token.clone().expect("device token")
    };

    let session_url = format!("{}/api/v1/receiver-lite/session", sim.base_url);
    let mut observed_received = 0u64;
    let mut observed_rejected = 0u64;
    for _ in 0..40 {
        tokio::time::sleep(Duration::from_millis(50)).await;
        if let Ok(res) = http_client
            .get(&session_url)
            .header("Authorization", format!("Bearer {device_token}"))
            .send()
            .await
        {
            if let Ok(m) = res.json::<Value>().await {
                observed_received = m["packets_received"].as_u64().unwrap_or(0);
                observed_rejected = m["packets_rejected"].as_u64().unwrap_or(0);
                if observed_received >= rtp_packets_sent {
                    break;
                }
            }
        }
    }
    assert!(
        observed_received >= rtp_packets_sent,
        "Simulator must have received at least {rtp_packets_sent} valid RTP packets; observed={observed_received}, rejected={observed_rejected}"
    );
    assert_eq!(
        observed_rejected, 0,
        "Simulator must have zero rejected RTP packets; observed={observed_rejected}"
    );

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
    let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let rev_res: Value = serde_json::from_slice(&bytes).unwrap();
    let revocation_verified = rev_res["status"] == "revoked";
    assert!(revocation_verified);

    // Verify roster reflects revocation immediately (revoked=true, online=false, authenticated=false)
    let roster_after_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/home/roster")
                .method("GET")
                .header("Authorization", &auth_header)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(roster_after_resp.status(), StatusCode::OK);
    let roster_after: Value = serde_json::from_slice(
        &axum::body::to_bytes(roster_after_resp.into_body(), 1024 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();
    let revoked_entry = roster_after["devices"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["device_michi_id"] == stream_michi_id)
        .expect("revoked receiver entry in roster");
    assert_eq!(revoked_entry["revoked"], true);
    assert_eq!(revoked_entry["online"], false);
    assert_eq!(revoked_entry["authenticated"], false);

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
    let post_revocation_rejected = revoked_session_res.is_err();
    assert!(
        post_revocation_rejected,
        "Session attempt to revoked device must fail closed"
    );

    // 11. Emit real observed JSON certification receipt
    let receipt = CrossRepoCertificationReceipt {
        test: "true_cross_repo_certification_e2e",
        simulator_script: sim_script.display().to_string(),
        python_executable: python_bin.display().to_string(),
        simulator_started: true,
        dependencies_verified: true,
        discovery_verified,
        canonical_identity_verified,
        handshake_completed,
        authentication_verified,
        home_membership_verified,
        receiver_registered,
        rtp_packets_sent,
        rtp_packets_received: observed_received,
        rtp_packets_rejected: observed_rejected,
        revocation_verified,
        post_revocation_rejected,
        result: "PASS",
    };
    let receipt_json = serde_json::to_string_pretty(&receipt).expect("serialize receipt");
    let receipt_path = std::env::var("MICHI_CERT_RECEIPT_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            repo_root.join("target/release-evidence/true-cross-repo-certification.json")
        });
    if let Some(parent) = receipt_path.parent() {
        std::fs::create_dir_all(parent).expect("create receipt dir");
    }
    std::fs::write(&receipt_path, &receipt_json).expect("write certification receipt");

    println!("CROSS-REPO CERTIFICATION RECEIPT:\n{receipt_json}");
    let _ = std::fs::remove_dir_all(&tmp);
}
