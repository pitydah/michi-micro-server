use axum::{http::StatusCode, Json};
use sqlx::SqlitePool;
use std::path::PathBuf;
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

/// All-or-nothing validation of a sequence of track IDs against database library.
pub async fn validate_and_load_tracks(
    pool: &SqlitePool,
    track_ids: &[Uuid],
    _music_paths: &[PathBuf],
) -> Result<Vec<michi_core::Track>, (StatusCode, Json<serde_json::Value>)> {
    let mut tracks = Vec::with_capacity(track_ids.len());
    let mut unknown_tracks = Vec::new();

    for tid in track_ids {
        match michi_db::get_track(pool, tid).await {
            Ok(Some(track)) => {
                tracks.push(track);
            }
            Ok(None) => {
                unknown_tracks.push(*tid);
            }
            Err(e) => {
                return Err(v1_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "DATABASE_ERROR",
                    &e.to_string(),
                ));
            }
        }
    }

    if !unknown_tracks.is_empty() {
        return Err(v1_error(
            StatusCode::NOT_FOUND,
            "TRACK_NOT_FOUND",
            &format!("tracks not in library: {unknown_tracks:?}"),
        ));
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
        if let Ok(id) = Uuid::parse_str(&id_str) {
            return Ok(id);
        }
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

    let mut runtime_tracks = Vec::new();
    for (tid_str, _) in &items_rows {
        if let Ok(tid) = Uuid::parse_str(tid_str) {
            if let Ok(Some(track)) = michi_db::get_track(&state.db, &tid).await {
                runtime_tracks.push(track);
            }
        }
    }

    if !runtime_tracks.is_empty() {
        let cur_idx = if let Some(ref cur_tid) = current_track_id {
            runtime_tracks
                .iter()
                .position(|t| t.id == *cur_tid)
                .unwrap_or(0)
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
                    e.error_code(),
                    &e.to_string(),
                )
            })?;
    }

    Ok(())
}
