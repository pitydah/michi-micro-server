use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::AppState;

pub async fn rooms_handler(
    State(_state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let snapcast = michi_rooms::check_snapcast().await;
    if !snapcast.available {
        return Ok(Json(serde_json::json!({
            "rooms": [],
            "snapcast_available": false,
        })));
    }

    let rooms = michi_rooms::get_groups().await.unwrap_or_default();
    let result: Vec<serde_json::Value> = rooms
        .into_iter()
        .map(|r| {
            serde_json::json!({
                "id": r.id,
                "name": r.name,
                "volume": r.volume,
                "muted": r.muted,
                "client_count": r.client_count,
            })
        })
        .collect();

    Ok(Json(serde_json::json!({
        "rooms": result,
        "snapcast_available": true,
    })))
}

#[derive(Debug, Deserialize)]
pub struct CreateRoomBody {
    pub name: String,
    pub receiver_ids: Option<Vec<Uuid>>,
}

pub async fn create_room_handler(
    Json(body): Json<CreateRoomBody>,
) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        "name": body.name,
    }))
}

#[derive(Debug, Deserialize)]
pub struct RoomPlayBody {
    pub track_id: Uuid,
    pub position_ms: Option<u64>,
}

pub async fn room_play_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<RoomPlayBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let mut current = state.playback_state.write().await;
    current.track_id = Some(body.track_id);
    current.position_ms = body.position_ms.unwrap_or(0);
    current.playing = true;
    current.updated_at = chrono::Utc::now();
    drop(current);

    let _ = state.tx.send(serde_json::json!({
        "type": "room_play",
        "room_id": id,
        "track_id": body.track_id,
    }).to_string());

    Ok(Json(serde_json::json!({ "status": "playing", "room_id": id })))
}
