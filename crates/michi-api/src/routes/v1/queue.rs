use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::playback_queue::{
    get_or_create_active_queue, reconcile_active_queue, restore_queue_items_or_degrade,
    snapshot_queue_items, sync_active_queue_to_engine, validate_and_load_tracks, QueueItemBackup,
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
    if state.disabled_modules.read().await.contains("playback") {
        return Err(v1_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "MODULE_DISABLED",
            "playback module is disabled",
        ));
    }
    let snap = state.playback_engine.snapshot().await.map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "ENGINE_UNAVAILABLE",
            &e.to_string(),
        )
    })?;

    let is_playing = matches!(
        snap.lifecycle,
        michi_playback::PlaybackLifecycle::AudioFlowing
            | michi_playback::PlaybackLifecycle::Playing
    );

    let position_ms = if snap.lifecycle == michi_playback::PlaybackLifecycle::Stopped
        || snap.lifecycle == michi_playback::PlaybackLifecycle::Ended
    {
        0
    } else {
        snap.position_ms
    };

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

    let current_index = if let Some(ref cur_tid) = snap.track_id {
        let cur_tid_str = cur_tid.to_string();
        snap.queue_index
            .filter(|&idx| {
                items
                    .get(idx)
                    .and_then(|it| it["track_id"].as_str())
                    .map(|tid| tid == cur_tid_str)
                    .unwrap_or(false)
            })
            .or_else(|| items.iter().position(|it| it["track_id"] == cur_tid_str))
            .map(|pos| pos as u32)
    } else {
        None
    };

    Ok(Json(serde_json::json!({
        "queue_id": active_queue_id,
        "queue_version": state.queue_coordinator.version(),
        "queue_degraded": state.queue_coordinator.is_degraded(),
        "queue_degraded_reason": state.queue_coordinator.degraded_reason(),
        "items_count": items.len(),
        "items": items,
        "current_track_id": snap.track_id,
        "current_index": current_index,
        "position_ms": position_ms,
        "playing": is_playing,
        "volume": snap.volume,
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
    if state.disabled_modules.read().await.contains("playback") {
        return Err(v1_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "MODULE_DISABLED",
            "playback module is disabled",
        ));
    }
    if body.track_ids.is_empty() {
        return Err(v1_error(
            StatusCode::BAD_REQUEST,
            "INVALID_REQUEST",
            "track_ids cannot be empty",
        ));
    }

    let _mutation_guard = state.queue_coordinator.lock().await;
    state.queue_coordinator.ensure_coherent()?;

    // P0-04 Functional Truth: All-or-nothing validation against DB, disk, and configured library roots
    let _validated =
        validate_and_load_tracks(&state.db, &body.track_ids, &state.config.music_paths).await?;

    let active_queue_id = get_or_create_active_queue(&state.db).await.map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DATABASE_ERROR",
            &e.to_string(),
        )
    })?;
    let previous_items = snapshot_queue_items(&state.db, &active_queue_id).await?;

    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await.map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DATABASE_ERROR",
            &e.to_string(),
        )
    })?;

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

    let cur_track_id = match state.playback_engine.snapshot().await {
        Ok(snap) => snap.track_id,
        Err(e) => {
            return Err(restore_queue_items_or_degrade(
                &state,
                &active_queue_id,
                &previous_items,
                None,
                &e.to_string(),
            )
            .await);
        }
    };

    if let Err((_, err_json)) =
        sync_active_queue_to_engine(&state, &active_queue_id, cur_track_id).await
    {
        let err_msg = err_json.0["error"]["message"]
            .as_str()
            .unwrap_or("runtime queue sync failed")
            .to_string();
        return Err(restore_queue_items_or_degrade(
            &state,
            &active_queue_id,
            &previous_items,
            None,
            &err_msg,
        )
        .await);
    }

    let queue_version = state.queue_coordinator.bump_version();

    let _ = state.tx.send(
        serde_json::json!({
            "type": "queue_updated",
            "queue_id": active_queue_id,
            "queue_version": queue_version,
            "added_count": body.track_ids.len(),
        })
        .to_string(),
    );

    Ok(Json(serde_json::json!({
        "status": "ok",
        "queue_id": active_queue_id,
        "queue_version": queue_version,
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
    if state.disabled_modules.read().await.contains("playback") {
        return Err(v1_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "MODULE_DISABLED",
            "playback module is disabled",
        ));
    }
    let _mutation_guard = state.queue_coordinator.lock().await;
    state.queue_coordinator.ensure_coherent()?;

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

    let previous_active_items = if queue_id != active_queue_id {
        Some(snapshot_queue_items(&state.db, &active_queue_id).await?)
    } else {
        None
    };

    if queue_id != active_queue_id {
        // V-P0-04: Promote to canonical active-queue in SQLite transaction
        let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await.map_err(|e| {
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

    if let Err(engine_err) = state
        .playback_engine
        .set_queue(validated_tracks, body.index as usize, Some(target_track_id))
        .await
    {
        if let Some(prev) = previous_active_items.as_deref() {
            return Err(restore_queue_items_or_degrade(
                &state,
                &active_queue_id,
                prev,
                None,
                &engine_err.to_string(),
            )
            .await);
        }
        return Err(v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "RUNTIME_SYNC_FAILED",
            &engine_err.to_string(),
        ));
    }

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

    let queue_version = state.queue_coordinator.bump_version();

    let _ = state.tx.send(
        serde_json::json!({
            "type": "playback_state_changed",
            "track_id": target_track_id,
            "index": body.index,
            "queue_version": queue_version,
        })
        .to_string(),
    );

    Ok(Json(serde_json::json!({
        "status": "ok",
        "index": body.index,
        "track_id": target_track_id,
        "queue_version": queue_version,
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
    if state.disabled_modules.read().await.contains("playback") {
        return Err(v1_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "MODULE_DISABLED",
            "playback module is disabled",
        ));
    }
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

    let _mutation_guard = state.queue_coordinator.lock().await;
    state.queue_coordinator.ensure_coherent()?;

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
    let previous_items = snapshot_queue_items(&state.db, &queue_id).await?;

    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await.map_err(|e| {
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

    // Sync full transferred queue to PlaybackEngine with verified compensating rollback
    let current_tid = body.track_ids.get(body.current_index as usize).copied();
    if let Err(engine_err) = state
        .playback_engine
        .set_queue(validated_tracks, body.current_index as usize, current_tid)
        .await
    {
        return Err(restore_queue_items_or_degrade(
            &state,
            &queue_id,
            &previous_items,
            None,
            &engine_err.to_string(),
        )
        .await);
    }

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

    let queue_version = state.queue_coordinator.bump_version();

    let _ = state.tx.send(
        serde_json::json!({
            "type": "queue_transferred",
            "session_id": session_id,
            "queue_version": queue_version,
        })
        .to_string(),
    );

    Ok(Json(serde_json::json!({
        "queue_id": queue_id,
        "queue_version": queue_version,
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
    if state.disabled_modules.read().await.contains("playback") {
        return Err(v1_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "MODULE_DISABLED",
            "playback module is disabled",
        ));
    }
    let _mutation_guard = state.queue_coordinator.lock().await;
    state.queue_coordinator.ensure_coherent()?;

    let active_queue_id = get_or_create_active_queue(&state.db).await.map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DATABASE_ERROR",
            &e.to_string(),
        )
    })?;
    let queue_id = body.queue_id.unwrap_or(active_queue_id);
    let previous_items = snapshot_queue_items(&state.db, &queue_id).await?;

    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await.map_err(|e| {
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
    let mut seen_ids = std::collections::HashSet::with_capacity(body.item_ids.len());
    for i_id in &body.item_ids {
        let i_id_str = i_id.to_string();
        if !existing_id_set.contains(&i_id_str) {
            return Err(v1_error(
                StatusCode::BAD_REQUEST,
                "UNKNOWN_ITEM_ID",
                &format!("item {i_id} does not belong to queue {queue_id}"),
            ));
        }
        if !seen_ids.insert(i_id_str) {
            return Err(v1_error(
                StatusCode::BAD_REQUEST,
                "DUPLICATE_ITEM_ID",
                &format!("duplicate item_id {i_id} in reorder payload"),
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

    // P1-03: Sync reordered active queue to engine preserving current_track_id, with verified rollback
    if queue_id == active_queue_id {
        let cur_track_id = match state.playback_engine.snapshot().await {
            Ok(snap) => snap.track_id,
            Err(e) => {
                return Err(restore_queue_items_or_degrade(
                    &state,
                    &queue_id,
                    &previous_items,
                    None,
                    &e.to_string(),
                )
                .await);
            }
        };
        if let Err((_, err_json)) =
            sync_active_queue_to_engine(&state, &queue_id, cur_track_id).await
        {
            let err_msg = err_json.0["error"]["message"]
                .as_str()
                .unwrap_or("runtime queue sync failed")
                .to_string();
            return Err(restore_queue_items_or_degrade(
                &state,
                &queue_id,
                &previous_items,
                None,
                &err_msg,
            )
            .await);
        }
    }

    let queue_version = state.queue_coordinator.bump_version();

    let _ = state.tx.send(
        serde_json::json!({
            "type": "queue_reordered",
            "queue_id": queue_id,
            "queue_version": queue_version,
        })
        .to_string(),
    );

    Ok(Json(serde_json::json!({
        "status": "ok",
        "reordered": body.item_ids.len(),
        "queue_version": queue_version,
    })))
}

pub async fn clear_queue_handler(
    State(state): State<AppState>,
    Path(queue_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if state.disabled_modules.read().await.contains("playback") {
        return Err(v1_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "MODULE_DISABLED",
            "playback module is disabled",
        ));
    }
    let _mutation_guard = state.queue_coordinator.lock().await;
    state.queue_coordinator.ensure_coherent()?;

    let active_queue_id = get_or_create_active_queue(&state.db).await.map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DATABASE_ERROR",
            &e.to_string(),
        )
    })?;
    let previous_items = snapshot_queue_items(&state.db, &queue_id).await?;

    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await.map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DATABASE_ERROR",
            &e.to_string(),
        )
    })?;
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

    // V-P0-04: Only clear engine queue if clearing the canonical active queue
    if queue_id == active_queue_id {
        if let Err(engine_err) = state.playback_engine.set_queue(Vec::new(), 0, None).await {
            return Err(restore_queue_items_or_degrade(
                &state,
                &queue_id,
                &previous_items,
                None,
                &engine_err.to_string(),
            )
            .await);
        }
    }

    let queue_version = state.queue_coordinator.bump_version();

    Ok(Json(serde_json::json!({
        "status": "cleared",
        "queue_id": queue_id,
        "queue_version": queue_version,
    })))
}

pub async fn queue_delete_handler(
    State(state): State<AppState>,
    Path(queue_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if state.disabled_modules.read().await.contains("playback") {
        return Err(v1_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "MODULE_DISABLED",
            "playback module is disabled",
        ));
    }
    let _mutation_guard = state.queue_coordinator.lock().await;
    state.queue_coordinator.ensure_coherent()?;

    let previous_items = snapshot_queue_items(&state.db, &queue_id).await?;

    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await.map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DATABASE_ERROR",
            &e.to_string(),
        )
    })?;

    let queue_name: Option<String> = sqlx::query_scalar("SELECT name FROM queues WHERE id = ?")
        .bind(queue_id.to_string())
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;

    let q_name = match queue_name {
        Some(n) => n,
        None => {
            return Err(v1_error(
                StatusCode::NOT_FOUND,
                "NOT_FOUND",
                &format!("queue {queue_id} not found"),
            ));
        }
    };

    let is_active = q_name == "active-queue";

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

    // V-P0-04: Only clear engine queue if deleting the canonical active queue
    if is_active {
        if let Err(engine_err) = state.playback_engine.set_queue(Vec::new(), 0, None).await {
            return Err(restore_queue_items_or_degrade(
                &state,
                &queue_id,
                &previous_items,
                Some(&q_name),
                &engine_err.to_string(),
            )
            .await);
        }
    }

    let queue_version = state.queue_coordinator.bump_version();

    Ok(Json(serde_json::json!({
        "status": "deleted",
        "queue_id": queue_id,
        "queue_version": queue_version,
    })))
}

#[derive(Debug, Deserialize)]
pub struct QueueRepeatBody {
    pub mode: String,
}

pub async fn queue_repeat_handler(
    State(state): State<AppState>,
    Json(body): Json<QueueRepeatBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if state.disabled_modules.read().await.contains("playback") {
        return Err(v1_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "MODULE_DISABLED",
            "playback module is disabled",
        ));
    }
    let mode = match body.mode.trim().to_ascii_lowercase().as_str() {
        "off" | "none" => michi_playback::RepeatMode::Off,
        "one" | "track" => michi_playback::RepeatMode::One,
        "all" | "queue" => michi_playback::RepeatMode::All,
        other => {
            return Err(v1_error(
                StatusCode::BAD_REQUEST,
                "INVALID_REQUEST",
                &format!("invalid repeat mode '{other}'; expected off, one, or all"),
            ));
        }
    };
    state.playback_engine.set_repeat(mode).await.map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            e.error_code(),
            &e.to_string(),
        )
    })?;
    Ok(Json(serde_json::json!({
        "status": "ok",
        "mode": mode.as_str(),
        "repeat": mode.as_str(),
    })))
}

#[derive(Debug, Deserialize)]
pub struct QueueShuffleBody {
    pub enabled: bool,
}

pub async fn queue_shuffle_handler(
    State(state): State<AppState>,
    Json(body): Json<QueueShuffleBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if state.disabled_modules.read().await.contains("playback") {
        return Err(v1_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "MODULE_DISABLED",
            "playback module is disabled",
        ));
    }
    state
        .playback_engine
        .set_shuffle(body.enabled)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                e.error_code(),
                &e.to_string(),
            )
        })?;
    Ok(Json(serde_json::json!({
        "status": "ok",
        "enabled": body.enabled,
        "shuffle": body.enabled,
    })))
}

pub async fn queue_item_delete_handler(
    State(state): State<AppState>,
    Path(item_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if state.disabled_modules.read().await.contains("playback") {
        return Err(v1_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "MODULE_DISABLED",
            "playback module is disabled",
        ));
    }

    // Serialize queue mutations across concurrent callers without holding a SQLite
    // transaction open while PlaybackEngine performs audio transitions.
    let _mutation_guard = state.queue_coordinator.lock().await;
    state.queue_coordinator.ensure_coherent()?;

    let active_queue_id = get_or_create_active_queue(&state.db).await.map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DATABASE_ERROR",
            &e.to_string(),
        )
    })?;

    // 1. Read ordered queue snapshot and current engine state before mutating anything.
    let existing_rows = sqlx::query_as::<_, (String, String, i64, String)>(
        "SELECT id, track_id, position, added_at FROM queue_items WHERE queue_id = ? ORDER BY position ASC",
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

    let engine_snap = state.playback_engine.snapshot().await.map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "ENGINE_UNAVAILABLE",
            &e.to_string(),
        )
    })?;

    // 2. Canonical lookup by queue_item_id (`id`). Legacy fallback by `track_id` deletes
    // AT MOST ONE matching row (preferring the active slot if its track_id matches) so
    // duplicate track entries in a queue are NEVER accidentally bulk-deleted.
    let item_id_str = item_id.to_string();
    let (removed_idx, matched_by) = if let Some(pos) = existing_rows
        .iter()
        .position(|(id, _, _, _)| id == &item_id_str)
    {
        (pos, "queue_item_id")
    } else {
        let active_matching_pos = engine_snap.queue_index.filter(|&idx| {
            existing_rows
                .get(idx)
                .map(|(_, tid, _, _)| tid == &item_id_str)
                .unwrap_or(false)
        });
        let pos = active_matching_pos.or_else(|| {
            existing_rows
                .iter()
                .position(|(_, tid, _, _)| tid == &item_id_str)
        });
        match pos {
            Some(p) => (p, "legacy_track_id"),
            None => {
                return Err(v1_error(
                    StatusCode::NOT_FOUND,
                    "NOT_FOUND",
                    "queue item not found",
                ));
            }
        }
    };

    let (removed_row_id, removed_track_id_str, _removed_orig_pos, _removed_added_at) =
        existing_rows[removed_idx].clone();
    let removed_track_id = Uuid::parse_str(&removed_track_id_str).map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DATABASE_INVARIANT_VIOLATION",
            &format!("invalid track UUID '{removed_track_id_str}' in queue: {e}"),
        )
    })?;

    // 3. Pre-validate remaining tracks BEFORE mutating SQLite or PlaybackEngine.
    let mut remaining_rows = Vec::with_capacity(existing_rows.len().saturating_sub(1));
    let mut remaining_track_ids = Vec::with_capacity(existing_rows.len().saturating_sub(1));
    for (idx, row) in existing_rows.iter().enumerate() {
        if idx == removed_idx {
            continue;
        }
        let tid = Uuid::parse_str(&row.1).map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_INVARIANT_VIOLATION",
                &format!("invalid track UUID '{}' in queue: {e}", row.1),
            )
        })?;
        remaining_rows.push(row.clone());
        remaining_track_ids.push(tid);
    }

    let remaining_tracks = if remaining_track_ids.is_empty() {
        Vec::new()
    } else {
        validate_and_load_tracks(&state.db, &remaining_track_ids, &state.config.music_paths).await?
    };

    // 4. Atomic SQLite mutation: delete ONLY the single targeted queue_item_id and compact
    // positions to contiguous 0..N-1. Commit BEFORE calling PlaybackEngine.
    let now = chrono::Utc::now().to_rfc3339();
    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await.map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DATABASE_ERROR",
            &e.to_string(),
        )
    })?;

    let del_res = sqlx::query("DELETE FROM queue_items WHERE queue_id = ? AND id = ?")
        .bind(active_queue_id.to_string())
        .bind(&removed_row_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;
    if del_res.rows_affected() == 0 {
        return Err(v1_error(
            StatusCode::NOT_FOUND,
            "NOT_FOUND",
            "queue item not found",
        ));
    }

    for (new_pos, (row_id, _, _, _)) in remaining_rows.iter().enumerate() {
        sqlx::query("UPDATE queue_items SET position = ? WHERE queue_id = ? AND id = ?")
            .bind(new_pos as i64)
            .bind(active_queue_id.to_string())
            .bind(row_id)
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

    // 5. Coordinate PlaybackEngine transition. If the engine rejects/fails, execute a
    // verified compensating SQLite rollback; if rollback also fails, enter fail-closed
    // degraded state (503 QUEUE_DEGRADED) until explicitly reconciled.
    let remaining_count = remaining_tracks.len();
    if let Err(engine_err) = state
        .playback_engine
        .remove_queue_item(removed_idx, removed_track_id, remaining_tracks)
        .await
    {
        let previous_items: Vec<QueueItemBackup> = existing_rows
            .iter()
            .map(|(id, track_id, position, added_at)| QueueItemBackup {
                id: id.clone(),
                track_id: track_id.clone(),
                position: *position,
                added_at: added_at.clone(),
            })
            .collect();
        return Err(restore_queue_items_or_degrade(
            &state,
            &active_queue_id,
            &previous_items,
            None,
            &engine_err.to_string(),
        )
        .await);
    }

    let queue_version = state.queue_coordinator.bump_version();

    let _ = state.tx.send(
        serde_json::json!({
            "type": "queue_updated",
            "queue_id": active_queue_id,
            "queue_version": queue_version,
            "removed_item_id": removed_row_id,
            "remaining_count": remaining_count,
        })
        .to_string(),
    );

    Ok(Json(serde_json::json!({
        "status": "deleted",
        "item_id": item_id,
        "removed_queue_item_id": removed_row_id,
        "removed_track_id": removed_track_id,
        "matched_by": matched_by,
        "remaining_count": remaining_count,
        "queue_version": queue_version,
    })))
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
    if state.disabled_modules.read().await.contains("playback") {
        return Err(v1_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "MODULE_DISABLED",
            "playback module is disabled",
        ));
    }
    let _mutation_guard = state.queue_coordinator.lock().await;
    state.queue_coordinator.ensure_coherent()?;

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
        "queue_version": state.queue_coordinator.version(),
    })))
}

/// Explicitly reconciles SQLite active queue state into PlaybackEngine and clears
/// any fail-closed `QUEUE_DEGRADED` status once synchronization succeeds.
pub async fn queue_reconcile_handler(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if state.disabled_modules.read().await.contains("playback") {
        return Err(v1_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "MODULE_DISABLED",
            "playback module is disabled",
        ));
    }
    let queue_version = reconcile_active_queue(&state).await?;
    Ok(Json(serde_json::json!({
        "status": "reconciled",
        "queue_degraded": false,
        "queue_version": queue_version,
    })))
}

pub async fn queue_saved_handler(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if state.disabled_modules.read().await.contains("playback") {
        return Err(v1_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "MODULE_DISABLED",
            "playback module is disabled",
        ));
    }
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
