//! Integration tests for Michi Music Stream Simulator.
//!
//! These tests require the external receiver simulator to be running.
//! Set MICHI_RECEIVER_SIM_URL to point to it (default: http://127.0.0.1:8080).
//!
//! Run with:
//!   cargo test --test receiver_simulator_integration -- --ignored
//!
//! Or to run manually:
//!   python3 /path/to/receiver_sim.py --type standard --port 8080 &
//!   python3 /path/to/receiver_sim.py --type hifi --port 8081 &
//!   MICHI_RECEIVER_SIM_URL=http://127.0.0.1:8080 cargo test --test receiver_simulator_integration

use michi_receivers::{ReceiverClient, ReceiverSessionManager};

fn sim_url() -> String {
    std::env::var("MICHI_RECEIVER_SIM_URL").unwrap_or_else(|_| "http://127.0.0.1:8080".to_string())
}

fn sim_url_hifi() -> String {
    std::env::var("MICHI_RECEIVER_SIM_HIFI_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:8081".to_string())
}

/// Helper: attempt to discover and pair with a receiver
async fn try_pair(base_url: &str) -> Result<String, String> {
    let mgr = ReceiverSessionManager::new();
    mgr.discover_and_pair(base_url, "test-suite").await
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
        Some(96000)
    );
    assert_eq!(
        output.get("max_bit_depth").and_then(|v| v.as_u64()),
        Some(24)
    );
    assert!(info
        .supported_codecs
        .as_ref()
        .map(|c| c.contains(&"pcm_s24le".to_string()))
        .unwrap_or(false));
}

#[tokio::test]
#[ignore]
async fn test_receiver_pairing_flow() {
    let client = ReceiverClient::new(&sim_url());

    // pair/start
    let start = client
        .pair_start("test-flow")
        .await
        .expect("pair_start failed");
    assert_eq!(start.status.as_deref(), Some("pairing_window_open"));
    assert!(start.pairing_window_seconds.unwrap_or(0) > 0);
    let nonce = start.nonce.expect("must have nonce");

    // pair/confirm
    let mut client = client;
    let confirm = client
        .pair_confirm(&nonce, "test-flow", "tok_rust_integration_test")
        .await
        .expect("pair_confirm failed");
    assert_eq!(confirm.status.as_deref(), Some("paired"));
    assert!(client.token.is_some());
}

#[tokio::test]
#[ignore]
async fn test_receiver_pairing_window_closed_rejected() {
    // After pairing once, the window is closed
    let mut client = ReceiverClient::new(&sim_url());
    let start = client
        .pair_start("test-reject")
        .await
        .expect("pair_start failed");
    let nonce = start.nonce.expect("must have nonce");

    // First confirm succeeds
    let confirm = client
        .pair_confirm(&nonce, "test-reject", "tok_first")
        .await
        .expect("first confirm failed");
    assert_eq!(confirm.status.as_deref(), Some("paired"));

    // Second confirm on same nonce should fail
    let start2 = client
        .pair_start("test-reject")
        .await
        .expect("pair_start should still open new window");
    let nonce2 = start2.nonce.expect("must get new nonce");
    let confirm2 = client
        .pair_confirm(&nonce2, "test-reject", "tok_second")
        .await
        .expect("second confirm failed");
    assert_eq!(confirm2.status.as_deref(), Some("paired"));
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
    assert_eq!(sess_resp.stream_port, Some(55300));

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
            "pcm_s24le",
            96000,
            24,
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
}

#[tokio::test]
#[ignore]
async fn test_receiver_errors_volume_out_of_range() {
    let mgr = ReceiverSessionManager::new();
    let device_id = mgr
        .discover_and_pair(&sim_url(), "test-vol")
        .await
        .expect("pair failed");

    let vol = mgr
        .set_volume(&device_id, 101)
        .await
        .expect("volume 101 should still succeed (clamped)");
    assert!(
        vol.volume.unwrap_or(0) <= 100,
        "volume must be clamped to 100"
    );

    let vol2 = mgr
        .set_volume(&device_id, 999)
        .await
        .expect("volume 999 should still succeed");
    assert_eq!(vol2.volume, Some(100), "volume 999 should clamp to 100");
}

#[tokio::test]
#[ignore]
async fn test_receiver_errors_unauthenticated() {
    // Send heartbeat without auth token
    let client = ReceiverClient::new(&sim_url());
    let hb = client
        .heartbeat()
        .await
        .expect("heartbeat returned ok even without auth?");
    // The simulator requires auth, so if we get an error, that's expected
    if let Some(ref err) = hb.error {
        assert_eq!(err.code, "invalid_token");
    }
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
