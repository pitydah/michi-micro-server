use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use michi_playback::{EngineEvent, EngineSnapshot, PlaybackEngineHandle, PlaybackLifecycle};
use serde::Serialize;
use sqlx::SqlitePool;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};
use uuid::Uuid;

/// Observability health model for playback event projection.
#[derive(Debug, Clone, Serialize)]
pub struct PlaybackProjectionHealth {
    pub healthy: bool,
    pub last_success_at: Option<DateTime<Utc>>,
    pub last_error_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub failures_total: u64,
    pub lag_recoveries_total: u64,
}

impl Default for PlaybackProjectionHealth {
    fn default() -> Self {
        Self {
            healthy: true,
            last_success_at: None,
            last_error_at: None,
            last_error: None,
            failures_total: 0,
            lag_recoveries_total: 0,
        }
    }
}

/// Normalized derived projection state for deduplicating SQLite writes.
#[derive(Debug, Clone, PartialEq)]
pub struct PersistentPlaybackProjection {
    pub current_track_id: Option<Uuid>,
    pub current_index: i32,
    pub position_ms: u64,
    pub playing: bool,
    pub volume: f64,
    pub shuffle: bool,
    pub repeat_mode: String,
}

/// Cohesive coordinator responsible for projecting PlaybackEngine runtime events
/// into authoritative in-memory state and persistent SQLite PlaybackSession.
pub struct PlaybackProjectionCoordinator {
    db: SqlitePool,
    legacy_playback_state: Arc<RwLock<michi_sync::PlaybackState>>,
    engine: PlaybackEngineHandle,
    health: Arc<RwLock<PlaybackProjectionHealth>>,
    last_projection: Arc<RwLock<Option<PersistentPlaybackProjection>>>,
}

impl PlaybackProjectionCoordinator {
    pub fn new(
        db: SqlitePool,
        legacy_playback_state: Arc<RwLock<michi_sync::PlaybackState>>,
        engine: PlaybackEngineHandle,
    ) -> (Self, Arc<RwLock<PlaybackProjectionHealth>>) {
        let health = Arc::new(RwLock::new(PlaybackProjectionHealth::default()));
        let coordinator = Self {
            db,
            legacy_playback_state,
            engine,
            health: health.clone(),
            last_projection: Arc::new(RwLock::new(None)),
        };
        (coordinator, health)
    }

    async fn record_success(&self) {
        let mut h = self.health.write().await;
        h.healthy = true;
        h.last_success_at = Some(Utc::now());
    }

    async fn record_failure(&self, err_msg: &str) {
        let mut h = self.health.write().await;
        h.healthy = false;
        h.last_error_at = Some(Utc::now());
        h.last_error = Some(err_msg.to_string());
        h.failures_total = h.failures_total.saturating_add(1);
        warn!(error = %err_msg, "playback projection coordinator failure recorded");
    }

    /// Spawns the background projection task listening to engine events.
    pub fn spawn(self, shutdown: CancellationToken) {
        let mut event_rx = self.engine.subscribe_events();
        let coordinator = Arc::new(self);

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = shutdown.cancelled() => {
                        let c = coordinator.clone();
                        let _ = tokio::time::timeout(Duration::from_millis(500), async move {
                            c.flush_shutdown().await;
                        }).await;
                        break;
                    }
                    event_res = event_rx.recv() => {
                        match event_res {
                            Ok(event) => {
                                if let Ok(snap) = coordinator.engine.snapshot().await {
                                    coordinator.handle_event(&event, &snap).await;
                                }
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(lagged)) => {
                                {
                                    let mut h = coordinator.health.write().await;
                                    h.lag_recoveries_total = h.lag_recoveries_total.saturating_add(1);
                                }
                                warn!(lagged, "playback projection task lagged by events; reconciling snapshot");
                                if let Ok(snap) = coordinator.engine.snapshot().await {
                                    coordinator.reconcile_from_snapshot(&snap).await;
                                }
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                                break;
                            }
                        }
                    }
                }
            }
        });
    }

    /// Handles a single EngineEvent paired with a fresh EngineSnapshot.
    pub async fn handle_event(&self, event: &EngineEvent, snap: &EngineSnapshot) {
        let is_playing = matches!(
            snap.lifecycle,
            PlaybackLifecycle::AudioFlowing | PlaybackLifecycle::Playing
        );
        let position_ms = if matches!(
            snap.lifecycle,
            PlaybackLifecycle::Stopped | PlaybackLifecycle::Ended
        ) {
            0
        } else {
            snap.position_ms
        };

        // 1. Sync legacy PlaybackState projection (RAM only)
        {
            let mut ps = self.legacy_playback_state.write().await;
            ps.track_id = snap.track_id;
            ps.position_ms = position_ms;
            ps.playing = is_playing;
            ps.volume = (snap.volume as f64) / 100.0;
            ps.shuffle = snap.shuffle;
            ps.repeat = snap.repeat.as_str().to_string();
            ps.updated_at = Utc::now();
        }

        // 2. Fetch existing session from SQLite
        let mut sess = match michi_db::get_latest_playback_session(&self.db).await {
            Ok(Some(s)) => s,
            Ok(None) => return,
            Err(e) => {
                self.record_failure(&format!("get_latest_playback_session failed: {e}"))
                    .await;
                return;
            }
        };

        // 3. Derive updated persistent state according to event semantics
        match event {
            EngineEvent::LifecycleChanged {
                lifecycle,
                track_id,
            } => {
                let is_flowing = matches!(
                    lifecycle,
                    PlaybackLifecycle::AudioFlowing | PlaybackLifecycle::Playing
                );
                sess.current_track_id = track_id.or(snap.track_id);
                if matches!(
                    lifecycle,
                    PlaybackLifecycle::Stopped | PlaybackLifecycle::Ended
                ) {
                    sess.position_ms = 0;
                } else if is_flowing {
                    sess.position_ms = snap.position_ms;
                }
                sess.playing = is_flowing;
                sess.volume = (snap.volume as f64) / 100.0;
                sess.shuffle = snap.shuffle;
                sess.repeat_mode = snap.repeat.as_str().to_string();
            }
            EngineEvent::Paused {
                track_id,
                position_ms,
            } => {
                sess.current_track_id = track_id.or(snap.track_id);
                sess.position_ms = *position_ms;
                sess.playing = false;
                sess.volume = (snap.volume as f64) / 100.0;
                sess.shuffle = snap.shuffle;
                sess.repeat_mode = snap.repeat.as_str().to_string();
            }
            EngineEvent::Stopped | EngineEvent::Ended { .. } => {
                sess.position_ms = 0;
                sess.playing = false;
            }
            EngineEvent::Failed { .. } => {
                sess.playing = false;
            }
            EngineEvent::Seeked { position_ms, .. } => {
                sess.position_ms = *position_ms;
            }
            EngineEvent::PositionCheckpoint { position_ms, .. } => {
                if is_playing {
                    sess.position_ms = *position_ms;
                    sess.playing = is_playing;
                }
            }
            EngineEvent::TrackChanged { track_id, index } => {
                sess.current_track_id = *track_id;
                sess.current_index = *index as i32;
                if is_playing {
                    sess.position_ms = snap.position_ms;
                }
            }
            EngineEvent::VolumeChanged { volume } => {
                sess.volume = (*volume as f64) / 100.0;
            }
            EngineEvent::ShuffleChanged { shuffle } => {
                sess.shuffle = *shuffle;
            }
            EngineEvent::RepeatChanged { repeat } => {
                sess.repeat_mode = repeat.as_str().to_string();
            }
            EngineEvent::QueueChanged { .. } | EngineEvent::OutputChanged { .. } => {}
        }

        // 4. Check dedup and persist
        self.persist_if_changed(&sess).await;
    }

    /// Reconciles state from an engine snapshot (e.g. after event lag or startup).
    pub async fn reconcile_from_snapshot(&self, snap: &EngineSnapshot) {
        let is_playing = matches!(
            snap.lifecycle,
            PlaybackLifecycle::AudioFlowing | PlaybackLifecycle::Playing
        );
        let position_ms = if matches!(
            snap.lifecycle,
            PlaybackLifecycle::Stopped | PlaybackLifecycle::Ended
        ) {
            0
        } else {
            snap.position_ms
        };

        {
            let mut ps = self.legacy_playback_state.write().await;
            ps.track_id = snap.track_id;
            ps.position_ms = position_ms;
            ps.playing = is_playing;
            ps.volume = (snap.volume as f64) / 100.0;
            ps.shuffle = snap.shuffle;
            ps.repeat = snap.repeat.as_str().to_string();
            ps.updated_at = Utc::now();
        }

        if let Ok(Some(mut sess)) = michi_db::get_latest_playback_session(&self.db).await {
            sess.current_track_id = snap.track_id;
            sess.position_ms = position_ms;
            sess.playing = is_playing;
            sess.volume = (snap.volume as f64) / 100.0;
            sess.shuffle = snap.shuffle;
            sess.repeat_mode = snap.repeat.as_str().to_string();
            self.persist_if_changed(&sess).await;
        }
    }

    /// Best-effort final state flush on shutdown.
    pub async fn flush_shutdown(&self) {
        if let Ok(snap) = self.engine.snapshot().await {
            if let Ok(Some(mut sess)) = michi_db::get_latest_playback_session(&self.db).await {
                sess.current_track_id = snap.track_id;
                sess.position_ms = snap.position_ms;
                sess.playing = false; // Server is terminating
                sess.volume = (snap.volume as f64) / 100.0;
                sess.shuffle = snap.shuffle;
                sess.repeat_mode = snap.repeat.as_str().to_string();
                if let Err(e) = michi_db::update_playback_session(&self.db, &sess).await {
                    self.record_failure(&format!("flush_shutdown failed: {e}"))
                        .await;
                } else {
                    self.record_success().await;
                    info!("playback projection flushed successfully on shutdown");
                }
            }
        }
    }

    async fn persist_if_changed(&self, sess: &michi_core::PlaybackSessionDb) {
        let projection = PersistentPlaybackProjection {
            current_track_id: sess.current_track_id,
            current_index: sess.current_index,
            position_ms: sess.position_ms,
            playing: sess.playing,
            volume: sess.volume,
            shuffle: sess.shuffle,
            repeat_mode: sess.repeat_mode.clone(),
        };

        let should_write = {
            let last = self.last_projection.read().await;
            last.as_ref() != Some(&projection)
        };

        if should_write {
            match michi_db::update_playback_session(&self.db, sess).await {
                Ok(_) => {
                    *self.last_projection.write().await = Some(projection);
                    self.record_success().await;
                }
                Err(e) => {
                    self.record_failure(&format!("update_playback_session failed: {e}"))
                        .await;
                }
            }
        }
    }
}
