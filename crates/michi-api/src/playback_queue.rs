use axum::{http::StatusCode, Json};
use michi_playback::TrackResolver;
use sqlx::SqlitePool;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::RwLock;
use uuid::Uuid;

use crate::AppState;

fn v1_error(
    status: StatusCode,
    code: &str,
    message: &str,
) -> (StatusCode, Json<serde_json::Value>) {
    (
        status,
        Json(serde_json::json!({
            "error": { "code": code, "message": message, "details": {} }
        })),
    )
}

/// Single authoritative mutation coordinator for the active playback queue.
///
/// Guarantees:
/// - All queue-mutating handlers (`create`, `add/append`, `jump`, `transfer`,
///   `reorder`, `clear`, `delete queue`, `delete item`, `save`, `restore`)
///   serialize via one async mutex without holding SQLite write transactions
///   open across audio engine operations.
/// - Tracks a monotonic `queue_version` so stale snapshots never overwrite newer
///   mutations.
/// - Tracks explicit `degraded` state if an engine sync fails AND compensating
///   SQLite rollback also fails; rejects new mutations fail-closed with
///   `503 QUEUE_DEGRADED` until `reconcile_active_queue` verifies coherence.
#[derive(Debug, Default)]
pub struct ActiveQueueCoordinator {
    mutation_lock: tokio::sync::Mutex<()>,
    version: AtomicU64,
    degraded: AtomicBool,
    degraded_reason: RwLock<Option<String>>,
}

impl ActiveQueueCoordinator {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn lock(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.mutation_lock.lock().await
    }

    pub fn version(&self) -> u64 {
        self.version.load(Ordering::SeqCst)
    }

    pub fn bump_version(&self) -> u64 {
        self.version.fetch_add(1, Ordering::SeqCst) + 1
    }

    pub fn is_degraded(&self) -> bool {
        self.degraded.load(Ordering::SeqCst)
    }

    pub fn degraded_reason(&self) -> Option<String> {
        self.degraded_reason.read().ok().and_then(|g| g.clone())
    }

    /// Rejects new mutations fail-closed whenever SQLite and PlaybackEngine have
    /// entered an unrecovered degraded state.
    pub fn ensure_coherent(&self) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
        if self.is_degraded() {
            let reason = self
                .degraded_reason()
                .unwrap_or_else(|| "queue storage and playback engine diverged".to_string());
            return Err(v1_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "QUEUE_DEGRADED",
                &format!("active queue is in degraded state until reconciled: {reason}"),
            ));
        }
        Ok(())
    }

    pub fn mark_degraded(&self, reason: impl Into<String>) {
        let msg = reason.into();
        if let Ok(mut guard) = self.degraded_reason.write() {
            *guard = Some(msg);
        }
        self.degraded.store(true, Ordering::SeqCst);
    }

    pub fn clear_degraded(&self) {
        self.degraded.store(false, Ordering::SeqCst);
        if let Ok(mut guard) = self.degraded_reason.write() {
            *guard = None;
        }
    }
}

/// Snapshot of a persisted `queue_items` row used for verified compensating
/// rollback if PlaybackEngine synchronization fails after SQLite commit.
#[derive(Debug, Clone)]
pub struct QueueItemBackup {
    pub id: String,
    pub track_id: String,
    pub position: i64,
    pub added_at: String,
}

/// Read the current ordered items of a queue prior to mutation so a verified
/// compensating rollback can restore exact row IDs, positions, and timestamps.
pub async fn snapshot_queue_items(
    pool: &SqlitePool,
    queue_id: &Uuid,
) -> Result<Vec<QueueItemBackup>, (StatusCode, Json<serde_json::Value>)> {
    let rows = sqlx::query_as::<_, (String, String, i64, String)>(
        "SELECT id, track_id, position, added_at FROM queue_items WHERE queue_id = ? ORDER BY position ASC",
    )
    .bind(queue_id.to_string())
    .fetch_all(pool)
    .await
    .map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DATABASE_ERROR",
            &e.to_string(),
        )
    })?;

    Ok(rows
        .into_iter()
        .map(|(id, track_id, position, added_at)| QueueItemBackup {
            id,
            track_id,
            position,
            added_at,
        })
        .collect())
}

/// Verified compensating rollback for `queue_items` after a PlaybackEngine sync
/// error. Every SQLite statement result is checked; if rollback fails, the
/// coordinator enters fail-closed `degraded` state (`503 QUEUE_DEGRADED`) until
/// `reconcile_active_queue` succeeds.
pub async fn restore_queue_items_or_degrade(
    state: &AppState,
    queue_id: &Uuid,
    previous_items: &[QueueItemBackup],
    ensure_queue_name: Option<&str>,
    engine_error: &str,
) -> (StatusCode, Json<serde_json::Value>) {
    let rollback_result: Result<(), sqlx::Error> = async {
        let mut rb_tx = state.db.begin_with("BEGIN IMMEDIATE").await?;
        let now = chrono::Utc::now().to_rfc3339();
        if let Some(q_name) = ensure_queue_name {
            sqlx::query(
                "INSERT OR IGNORE INTO queues (id, name, created_at, updated_at) VALUES (?, ?, ?, ?)",
            )
            .bind(queue_id.to_string())
            .bind(q_name)
            .bind(&now)
            .bind(&now)
            .execute(&mut *rb_tx)
            .await?;
        }
        sqlx::query("DELETE FROM queue_items WHERE queue_id = ?")
            .bind(queue_id.to_string())
            .execute(&mut *rb_tx)
            .await?;
        for item in previous_items {
            sqlx::query(
                "INSERT INTO queue_items (id, queue_id, track_id, position, added_at) VALUES (?, ?, ?, ?, ?)",
            )
            .bind(&item.id)
            .bind(queue_id.to_string())
            .bind(&item.track_id)
            .bind(item.position)
            .bind(&item.added_at)
            .execute(&mut *rb_tx)
            .await?;
        }
        sqlx::query("UPDATE queues SET updated_at = ? WHERE id = ?")
            .bind(&now)
            .bind(queue_id.to_string())
            .execute(&mut *rb_tx)
            .await?;
        rb_tx.commit().await?;
        Ok(())
    }
    .await;

    match rollback_result {
        Ok(()) => v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "RUNTIME_SYNC_FAILED",
            engine_error,
        ),
        Err(rb_err) => {
            let reason = format!(
                "engine sync failed ({engine_error}) and compensating SQLite rollback failed ({rb_err})"
            );
            tracing::error!("ActiveQueueCoordinator entering DEGRADED state: {reason}");
            state.queue_coordinator.mark_degraded(reason.clone());
            v1_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "QUEUE_DEGRADED",
                &format!("active queue entered degraded state until reconciled: {reason}"),
            )
        }
    }
}

/// Explicitly reconciles SQLite active queue state into PlaybackEngine and clears
/// degraded status ONLY after synchronization succeeds.
pub async fn reconcile_active_queue(
    state: &AppState,
) -> Result<u64, (StatusCode, Json<serde_json::Value>)> {
    let _guard = state.queue_coordinator.lock().await;
    let active_queue_id = get_or_create_active_queue(&state.db).await.map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DATABASE_ERROR",
            &e.to_string(),
        )
    })?;
    let cur_track_id = state
        .playback_engine
        .snapshot()
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "ENGINE_UNAVAILABLE",
                &e.to_string(),
            )
        })?
        .track_id;
    sync_active_queue_to_engine(state, &active_queue_id, cur_track_id).await?;
    state.queue_coordinator.clear_degraded();
    Ok(state.queue_coordinator.bump_version())
}

/// All-or-nothing validation of a sequence of track IDs using SqliteTrackResolver
pub async fn validate_and_load_tracks(
    pool: &SqlitePool,
    track_ids: &[Uuid],
    music_paths: &[PathBuf],
) -> Result<Vec<michi_core::Track>, (StatusCode, Json<serde_json::Value>)> {
    let resolver = michi_playback::SqliteTrackResolver::new(pool.clone(), music_paths.to_vec());
    let mut tracks = Vec::with_capacity(track_ids.len());

    for tid in track_ids {
        match resolver.get_track(*tid).await {
            Ok(track) => tracks.push(track),
            Err(e) => {
                let status = match e {
                    michi_playback::PlaybackError::TrackNotFound(_)
                    | michi_playback::PlaybackError::TrackFileMissing(_)
                    | michi_playback::PlaybackError::TrackOutsideLibrary(_) => {
                        StatusCode::NOT_FOUND
                    }
                    _ => StatusCode::BAD_REQUEST,
                };
                return Err(v1_error(
                    status,
                    e.error_code(),
                    &format!("track {tid} validation failed: {e}"),
                ));
            }
        }
    }

    Ok(tracks)
}

/// Helper to get or create the single canonical active queue
pub async fn get_or_create_active_queue(pool: &SqlitePool) -> Result<Uuid, sqlx::Error> {
    let row = sqlx::query_as::<_, (String,)>(
        "SELECT id FROM queues WHERE name = 'active-queue' ORDER BY datetime(created_at) DESC LIMIT 1"
    )
    .fetch_optional(pool)
    .await?;

    if let Some((id_str,)) = row {
        let id = Uuid::parse_str(&id_str).map_err(|e| {
            sqlx::Error::Decode(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("corrupt queue UUID '{id_str}': {e}"),
            )))
        })?;
        return Ok(id);
    }

    let new_id = Uuid::new_v4();
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO queues (id, name, created_at, updated_at) VALUES (?, 'active-queue', ?, ?)",
    )
    .bind(new_id.to_string())
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await?;

    Ok(new_id)
}

/// Synchronize the canonical active queue from SQLite to the autonomous PlaybackEngine
pub async fn sync_active_queue_to_engine(
    state: &AppState,
    active_queue_id: &Uuid,
    current_track_id: Option<Uuid>,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    let items_rows = sqlx::query_as::<_, (String, i64)>(
        "SELECT track_id, position FROM queue_items WHERE queue_id = ? ORDER BY position ASC",
    )
    .bind(active_queue_id.to_string())
    .fetch_all(&state.db)
    .await
    .map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DATABASE_ERROR",
            &e.to_string(),
        )
    })?;

    if items_rows.is_empty() {
        state
            .playback_engine
            .set_queue(Vec::new(), 0, None)
            .await
            .map_err(|e| {
                v1_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "RUNTIME_SYNC_FAILED",
                    &e.to_string(),
                )
            })?;
        return Ok(());
    }

    let resolver = michi_playback::SqliteTrackResolver::new(
        state.db.clone(),
        state.config.music_paths.clone(),
    );

    let mut runtime_tracks = Vec::with_capacity(items_rows.len());
    for (tid_str, _) in &items_rows {
        let tid = Uuid::parse_str(tid_str).map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "RUNTIME_SYNC_FAILED",
                &format!("invalid track UUID {tid_str} in queue: {e}"),
            )
        })?;

        let track = resolver.get_track(tid).await.map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "RUNTIME_SYNC_FAILED",
                &format!("failed to resolve queue track {tid}: {e}"),
            )
        })?;

        runtime_tracks.push(track);
    }

    let cur_idx = if let Some(ref cur_tid) = current_track_id {
        let preferred_idx = state
            .playback_engine
            .snapshot()
            .await
            .ok()
            .and_then(|s| s.queue_index)
            .filter(|&idx| runtime_tracks.get(idx).map(|t| t.id) == Some(*cur_tid));
        preferred_idx.unwrap_or_else(|| {
            runtime_tracks
                .iter()
                .position(|t| t.id == *cur_tid)
                .unwrap_or(0)
        })
    } else {
        0
    };

    state
        .playback_engine
        .set_queue(runtime_tracks, cur_idx, current_track_id)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "RUNTIME_SYNC_FAILED",
                &e.to_string(),
            )
        })?;

    Ok(())
}
