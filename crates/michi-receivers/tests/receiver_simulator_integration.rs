//! Integration tests for Michi Music Stream Simulator.
//!
//! These tests require the external receiver simulator to be running.
//! Set MICHI_RECEIVER_SIM_URL to point to it (default: http://127.0.0.1:8080).
//!
//! Run with:
//!   cargo test --test receiver_simulator_integration -- --ignored

use michi_receivers::{ReceiverClient, ReceiverSessionManager};
use std::sync::Arc;

fn make_test_identity() -> Arc<michi_identity::IdentityManager> {
    let dir = std::env::temp_dir().join(format!("michi-rec-test-{}", uuid::Uuid::new_v4()));
    let _ = std::fs::create_dir_all(&dir);
    Arc::new(
        michi_identity::IdentityManager::generate(&dir, "Michi Micro Server", "")
            .expect("test identity generation"),
    )
}

fn make_test_client(url: &str) -> ReceiverClient {
    ReceiverClient::with_identity(url, make_test_identity())
}

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
    let mut client = make_test_client(&sim_url());

    // pair/start
    let start = client
        .pair_start("test-flow")
        .await
        .expect("pair_start failed");
    assert!(start.expires_at.is_some());
    let session_id = start.session_id.expect("must have session_id");

    // pair/confirm with 6-digit PIN
    let confirm = client
        .pair_confirm(&session_id, "test-flow", "482391")
        .await
        .expect("pair_confirm failed");
    assert_eq!(confirm.status.as_deref(), Some("paired"));
    assert!(client.token.is_some());
}

#[tokio::test]
#[ignore]
async fn test_receiver_pairing_window_closed_rejected() {
    let mut client = make_test_client(&sim_url());
    let start = client
        .pair_start("test-reject")
        .await
        .expect("pair_start failed");
    let session_id = start.session_id.expect("must have session_id");

    // First confirm succeeds
    let confirm = client
        .pair_confirm(&session_id, "test-reject", "482391")
        .await
        .expect("first confirm failed");
    assert_eq!(confirm.status.as_deref(), Some("paired"));

    // Second confirm on same session_id should fail
    let confirm2 = client
        .pair_confirm(&session_id, "test-reject", "482391")
        .await;
    assert!(
        confirm2.is_err(),
        "second confirm on consumed nonce must fail"
    );
}

#[tokio::test]
#[ignore]
async fn test_receiver_standard_full_lifecycle() {
    let mgr = ReceiverSessionManager::new_with_identity(make_test_identity());
    let base_url = sim_url();
    let device_id = mgr
        .discover_and_pair(&base_url, "test-lifecycle", "482391")
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
    assert!(!sess_resp.session_id.is_empty());
    assert!(sess_resp.stream_port >= 49152);
    assert_eq!(sess_resp.lease_seconds, 30);
    assert_eq!(sess_resp.sample_rate, 48000);
    assert_eq!(sess_resp.bit_depth, 16);
    assert_eq!(sess_resp.channels, 2);

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
    let mgr = ReceiverSessionManager::new_with_identity(make_test_identity());
    let base_url = sim_url_hifi();
    let device_id = mgr
        .discover_and_pair(&base_url, "test-hifi", "482391")
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
    assert!(!sess_resp.session_id.is_empty());
    assert!(sess_resp.stream_port >= 49152);
    assert_eq!(sess_resp.lease_seconds, 30);

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
    let mgr = ReceiverSessionManager::new_with_identity(make_test_identity());
    let device_id = mgr
        .discover_and_pair(&sim_url(), "test-codec", "482391")
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
    let mgr = ReceiverSessionManager::new_with_identity(make_test_identity());
    let device_id = mgr
        .discover_and_pair(&sim_url(), "test-sr", "482391")
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
    let mgr = ReceiverSessionManager::new_with_identity(make_test_identity());
    let device_id = mgr
        .discover_and_pair(&sim_url(), "test-dupe", "482391")
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
    let mgr = ReceiverSessionManager::new_with_identity(make_test_identity());
    let device_id = mgr
        .discover_and_pair(&sim_url(), "test-vol", "482391")
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
    let client = make_test_client(&sim_url());
    let hb = client.heartbeat().await;
    assert!(hb.is_err(), "unauthenticated heartbeat must fail");
}

#[tokio::test]
#[ignore]
async fn test_receiver_heartbeat_monotonic_sequence() {
    let mut client = make_test_client(&sim_url());
    let start = client
        .pair_start("hb-test")
        .await
        .expect("pair_start failed");
    let session_id = start.session_id.expect("session_id missing");
    client
        .pair_confirm(&session_id, "hb-test", "482391")
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
    let mgr = ReceiverSessionManager::new_with_identity(make_test_identity());
    let device_id = mgr
        .discover_and_pair(&sim_url(), "test-reg", "482391")
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
    let mut client = make_test_client(&url);

    // 1. Discovery
    let info = client.get_info().await.expect("discovery info failed");
    let _device_id = info.device_id.expect("missing device_id");

    // 2. Pairing
    let pair_start = client
        .pair_start("e2e-matrix")
        .await
        .expect("pair start failed");
    let session_id = pair_start.session_id.expect("missing session_id");
    let confirm = client
        .pair_confirm(&session_id, "e2e-matrix", "482391")
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
    assert!(!start_res.session_id.is_empty());
    assert_eq!(start_res.lease_seconds, 30);
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

#[tokio::test]
#[ignore]
async fn test_receiver_pairing_wrong_pin_rejected() {
    let mut client = make_test_client(&sim_url());
    let start = client
        .pair_start("test-wrong-pin")
        .await
        .expect("pair_start failed");
    let session_id = start.session_id.expect("session_id missing");

    let confirm = client
        .pair_confirm(&session_id, "test-wrong-pin", "000000")
        .await;
    assert!(confirm.is_err(), "wrong PIN must be rejected with 401");
}

#[tokio::test]
#[ignore]
async fn test_receiver_heartbeat_replay_rejected() {
    let mut client = make_test_client(&sim_url());
    let start = client
        .pair_start("hb-replay")
        .await
        .expect("pair_start failed");
    let session_id = start.session_id.expect("session_id missing");
    client
        .pair_confirm(&session_id, "hb-replay", "482391")
        .await
        .expect("pair_confirm failed");

    let sess_id = format!("sess_hb_{}", uuid::Uuid::new_v4());
    client
        .session_start(&sess_id, "pcm_s16le", 48000, 16, 2, 55300, 120, 50)
        .await
        .expect("session_start failed");

    // First heartbeat succeeds
    let hb1 = client.heartbeat().await.expect("hb 1 should succeed");
    assert_eq!(hb1.status.as_deref(), Some("alive"));

    // Reset sequence counter to simulate replayed/stale sequence
    client
        .heartbeat_sequence
        .store(0, std::sync::atomic::Ordering::SeqCst);
    let hb_replayed = client.heartbeat().await;
    assert!(
        hb_replayed.is_err(),
        "replayed/stale heartbeat sequence must fail with 409"
    );

    let _ = client.session_stop().await;
}

#[tokio::test]
#[ignore]
async fn test_receiver_rtp_transport_to_simulator() {
    let url = sim_url();
    let mut client = make_test_client(&url);

    // 1. Pair with receiver simulator
    let start = client
        .pair_start("data-plane-e2e")
        .await
        .expect("pair_start failed");
    let session_id = start.session_id.expect("session_id missing");
    client
        .pair_confirm(&session_id, "data-plane-e2e", "482391")
        .await
        .expect("pair_confirm failed");

    // 2. Start session on receiver
    let sess_id = format!("sess_dp_{}", uuid::Uuid::new_v4());
    let sess_resp = client
        .session_start(&sess_id, "pcm_s16le", 48000, 16, 2, 55400, 150, 80)
        .await
        .expect("session_start failed");

    let stream_port = sess_resp.stream_port;
    let ssrc = sess_resp.ssrc;

    use michi_receivers::transport::{AudioTransport, RtpReceiverTransport, TransportStreamConfig};

    // 3. Create RTP transport and write real PCM packets
    let target_addr = format!("127.0.0.1:{stream_port}");
    let mut transport = RtpReceiverTransport::new(&target_addr, ssrc);
    transport
        .start(TransportStreamConfig::receiver_v1_lite_default())
        .await
        .expect("transport start failed");

    // 10ms of 48000Hz 16-bit stereo PCM = 480 frames * 4 bytes = 1920 bytes
    let sample_chunk = vec![0x24u8; 1920];
    for _ in 0..10 {
        transport
            .write_pcm(&sample_chunk)
            .await
            .expect("write_pcm failed");
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    // 4. Verify simulator received RTP packets via test metrics endpoint
    let metrics: serde_json::Value = reqwest::get(&format!("{url}/api/v1/test/metrics"))
        .await
        .expect("get metrics request failed")
        .json()
        .await
        .expect("parse metrics json failed");

    let packets_received = metrics
        .get("packets_received")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    assert!(
        packets_received >= 5,
        "simulator must have received at least 5 packets, got {packets_received}"
    );

    // 5. Test volume change propagation
    let vol_resp = client.set_volume(60).await.expect("set_volume failed");
    assert_eq!(vol_resp.volume, Some(60));

    // 6. Stop session cleanly
    client.session_stop().await.expect("session_stop failed");
}

fn create_test_wav(path: &std::path::Path, duration_ms: u64, freq_hz: f32) {
    let sample_rate = 48000u32;
    let num_channels = 2u16;
    let bits_per_sample = 16u16;
    let num_samples = (sample_rate as u64 * duration_ms / 1000) as u32;
    let data_len = num_samples * num_channels as u32 * (bits_per_sample as u32 / 8);
    let riff_len = 36 + data_len;

    let mut buf = Vec::with_capacity(44 + data_len as usize);
    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&riff_len.to_le_bytes());
    buf.extend_from_slice(b"WAVE");
    buf.extend_from_slice(b"fmt ");
    buf.extend_from_slice(&16u32.to_le_bytes());
    buf.extend_from_slice(&1u16.to_le_bytes());
    buf.extend_from_slice(&num_channels.to_le_bytes());
    buf.extend_from_slice(&sample_rate.to_le_bytes());
    let byte_rate = sample_rate * num_channels as u32 * (bits_per_sample as u32 / 8);
    buf.extend_from_slice(&byte_rate.to_le_bytes());
    let block_align = num_channels * (bits_per_sample / 8);
    buf.extend_from_slice(&block_align.to_le_bytes());
    buf.extend_from_slice(&bits_per_sample.to_le_bytes());
    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&data_len.to_le_bytes());

    for i in 0..num_samples {
        let t = i as f32 / sample_rate as f32;
        let sample = (t * freq_hz * 2.0 * std::f32::consts::PI).sin();
        let val = (sample * 16000.0) as i16;
        for _ in 0..num_channels {
            buf.extend_from_slice(&val.to_le_bytes());
        }
    }

    std::fs::write(path, buf).expect("write wav file");
}

struct TestReceiverSink {
    receiver_id: String,
    session_manager: ReceiverSessionManager,
    state: michi_playback::SinkState,
    bytes_received: u64,
    bytes_sent_to_transport: u64,
    volume: u8,
    muted: bool,
}

impl TestReceiverSink {
    fn new(receiver_id: String, session_manager: ReceiverSessionManager) -> Self {
        Self {
            receiver_id,
            session_manager,
            state: michi_playback::SinkState::Preparing,
            bytes_received: 0,
            bytes_sent_to_transport: 0,
            volume: 80,
            muted: false,
        }
    }
}

#[async_trait::async_trait]
impl michi_playback::AudioSink for TestReceiverSink {
    fn id(&self) -> &str {
        &self.receiver_id
    }

    fn kind(&self) -> michi_playback::SinkKind {
        michi_playback::SinkKind::Receiver
    }

    async fn prepare(
        &mut self,
        format: michi_playback::PcmFormat,
    ) -> Result<(), michi_playback::PlaybackError> {
        let active = self
            .session_manager
            .get_active_session(&self.receiver_id)
            .await;
        if active.is_none() {
            let session_id = uuid::Uuid::new_v4().to_string();
            self.session_manager
                .start_session(
                    &self.receiver_id,
                    &session_id,
                    "pcm_s16le",
                    format.sample_rate,
                    format.bit_depth as u32,
                    format.channels as u32,
                    0,
                    200,
                    self.volume as u32,
                )
                .await
                .map_err(michi_playback::PlaybackError::PlaybackFailed)?;
        }
        self.state = michi_playback::SinkState::Ready;
        Ok(())
    }

    async fn write_pcm(&mut self, data: &[u8]) -> Result<usize, michi_playback::PlaybackError> {
        self.bytes_received += data.len() as u64;
        if self.muted {
            self.state = michi_playback::SinkState::Ready;
            return Ok(0);
        }
        match self
            .session_manager
            .write_pcm(&self.receiver_id, data)
            .await
        {
            Ok(written) => {
                self.bytes_sent_to_transport += written as u64;
                self.state = michi_playback::SinkState::AudioFlowing;
                Ok(written)
            }
            Err(e) => {
                self.state = michi_playback::SinkState::Failed;
                Err(michi_playback::PlaybackError::PlaybackFailed(e))
            }
        }
    }

    async fn pause(&mut self) -> Result<(), michi_playback::PlaybackError> {
        self.state = michi_playback::SinkState::Paused;
        Ok(())
    }

    async fn resume(&mut self) -> Result<(), michi_playback::PlaybackError> {
        self.state = michi_playback::SinkState::AudioFlowing;
        Ok(())
    }

    async fn set_volume(&mut self, volume: u8) -> Result<(), michi_playback::PlaybackError> {
        self.volume = volume;
        let _ = self
            .session_manager
            .set_volume(&self.receiver_id, volume as u32)
            .await;
        Ok(())
    }

    async fn health(&self) -> Result<(), michi_playback::PlaybackError> {
        Ok(())
    }

    async fn stop(&mut self) -> Result<(), michi_playback::PlaybackError> {
        self.state = michi_playback::SinkState::Stopped;
        let _ = self.session_manager.stop_session(&self.receiver_id).await;
        Ok(())
    }

    fn snapshot(&self) -> michi_playback::SinkSnapshot {
        michi_playback::SinkSnapshot {
            sink_id: self.receiver_id.clone(),
            kind: michi_playback::SinkKind::Receiver,
            state: self.state,
            bytes_received: self.bytes_received,
            bytes_sent_to_transport: self.bytes_sent_to_transport,
            last_error: None,
            muted: self.muted,
        }
    }
}

struct TestTrackResolver {
    tracks: std::collections::HashMap<uuid::Uuid, michi_core::Track>,
}

#[async_trait::async_trait]
impl michi_playback::TrackResolver for TestTrackResolver {
    async fn get_track(
        &self,
        track_id: uuid::Uuid,
    ) -> Result<michi_core::Track, michi_playback::PlaybackError> {
        self.tracks
            .get(&track_id)
            .cloned()
            .ok_or(michi_playback::PlaybackError::TrackNotFound(track_id))
    }
}

fn make_test_track(
    id: uuid::Uuid,
    title: &str,
    path: &std::path::Path,
    duration_ms: u64,
) -> michi_core::Track {
    michi_core::Track {
        id,
        title: Some(title.to_string()),
        artist: Some("Michi".to_string()),
        album: Some("E2E Album".to_string()),
        album_artist: Some("Michi".to_string()),
        duration_ms: Some(duration_ms),
        file_path: path.display().to_string(),
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
        file_size: std::fs::metadata(path).ok().map(|m| m.len()),
        file_mtime_ns: None,
        starred: false,
        rating: 0,
        starred_at: None,
        replaygain_track_gain: None,
        replaygain_track_peak: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    }
}

#[tokio::test]
#[ignore]
async fn test_autonomous_playback_receiver_data_plane_e2e() {
    let url = sim_url();
    let temp_dir = std::env::temp_dir().join(format!("michi-e2e-wav-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&temp_dir).expect("create temp dir");

    let wav_path1 = temp_dir.join("track1.wav");
    let wav_path2 = temp_dir.join("track2.wav");
    let wav_path_short = temp_dir.join("short.wav");

    create_test_wav(&wav_path1, 2000, 440.0);
    create_test_wav(&wav_path2, 1000, 880.0);
    create_test_wav(&wav_path_short, 250, 660.0);

    let id1 = uuid::Uuid::new_v4();
    let id2 = uuid::Uuid::new_v4();
    let id_short = uuid::Uuid::new_v4();

    let track1 = make_test_track(id1, "Test Track 1", &wav_path1, 2000);
    let track2 = make_test_track(id2, "Test Track 2", &wav_path2, 1000);
    let track_short = make_test_track(id_short, "Short Track", &wav_path_short, 250);

    let mut track_map = std::collections::HashMap::new();
    track_map.insert(id1, track1.clone());
    track_map.insert(id2, track2.clone());
    track_map.insert(id_short, track_short.clone());

    let resolver = Arc::new(TestTrackResolver { tracks: track_map });

    // 1. Pair with simulator
    let identity = make_test_identity();
    let mut client = ReceiverClient::with_identity(&url, identity.clone());
    let start = client.pair_start("e2e-suite").await.expect("pair_start");
    let pair_sess_id = start.session_id.expect("session_id");
    let confirm = client
        .pair_confirm(&pair_sess_id, "e2e-suite", "482391")
        .await
        .expect("pair_confirm");

    let token = confirm.token.expect("token");
    let rec_id = "rec-sim-e2e".to_string();

    // 2. Set up ReceiverSessionManager
    let session_mgr = ReceiverSessionManager::new();
    let reg_arc = session_mgr.registry().await;
    {
        let mut reg = reg_arc.write().await;
        reg.add(michi_receivers::ReceiverRegistryEntry {
            receiver_id: rec_id.clone(),
            name: "Standard Simulator".to_string(),
            device_type: "michi_stream_standard".to_string(),
            base_url: url.clone(),
            paired: true,
            token: Some(token),
            last_seen: Some(chrono::Utc::now()),
            capabilities: vec!["pcm_s16le".to_string()],
            active_session_id: None,
            max_sample_rate: 48000,
            max_bit_depth: 16,
            supported_transports: vec!["rtp_udp".to_string()],
            supported_codecs: vec!["pcm_s16le".to_string()],
            supported_sample_rates: vec![48000],
            supported_bit_depths: vec![16],
            supported_channels: vec![2],
            maximum_safe_volume: Some(100),
        });
    }

    // 3. Reset simulator metrics
    let http_client = reqwest::Client::new();
    let _ = http_client
        .post(format!("{url}/api/v1/test/metrics/reset"))
        .send()
        .await;

    // 4. Instantiate PlaybackEngine
    let (tx, rx) = tokio::sync::mpsc::channel(64);
    let engine_handle = michi_playback::PlaybackEngineHandle::new(tx);
    let engine =
        michi_playback::PlaybackEngine::new(rx, resolver, michi_playback::PcmFormat::default());
    tokio::spawn(async move {
        engine.run().await;
    });

    let sink = Box::new(TestReceiverSink::new(rec_id.clone(), session_mgr.clone()));

    // 5. Start real playback through engine -> decoder -> sink -> transport -> simulator
    engine_handle
        .play(
            track1.clone(),
            vec![sink],
            michi_playback::PlaybackOutputDescription {
                target_id: "rec-sim-e2e".to_string(),
                target_name: "Standard Simulator".to_string(),
                kind: "receiver".to_string(),
                receiver_count: 1,
            },
            0,
        )
        .await
        .expect("play failed");

    // Poll until packets arrive in simulator
    let mut packets_seen = 0u64;
    let mut payload_bytes = 0u64;
    for _ in 0..40 {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        if let Ok(resp) = http_client
            .get(format!("{url}/api/v1/test/metrics"))
            .send()
            .await
        {
            if let Ok(metrics) = resp.json::<serde_json::Value>().await {
                packets_seen = metrics
                    .get("packets_received")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                payload_bytes = metrics
                    .get("payload_bytes_received")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                let pt = metrics.get("last_payload_type").and_then(|v| v.as_u64());
                let malformed = metrics.get("malformed_packets").and_then(|v| v.as_u64());
                if packets_seen >= 10 {
                    assert_eq!(pt, Some(97), "RTP payload type must be 97");
                    assert_eq!(malformed, Some(0), "must have 0 malformed packets");
                    break;
                }
            }
        }
    }

    assert!(
        packets_seen >= 10,
        "engine must stream real RTP packets to receiver simulator, got {packets_seen}"
    );
    assert!(
        payload_bytes >= 9600,
        "payload bytes must be > 0, got {payload_bytes}"
    );

    // 6. Verify Engine Snapshot telemetry
    let snap = engine_handle.snapshot().await.expect("snapshot failed");
    assert!(
        snap.track_bytes_decoded > 0,
        "track_bytes_decoded must be > 0"
    );
    assert!(
        snap.track_pcm_timeline_bytes > 0,
        "track_pcm_timeline_bytes must be > 0"
    );
    assert!(
        snap.network_bytes_sent_total > 0,
        "network_bytes_sent_total must be > 0"
    );
    assert_eq!(snap.output_health, "healthy");
    assert!(snap.is_playing());

    // 7. Verify Pause effect
    engine_handle.pause().await.expect("pause failed");
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let m1: serde_json::Value = http_client
        .get(format!("{url}/api/v1/test/metrics"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let p_paused1 = m1["packets_received"].as_u64().unwrap_or(0);
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let m2: serde_json::Value = http_client
        .get(format!("{url}/api/v1/test/metrics"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let p_paused2 = m2["packets_received"].as_u64().unwrap_or(0);
    assert_eq!(
        p_paused1, p_paused2,
        "packets must not increment while paused"
    );

    // 8. Verify Resume effect
    engine_handle.resume().await.expect("resume failed");
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    let m3: serde_json::Value = http_client
        .get(format!("{url}/api/v1/test/metrics"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let p_resumed = m3["packets_received"].as_u64().unwrap_or(0);
    assert!(
        p_resumed > p_paused2,
        "packets must resume flowing after resume"
    );

    // 9. Verify Seek effect & generation increment
    let gen_before = engine_handle.snapshot().await.unwrap().generation_id;
    engine_handle.seek(500).await.expect("seek failed");
    let gen_after = engine_handle.snapshot().await.unwrap().generation_id;
    assert!(
        gen_after > gen_before,
        "seek must increment engine generation"
    );

    // 10. Verify EOF queue advance
    engine_handle
        .set_queue(
            vec![track_short.clone(), track2.clone()],
            0,
            Some(track_short.id),
        )
        .await
        .expect("set_queue");

    let sink2 = Box::new(TestReceiverSink::new(rec_id.clone(), session_mgr.clone()));
    engine_handle
        .play(
            track_short.clone(),
            vec![sink2],
            michi_playback::PlaybackOutputDescription {
                target_id: "rec-sim-e2e".to_string(),
                target_name: "Standard Simulator".to_string(),
                kind: "receiver".to_string(),
                receiver_count: 1,
            },
            0,
        )
        .await
        .expect("play short");

    // Wait for short track to finish and advance to track2
    let mut advanced = false;
    for _ in 0..40 {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        if let Ok(snap) = engine_handle.snapshot().await {
            if snap.track_id == Some(track2.id) {
                advanced = true;
                break;
            }
        }
    }
    assert!(
        advanced,
        "playback engine must automatically advance to next track on EOF"
    );

    // 11. Verify Stop cleanly stops receiver session
    engine_handle.stop().await.expect("stop failed");
    let final_snap = engine_handle.snapshot().await.unwrap();
    assert_eq!(
        final_snap.lifecycle,
        michi_playback::PlaybackLifecycle::Stopped
    );

    // Clean up temp dir
    let _ = std::fs::remove_dir_all(&temp_dir);
}
