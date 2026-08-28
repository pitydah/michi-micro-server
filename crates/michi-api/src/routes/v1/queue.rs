use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::playback_queue::{
    get_or_create_active_queue, sync_active_queue_to_engine, validate_and_load_tracks,
};
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

pub async fn queue_handler(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let current = state.playback_state.read().await;
    let active_queue_id = get_or_create_active_queue(&state.db).await.map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DATABASE_ERROR",
            &e.to_string(),
        )
    })?;

    let items_rows = sqlx::query_as::<_, (String, String, i64)>(
        "SELECT id, track_id, position FROM queue_items WHERE queue_id = ? ORDER BY position ASC",
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

    let items = items_rows
        .into_iter()
        .map(|(id, track_id, pos)| {
            serde_json::json!({
                "id": id,
                "track_id": track_id,
                "position": pos,
            })
        })
        .collect::<Vec<_>>();

    let current_index = if let Some(ref cur_tid) = current.track_id {
        items
            .iter()
            .position(|it| it["track_id"] == cur_tid.to_string())
            .unwrap_or(0) as u32
    } else {
        0
    };

    Ok(Json(serde_json::json!({
        "queue_id": active_queue_id,
        "items_count": items.len(),
        "items": items,
        "current_track_id": current.track_id,
        "current_index": current_index,
        "position_ms": current.position_ms,
        "playing": current.playing,
        "volume": (current.volume * 100.0) as u32,
    })))
}

#[derive(Debug, Deserialize)]
pub struct QueueItemsBody {
    pub track_ids: Vec<Uuid>,
    pub name: Option<String>,
}

pub async fn queue_items_handler(
    State(state): State<AppState>,
    Json(body): Json<QueueItemsBody>,
) -> Result<Json<serde_json::value::Value>, (StatusCode, Json<serde_json::Value>)> {
    if body.track_ids.is_empty() {
        return Err(v1_error(
            StatusCode::BAD_REQUEST,
            "INVALID_REQUEST",
            "track_ids cannot be empty",
        ));
    }

    // P0-04 Functional Truth: All-or-nothing validation against DB, disk, and configured library roots
    let _validated =
        validate_and_load_tracks(&state.db, &body.track_ids, &state.config.music_paths).await?;

    let mut tx = state.db.begin().await.map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DATABASE_ERROR",
            &e.to_string(),
        )
    })?;

    let active_queue_id = {
        let row = sqlx::query_as::<_, (String,)>(
            "SELECT id FROM queues WHERE name = 'active-queue' ORDER BY datetime(created_at) DESC LIMIT 1"
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| v1_error(StatusCode::INTERNAL_SERVER_ERROR, "DATABASE_ERROR", &e.to_string()))?;

        if let Some((id_str,)) = row {
            Uuid::parse_str(&id_str).unwrap_or_else(|_| Uuid::new_v4())
        } else {
            let new_id = Uuid::new_v4();
            let now = chrono::Utc::now().to_rfc3339();
            sqlx::query("INSERT INTO queues (id, name, created_at, updated_at) VALUES (?, 'active-queue', ?, ?)")
                .bind(new_id.to_string())
                .bind(&now)
                .bind(&now)
                .execute(&mut *tx)
                .await
                .map_err(|e| v1_error(StatusCode::INTERNAL_SERVER_ERROR, "DATABASE_ERROR", &e.to_string()))?;
            new_id
        }
    };

    let max_pos: i64 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(position), -1) FROM queue_items WHERE queue_id = ?",
    )
    .bind(active_queue_id.to_string())
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DATABASE_ERROR",
            &e.to_string(),
        )
    })?;

    let now = chrono::Utc::now().to_rfc3339();
    let mut current_pos = max_pos + 1;
    for track_id in &body.track_ids {
        let item_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO queue_items (id, queue_id, track_id, position, added_at) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(item_id.to_string())
        .bind(active_queue_id.to_string())
        .bind(track_id.to_string())
        .bind(current_pos)
        .bind(&now)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;
        current_pos += 1;
    }

    // Update queue updated_at
    sqlx::query("UPDATE queues SET updated_at = ? WHERE id = ?")
        .bind(&now)
        .bind(active_queue_id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;

    tx.commit().await.map_err(|e| {
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
        .ok()
        .and_then(|s| s.track_id);

    sync_active_queue_to_engine(&state, &active_queue_id, cur_track_id).await?;

    let _ = state.tx.send(
        serde_json::json!({
            "type": "queue_updated",
            "queue_id": active_queue_id,
            "added_count": body.track_ids.len(),
        })
        .to_string(),
    );

    Ok(Json(serde_json::json!({
        "status": "ok",
        "queue_id": active_queue_id,
        "added": body.track_ids.len(),
    })))
}

#[derive(Debug, Deserialize)]
pub struct QueueJumpBody {
    pub queue_id: Option<Uuid>,
    pub index: u32,
}

pub async fn queue_jump_handler(
    State(state): State<AppState>,
    Json(body): Json<QueueJumpBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let active_queue_id = get_or_create_active_queue(&state.db).await.map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DATABASE_ERROR",
            &e.to_string(),
        )
    })?;

    let queue_id = body.queue_id.unwrap_or(active_queue_id);

    let items = sqlx::query_as::<_, (String, String, i64)>(
        "SELECT id, track_id, position FROM queue_items WHERE queue_id = ? ORDER BY position ASC",
    )
    .bind(queue_id.to_string())
    .fetch_all(&state.db)
    .await
    .map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DATABASE_ERROR",
            &e.to_string(),
        )
    })?;

    if items.is_empty() {
        return Err(v1_error(
            StatusCode::BAD_REQUEST,
            "QUEUE_EMPTY",
            "queue has no items",
        ));
    }

    if body.index as usize >= items.len() {
        return Err(v1_error(
            StatusCode::BAD_REQUEST,
            "INVALID_INDEX",
            &format!(
                "index {} out of bounds for queue length {}",
                body.index,
                items.len()
            ),
        ));
    }

    let target_track_id_str = &items[body.index as usize].1;
    let target_track_id = Uuid::parse_str(target_track_id_str).map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "INVALID_TRACK_ID",
            &e.to_string(),
        )
    })?;

    // P0-08: Guarantee the requested queue is loaded and validated before jumping
    let track_ids: Vec<Uuid> = items
        .iter()
        .map(|(_, tid_str, _)| {
            Uuid::parse_str(tid_str).map_err(|e| {
                v1_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "INVALID_TRACK_ID",
                    &e.to_string(),
                )
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    let validated_tracks =
        validate_and_load_tracks(&state.db, &track_ids, &state.config.music_paths).await?;

    if queue_id != active_queue_id {
        // V-P0-04: Promote to canonical active-queue in SQLite transaction
        let mut tx = state.db.begin().await.map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;
        sqlx::query("DELETE FROM queue_items WHERE queue_id = ?")
            .bind(active_queue_id.to_string())
            .execute(&mut *tx)
            .await
            .map_err(|e| {
                v1_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "DATABASE_ERROR",
                    &e.to_string(),
                )
            })?;
        let now = chrono::Utc::now().to_rfc3339();
        for (pos, tid) in track_ids.iter().enumerate() {
            let item_id = Uuid::new_v4();
            sqlx::query(
                "INSERT INTO queue_items (id, queue_id, track_id, position, added_at) VALUES (?, ?, ?, ?, ?)",
            )
            .bind(item_id.to_string())
            .bind(active_queue_id.to_string())
            .bind(tid.to_string())
            .bind(pos as i64)
            .bind(&now)
            .execute(&mut *tx)
            .await
            .map_err(|e| {
                v1_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "DATABASE_ERROR",
                    &e.to_string(),
                )
            })?;
        }
        sqlx::query("UPDATE queues SET updated_at = ? WHERE id = ?")
            .bind(&now)
            .bind(active_queue_id.to_string())
            .execute(&mut *tx)
            .await
            .map_err(|e| {
                v1_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "DATABASE_ERROR",
                    &e.to_string(),
                )
            })?;
        tx.commit().await.map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;
    }

    state
        .playback_engine
        .set_queue(validated_tracks, body.index as usize, Some(target_track_id))
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "RUNTIME_SYNC_FAILED",
                &e.to_string(),
            )
        })?;

    // Now jump to the requested index in the guaranteed synchronized queue
    state
        .playback_engine
        .jump_to_index(body.index as usize)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                e.error_code(),
                &e.to_string(),
            )
        })?;

    let snap = state.playback_engine.snapshot().await.map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "ENGINE_ERROR",
            &e.to_string(),
        )
    })?;

    {
        let mut current = state.playback_state.write().await;
        current.track_id = Some(target_track_id);
        current.position_ms = snap.position_ms;
        current.playing = snap.is_playing();
        current.updated_at = chrono::Utc::now();
    }

    let _ = state.tx.send(
        serde_json::json!({
            "type": "playback_state_changed",
            "track_id": target_track_id,
            "index": body.index,
        })
        .to_string(),
    );

    Ok(Json(serde_json::json!({
        "status": "ok",
        "index": body.index,
        "track_id": target_track_id,
    })))
}

// ── Queue Transfer (Player → Server) ───────────────────────

#[derive(Debug, Deserialize)]
pub struct QueueTransferBody {
    pub track_ids: Vec<Uuid>,
    pub current_index: u32,
    pub position_ms: u64,
    pub source: String,
}

pub async fn queue_transfer_handler(
    State(state): State<AppState>,
    Json(body): Json<QueueTransferBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if body.track_ids.is_empty() {
        return Err(v1_error(
            StatusCode::BAD_REQUEST,
            "INVALID_REQUEST",
            "track_ids must not be empty",
        ));
    }
    if body.current_index as usize >= body.track_ids.len() {
        return Err(v1_error(
            StatusCode::BAD_REQUEST,
            "INVALID_INDEX",
            "current_index exceeds track_ids length",
        ));
    }

    // P0-04 Functional Truth: All-or-nothing validation of every track in transferred queue
    let validated_tracks =
        match validate_and_load_tracks(&state.db, &body.track_ids, &state.config.music_paths).await
        {
            Ok(t) => t,
            Err((StatusCode::NOT_FOUND, _)) => {
                return Err(v1_error(
                    StatusCode::BAD_REQUEST,
                    "UNKNOWN_TRACKS",
                    "tracks not in library",
                ));
            }
            Err(e) => return Err(e),
        };

    let queue_id = get_or_create_active_queue(&state.db).await.map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DATABASE_ERROR",
            &e.to_string(),
        )
    })?;

    let mut tx = state.db.begin().await.map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DATABASE_ERROR",
            &e.to_string(),
        )
    })?;

    // Clear existing active queue items and replace with transferred items
    sqlx::query("DELETE FROM queue_items WHERE queue_id = ?")
        .bind(queue_id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;

    let now = chrono::Utc::now().to_rfc3339();
    for (pos, track_id) in body.track_ids.iter().enumerate() {
        let item_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO queue_items (id, queue_id, track_id, position, added_at) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(item_id.to_string())
        .bind(queue_id.to_string())
        .bind(track_id.to_string())
        .bind(pos as i64)
        .bind(&now)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;
    }

    tx.commit().await.map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DATABASE_ERROR",
            &e.to_string(),
        )
    })?;

    let session_id = michi_db::save_queue_state(
        &state.db,
        &body.source,
        &body.track_ids,
        body.current_index as i32,
        body.position_ms,
    )
    .await
    .map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DATABASE_ERROR",
            &e.to_string(),
        )
    })?;

    // Sync full transferred queue to PlaybackEngine
    let current_tid = body.track_ids.get(body.current_index as usize).copied();
    state
        .playback_engine
        .set_queue(validated_tracks, body.current_index as usize, current_tid)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "RUNTIME_SYNC_FAILED",
                &e.to_string(),
            )
        })?;

    if body.position_ms > 0 {
        state
            .playback_engine
            .seek(body.position_ms)
            .await
            .map_err(|e| {
                v1_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    e.error_code(),
                    &e.to_string(),
                )
            })?;
    }

    let _ = state.tx.send(
        serde_json::json!({
            "type": "queue_transferred", "session_id": session_id,
        })
        .to_string(),
    );

    Ok(Json(serde_json::json!({
        "queue_id": queue_id,
        "session_id": session_id,
        "accepted": true,
        "current_index": body.current_index,
        "position_ms": body.position_ms,
    })))
}

#[derive(Debug, Deserialize)]
pub struct QueueReorderBody {
    pub item_ids: Vec<Uuid>,
    pub queue_id: Option<Uuid>,
}

pub async fn queue_reorder_handler(
    State(state): State<AppState>,
    Json(body): Json<QueueReorderBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let queue_id = if let Some(qid) = body.queue_id {
        qid
    } else {
        get_or_create_active_queue(&state.db).await.map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?
    };

    let mut tx = state.db.begin().await.map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DATABASE_ERROR",
            &e.to_string(),
        )
    })?;

    let existing_items = sqlx::query_as::<_, (String, String)>(
        "SELECT id, track_id FROM queue_items WHERE queue_id = ?",
    )
    .bind(queue_id.to_string())
    .fetch_all(&mut *tx)
    .await
    .map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DATABASE_ERROR",
            &e.to_string(),
        )
    })?;

    if existing_items.len() != body.item_ids.len() {
        return Err(v1_error(
            StatusCode::BAD_REQUEST,
            "COUNT_MISMATCH",
            &format!(
                "expected {} items, got {}",
                existing_items.len(),
                body.item_ids.len()
            ),
        ));
    }

    let existing_id_set: std::collections::HashSet<String> =
        existing_items.iter().map(|(id, _)| id.clone()).collect();
    for i_id in &body.item_ids {
        if !existing_id_set.contains(&i_id.to_string()) {
            return Err(v1_error(
                StatusCode::BAD_REQUEST,
                "UNKNOWN_ITEM_ID",
                &format!("item {i_id} does not belong to queue {queue_id}"),
            ));
        }
    }

    let now = chrono::Utc::now().to_rfc3339();
    for (pos, item_id) in body.item_ids.iter().enumerate() {
        let updated =
            sqlx::query("UPDATE queue_items SET position = ? WHERE id = ? AND queue_id = ?")
                .bind(pos as i64)
                .bind(item_id.to_string())
                .bind(queue_id.to_string())
                .execute(&mut *tx)
                .await
                .map_err(|e| {
                    v1_error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "DATABASE_ERROR",
                        &e.to_string(),
                    )
                })?;

        if updated.rows_affected() == 0 {
            return Err(v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "REORDER_FAILED",
                &format!("failed to update position for item {item_id}"),
            ));
        }
    }

    sqlx::query("UPDATE queues SET updated_at = ? WHERE id = ?")
        .bind(&now)
        .bind(queue_id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;

    tx.commit().await.map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DATABASE_ERROR",
            &e.to_string(),
        )
    })?;

    // P1-03: Sync reordered queue to engine preserving current_track_id
    let cur_track_id = state
        .playback_engine
        .snapshot()
        .await
        .ok()
        .and_then(|s| s.track_id);
    sync_active_queue_to_engine(&state, &queue_id, cur_track_id).await?;

    let _ = state.tx.send(
        serde_json::json!({
            "type": "queue_reordered",
            "queue_id": queue_id,
        })
        .to_string(),
    );

    Ok(Json(
        serde_json::json!({ "status": "ok", "reordered": body.item_ids.len() }),
    ))
}

pub async fn clear_queue_handler(
    State(state): State<AppState>,
    Path(queue_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let active_queue_id = get_or_create_active_queue(&state.db).await.map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DATABASE_ERROR",
            &e.to_string(),
        )
    })?;

    sqlx::query("DELETE FROM queue_items WHERE queue_id = ?")
        .bind(queue_id.to_string())
        .execute(&state.db)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;

    // V-P0-04: Only clear engine queue if clearing the canonical active queue
    if queue_id == active_queue_id {
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
    }

    Ok(Json(
        serde_json::json!({ "status": "cleared", "queue_id": queue_id }),
    ))
}

pub async fn queue_delete_handler(
    State(state): State<AppState>,
    Path(queue_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let mut tx = state.db.begin().await.map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DATABASE_ERROR",
            &e.to_string(),
        )
    })?;

    let exists: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM queues WHERE id = ?")
        .bind(queue_id.to_string())
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;

    if exists == 0 {
        return Err(v1_error(
            StatusCode::NOT_FOUND,
            "NOT_FOUND",
            "queue not found",
        ));
    }

    let queue_name: Option<String> = sqlx::query_scalar("SELECT name FROM queues WHERE id = ?")
        .bind(queue_id.to_string())
        .fetch_optional(&mut *tx)
        .await
        .unwrap_or(None);

    let is_active = queue_name.as_deref() == Some("active-queue");

    sqlx::query("DELETE FROM queue_items WHERE queue_id = ?")
        .bind(queue_id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;

    sqlx::query("DELETE FROM queues WHERE id = ?")
        .bind(queue_id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;

    tx.commit().await.map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DATABASE_ERROR",
            &e.to_string(),
        )
    })?;

    if is_active {
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
    }

    Ok(Json(
        serde_json::json!({ "status": "deleted", "queue_id": queue_id }),
    ))
}

#[derive(Debug, Deserialize)]
pub struct QueueSaveBody {
    pub track_ids: Vec<Uuid>,
    pub current_index: u32,
    pub position_ms: u64,
}

pub async fn queue_save_handler(
    State(state): State<AppState>,
    Json(body): Json<QueueSaveBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let session_id = michi_db::save_queue_state(
        &state.db,
        "saved-queue",
        &body.track_ids,
        body.current_index as i32,
        body.position_ms,
    )
    .await
    .map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DATABASE_ERROR",
            &e.to_string(),
        )
    })?;

    Ok(Json(serde_json::json!({
        "status": "saved",
        "session_id": session_id,
        "queue_size": body.track_ids.len(),
    })))
}

pub async fn queue_saved_handler(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let session = michi_db::get_latest_playback_session(&state.db)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;

    match session {
        Some(s) => {
            let queue_items = if let Some(qid) = s.queue_id {
                michi_db::get_queue_items(&state.db, &qid)
                    .await
                    .ok()
                    .unwrap_or_default()
            } else {
                Vec::new()
            };

            Ok(Json(serde_json::json!({
                "found": true,
                "session_id": s.id,
                "queue_id": s.queue_id,
                "current_index": s.current_index,
                "position_ms": s.position_ms,
                "source": s.source,
                "items": queue_items.iter().map(|(tid, pos)| serde_json::json!({
                    "track_id": tid,
                    "position": pos,
                })).collect::<Vec<_>>(),
            })))
        }
        None => {
            let active_id = get_or_create_active_queue(&state.db).await.map_err(|e| {
                v1_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "DATABASE_ERROR",
                    &e.to_string(),
                )
            })?;

            let queue_items = michi_db::get_queue_items(&state.db, &active_id)
                .await
                .ok()
                .unwrap_or_default();

            if queue_items.is_empty() {
                Ok(Json(serde_json::json!({
                    "found": false,
                    "queue_id": active_id,
                    "items": [],
                })))
            } else {
                Ok(Json(serde_json::json!({
                    "found": true,
                    "session_id": serde_json::Value::Null,
                    "queue_id": active_id,
                    "current_index": 0,
                    "position_ms": 0,
                    "source": "queue",
                    "items": queue_items.iter().map(|(tid, pos)| serde_json::json!({
                        "track_id": tid,
                        "position": pos,
                    })).collect::<Vec<_>>(),
                })))
            }
        }
    }
}
