use axum::{http::StatusCode, Json};
use michi_playback::TrackResolver;
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
                "RUNTIME_SYNC_FAILED",
                &e.to_string(),
            )
        })?;

    Ok(())
}
