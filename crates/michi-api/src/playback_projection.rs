use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use michi_playback::{EngineEvent, EngineSnapshot, PlaybackEngineHandle, PlaybackLifecycle};
use serde::Serialize;
use sqlx::SqlitePool;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};
use uuid::Uuid;

/// Category of playback projection failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionFailureKind {
    EngineSnapshot,
    SessionLoad,
    SessionPersist,
    LagReconciliation,
    ShutdownFlush,
}

/// Observability health model for playback event projection.
#[derive(Debug, Clone, Serialize)]
pub struct PlaybackProjectionHealth {
    pub healthy: bool,
    pub consecutive_failures: u64,
    pub last_success_at: Option<DateTime<Utc>>,
    pub last_error_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub last_error_kind: Option<ProjectionFailureKind>,
    pub failures_total: u64,
    pub lag_events_total: u64,
    pub lag_recoveries_total: u64,
    pub lag_recovery_failures_total: u64,
}

impl Default for PlaybackProjectionHealth {
    fn default() -> Self {
        Self {
            healthy: true,
            consecutive_failures: 0,
            last_success_at: None,
            last_error_at: None,
            last_error: None,
            last_error_kind: None,
            failures_total: 0,
            lag_events_total: 0,
            lag_recoveries_total: 0,
            lag_recovery_failures_total: 0,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReconcileOutcome {
    NoPersistentSessionNeeded,
    AlreadyConverged,
    Persisted,
}

#[derive(Debug)]
pub enum ProjectionError {
    SessionLoad(String),
    SessionPersist(String),
}

impl std::fmt::Display for ProjectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SessionLoad(msg) => write!(f, "Database session load error: {msg}"),
            Self::SessionPersist(msg) => write!(f, "Database session persist error: {msg}"),
        }
    }
}

impl std::error::Error for ProjectionError {}

/// Remote provenance context tracking when an engine event was caused by applying a remote state.
#[derive(Debug, Clone)]
pub struct RemoteProvenanceContext {
    pub origin_device_id: String,
    pub event_id: Uuid,
    pub sequence: Option<u64>,
    pub expires_at: tokio::time::Instant,
}

/// Cohesive coordinator responsible for projecting PlaybackEngine runtime events
/// into authoritative in-memory state and persistent SQLite PlaybackSession.
#[derive(Debug, Clone)]
pub struct PlaybackProjectionCoordinator {
    db: SqlitePool,
    legacy_playback_state: Arc<RwLock<michi_sync::PlaybackState>>,
    engine: PlaybackEngineHandle,
    sync_tx: tokio::sync::broadcast::Sender<michi_sync::SyncMessage>,
    server_id: String,
    local_sequence: Arc<AtomicU64>,
    active_remote_context: Arc<RwLock<Option<RemoteProvenanceContext>>>,
    processed_events: Arc<RwLock<HashSet<Uuid>>>,
    health: Arc<RwLock<PlaybackProjectionHealth>>,
    last_projection: Arc<RwLock<Option<PersistentPlaybackProjection>>>,
}

impl PlaybackProjectionCoordinator {
    pub fn new(
        db: SqlitePool,
        legacy_playback_state: Arc<RwLock<michi_sync::PlaybackState>>,
        engine: PlaybackEngineHandle,
        sync_tx: tokio::sync::broadcast::Sender<michi_sync::SyncMessage>,
        server_id: String,
    ) -> (Self, Arc<RwLock<PlaybackProjectionHealth>>) {
        let health = Arc::new(RwLock::new(PlaybackProjectionHealth::default()));
        let coordinator = Self {
            db,
            legacy_playback_state,
            engine,
            sync_tx,
            server_id,
            local_sequence: Arc::new(AtomicU64::new(0)),
            active_remote_context: Arc::new(RwLock::new(None)),
            processed_events: Arc::new(RwLock::new(HashSet::new())),
            health: health.clone(),
            last_projection: Arc::new(RwLock::new(None)),
        };
        (coordinator, health)
    }

    /// Sets the active remote provenance context during remote state application for a duration.
    pub async fn set_remote_context(
        &self,
        origin_device_id: String,
        event_id: Uuid,
        sequence: Option<u64>,
        duration: std::time::Duration,
    ) {
        let mut ctx = self.active_remote_context.write().await;
        *ctx = Some(RemoteProvenanceContext {
            origin_device_id,
            event_id,
            sequence,
            expires_at: tokio::time::Instant::now() + duration,
        });
    }

    /// Clears the active remote provenance context immediately.
    pub async fn clear_remote_context(&self) {
        let mut ctx = self.active_remote_context.write().await;
        *ctx = None;
    }

    /// Returns the active remote provenance context if not expired.
    pub async fn get_active_remote_context(&self) -> Option<RemoteProvenanceContext> {
        let ctx = self.active_remote_context.read().await;
        match &*ctx {
            Some(c) if tokio::time::Instant::now() <= c.expires_at => Some(c.clone()),
            _ => None,
        }
    }

    /// Checks whether an event_id was already processed (deduplication).
    pub async fn is_event_processed(&self, event_id: &Uuid) -> bool {
        let events = self.processed_events.read().await;
        events.contains(event_id)
    }

    /// Records an event_id as processed in bounded memory.
    pub async fn record_processed_event(&self, event_id: Uuid) {
        let mut events = self.processed_events.write().await;
        if events.len() > 10000 {
            events.clear();
        }
        events.insert(event_id);
    }

    async fn record_success(&self) {
        let mut h = self.health.write().await;
        h.healthy = true;
        h.consecutive_failures = 0;
        h.last_success_at = Some(Utc::now());
        h.last_error = None;
        h.last_error_kind = None;
    }

    async fn record_failure(&self, kind: ProjectionFailureKind, err_msg: &str) {
        let mut h = self.health.write().await;
        h.healthy = false;
        h.consecutive_failures = h.consecutive_failures.saturating_add(1);
        h.last_error_at = Some(Utc::now());
        h.last_error = Some(err_msg.to_string());
        h.last_error_kind = Some(kind);
        h.failures_total = h.failures_total.saturating_add(1);
        warn!(kind = ?kind, error = %err_msg, "playback projection coordinator failure recorded");
    }

    /// Spawns the background projection task listening to engine events.
    pub fn spawn(self, shutdown: CancellationToken) -> tokio::task::JoinHandle<()> {
        let mut event_rx = self.engine.subscribe_events();
        let coordinator = Arc::new(self);

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = shutdown.cancelled() => {
                        debug!("playback projection task exiting on shutdown cancellation");
                        break;
                    }
                    event_res = event_rx.recv() => {
                        match event_res {
                            Ok(event) => {
                                match coordinator.engine.snapshot().await {
                                    Ok(snap) => {
                                        coordinator.handle_event(&event, &snap).await;
                                    }
                                    Err(e) => {
                                        coordinator.record_failure(ProjectionFailureKind::EngineSnapshot, &format!("snapshot failed: {e}")).await;
                                    }
                                }
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(lagged)) => {
                                {
                                    let mut h = coordinator.health.write().await;
                                    h.lag_events_total = h.lag_events_total.saturating_add(1);
                                }
                                warn!(lagged, "playback projection task lagged by events; reconciling snapshot");
                                match coordinator.engine.snapshot().await {
                                    Ok(snap) => {
                                        match coordinator.reconcile_from_snapshot(&snap).await {
                                            Ok(_) => {
                                                let mut h = coordinator.health.write().await;
                                                h.lag_recoveries_total = h.lag_recoveries_total.saturating_add(1);
                                            }
                                            Err(_) => {
                                                let mut h = coordinator.health.write().await;
                                                h.lag_recovery_failures_total = h.lag_recovery_failures_total.saturating_add(1);
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        {
                                            let mut h = coordinator.health.write().await;
                                            h.lag_recovery_failures_total = h.lag_recovery_failures_total.saturating_add(1);
                                        }
                                        coordinator.record_failure(ProjectionFailureKind::LagReconciliation, &format!("reconciliation snapshot failed: {e}")).await;
                                    }
                                }
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                                break;
                            }
                        }
                    }
                }
            }
        })
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

        // Determine provenance: local origin vs remote application
        let remote_ctx = self.get_active_remote_context().await;
        let (device_id, event_id, sequence, is_local) = match remote_ctx {
            Some(ctx) => (
                Some(ctx.origin_device_id),
                Some(ctx.event_id),
                ctx.sequence,
                false,
            ),
            None => {
                let seq = self.local_sequence.fetch_add(1, Ordering::SeqCst) + 1;
                let eid = Uuid::new_v4();
                (Some(self.server_id.clone()), Some(eid), Some(seq), true)
            }
        };

        // 1. Sync legacy PlaybackState projection (RAM only)
        let out_state = {
            let mut ps = self.legacy_playback_state.write().await;
            ps.track_id = snap.track_id;
            ps.position_ms = position_ms;
            ps.playing = is_playing;
            ps.volume = (snap.volume as f64) / 100.0;
            ps.shuffle = snap.shuffle;
            ps.repeat = snap.repeat.as_str().to_string();
            ps.updated_at = Utc::now();
            ps.device_id = device_id;
            ps.event_id = event_id;
            ps.sequence = sequence;
            ps.clone()
        };

        // Echo Suppression: ONLY broadcast to sync bus if this event was originated LOCALLY on this server.
        // If it was applied from a remote peer, do NOT re-broadcast back to sync_tx to prevent feedback loops.
        if is_local {
            let _ = self.sync_tx.send(out_state.into());
        }

        // 2. Fetch existing or default session from SQLite
        let mut sess = match michi_db::get_or_create_latest_playback_session(&self.db).await {
            Ok(s) => s,
            Err(e) => {
                self.record_failure(
                    ProjectionFailureKind::SessionLoad,
                    &format!("get_or_create_latest_playback_session failed: {e}"),
                )
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
        let _ = self.persist_if_changed(&sess).await;
    }

    /// Reconciles state from an engine snapshot (e.g. after event lag or startup).
    pub async fn reconcile_from_snapshot(
        &self,
        snap: &EngineSnapshot,
    ) -> Result<ReconcileOutcome, ProjectionError> {
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

        let remote_ctx = self.get_active_remote_context().await;
        let (device_id, event_id, sequence, is_local) = match remote_ctx {
            Some(ctx) => (
                Some(ctx.origin_device_id),
                Some(ctx.event_id),
                ctx.sequence,
                false,
            ),
            None => {
                let seq = self.local_sequence.fetch_add(1, Ordering::SeqCst) + 1;
                let eid = Uuid::new_v4();
                (Some(self.server_id.clone()), Some(eid), Some(seq), true)
            }
        };

        let out_state = {
            let mut ps = self.legacy_playback_state.write().await;
            ps.track_id = snap.track_id;
            ps.position_ms = position_ms;
            ps.playing = is_playing;
            ps.volume = (snap.volume as f64) / 100.0;
            ps.shuffle = snap.shuffle;
            ps.repeat = snap.repeat.as_str().to_string();
            ps.updated_at = Utc::now();
            ps.device_id = device_id;
            ps.event_id = event_id;
            ps.sequence = sequence;
            ps.clone()
        };
        if is_local {
            let _ = self.sync_tx.send(out_state.into());
        }

        // If the engine has no track and is stopped, do not overwrite a persisted session
        if snap.track_id.is_none() && !is_playing {
            return Ok(ReconcileOutcome::NoPersistentSessionNeeded);
        }

        match michi_db::get_or_create_latest_playback_session(&self.db).await {
            Ok(mut sess) => {
                sess.current_track_id = snap.track_id;
                sess.position_ms = position_ms;
                sess.playing = is_playing;
                sess.volume = (snap.volume as f64) / 100.0;
                sess.shuffle = snap.shuffle;
                sess.repeat_mode = snap.repeat.as_str().to_string();
                match self.persist_if_changed(&sess).await {
                    Ok(true) => Ok(ReconcileOutcome::Persisted),
                    Ok(false) => Ok(ReconcileOutcome::AlreadyConverged),
                    Err(e) => Err(e),
                }
            }
            Err(e) => {
                self.record_failure(
                    ProjectionFailureKind::SessionLoad,
                    &format!("reconciliation get_or_create_latest_playback_session failed: {e}"),
                )
                .await;
                Err(ProjectionError::SessionLoad(e.to_string()))
            }
        }
    }

    /// Best-effort final state flush on shutdown.
    pub async fn flush_shutdown(&self) {
        match self.engine.snapshot().await {
            Ok(snap) => {
                match michi_db::get_or_create_latest_playback_session(&self.db).await {
                    Ok(mut sess) => {
                        sess.current_track_id = snap.track_id;
                        sess.position_ms = snap.position_ms;
                        sess.playing = false; // Server is terminating
                        sess.volume = (snap.volume as f64) / 100.0;
                        sess.shuffle = snap.shuffle;
                        sess.repeat_mode = snap.repeat.as_str().to_string();
                        if let Err(e) = michi_db::update_playback_session(&self.db, &sess).await {
                            self.record_failure(
                                ProjectionFailureKind::ShutdownFlush,
                                &format!("flush_shutdown session update failed: {e}"),
                            )
                            .await;
                        } else {
                            self.record_success().await;
                            info!("playback projection flushed successfully on shutdown");
                        }
                    }
                    Err(e) => {
                        self.record_failure(
                            ProjectionFailureKind::ShutdownFlush,
                            &format!(
                                "flush_shutdown get_or_create_latest_playback_session failed: {e}"
                            ),
                        )
                        .await;
                    }
                }
            }
            Err(e) => {
                self.record_failure(
                    ProjectionFailureKind::ShutdownFlush,
                    &format!("flush_shutdown engine snapshot failed: {e}"),
                )
                .await;
            }
        }
    }

    async fn persist_if_changed(
        &self,
        sess: &michi_core::PlaybackSessionDb,
    ) -> Result<bool, ProjectionError> {
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
                    Ok(true)
                }
                Err(e) => {
                    self.record_failure(
                        ProjectionFailureKind::SessionPersist,
                        &format!("update_playback_session failed: {e}"),
                    )
                    .await;
                    Err(ProjectionError::SessionPersist(e.to_string()))
                }
            }
        } else {
            Ok(false)
        }
    }
}
