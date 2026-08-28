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
                return Err(PlaybackError::PlaybackFailed("mock prepare failure".to_string()));
            }
            self.prepared = true;
            self.state = SinkState::Ready;
            Ok(())
        }

        async fn write_pcm(&mut self, data: &[u8]) -> Result<usize, PlaybackError> {
            if self.should_fail {
                self.state = SinkState::Failed;
                return Err(PlaybackError::PlaybackFailed("mock write failure".to_string()));
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
            SinkSnapshot {
                sink_id: self.id.clone(),
                kind: self.kind,
                state: self.state,
                bytes_written: self.bytes_written.load(Ordering::SeqCst) as u64,
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

        handle.set_queue(vec![t1.clone(), t2.clone()], 0).await.unwrap();

        let snap = handle.snapshot().await.unwrap();
        assert_eq!(snap.lifecycle, PlaybackLifecycle::Idle);

        handle.shutdown().await;
        let _ = join.await;
    }
}
