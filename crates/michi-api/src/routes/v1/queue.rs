use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
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

pub async fn queue_handler(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let current = state.playback_state.read().await;
    Ok(Json(serde_json::json!({
        "current_track_id": current.track_id,
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
    let queue_id = Uuid::new_v4();
    let now = chrono::Utc::now().to_rfc3339();
    let name = body.name.unwrap_or_else(|| "v1-queue".into());

    sqlx::query("INSERT INTO queues (id, name, created_at, updated_at) VALUES (?, ?, ?, ?)")
        .bind(queue_id.to_string())
        .bind(&name)
        .bind(&now)
        .bind(&now)
        .execute(&state.db)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;

    for (i, track_id) in body.track_ids.iter().enumerate() {
        let item_id = Uuid::new_v4();
        let _ = sqlx::query(
            "INSERT INTO queue_items (id, queue_id, track_id, position, added_at) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(item_id.to_string()).bind(queue_id.to_string())
        .bind(track_id.to_string()).bind(i as i64).bind(&now)
        .execute(&state.db).await;
    }

    Ok(Json(serde_json::json!({
        "queue_id": queue_id, "items_count": body.track_ids.len(),
    })))
}

#[derive(Debug, Deserialize)]
pub struct QueueJumpBody {
    pub index: u32,
    pub queue_id: Option<Uuid>,
}

pub async fn queue_jump_handler(
    State(state): State<AppState>,
    Json(body): Json<QueueJumpBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let mut current = state.playback_state.write().await;
    current.position_ms = 0;
    current.updated_at = chrono::Utc::now();
    drop(current);

    let _ = state
        .tx
        .send(serde_json::json!({ "type": "queue_jumped", "index": body.index }).to_string());
    Ok(Json(
        serde_json::json!({ "status": "ok", "index": body.index }),
    ))
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

    // Validate all track_ids exist in library
    let mut unknown_tracks: Vec<Uuid> = Vec::new();
    for tid in &body.track_ids {
        let exists = michi_db::get_track(&state.db, tid).await.map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;
        if exists.is_none() {
            unknown_tracks.push(*tid);
        }
    }

    if !unknown_tracks.is_empty() {
        return Err(v1_error(
            StatusCode::BAD_REQUEST,
            "UNKNOWN_TRACKS",
            &format!("tracks not found: {:?}", unknown_tracks),
        ));
    }

    // Create new queue
    let queue_id = Uuid::new_v4();
    let name = format!("transfer-{}", &body.source);
    let now = chrono::Utc::now().to_rfc3339();

    sqlx::query("INSERT INTO queues (id, name, source_device_id, created_at, updated_at) VALUES (?, ?, ?, ?, ?)")
        .bind(queue_id.to_string()).bind(&name).bind(&body.source).bind(&now).bind(&now)
        .execute(&state.db)
        .await
        .map_err(|e| v1_error(StatusCode::INTERNAL_SERVER_ERROR, "DATABASE_ERROR", &e.to_string()))?;

    for (i, track_id) in body.track_ids.iter().enumerate() {
        let item_id = Uuid::new_v4();
        let _ = sqlx::query(
            "INSERT INTO queue_items (id, queue_id, track_id, position, added_at) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(item_id.to_string()).bind(queue_id.to_string())
        .bind(track_id.to_string()).bind(i as i64).bind(&now)
        .execute(&state.db).await;
    }

    // Update playback state
    {
        let mut current = state.playback_state.write().await;
        current.track_id = body.track_ids.get(body.current_index as usize).copied();
        current.position_ms = body.position_ms;
        current.playing = true;
        current.updated_at = chrono::Utc::now();
    }

    // Create playback session
    let session_id = Uuid::new_v4();
    let queue_json = serde_json::to_string(&body.track_ids).unwrap_or_default();
    let db_session = michi_core::PlaybackSessionDb {
        id: session_id,
        device_id: Uuid::nil(),
        queue_id: Some(queue_id),
        queue_state_json: queue_json,
        current_index: body.current_index as i32,
        current_track_id: body.track_ids.get(body.current_index as usize).copied(),
        position_ms: body.position_ms,
        playing: true,
        repeat_mode: "none".into(),
        shuffle: false,
        volume: 0.8,
        source: body.source.clone(),
        resume_policy: "manual".into(),
        restored: false,
    };
    michi_db::create_playback_session(&state.db, &db_session)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;

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
    let queue_id = body.queue_id.ok_or_else(|| {
        v1_error(
            StatusCode::BAD_REQUEST,
            "INVALID_REQUEST",
            "queue_id is required",
        )
    })?;

    let now = chrono::Utc::now().to_rfc3339();
    let mut tx = state.db.begin().await.map_err(|e| {
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
        .ok();

    for (i, item_id) in body.item_ids.iter().enumerate() {
        sqlx::query("INSERT INTO queue_items (id, queue_id, track_id, position, added_at) VALUES (?, ?, ?, ?, ?)")
            .bind(Uuid::new_v4().to_string()).bind(queue_id.to_string())
            .bind(item_id.to_string()).bind(i as i64).bind(&now)
            .execute(&mut *tx)
            .await
            .ok();
    }

    tx.commit().await.map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DATABASE_ERROR",
            &e.to_string(),
        )
    })?;

    Ok(Json(
        serde_json::json!({ "status": "ok", "reordered": body.item_ids.len() }),
    ))
}

#[derive(Debug, Deserialize)]
pub struct QueueDeleteBody {
    pub queue_id: Uuid,
}

pub async fn queue_delete_handler(
    State(state): State<AppState>,
    Path(queue_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    sqlx::query("DELETE FROM queue_items WHERE queue_id = ?")
        .bind(queue_id.to_string())
        .execute(&state.db)
        .await
        .ok();
    sqlx::query("DELETE FROM queues WHERE id = ?")
        .bind(queue_id.to_string())
        .execute(&state.db)
        .await
        .ok();

    Ok(Json(serde_json::json!({ "status": "deleted" })))
}

// ── Queue Save/Load (cross-device) ─────────────────────────────

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
        None => Ok(Json(serde_json::json!({
            "found": false,
            "queue_id": null,
        }))),
    }
}
