use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};

use crate::AppState;

#[derive(Debug, Serialize)]
pub struct RoomsStatus {
    available: bool,
    version: Option<String>,
    rooms: Vec<michi_rooms::Room>,
}

#[derive(Debug, Deserialize)]
pub struct VolumeRequest {
    volume: u32,
}

#[derive(Debug, Deserialize)]
pub struct MuteRequest {
    muted: bool,
}

pub async fn rooms_status_handler(
    State(_state): State<AppState>,
) -> Result<Json<RoomsStatus>, (StatusCode, Json<serde_json::Value>)> {
    let snapcast = michi_rooms::check_snapcast().await;
    if !snapcast.available {
        return Ok(Json(RoomsStatus {
            available: false,
            version: None,
            rooms: vec![],
        }));
    }
    let rooms = michi_rooms::get_groups().await.unwrap_or_default();
    Ok(Json(RoomsStatus {
        available: true,
        version: snapcast.version,
        rooms,
    }))
}

pub async fn rooms_volume_handler(
    State(_state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<VolumeRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    michi_rooms::set_group_volume(&id, body.volume)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e})),
            )
        })?;
    Ok(Json(serde_json::json!({"status": "ok"})))
}

pub async fn rooms_mute_handler(
    State(_state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<MuteRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    michi_rooms::set_group_mute(&id, body.muted)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e})),
            )
        })?;
    Ok(Json(serde_json::json!({"status": "ok"})))
}

pub fn rooms_router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/rooms/status", get(rooms_status_handler))
        .route("/api/v1/rooms/:id/volume", post(rooms_volume_handler))
        .route("/api/v1/rooms/:id/mute", post(rooms_mute_handler))
}
