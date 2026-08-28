pub mod decoder;
pub mod engine;
pub mod error;
pub mod model;
pub mod resolver;
pub mod sink;

pub use decoder::FfmpegPcmDecoder;
pub use engine::{spawn_playback_engine, PlaybackEngine, PlaybackEngineHandle};
pub use error::PlaybackError;
pub use model::*;
pub use resolver::{SqliteTrackResolver, TrackResolver};
pub use sink::AudioSink;

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use uuid::Uuid;

    struct MockSink {
        id: String,
        kind: SinkKind,
        state: SinkState,
        bytes_written: Arc<AtomicUsize>,
        should_fail: bool,
        prepared: bool,
    }

    impl MockSink {
        fn new(id: &str, kind: SinkKind, should_fail: bool) -> (Self, Arc<AtomicUsize>) {
            let written = Arc::new(AtomicUsize::new(0));
            (
                Self {
                    id: id.to_string(),
                    kind,
                    state: SinkState::Ready,
                    bytes_written: written.clone(),
                    should_fail,
                    prepared: false,
                },
                written,
            )
        }
    }

    #[async_trait]
    impl AudioSink for MockSink {
        fn id(&self) -> &str {
            &self.id
        }

        fn kind(&self) -> SinkKind {
            self.kind
        }

        async fn prepare(&mut self, _format: PcmFormat) -> Result<(), PlaybackError> {
            if self.should_fail {
                self.state = SinkState::Failed;
                return Err(PlaybackError::PlaybackFailed(
                    "mock prepare failure".to_string(),
                ));
            }
            self.prepared = true;
            self.state = SinkState::Ready;
            Ok(())
        }

        async fn write_pcm(&mut self, data: &[u8]) -> Result<usize, PlaybackError> {
            if self.should_fail {
                self.state = SinkState::Failed;
                return Err(PlaybackError::PlaybackFailed(
                    "mock write failure".to_string(),
                ));
            }
            self.state = SinkState::AudioFlowing;
            self.bytes_written.fetch_add(data.len(), Ordering::SeqCst);
            Ok(data.len())
        }

        async fn pause(&mut self) -> Result<(), PlaybackError> {
            self.state = SinkState::Paused;
            Ok(())
        }

        async fn resume(&mut self) -> Result<(), PlaybackError> {
            self.state = SinkState::AudioFlowing;
            Ok(())
        }

        async fn set_volume(&mut self, _volume: u8) -> Result<(), PlaybackError> {
            Ok(())
        }

        async fn health(&self) -> Result<(), PlaybackError> {
            if self.should_fail {
                Err(PlaybackError::OutputUnavailable(self.id.clone()))
            } else {
                Ok(())
            }
        }

        async fn stop(&mut self) -> Result<(), PlaybackError> {
            self.state = SinkState::Stopped;
            Ok(())
        }

        fn snapshot(&self) -> SinkSnapshot {
            let written = self.bytes_written.load(Ordering::SeqCst) as u64;
            SinkSnapshot {
                sink_id: self.id.clone(),
                kind: self.kind,
                state: self.state,
                bytes_received: written,
                bytes_sent_to_transport: written,
                muted: false,
                last_error: if self.should_fail {
                    Some("mock failure".to_string())
                } else {
                    None
                },
            }
        }
    }

    use chrono::Utc;
    use michi_core::{AudioFormat, Track};

    fn make_test_track(id: Uuid, file_path: &str) -> Track {
        Track {
            id,
            title: Some("Test Track".to_string()),
            artist: Some("Test Artist".to_string()),
            album: Some("Test Album".to_string()),
            album_artist: None,
            duration_ms: Some(180_000),
            file_path: file_path.to_string(),
            format: AudioFormat::Wav,
            sample_rate: Some(48000),
            bit_depth: Some(16),
            channels: Some(2),
            artwork_id: None,
            genre: Some("Test".to_string()),
            year: Some(2026),
            track_number: Some(1),
            disc_number: Some(1),
            content_hash: None,
            file_size: Some(1024),
            file_mtime_ns: None,
            starred: false,
            rating: 0,
            starred_at: None,
            replaygain_track_gain: None,
            replaygain_track_peak: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    struct MockResolver;

    #[async_trait]
    impl TrackResolver for MockResolver {
        async fn get_track(&self, id: Uuid) -> Result<Track, PlaybackError> {
            Ok(make_test_track(id, "/nonexistent/test.wav"))
        }
    }

    #[tokio::test]
    async fn test_playback_engine_no_output_fails_fast() {
        let (handle, join) = spawn_playback_engine(Arc::new(MockResolver), PcmFormat::default());
        let track = make_test_track(Uuid::new_v4(), "/nonexistent.wav");

        let res = handle
            .play(
                track,
                Vec::new(),
                PlaybackOutputDescription {
                    target_id: "empty".to_string(),
                    target_name: "Empty".to_string(),
                    kind: "none".to_string(),
                    receiver_count: 0,
                },
                0,
            )
            .await;

        assert!(matches!(res, Err(PlaybackError::NoOutputSelected)));

        let snap = handle.snapshot().await.unwrap();
        assert!(!snap.is_playing());
        assert_eq!(snap.lifecycle, PlaybackLifecycle::Idle);

        handle.shutdown().await;
        let _ = join.await;
    }

    #[tokio::test]
    async fn test_playback_engine_controls_and_state_transitions() {
        let (handle, join) = spawn_playback_engine(Arc::new(MockResolver), PcmFormat::default());

        // Initial state
        let snap = handle.snapshot().await.unwrap();
        assert_eq!(snap.lifecycle, PlaybackLifecycle::Idle);
        assert_eq!(snap.volume, 80);

        // Volume
        handle.set_volume(95).await.unwrap();
        let snap = handle.snapshot().await.unwrap();
        assert_eq!(snap.volume, 95);

        // Repeat & Shuffle
        handle.set_repeat(RepeatMode::All).await.unwrap();
        handle.set_shuffle(true).await.unwrap();
        let snap = handle.snapshot().await.unwrap();
        assert_eq!(snap.repeat, RepeatMode::All);
        assert!(snap.shuffle);

        // Stop on idle
        handle.stop().await.unwrap();
        let snap = handle.snapshot().await.unwrap();
        assert_eq!(snap.lifecycle, PlaybackLifecycle::Stopped);

        handle.shutdown().await;
        let _ = join.await;
    }

    #[tokio::test]
    async fn test_playback_engine_sink_failure_policy() {
        let (handle, join) = spawn_playback_engine(Arc::new(MockResolver), PcmFormat::default());
        let (failing_sink, _) = MockSink::new("sink-fail", SinkKind::Receiver, true);

        let track = make_test_track(Uuid::new_v4(), "/nonexistent.wav");
        let res = handle
            .play(
                track,
                vec![Box::new(failing_sink)],
                PlaybackOutputDescription {
                    target_id: "sink-fail".to_string(),
                    target_name: "Failing Sink".to_string(),
                    kind: "receiver".to_string(),
                    receiver_count: 1,
                },
                0,
            )
            .await;

        // Fails during preparation
        assert!(res.is_err());
        let snap = handle.snapshot().await.unwrap();
        assert!(!snap.is_playing());

        handle.shutdown().await;
        let _ = join.await;
    }

    #[tokio::test]
    async fn test_playback_engine_queue_and_navigation() {
        let (handle, join) = spawn_playback_engine(Arc::new(MockResolver), PcmFormat::default());
        let t1 = make_test_track(Uuid::new_v4(), "/track1.wav");
        let t2 = make_test_track(Uuid::new_v4(), "/track2.wav");

        handle
            .set_queue(vec![t1.clone(), t2.clone()], 0, None)
            .await
            .unwrap();

        let snap = handle.snapshot().await.unwrap();
        assert_eq!(snap.lifecycle, PlaybackLifecycle::Idle);

        handle.shutdown().await;
        let _ = join.await;
    }

    fn generate_test_wav(
        path: &std::path::Path,
        sample_rate: u32,
        channels: u16,
        duration_seconds: f32,
    ) -> std::io::Result<()> {
        use std::io::Write;
        let num_samples = (sample_rate as f32 * duration_seconds) as u32;
        let data_len = num_samples * (channels as u32) * 2;
        let file_len = 36 + data_len;
        let mut file = std::fs::File::create(path)?;
        file.write_all(b"RIFF")?;
        file.write_all(&file_len.to_le_bytes())?;
        file.write_all(b"WAVEfmt ")?;
        file.write_all(&16u32.to_le_bytes())?;
        file.write_all(&1u16.to_le_bytes())?;
        file.write_all(&channels.to_le_bytes())?;
        file.write_all(&sample_rate.to_le_bytes())?;
        let byte_rate = sample_rate * (channels as u32) * 2;
        file.write_all(&byte_rate.to_le_bytes())?;
        let block_align = channels * 2;
        file.write_all(&block_align.to_le_bytes())?;
        file.write_all(&16u16.to_le_bytes())?;
        file.write_all(b"data")?;
        file.write_all(&data_len.to_le_bytes())?;
        for i in 0..num_samples {
            let t = i as f32 / sample_rate as f32;
            let sample = (3000.0 * (2.0 * std::f32::consts::PI * 440.0 * t).sin()) as i16;
            for _ in 0..channels {
                file.write_all(&sample.to_le_bytes())?;
            }
        }
        file.flush()
    }

    #[tokio::test]
    async fn test_suite_a_real_wav_fixture_decoding() {
        let temp_dir = std::env::temp_dir().join(format!("michi-playback-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let wav_path = temp_dir.join("test_tone.wav");
        generate_test_wav(&wav_path, 48000, 2, 0.5).unwrap();

        let mut decoder = FfmpegPcmDecoder::new(
            wav_path.to_str().unwrap().to_string(),
            PcmFormat {
                sample_rate: 48000,
                channels: 2,
                bit_depth: 16,
            },
        );

        decoder.start(0).await.unwrap();
        let mut total_bytes = 0;
        let mut buf = vec![0u8; 4096];
        loop {
            match decoder.read_pcm(&mut buf).await {
                Ok(0) => break,
                Ok(n) => total_bytes += n,
                Err(e) => panic!("decoder failed: {e}"),
            }
        }

        assert!(
            total_bytes > 0,
            "Decoder must extract real PCM bytes from valid WAV fixture"
        );
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn test_suite_i_partial_output_policy() {
        let temp_dir = std::env::temp_dir().join(format!("michi-playback-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let wav_path = temp_dir.join("partial_test.wav");
        generate_test_wav(&wav_path, 48000, 2, 0.5).unwrap();

        let (handle, join) = spawn_playback_engine(Arc::new(MockResolver), PcmFormat::default());
        let (good_sink, _good_written) = MockSink::new("sink-good", SinkKind::Receiver, false);
        let (fail_sink, _) = MockSink::new("sink-fail", SinkKind::Receiver, true);

        let track = make_test_track(Uuid::new_v4(), wav_path.to_str().unwrap());
        let res = handle
            .play(
                track,
                vec![Box::new(good_sink), Box::new(fail_sink)],
                PlaybackOutputDescription {
                    target_id: "group-1".to_string(),
                    target_name: "Partial Group".to_string(),
                    kind: "room_group".to_string(),
                    receiver_count: 2,
                },
                0,
            )
            .await;

        assert!(
            res.is_ok(),
            "Playback should proceed if at least 1 sink succeeds"
        );
        let snap = handle.snapshot().await.unwrap();
        assert_eq!(snap.output_health, "partial");

        handle.shutdown().await;
        let _ = join.await;
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn test_suite_l_concurrent_fanout() {
        let temp_dir = std::env::temp_dir().join(format!("michi-playback-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let wav_path = temp_dir.join("fanout_test.wav");
        generate_test_wav(&wav_path, 48000, 2, 0.5).unwrap();

        let (handle, join) = spawn_playback_engine(Arc::new(MockResolver), PcmFormat::default());
        let (s1, _w1) = MockSink::new("sink-1", SinkKind::Receiver, false);
        let (s2, _w2) = MockSink::new("sink-2", SinkKind::Receiver, false);
        let (s3, _w3) = MockSink::new("sink-3", SinkKind::Receiver, false);

        let track = make_test_track(Uuid::new_v4(), wav_path.to_str().unwrap());
        let res = handle
            .play(
                track,
                vec![Box::new(s1), Box::new(s2), Box::new(s3)],
                PlaybackOutputDescription {
                    target_id: "chain-1".to_string(),
                    target_name: "Chain All".to_string(),
                    kind: "chain".to_string(),
                    receiver_count: 3,
                },
                0,
            )
            .await;

        assert!(res.is_ok());
        let snap = handle.snapshot().await.unwrap();
        assert_eq!(snap.output_health, "healthy");
        assert_eq!(snap.sinks.len(), 3);

        handle.shutdown().await;
        let _ = join.await;
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn test_suite_c_missing_file_fails_closed() {
        let (handle, join) = spawn_playback_engine(Arc::new(MockResolver), PcmFormat::default());
        let (sink, _) = MockSink::new("sink-1", SinkKind::Receiver, false);
        let track = make_test_track(Uuid::new_v4(), "/nonexistent_test_track.flac");

        let res = handle
            .play(
                track,
                vec![Box::new(sink)],
                PlaybackOutputDescription {
                    target_id: "sink-1".to_string(),
                    target_name: "Sink 1".to_string(),
                    kind: "receiver".to_string(),
                    receiver_count: 1,
                },
                0,
            )
            .await;

        assert!(matches!(res, Err(PlaybackError::TrackFileMissing(_))));
        let snap = handle.snapshot().await.unwrap();
        assert_eq!(snap.lifecycle, PlaybackLifecycle::Idle);
        assert!(!snap.is_playing());

        handle.shutdown().await;
        let _ = join.await;
    }

    #[tokio::test]
    async fn test_suite_j_queue_jump_changes_track() {
        let (handle, join) = spawn_playback_engine(Arc::new(MockResolver), PcmFormat::default());
        let t1 = make_test_track(Uuid::new_v4(), "/track1.wav");
        let t2 = make_test_track(Uuid::new_v4(), "/track2.wav");
        let t3 = make_test_track(Uuid::new_v4(), "/track3.wav");

        handle
            .set_queue(vec![t1.clone(), t2.clone(), t3.clone()], 0, None)
            .await
            .unwrap();

        handle.jump_to_index(2).await.unwrap();
        let snap = handle.snapshot().await.unwrap();
        assert_eq!(snap.track_id, Some(t3.id));
        assert_eq!(snap.generation_id, 1);

        handle.shutdown().await;
        let _ = join.await;
    }

    #[tokio::test]
    async fn test_suite_p_generation_reset_on_track_change() {
        let (handle, join) = spawn_playback_engine(Arc::new(MockResolver), PcmFormat::default());
        let t1 = make_test_track(Uuid::new_v4(), "/track1.wav");
        let t2 = make_test_track(Uuid::new_v4(), "/track2.wav");

        handle
            .set_queue(vec![t1.clone(), t2.clone()], 0, None)
            .await
            .unwrap();

        let initial_gen = handle.snapshot().await.unwrap().generation_id;
        handle.jump_to_index(1).await.unwrap();
        let new_gen = handle.snapshot().await.unwrap().generation_id;
        assert_eq!(new_gen, initial_gen + 1);

        handle.shutdown().await;
        let _ = join.await;
    }
}
