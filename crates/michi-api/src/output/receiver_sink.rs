use async_trait::async_trait;
use michi_playback::{AudioSink, PcmFormat, PlaybackError, SinkKind, SinkSnapshot, SinkState};
use michi_receivers::ReceiverSessionManager;
use tracing::{debug, warn};
use uuid::Uuid;

pub struct ReceiverAudioSink {
    receiver_id: String,
    session_manager: ReceiverSessionManager,
    state: SinkState,
    bytes_received: u64,
    bytes_sent_to_transport: u64,
    last_error: Option<String>,
    volume: u8,
    muted: bool,
}

impl ReceiverAudioSink {
    pub fn new(receiver_id: String, session_manager: ReceiverSessionManager) -> Self {
        Self {
            receiver_id,
            session_manager,
            state: SinkState::Preparing,
            bytes_received: 0,
            bytes_sent_to_transport: 0,
            last_error: None,
            volume: 80,
            muted: false,
        }
    }

    pub fn new_with_config(
        receiver_id: String,
        session_manager: ReceiverSessionManager,
        volume: u8,
        muted: bool,
    ) -> Self {
        Self {
            receiver_id,
            session_manager,
            state: SinkState::Preparing,
            bytes_received: 0,
            bytes_sent_to_transport: 0,
            last_error: None,
            volume,
            muted,
        }
    }
}

#[async_trait]
impl AudioSink for ReceiverAudioSink {
    fn id(&self) -> &str {
        &self.receiver_id
    }

    fn kind(&self) -> SinkKind {
        SinkKind::Receiver
    }

    async fn prepare(&mut self, format: PcmFormat) -> Result<(), PlaybackError> {
        let registry_arc = self.session_manager.registry().await;
        let registry = registry_arc.read().await;
        let entry = registry
            .get(&self.receiver_id)
            .ok_or_else(|| PlaybackError::ReceiverNotPaired(self.receiver_id.clone()))?;

        if !entry.paired {
            self.state = SinkState::Failed;
            self.last_error = Some("receiver is not paired".to_string());
            return Err(PlaybackError::ReceiverNotPaired(self.receiver_id.clone()));
        }
        drop(registry);

        // Check if session is already active; if not, start session
        let active = self
            .session_manager
            .get_active_session(&self.receiver_id)
            .await;

        if active.is_none() {
            let session_id = Uuid::new_v4().to_string();
            match self
                .session_manager
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
            {
                Ok(_) => {
                    debug!("started receiver session for {}", self.receiver_id);
                }
                Err(e) => {
                    self.state = SinkState::Failed;
                    self.last_error = Some(e.clone());
                    return Err(PlaybackError::PlaybackFailed(format!(
                        "failed to start receiver session for {}: {}",
                        self.receiver_id, e
                    )));
                }
            }
        }

        self.state = SinkState::Ready;
        self.last_error = None;
        Ok(())
    }

    async fn write_pcm(&mut self, data: &[u8]) -> Result<usize, PlaybackError> {
        self.bytes_received += data.len() as u64;

        if self.muted {
            self.state = SinkState::Ready; // P1-07: Muted sink does not declare AudioFlowing
            return Ok(0); // 0 bytes sent to network transport when muted
        }

        match self
            .session_manager
            .write_pcm(&self.receiver_id, data)
            .await
        {
            Ok(written) => {
                self.bytes_sent_to_transport += written as u64;
                self.state = SinkState::AudioFlowing;
                self.last_error = None;
                Ok(written)
            }
            Err(e) => {
                warn!("receiver {} write_pcm error: {}", self.receiver_id, e);
                self.state = SinkState::Failed;
                self.last_error = Some(e.clone());
                Err(PlaybackError::PlaybackFailed(e))
            }
        }
    }

    async fn pause(&mut self) -> Result<(), PlaybackError> {
        self.state = SinkState::Paused;
        Ok(())
    }

    async fn resume(&mut self) -> Result<(), PlaybackError> {
        self.state = SinkState::AudioFlowing;
        Ok(())
    }

    async fn set_volume(&mut self, volume: u8) -> Result<(), PlaybackError> {
        self.volume = volume;
        self.session_manager
            .set_volume(&self.receiver_id, volume as u32)
            .await
            .map_err(|e| {
                PlaybackError::PlaybackFailed(format!(
                    "failed to set volume on receiver {}: {}",
                    self.receiver_id, e
                ))
            })?;
        Ok(())
    }

    async fn health(&self) -> Result<(), PlaybackError> {
        let registry_arc = self.session_manager.registry().await;
        let registry = registry_arc.read().await;
        if let Some(entry) = registry.get(&self.receiver_id) {
            if entry.paired {
                Ok(())
            } else {
                Err(PlaybackError::ReceiverNotPaired(self.receiver_id.clone()))
            }
        } else {
            Err(PlaybackError::ReceiverNotPaired(self.receiver_id.clone()))
        }
    }

    async fn stop(&mut self) -> Result<(), PlaybackError> {
        let res = self.session_manager.stop_session(&self.receiver_id).await;
        self.state = SinkState::Stopped;
        res.map(|_| ()).map_err(|e| {
            PlaybackError::PlaybackFailed(format!(
                "failed to stop session on receiver {}: {}",
                self.receiver_id, e
            ))
        })
    }

    fn snapshot(&self) -> SinkSnapshot {
        SinkSnapshot {
            sink_id: self.receiver_id.clone(),
            kind: SinkKind::Receiver,
            state: self.state,
            bytes_received: self.bytes_received,
            bytes_sent_to_transport: self.bytes_sent_to_transport,
            muted: self.muted,
            last_error: self.last_error.clone(),
        }
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    use michi_playback::{
        PcmFormat, PlaybackEngine, PlaybackEngineHandle, PlaybackLifecycle,
        PlaybackOutputDescription, TrackResolver,
    };
    use michi_receivers::ReceiverClient;
    use std::sync::Arc;

    fn make_test_identity() -> Arc<michi_identity::IdentityManager> {
        let dir = std::env::temp_dir().join(format!("michi-rec-test-{}", uuid::Uuid::new_v4()));
        let _ = std::fs::create_dir_all(&dir);
        Arc::new(
            michi_identity::IdentityManager::generate(&dir, "Michi Micro Server", "")
                .expect("test identity generation"),
        )
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

    struct TestTrackResolver {
        tracks: std::collections::HashMap<uuid::Uuid, michi_core::Track>,
    }

    #[async_trait]
    impl TrackResolver for TestTrackResolver {
        async fn get_track(
            &self,
            track_id: uuid::Uuid,
        ) -> Result<michi_core::Track, PlaybackError> {
            self.tracks
                .get(&track_id)
                .cloned()
                .ok_or(PlaybackError::TrackNotFound(track_id))
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
    async fn test_autonomous_playback_to_production_receiver_sink_e2e() {
        let url = std::env::var("MICHI_RECEIVER_SIM_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:8080".to_string());

        let temp_dir =
            std::env::temp_dir().join(format!("michi-prod-sink-e2e-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).expect("create temp dir");

        let wav_path1 = temp_dir.join("track1.wav");
        let wav_path2 = temp_dir.join("track2.wav");
        let wav_path_short = temp_dir.join("short.wav");
        let wav_path_missing = temp_dir.join("missing.wav");

        create_test_wav(&wav_path1, 2000, 440.0);
        create_test_wav(&wav_path2, 1000, 880.0);
        create_test_wav(&wav_path_short, 250, 660.0);

        let id1 = uuid::Uuid::new_v4();
        let id2 = uuid::Uuid::new_v4();
        let id_short = uuid::Uuid::new_v4();
        let id_missing = uuid::Uuid::new_v4();

        let track1 = make_test_track(id1, "Track 1", &wav_path1, 2000);
        let track2 = make_test_track(id2, "Track 2", &wav_path2, 1000);
        let track_short = make_test_track(id_short, "Short Track", &wav_path_short, 250);
        let track_missing = make_test_track(id_missing, "Missing Track", &wav_path_missing, 1000);

        let mut track_map = std::collections::HashMap::new();
        track_map.insert(id1, track1.clone());
        track_map.insert(id2, track2.clone());
        track_map.insert(id_short, track_short.clone());
        track_map.insert(id_missing, track_missing.clone());

        let resolver = Arc::new(TestTrackResolver { tracks: track_map });

        // 1. Pair with simulator
        let identity = make_test_identity();
        let mut client = ReceiverClient::with_identity(&url, identity.clone());
        let start = client.pair_start("prod-e2e").await.expect("pair_start");
        let pair_sess_id = start.session_id.expect("session_id");
        let confirm = client
            .pair_confirm(&pair_sess_id, "prod-e2e", "482391")
            .await
            .expect("pair_confirm");

        let token = confirm.token.expect("token");
        let rec_id = "rec-prod-sink-e2e".to_string();

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
        let engine_handle = PlaybackEngineHandle::new(tx);
        let engine = PlaybackEngine::new(rx, resolver, PcmFormat::default());
        tokio::spawn(async move {
            engine.run().await;
        });

        // 5. PRODUCTION ReceiverAudioSink instantiated and passed to PlaybackEngine
        let prod_sink = Box::new(ReceiverAudioSink::new(rec_id.clone(), session_mgr.clone()));

        engine_handle
            .play(
                track1.clone(),
                vec![prod_sink],
                PlaybackOutputDescription {
                    target_id: "rec-prod-sink-e2e".to_string(),
                    target_name: "Standard Simulator".to_string(),
                    kind: "receiver".to_string(),
                    receiver_count: 1,
                },
                0,
            )
            .await
            .expect("play with production ReceiverAudioSink failed");

        // 6. Poll metrics from canonical simulator
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
            "production sink must stream real RTP packets to receiver simulator, got {packets_seen}"
        );
        assert!(
            payload_bytes >= 9600,
            "payload bytes must be > 0, got {payload_bytes}"
        );

        // 7. Verify Engine Snapshot telemetry with production sink
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
        assert_eq!(snap.sinks.len(), 1);
        assert_eq!(snap.sinks[0].state, SinkState::AudioFlowing);
        assert!(snap.sinks[0].bytes_sent_to_transport > 0);

        // 8. Verify Pause effect
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

        // 9. Verify Resume effect
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

        // 10. Verify Seek effect & generation increment
        let gen_before = engine_handle.snapshot().await.unwrap().generation_id;
        engine_handle.seek(500).await.expect("seek failed");
        let gen_after = engine_handle.snapshot().await.unwrap().generation_id;
        assert!(
            gen_after > gen_before,
            "seek must increment engine generation"
        );

        // 11. Verify EOF queue advance with production sink
        engine_handle
            .set_queue(
                vec![track_short.clone(), track2.clone()],
                0,
                Some(track_short.id),
            )
            .await
            .expect("set_queue");

        let prod_sink2 = Box::new(ReceiverAudioSink::new(rec_id.clone(), session_mgr.clone()));
        engine_handle
            .play(
                track_short.clone(),
                vec![prod_sink2],
                PlaybackOutputDescription {
                    target_id: "rec-prod-sink-e2e".to_string(),
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

        // 12. Verify Next Track Missing failure mode
        engine_handle
            .set_queue(
                vec![track_short.clone(), track_missing.clone()],
                0,
                Some(track_short.id),
            )
            .await
            .expect("set_queue");

        let prod_sink3 = Box::new(ReceiverAudioSink::new(rec_id.clone(), session_mgr.clone()));
        engine_handle
            .play(
                track_short.clone(),
                vec![prod_sink3],
                PlaybackOutputDescription {
                    target_id: "rec-prod-sink-e2e".to_string(),
                    target_name: "Standard Simulator".to_string(),
                    kind: "receiver".to_string(),
                    receiver_count: 1,
                },
                0,
            )
            .await
            .expect("play short before missing");

        // Wait for short track to hit EOF and fail when starting missing track
        let mut failed = false;
        for _ in 0..40 {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            if let Ok(snap) = engine_handle.snapshot().await {
                if snap.lifecycle == PlaybackLifecycle::Failed {
                    failed = true;
                    break;
                }
            }
        }
        assert!(
            failed,
            "playback engine must transition to Failed when next track file is missing"
        );

        // 13. Verify Stop cleanly stops receiver session
        engine_handle.stop().await.expect("stop failed");
        let final_snap = engine_handle.snapshot().await.unwrap();
        assert_eq!(final_snap.lifecycle, PlaybackLifecycle::Stopped);

        // Clean up temp dir
        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
