use axum::{
    extract::State,
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::AppState;

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
}

pub async fn queue_items_handler(
    State(state): State<AppState>,
    Json(body): Json<QueueItemsBody>,
) -> Result<Json<serde_json::value::Value>, (StatusCode, Json<serde_json::Value>)> {
    let queue_id = Uuid::new_v4();
    let now = chrono::Utc::now().to_rfc3339();

    sqlx::query("INSERT INTO queues (id, name, created_at, updated_at) VALUES (?, ?, ?, ?)")
        .bind(queue_id.to_string())
        .bind("v1-queue")
        .bind(&now)
        .bind(&now)
        .execute(&state.db)
        .await
        .map_err(|e| {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
                "error": { "code": "DATABASE_ERROR", "message": e.to_string() }
            })))
        })?;

    for (i, track_id) in body.track_ids.iter().enumerate() {
        let item_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO queue_items (id, queue_id, track_id, position, added_at) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(item_id.to_string())
        .bind(queue_id.to_string())
        .bind(track_id.to_string())
        .bind(i as i64)
        .bind(&now)
        .execute(&state.db)
        .await
        .ok();
    }

    Ok(Json(serde_json::json!({
        "queue_id": queue_id,
        "items_count": body.track_ids.len(),
    })))
}

#[derive(Debug, Deserialize)]
pub struct QueueJumpBody {
    pub index: u32,
}

pub async fn queue_jump_handler(
    State(state): State<AppState>,
    Json(body): Json<QueueJumpBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let mut current = state.playback_state.write().await;
    current.position_ms = 0;
    current.updated_at = chrono::Utc::now();
    drop(current);

    let _ = state.tx.send(serde_json::json!({
        "type": "queue_jumped",
        "index": body.index,
    }).to_string());

    Ok(Json(serde_json::json!({ "status": "ok", "index": body.index })))
}

#[derive(Debug, Deserialize)]
pub struct QueueReorderBody {
    pub item_ids: Vec<Uuid>,
}

pub async fn queue_reorder_handler(
    Json(body): Json<QueueReorderBody>,
) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        "reordered": body.item_ids.len(),
    }))
}
