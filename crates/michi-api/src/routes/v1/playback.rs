use axum::{
    extract::State,
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::AppState;

pub async fn playback_state_handler(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let current = state.playback_state.read().await;
    Ok(Json(serde_json::json!({
        "track_id": current.track_id,
        "position_ms": current.position_ms,
        "playing": current.playing,
        "volume": (current.volume * 100.0) as u32,
        "updated_at": current.updated_at,
    })))
}

#[derive(Debug, Deserialize)]
pub struct PlaybackControlBody {
    pub command: Option<String>,
    pub action: Option<String>,
    pub value: Option<serde_json::Value>,
}

pub async fn playback_control_handler(
    State(state): State<AppState>,
    Json(body): Json<PlaybackControlBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let cmd = body
        .command
        .as_deref()
        .or(body.action.as_deref())
        .ok_or_else(|| {
            (StatusCode::BAD_REQUEST, Json(serde_json::json!({
                "error": {
                    "code": "INVALID_REQUEST",
                    "message": "command is required"
                }
            })))
        })?;

    let mut current = state.playback_state.write().await;

    match cmd {
        "play" => {
            current.playing = true;
            if let Some(val) = &body.value {
                if let Some(track_id) = val.get("track_id").and_then(|v| v.as_str()) {
                    if let Ok(uid) = Uuid::parse_str(track_id) {
                        current.track_id = Some(uid);
                    }
                }
                if let Some(pos) = val.get("position_ms").and_then(|v| v.as_u64()) {
                    current.position_ms = pos;
                }
            }
        }
        "pause" => {
            current.playing = false;
        }
        "toggle" => {
            current.playing = !current.playing;
        }
        "next" => {
            current.track_id = None;
            current.position_ms = 0;
            current.playing = false;
        }
        "previous" => {
            current.position_ms = 0;
        }
        "stop" => {
            current.playing = false;
            current.position_ms = 0;
        }
        "seek" => {
            if let Some(val) = &body.value {
                if let Some(pos) = val.get("position_ms").and_then(|v| v.as_u64()) {
                    current.position_ms = pos;
                }
            }
        }
        "set_volume" => {
            if let Some(val) = &body.value {
                if let Some(vol) = val.get("volume").and_then(|v| v.as_u64()) {
                    current.volume = (vol.min(100) as f64) / 100.0;
                }
            }
        }
        "mute" => {
            current.volume = 0.0;
        }
        "unmute" => {
            if current.volume == 0.0 {
                current.volume = 0.8;
            }
        }
        _ => {
            return Err((StatusCode::BAD_REQUEST, Json(serde_json::json!({
                "error": {
                    "code": "INVALID_COMMAND",
                    "message": format!("unknown command: {}", cmd)
                }
            }))));
        }
    }

    current.updated_at = chrono::Utc::now();
    let state_clone = current.clone();
    drop(current);

    let _ = state.sync_tx.send(state_clone.into());
    let _ = state.tx.send(serde_json::json!({
        "type": "playback_state_changed",
        "command": cmd,
    }).to_string());

    Ok(Json(serde_json::json!({ "status": "ok" })))
}

#[derive(Debug, Deserialize)]
pub struct PlaybackSessionBody {
    pub queue: Vec<Uuid>,
    pub current_track_id: Option<Uuid>,
    pub position_ms: u64,
    pub playing: bool,
    pub volume: Option<f64>,
}

pub async fn playback_session_handler(
    State(state): State<AppState>,
    Json(body): Json<PlaybackSessionBody>,
) -> Result<Json<serde_json::value::Value>, (StatusCode, Json<serde_json::Value>)> {
    let session_id = Uuid::new_v4();
    let queue_json = serde_json::to_string(&body.queue).unwrap_or_default();

    let db_session = michi_core::PlaybackSessionDb {
        id: session_id,
        device_id: Uuid::nil(),
        queue_state_json: queue_json,
        current_index: 0,
        current_track_id: body.current_track_id,
        position_ms: body.position_ms,
        playing: body.playing,
        repeat_mode: "none".into(),
        shuffle: false,
        volume: body.volume.unwrap_or(0.8),
    };

    michi_db::create_playback_session(&state.db, &db_session).await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
            "error": {
                "code": "DATABASE_ERROR",
                "message": e.to_string()
            }
        })))
    })?;

    {
        let mut current = state.playback_state.write().await;
        current.track_id = body.current_track_id;
        current.position_ms = body.position_ms;
        current.playing = body.playing;
        current.volume = body.volume.unwrap_or(0.8);
        current.updated_at = chrono::Utc::now();
    }

    let _ = state.tx.send(serde_json::json!({
        "type": "playback_session_created",
        "session_id": session_id,
    }).to_string());

    Ok(Json(serde_json::json!({
        "session_id": session_id,
        "accepted": true,
    })))
}
