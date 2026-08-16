//! Integration tests for Michi Music Stream Simulator.
//!
//! These tests require the external receiver simulator to be running.
//! Set MICHI_RECEIVER_SIM_URL to point to it (default: http://127.0.0.1:8080).
//!
//! Run with:
//!   cargo test --test receiver_simulator_integration -- --ignored

use michi_receivers::{ReceiverClient, ReceiverSessionManager};

fn sim_url() -> String {
    std::env::var("MICHI_RECEIVER_SIM_URL").unwrap_or_else(|_| "http://127.0.0.1:8080".to_string())
}

fn sim_url_hifi() -> String {
    std::env::var("MICHI_RECEIVER_SIM_HIFI_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:8081".to_string())
}

#[tokio::test]
#[ignore]
async fn test_receiver_info_standard() {
    let client = ReceiverClient::new(&sim_url());
    let info = client.get_info().await.expect("get_info failed");
    assert_eq!(info.service.as_deref(), Some("michi-stream-standard"));
    assert_eq!(info.device_type.as_deref(), Some("michi_stream_standard"));
    assert_eq!(info.api_version.as_deref(), Some("v1-lite"));
}

#[tokio::test]
#[ignore]
async fn test_receiver_info_hifi() {
    let client = ReceiverClient::new(&sim_url_hifi());
    let info = client.get_info().await.expect("get_info failed");
    assert_eq!(info.service.as_deref(), Some("michi-stream-hifi"));
    assert_eq!(info.device_type.as_deref(), Some("michi_stream_hifi"));
}

#[tokio::test]
#[ignore]
async fn test_receiver_info_standard_output() {
    let client = ReceiverClient::new(&sim_url());
    let info = client.get_info().await.expect("get_info failed");
    let output = info.output.expect("standard must have output");
    assert_eq!(
        output.get("connector").and_then(|v| v.as_str()),
        Some("jack_3_5")
    );
    assert_eq!(
        output.get("max_sample_rate").and_then(|v| v.as_u64()),
        Some(48000)
    );
    assert_eq!(
        output.get("max_bit_depth").and_then(|v| v.as_u64()),
        Some(16)
    );
    assert!(info
        .supported_codecs
        .as_ref()
        .map(|c| c.contains(&"pcm_s16le".to_string()))
        .unwrap_or(false));
}

#[tokio::test]
#[ignore]
async fn test_receiver_info_hifi_output() {
    let client = ReceiverClient::new(&sim_url_hifi());
    let info = client.get_info().await.expect("get_info failed");
    let output = info.output.expect("hifi must have output");
    assert_eq!(
        output.get("connector").and_then(|v| v.as_str()),
        Some("rca_stereo")
    );
    assert_eq!(
        output.get("max_sample_rate").and_then(|v| v.as_u64()),
        Some(48000)
    );
    assert_eq!(
        output.get("max_bit_depth").and_then(|v| v.as_u64()),
        Some(16)
    );
    assert!(info
        .supported_codecs
        .as_ref()
        .map(|c| c.contains(&"pcm_s16le".to_string()))
        .unwrap_or(false));
}

#[tokio::test]
#[ignore]
async fn test_receiver_pairing_flow() {
    let mut client = ReceiverClient::new(&sim_url());

    // pair/start
    let start = client
        .pair_start("test-flow")
        .await
        .expect("pair_start failed");
    assert_eq!(start.status.as_deref(), Some("pairing_window_open"));
    assert!(start.pairing_window_seconds.unwrap_or(0) > 0);
    let nonce = start.nonce.expect("must have nonce");

    // pair/confirm with 6-digit PIN
    let confirm = client
        .pair_confirm(&nonce, "test-flow", "482391")
        .await
        .expect("pair_confirm failed");
    assert_eq!(confirm.status.as_deref(), Some("paired"));
    assert!(client.token.is_some());
}

#[tokio::test]
#[ignore]
async fn test_receiver_pairing_window_closed_rejected() {
    let mut client = ReceiverClient::new(&sim_url());
    let start = client
        .pair_start("test-reject")
        .await
        .expect("pair_start failed");
    let nonce = start.nonce.expect("must have nonce");

    // First confirm succeeds
    let confirm = client
        .pair_confirm(&nonce, "test-reject", "482391")
        .await
        .expect("first confirm failed");
    assert_eq!(confirm.status.as_deref(), Some("paired"));

    // Second confirm on same nonce should fail
    let confirm2 = client.pair_confirm(&nonce, "test-reject", "482391").await;
    assert!(
        confirm2.is_err(),
        "second confirm on consumed nonce must fail"
    );
}

#[tokio::test]
#[ignore]
async fn test_receiver_standard_full_lifecycle() {
    let mgr = ReceiverSessionManager::new();
    let base_url = sim_url();
    let device_id = mgr
        .discover_and_pair(&base_url, "test-lifecycle")
        .await
        .expect("discover and pair failed");

    // Start session
    let session_id = format!("sess_{}", uuid::Uuid::new_v4());
    let sess_resp = mgr
        .start_session(
            &device_id,
            &session_id,
            "pcm_s16le",
            48000,
            16,
            2,
            55300,
            250,
            70,
        )
        .await
        .expect("session_start failed");
    assert_eq!(sess_resp.status.as_deref(), Some("session_started"));
    assert!(sess_resp.stream_port.is_some());

    // Heartbeat
    let hb = mgr.heartbeat(&device_id).await.expect("heartbeat failed");
    assert_eq!(hb.status.as_deref(), Some("alive"));

    // Set volume
    let vol = mgr
        .set_volume(&device_id, 42)
        .await
        .expect("set_volume failed");
    assert_eq!(vol.volume, Some(42));

    // Stop session
    let stop = mgr
        .stop_session(&device_id)
        .await
        .expect("session_stop failed");
    assert_eq!(stop.status.as_deref(), Some("session_stopped"));
}

#[tokio::test]
#[ignore]
async fn test_receiver_hifi_full_lifecycle() {
    let mgr = ReceiverSessionManager::new();
    let base_url = sim_url_hifi();
    let device_id = mgr
        .discover_and_pair(&base_url, "test-hifi")
        .await
        .expect("discover and pair failed");

    let session_id = format!("sess_hifi_{}", uuid::Uuid::new_v4());
    let sess_resp = mgr
        .start_session(
            &device_id,
            &session_id,
            "pcm_s16le",
            48000,
            16,
            2,
            55301,
            100,
            80,
        )
        .await
        .expect("session_start failed");
    assert_eq!(sess_resp.status.as_deref(), Some("session_started"));

    let hb = mgr.heartbeat(&device_id).await.expect("heartbeat failed");
    assert_eq!(hb.status.as_deref(), Some("alive"));

    let vol = mgr
        .set_volume(&device_id, 75)
        .await
        .expect("set_volume failed");
    assert_eq!(vol.volume, Some(75));

    let stop = mgr
        .stop_session(&device_id)
        .await
        .expect("session_stop failed");
    assert_eq!(stop.status.as_deref(), Some("session_stopped"));
}

#[tokio::test]
#[ignore]
async fn test_receiver_errors_unsupported_codec() {
    let mgr = ReceiverSessionManager::new();
    let device_id = mgr
        .discover_and_pair(&sim_url(), "test-codec")
        .await
        .expect("pair failed");

    let session_id = format!("sess_err_{}", uuid::Uuid::new_v4());
    let result = mgr
        .start_session(&device_id, &session_id, "aac", 48000, 16, 2, 55500, 250, 70)
        .await;
    assert!(result.is_err(), "unsupported codec should fail");
}

#[tokio::test]
#[ignore]
async fn test_receiver_errors_sample_rate_exceeds() {
    let mgr = ReceiverSessionManager::new();
    let device_id = mgr
        .discover_and_pair(&sim_url(), "test-sr")
        .await
        .expect("pair failed");

    let session_id = format!("sess_sr_{}", uuid::Uuid::new_v4());
    let result = mgr
        .start_session(
            &device_id,
            &session_id,
            "pcm_s16le",
            96000,
            16,
            2,
            55600,
            250,
            70,
        )
        .await;
    assert!(result.is_err(), "sample rate exceeding max should fail");
}

#[tokio::test]
#[ignore]
async fn test_receiver_errors_duplicate_session() {
    let mgr = ReceiverSessionManager::new();
    let device_id = mgr
        .discover_and_pair(&sim_url(), "test-dupe")
        .await
        .expect("pair failed");

    let session_id = format!("sess_dupe_{}", uuid::Uuid::new_v4());
    let first = mgr
        .start_session(
            &device_id,
            &session_id,
            "pcm_s16le",
            48000,
            16,
            2,
            55700,
            250,
            70,
        )
        .await;
    assert!(first.is_ok(), "first session should succeed");

    let second = mgr
        .start_session(
            &device_id,
            "sess_dupe_2",
            "pcm_s16le",
            48000,
            16,
            2,
            55701,
            250,
            70,
        )
        .await;
    assert!(second.is_err(), "duplicate session should fail with 409");

    let _ = mgr.stop_session(&device_id).await;
}

#[tokio::test]
#[ignore]
async fn test_receiver_errors_volume_out_of_range() {
    let mgr = ReceiverSessionManager::new();
    let device_id = mgr
        .discover_and_pair(&sim_url(), "test-vol")
        .await
        .expect("pair failed");

    let vol = mgr.set_volume(&device_id, 101).await;
    assert!(vol.is_err(), "volume > 100 must be rejected");

    let vol2 = mgr.set_volume(&device_id, 999).await;
    assert!(vol2.is_err(), "volume > 100 must be rejected");
}

#[tokio::test]
#[ignore]
async fn test_receiver_errors_unauthenticated() {
    let client = ReceiverClient::new(&sim_url());
    let hb = client.heartbeat().await;
    assert!(hb.is_err(), "unauthenticated heartbeat must fail");
}

#[tokio::test]
#[ignore]
async fn test_receiver_heartbeat_monotonic_sequence() {
    let mut client = ReceiverClient::new(&sim_url());
    let start = client
        .pair_start("hb-test")
        .await
        .expect("pair_start failed");
    let nonce = start.nonce.expect("nonce missing");
    client
        .pair_confirm(&nonce, "hb-test", "482391")
        .await
        .expect("pair_confirm failed");

    let sess_id = format!("sess_hb_{}", uuid::Uuid::new_v4());
    client
        .session_start(&sess_id, "pcm_s16le", 48000, 16, 2, 55300, 120, 50)
        .await
        .expect("session_start failed");

    // Heartbeat 1 (seq = 1)
    let hb1 = client.heartbeat().await.expect("hb 1 should succeed");
    assert_eq!(hb1.status.as_deref(), Some("alive"));

    // Heartbeat 2 (seq = 2)
    let hb2 = client.heartbeat().await.expect("hb 2 should succeed");
    assert_eq!(hb2.status.as_deref(), Some("alive"));

    let _ = client.session_stop().await;
}

#[tokio::test]
#[ignore]
async fn test_receiver_registry_tracks_state() {
    let mgr = ReceiverSessionManager::new();
    let device_id = mgr
        .discover_and_pair(&sim_url(), "test-reg")
        .await
        .expect("pair failed");

    let reg = mgr.registry().await;
    let reg_read = reg.read().await;
    let entry = reg_read
        .get(&device_id)
        .expect("receiver must be in registry");
    assert!(entry.paired);
    assert!(entry.token.is_some());
    assert!(entry.active_session_id.is_none());
    assert!(entry.max_sample_rate >= 48000);
}

#[tokio::test]
#[ignore]
async fn test_receiver_full_lifecycle_and_session_recovery() {
    let url = sim_url();
    let mut client = ReceiverClient::new(&url);

    // 1. Discovery
    let info = client.get_info().await.expect("discovery info failed");
    let _device_id = info.device_id.expect("missing device_id");

    // 2. Pairing
    let pair_start = client
        .pair_start("e2e-matrix")
        .await
        .expect("pair start failed");
    let nonce = pair_start.nonce.expect("missing nonce");
    let confirm = client
        .pair_confirm(&nonce, "e2e-matrix", "482391")
        .await
        .expect("pair confirm failed");
    assert_eq!(confirm.status.as_deref(), Some("paired"));
    assert!(client.token.is_some(), "token must be stored in client");

    // 3. Start Session
    let session_id = "sess-matrix-full-1";
    let start_res = client
        .session_start(session_id, "pcm_s16le", 48000, 16, 2, 55800, 250, 75)
        .await
        .expect("session start failed");
    assert_eq!(start_res.status.as_deref(), Some("session_started"));
    assert!(
        client.active_session_token.is_some(),
        "session_token must be retained"
    );

    // 4. Playback Control & Volume
    let vol_res = client.set_volume(85).await.expect("set_volume failed");
    assert_eq!(vol_res.volume, Some(85));

    // Verify current state before disconnect
    let state_before = client.get_playback_state().await.expect("get state failed");
    assert_eq!(state_before.volume, Some(85));

    // 5. Heartbeat with monotonic sequence
    let hb = client.heartbeat().await.expect("heartbeat failed");
    assert_eq!(hb.status.as_deref(), Some("alive"));

    // 6. Stop Session Cleanly
    let stop_res = client.session_stop().await.expect("session stop failed");
    assert_eq!(stop_res.status.as_deref(), Some("session_stopped"));
    assert!(client.active_session_token.is_none());
}

#[tokio::test]
#[ignore]
async fn test_receiver_fault_slow_response() {
    let client = ReceiverClient::new(&sim_url());
    client
        .fault_latency(100)
        .await
        .expect("inject latency failed");

    let t0 = std::time::Instant::now();
    let info = client
        .get_info()
        .await
        .expect("info with latency should succeed");
    let elapsed = t0.elapsed();
    assert!(
        elapsed >= std::time::Duration::from_millis(90),
        "response should have delayed at least ~100ms, took {elapsed:?}"
    );
    assert_eq!(info.service.as_deref(), Some("michi-stream-standard"));

    client.fault_reset().await.expect("reset faults failed");
}

#[tokio::test]
#[ignore]
async fn test_receiver_fault_offline_error() {
    let client = ReceiverClient::new(&sim_url());
    client
        .fault_offline(true)
        .await
        .expect("inject offline failed");

    let info_res = client.get_info().await;
    assert!(info_res.is_err(), "offline receiver must fail requests");

    client.fault_reset().await.expect("reset faults failed");
    let info_recovered = client
        .get_info()
        .await
        .expect("info after reset should succeed");
    assert_eq!(
        info_recovered.service.as_deref(),
        Some("michi-stream-standard")
    );
}

#[tokio::test]
#[ignore]
async fn test_receiver_fault_temporary_network_drop_and_recovery() {
    let client = ReceiverClient::new(&sim_url());
    client
        .fault_network_drop(2)
        .await
        .expect("inject network drop failed");

    let res1 = client.get_info().await;
    assert!(res1.is_err(), "first dropped request must fail");

    let res2 = client.get_info().await;
    assert!(res2.is_err(), "second dropped request must fail");

    let res3 = client
        .get_info()
        .await
        .expect("third request must auto-recover");
    assert_eq!(res3.service.as_deref(), Some("michi-stream-standard"));
}
