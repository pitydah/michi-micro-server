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
    pub available: bool,
    pub version: Option<String>,
    pub degraded: bool,
    pub error: Option<String>,
    pub rooms: Vec<michi_rooms::Room>,
}

#[derive(Debug, Deserialize)]
pub struct VolumeRequest {
    pub volume: u32,
}

#[derive(Debug, Deserialize)]
pub struct MuteRequest {
    pub muted: bool,
}

pub async fn rooms_status_handler(
    State(_state): State<AppState>,
) -> Result<Json<RoomsStatus>, (StatusCode, Json<serde_json::Value>)> {
    let snapcast = michi_rooms::check_snapcast().await;
    if !snapcast.available {
        return Ok(Json(RoomsStatus {
            available: false,
            version: None,
            degraded: snapcast.degraded,
            error: snapcast.error,
            rooms: vec![],
        }));
    }
    match michi_rooms::get_groups().await {
        Ok(rooms) => Ok(Json(RoomsStatus {
            available: true,
            version: snapcast.version,
            degraded: false,
            error: None,
            rooms,
        })),
        Err(e) => Ok(Json(RoomsStatus {
            available: false,
            version: snapcast.version,
            degraded: true,
            error: Some(e.to_string()),
            rooms: vec![],
        })),
    }
}

pub async fn rooms_volume_handler(
    State(_state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<VolumeRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    michi_rooms::set_group_volume(&id, body.volume)
        .await
        .map_err(|e| {
            let status = match e {
                michi_rooms::SnapcastError::Transport(_) | michi_rooms::SnapcastError::Timeout => {
                    StatusCode::SERVICE_UNAVAILABLE
                }
                michi_rooms::SnapcastError::HttpStatus(code) => {
                    StatusCode::from_u16(code).unwrap_or(StatusCode::BAD_GATEWAY)
                }
                _ => StatusCode::BAD_GATEWAY,
            };
            (
                status,
                Json(serde_json::json!({
                    "error": {
                        "code": "SNAPCAST_RPC_FAILED",
                        "message": e.to_string()
                    }
                })),
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
            let status = match e {
                michi_rooms::SnapcastError::Transport(_) | michi_rooms::SnapcastError::Timeout => {
                    StatusCode::SERVICE_UNAVAILABLE
                }
                michi_rooms::SnapcastError::HttpStatus(code) => {
                    StatusCode::from_u16(code).unwrap_or(StatusCode::BAD_GATEWAY)
                }
                _ => StatusCode::BAD_GATEWAY,
            };
            (
                status,
                Json(serde_json::json!({
                    "error": {
                        "code": "SNAPCAST_RPC_FAILED",
                        "message": e.to_string()
                    }
                })),
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
