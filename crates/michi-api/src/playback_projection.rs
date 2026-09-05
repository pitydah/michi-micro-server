use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use michi_playback::{
    CommandOrigin, EngineEvent, EngineSnapshot, PlaybackEngineHandle, PlaybackLifecycle,
    TrackedEngineEvent,
};
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

/// Cursor tracking latest known epoch, boot_id, and sequence for a remote peer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeerCursor {
    pub epoch: u64,
    pub boot_id: Option<Uuid>,
    pub sequence: u64,
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
    server_epoch: Arc<AtomicU64>,
    boot_id: Uuid,
    local_sequence: Arc<AtomicU64>,
    local_lamport: Arc<AtomicU64>,
    peer_vectors: Arc<RwLock<HashMap<String, PeerCursor>>>,
    processed_events: Arc<RwLock<HashSet<Uuid>>>,
    health: Arc<RwLock<PlaybackProjectionHealth>>,
    last_projection: Arc<RwLock<Option<PersistentPlaybackProjection>>>,
}

impl PlaybackProjectionCoordinator {
    pub async fn new_with_db_epoch(
        db: SqlitePool,
        legacy_playback_state: Arc<RwLock<michi_sync::PlaybackState>>,
        engine: PlaybackEngineHandle,
        sync_tx: tokio::sync::broadcast::Sender<michi_sync::SyncMessage>,
        server_id: String,
    ) -> (Self, Arc<RwLock<PlaybackProjectionHealth>>) {
        let boot_id = Uuid::new_v4();
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let epoch = match michi_db::get_server_config(&db, "sync_server_epoch").await {
            Ok(Some(val)) => {
                let last_ep = val.parse::<u64>().unwrap_or(0);
                let next_ep = last_ep.saturating_add(1).max(now_secs);
                let _ = michi_db::set_server_config(&db, "sync_server_epoch", &next_ep.to_string())
                    .await;
                next_ep
            }
            _ => {
                let _ =
                    michi_db::set_server_config(&db, "sync_server_epoch", &now_secs.to_string())
                        .await;
                now_secs
            }
        };

        let lamport = match michi_db::get_server_config(&db, "sync_server_lamport").await {
            Ok(Some(val)) => {
                let last_lamp = val.parse::<u64>().unwrap_or(0);
                let next_lamp = last_lamp.saturating_add(10_000);
                let _ =
                    michi_db::set_server_config(&db, "sync_server_lamport", &next_lamp.to_string())
                        .await;
                next_lamp
            }
            _ => {
                let _ = michi_db::set_server_config(&db, "sync_server_lamport", "10000").await;
                10_000
            }
        };

        Self::new_with_epoch_boot_and_lamport(
            db,
            legacy_playback_state,
            engine,
            sync_tx,
            server_id,
            epoch,
            boot_id,
            lamport,
        )
    }

    pub fn new(
        db: SqlitePool,
        legacy_playback_state: Arc<RwLock<michi_sync::PlaybackState>>,
        engine: PlaybackEngineHandle,
        sync_tx: tokio::sync::broadcast::Sender<michi_sync::SyncMessage>,
        server_id: String,
    ) -> (Self, Arc<RwLock<PlaybackProjectionHealth>>) {
        let boot_id = Uuid::new_v4();
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let (coord, health) = Self::new_with_epoch_and_boot(
            db.clone(),
            legacy_playback_state,
            engine,
            sync_tx,
            server_id,
            now_secs,
            boot_id,
        );

        let coord_clone = coord.clone();
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                coord_clone.initialize_db_state().await;
            });
        }

        (coord, health)
    }

    pub async fn initialize_db_state(&self) {
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        // 1. Monotonically advance and persist server_epoch
        if let Ok(val_opt) = michi_db::get_server_config(&self.db, "sync_server_epoch").await {
            let next_ep = match val_opt {
                Some(val) => {
                    let last_ep = val.parse::<u64>().unwrap_or(0);
                    last_ep.saturating_add(1).max(now_secs)
                }
                None => now_secs,
            };
            let _ =
                michi_db::set_server_config(&self.db, "sync_server_epoch", &next_ep.to_string())
                    .await;
            self.server_epoch.store(next_ep, Ordering::SeqCst);
        }

        // 2. Monotonically reserve block and persist sync_server_lamport
        let current_lamp = self.local_lamport.load(Ordering::SeqCst);
        let next_lamp = match michi_db::get_server_config(&self.db, "sync_server_lamport").await {
            Ok(Some(val)) => {
                let last_lamp = val.parse::<u64>().unwrap_or(0);
                last_lamp.max(current_lamp).saturating_add(10_000)
            }
            _ => current_lamp.saturating_add(10_000),
        };
        let _ =
            michi_db::set_server_config(&self.db, "sync_server_lamport", &next_lamp.to_string())
                .await;
        self.local_lamport.store(next_lamp, Ordering::SeqCst);
    }

    pub fn new_with_epoch(
        db: SqlitePool,
        legacy_playback_state: Arc<RwLock<michi_sync::PlaybackState>>,
        engine: PlaybackEngineHandle,
        sync_tx: tokio::sync::broadcast::Sender<michi_sync::SyncMessage>,
        server_id: String,
        server_epoch: u64,
    ) -> (Self, Arc<RwLock<PlaybackProjectionHealth>>) {
        let boot_id = Uuid::new_v4();
        Self::new_with_epoch_and_boot(
            db,
            legacy_playback_state,
            engine,
            sync_tx,
            server_id,
            server_epoch,
            boot_id,
        )
    }

    pub fn new_with_epoch_and_boot(
        db: SqlitePool,
        legacy_playback_state: Arc<RwLock<michi_sync::PlaybackState>>,
        engine: PlaybackEngineHandle,
        sync_tx: tokio::sync::broadcast::Sender<michi_sync::SyncMessage>,
        server_id: String,
        server_epoch: u64,
        boot_id: Uuid,
    ) -> (Self, Arc<RwLock<PlaybackProjectionHealth>>) {
        Self::new_with_epoch_boot_and_lamport(
            db,
            legacy_playback_state,
            engine,
            sync_tx,
            server_id,
            server_epoch,
            boot_id,
            0,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_with_epoch_boot_and_lamport(
        db: SqlitePool,
        legacy_playback_state: Arc<RwLock<michi_sync::PlaybackState>>,
        engine: PlaybackEngineHandle,
        sync_tx: tokio::sync::broadcast::Sender<michi_sync::SyncMessage>,
        server_id: String,
        server_epoch: u64,
        boot_id: Uuid,
        initial_lamport: u64,
    ) -> (Self, Arc<RwLock<PlaybackProjectionHealth>>) {
        let health = Arc::new(RwLock::new(PlaybackProjectionHealth::default()));
        let coordinator = Self {
            db,
            legacy_playback_state,
            engine,
            sync_tx,
            server_id,
            server_epoch: Arc::new(AtomicU64::new(server_epoch)),
            boot_id,
            local_sequence: Arc::new(AtomicU64::new(0)),
            local_lamport: Arc::new(AtomicU64::new(initial_lamport)),
            peer_vectors: Arc::new(RwLock::new(HashMap::new())),
            processed_events: Arc::new(RwLock::new(HashSet::new())),
            health: health.clone(),
            last_projection: Arc::new(RwLock::new(None)),
        };
        (coordinator, health)
    }

    pub fn server_epoch(&self) -> u64 {
        self.server_epoch.load(Ordering::SeqCst)
    }

    pub fn boot_id(&self) -> Uuid {
        self.boot_id
    }

    pub fn next_local_sequence(&self) -> u64 {
        self.local_sequence.fetch_add(1, Ordering::SeqCst) + 1
    }

    pub fn next_lamport(&self) -> u64 {
        let mut current = self.local_lamport.load(Ordering::SeqCst);
        loop {
            let target = current.saturating_add(1);
            match self.local_lamport.compare_exchange_weak(
                current,
                target,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => return target,
                Err(actual) => current = actual,
            }
        }
    }

    pub fn observe_lamport(&self, remote: u64) -> u64 {
        // Prevent integer overflow or wrapping from malicious or corrupted peer input
        if remote >= u64::MAX - 100_000 {
            warn!(
                remote,
                "sync: rejecting near-overflow or corrupt lamport clock"
            );
            return self.local_lamport.load(Ordering::SeqCst);
        }
        let mut current = self.local_lamport.load(Ordering::SeqCst);
        loop {
            let target = current.max(remote).saturating_add(1);
            match self.local_lamport.compare_exchange_weak(
                current,
                target,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => return target,
                Err(actual) => current = actual,
            }
        }
    }

    pub fn current_lamport(&self) -> u64 {
        self.local_lamport.load(Ordering::SeqCst)
    }

    pub async fn is_stale_peer_state(
        &self,
        peer_id: &str,
        epoch: Option<u64>,
        boot_id: Option<Uuid>,
        sequence: u64,
    ) -> bool {
        let ep = epoch.unwrap_or(0);
        let vectors = self.peer_vectors.read().await;
        if let Some(cursor) = vectors.get(peer_id) {
            if ep < cursor.epoch {
                return true;
            }
            if ep > cursor.epoch {
                return false;
            }
            // Same epoch: check if boot_id indicates a new instance reboot
            if boot_id.is_some() && cursor.boot_id.is_some() && boot_id != cursor.boot_id {
                return false;
            }
            if sequence <= cursor.sequence {
                return true;
            }
        }
        false
    }

    pub async fn record_peer_state(
        &self,
        peer_id: &str,
        epoch: Option<u64>,
        boot_id: Option<Uuid>,
        sequence: u64,
    ) {
        let ep = epoch.unwrap_or(0);
        let mut vectors = self.peer_vectors.write().await;
        vectors.insert(
            peer_id.to_string(),
            PeerCursor {
                epoch: ep,
                boot_id,
                sequence,
            },
        );
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

    /// Handles a single TrackedEngineEvent paired with a fresh EngineSnapshot.
    pub async fn handle_event(&self, event: &TrackedEngineEvent, snap: &EngineSnapshot) {
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

        // Determine provenance directly attached to event without timing fallbacks
        let (device_id, event_id, sequence, epoch, boot_id, lamport, is_local) = match &event.origin
        {
            CommandOrigin::Remote {
                origin_device_id,
                event_id,
                sequence,
                epoch,
                boot_id,
                lamport,
            } => (
                Some(origin_device_id.clone()),
                Some(*event_id),
                *sequence,
                *epoch,
                *boot_id,
                *lamport,
                false,
            ),
            CommandOrigin::Local => {
                let seq = self.next_local_sequence();
                let eid = Uuid::new_v4();
                let lamp = self.next_lamport();
                (
                    Some(self.server_id.clone()),
                    Some(eid),
                    Some(seq),
                    Some(self.server_epoch()),
                    Some(self.boot_id),
                    Some(lamp),
                    true,
                )
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
            ps.epoch = epoch;
            ps.boot_id = boot_id;
            ps.lamport = lamport;
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
        match &event.event {
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

        let (device_id, event_id, sequence, epoch, boot_id, lamport, is_local) =
            match &snap.timeline_origin {
                CommandOrigin::Remote {
                    origin_device_id,
                    event_id,
                    sequence,
                    epoch,
                    boot_id,
                    lamport,
                } => (
                    Some(origin_device_id.clone()),
                    Some(*event_id),
                    *sequence,
                    *epoch,
                    *boot_id,
                    *lamport,
                    false,
                ),
                CommandOrigin::Local => {
                    let seq = self.next_local_sequence();
                    let eid = Uuid::new_v4();
                    let lamp = self.next_lamport();
                    (
                        Some(self.server_id.clone()),
                        Some(eid),
                        Some(seq),
                        Some(self.server_epoch()),
                        Some(self.boot_id),
                        Some(lamp),
                        true,
                    )
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
            ps.updated_at = snap.updated_at;
            ps.device_id = device_id;
            ps.event_id = event_id;
            ps.sequence = sequence;
            ps.epoch = epoch;
            ps.boot_id = boot_id;
            ps.lamport = lamport;
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

        let cur_lamport = self.current_lamport();
        let _ =
            michi_db::set_server_config(&self.db, "sync_server_lamport", &cur_lamport.to_string())
                .await;
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
