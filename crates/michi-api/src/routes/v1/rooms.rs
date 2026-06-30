use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::AppState;

fn v1_error(status: StatusCode, code: &str, message: &str) -> (StatusCode, Json<serde_json::Value>) {
    (status, Json(serde_json::json!({
        "error": { "code": code, "message": message, "details": {} }
    })))
}

pub async fn rooms_handler(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let reg = state.receiver_manager.registry().await;
    let reg_read = reg.read().await;
    let receivers_in_rooms: Vec<serde_json::Value> = reg_read.list().iter().filter(|e| e.active_session_id.is_some()).map(|e| {
        serde_json::json!({
            "receiver_id": e.receiver_id,
            "name": e.name,
            "device_type": e.device_type,
            "active_session_id": e.active_session_id,
        })
    }).collect();

    Ok(Json(serde_json::json!({
        "rooms": [{
            "id": "default",
            "name": "Default Room",
            "receivers": receivers_in_rooms,
            "snapcast_available": false,
        }],
    })))
}

#[derive(Debug, Deserialize)]
pub struct CreateRoomBody {
    pub name: String,
    pub receiver_ids: Vec<String>,
}

pub async fn create_room_handler(
    State(state): State<AppState>,
    Json(body): Json<CreateRoomBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let reg = state.receiver_manager.registry().await;
    let reg_read = reg.read().await;
    let mut unknown = Vec::new();
    for rid in &body.receiver_ids {
        if reg_read.get(rid).is_none() {
            unknown.push(rid.clone());
        }
    }
    if !unknown.is_empty() {
        return Err(v1_error(StatusCode::BAD_REQUEST, "UNKNOWN_RECEIVERS", &format!("{:?}", unknown)));
    }
    Ok(Json(serde_json::json!({
        "status": "created",
        "name": body.name,
        "receiver_ids": body.receiver_ids,
        "message": "room created (receivers must be started individually via session/start)",
    })))
}

#[derive(Debug, Deserialize)]
pub struct RoomPlayBody {
    pub track_id: Uuid,
    pub position_ms: Option<u64>,
}

pub async fn room_play_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: Json<RoomPlayBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let mut current = state.playback_state.write().await;
    current.track_id = Some(body.track_id);
    current.position_ms = body.position_ms.unwrap_or(0);
    current.playing = true;
    current.updated_at = chrono::Utc::now();
    drop(current);
    let _ = state.tx.send(serde_json::json!({
        "type": "room_play", "room_id": id, "track_id": body.track_id,
    }).to_string());
    Ok(Json(serde_json::json!({ "status": "playing", "room_id": id })))
}
