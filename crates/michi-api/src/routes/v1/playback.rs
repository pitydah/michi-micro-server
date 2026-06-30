use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use tracing::info;
use uuid::Uuid;

use crate::AppState;

fn v1_error(status: StatusCode, code: &str, message: &str) -> (StatusCode, Json<serde_json::Value>) {
    (status, Json(serde_json::json!({
        "error": { "code": code, "message": message, "details": {} }
    })))
}

fn state_string(playing: bool) -> &'static str {
    if playing { "playing" } else { "paused" }
}

#[derive(Debug, Serialize)]
pub struct PlaybackStateResponse {
    pub state: String,
    pub track_id: Option<Uuid>,
    pub current_track: Option<serde_json::Value>,
    pub position_ms: u64,
    pub duration_ms: Option<u64>,
    pub volume: u32,
    pub shuffle: bool,
    pub repeat: String,
    pub playing: bool,
    pub restored: bool,
}

pub async fn playback_state_handler(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let current = state.playback_state.read().await;

    let current_track = if let Some(tid) = current.track_id {
        michi_db::get_track(&state.db, &tid).await.ok().flatten().map(|t| {
            serde_json::json!({
                "id": t.id, "title": t.title, "artist": t.artist,
                "album": t.album, "duration_ms": t.duration_ms,
            })
        })
    } else {
        None
    };

    Ok(Json(serde_json::json!({
        "state": state_string(current.playing),
        "track_id": current.track_id,
        "current_track": current_track,
        "position_ms": current.position_ms,
        "duration_ms": current_track.as_ref().and_then(|t| t.get("duration_ms")).and_then(|v| v.as_u64()),
        "volume": (current.volume * 100.0) as u32,
        "shuffle": false,
        "repeat": "none",
        "playing": current.playing,
        "restored": false,
    })))
}

#[derive(Debug, Deserialize)]
pub struct PlaybackControlBody {
    pub command: Option<String>,
    pub action: Option<String>,
    pub value: Option<serde_json::Value>,
    pub position_ms: Option<u64>,
    pub volume: Option<u32>,
}

pub async fn playback_control_handler(
    State(state): State<AppState>,
    Json(body): Json<PlaybackControlBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let cmd = body.command.as_deref().or(body.action.as_deref()).ok_or_else(|| {
        v1_error(StatusCode::BAD_REQUEST, "INVALID_REQUEST", "command is required")
    })?;

    let mut current = state.playback_state.write().await;

    match cmd {
        "play" => {
            current.playing = true;
            if let Some(val) = &body.value {
                if let Some(track_id) = val.get("track_id").and_then(|v| v.as_str()) {
                    if let Ok(uid) = Uuid::parse_str(track_id) { current.track_id = Some(uid); }
                }
            }
            if let Some(pos) = body.position_ms.or_else(|| {
                body.value.as_ref().and_then(|v| v.get("position_ms").and_then(|p| p.as_u64()))
            }) { current.position_ms = pos; }
        }
        "pause" => { current.playing = false; }
        "toggle" => { current.playing = !current.playing; }
        "next" => { current.track_id = None; current.position_ms = 0; current.playing = false; }
        "previous" => { current.position_ms = 0; }
        "stop" => { current.playing = false; current.position_ms = 0; }
        "seek" => {
            if let Some(p) = body.position_ms.or_else(|| {
                body.value.as_ref().and_then(|v| v.get("position_ms").and_then(|p| p.as_u64()))
            }) { current.position_ms = p; }
        }
        "set_volume" => {
            let vol = body.volume.or_else(|| {
                body.value.as_ref().and_then(|v| v.get("volume").and_then(|p| p.as_u64().or_else(|| p.as_f64().map(|f| f as u64))).map(|v| v as u32))
            });
            if let Some(v) = vol { current.volume = (v.min(100) as f64) / 100.0; }
        }
        "mute" => { current.volume = 0.0; }
        "unmute" => { if current.volume == 0.0 { current.volume = 0.8; } }
        _ => {
            return Err(v1_error(StatusCode::BAD_REQUEST, "INVALID_COMMAND", &format!("unknown command: {}", cmd)));
        }
    }

    current.updated_at = chrono::Utc::now();
    let state_clone = current.clone();
    drop(current);

    let _ = state.sync_tx.send(state_clone.into());
    let _ = state.tx.send(serde_json::json!({
        "type": "playback_state_changed", "command": cmd,
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
    pub source: Option<String>,
    pub resume_policy: Option<String>,
}

pub async fn playback_session_handler(
    State(state): State<AppState>,
    Json(body): Json<PlaybackSessionBody>,
) -> Result<Json<serde_json::value::Value>, (StatusCode, Json<serde_json::Value>)> {
    let session_id = Uuid::new_v4();
    let queue_id = Uuid::new_v4();
    let now = chrono::Utc::now().to_rfc3339();
    let queue_json = serde_json::to_string(&body.queue).unwrap_or_default();

    // Create queue
    sqlx::query("INSERT INTO queues (id, name, created_at, updated_at) VALUES (?, ?, ?, ?)")
        .bind(queue_id.to_string()).bind("playback-session").bind(&now).bind(&now)
        .execute(&state.db).await.ok();

    for (i, track_id) in body.queue.iter().enumerate() {
        let item_id = Uuid::new_v4();
        let _ = sqlx::query(
            "INSERT INTO queue_items (id, queue_id, track_id, position, added_at) VALUES (?, ?, ?, ?, ?)",
        ).bind(item_id.to_string()).bind(queue_id.to_string())
         .bind(track_id.to_string()).bind(i as i64).bind(&now)
         .execute(&state.db).await;
    }

    let db_session = michi_core::PlaybackSessionDb {
        id: session_id, device_id: Uuid::nil(), queue_id: Some(queue_id),
        queue_state_json: queue_json, current_index: 0,
        current_track_id: body.current_track_id, position_ms: body.position_ms,
        playing: body.playing, repeat_mode: "none".into(), shuffle: false,
        volume: body.volume.unwrap_or(0.8),
        source: body.source.unwrap_or_else(|| "player".into()),
        resume_policy: body.resume_policy.unwrap_or_else(|| "manual".into()),
        restored: false,
    };

    michi_db::create_playback_session(&state.db, &db_session).await
        .map_err(|e| v1_error(StatusCode::INTERNAL_SERVER_ERROR, "DATABASE_ERROR", &e.to_string()))?;

    {
        let mut current = state.playback_state.write().await;
        current.track_id = body.current_track_id;
        current.position_ms = body.position_ms;
        current.playing = body.playing;
        current.volume = body.volume.unwrap_or(0.8);
        current.updated_at = chrono::Utc::now();
    }

    let _ = state.tx.send(serde_json::json!({
        "type": "playback_session_created", "session_id": session_id,
    }).to_string());

    Ok(Json(serde_json::json!({
        "session_id": session_id, "queue_id": queue_id, "accepted": true,
    })))
}

#[derive(Debug, Deserialize)]
pub struct PlaybackSessionGetQuery {
    pub session_id: Option<Uuid>,
}

pub async fn playback_session_get_handler(
    State(state): State<AppState>,
    Path(session_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let session = michi_db::get_playback_session(&state.db, &session_id).await
        .map_err(|e| v1_error(StatusCode::INTERNAL_SERVER_ERROR, "DATABASE_ERROR", &e.to_string()))?
        .ok_or_else(|| v1_error(StatusCode::NOT_FOUND, "SESSION_NOT_FOUND", "playback session not found"))?;

    let queue_items = if let Some(qid) = session.queue_id {
        michi_db::get_queue_items(&state.db, &qid).await.ok().unwrap_or_default()
    } else {
        Vec::new()
    };

    Ok(Json(serde_json::json!({
        "session_id": session.id,
        "queue_id": session.queue_id,
        "current_track_id": session.current_track_id,
        "position_ms": session.position_ms,
        "playing": session.playing,
        "volume": (session.volume * 100.0) as u32,
        "source": session.source,
        "resume_policy": session.resume_policy,
        "restored": session.restored,
        "queue_items": queue_items.iter().map(|(tid, pos)| serde_json::json!({"track_id": tid, "position": pos})).collect::<Vec<_>>(),
    })))
}

pub async fn playback_session_restore_handler(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let latest = michi_db::get_latest_playback_session(&state.db).await
        .map_err(|e| v1_error(StatusCode::INTERNAL_SERVER_ERROR, "DATABASE_ERROR", &e.to_string()))?;

    match latest {
        Some(session) => {
            {
                let mut current = state.playback_state.write().await;
                current.track_id = session.current_track_id;
                current.position_ms = session.position_ms;
                current.playing = session.playing;
                current.volume = session.volume;
                current.updated_at = chrono::Utc::now();
            }

            let mut updated = session;
            updated.restored = true;
            michi_db::update_playback_session(&state.db, &updated).await.ok();

            Ok(Json(serde_json::json!({
                "restored": true,
                "session_id": updated.id,
                "track_id": updated.current_track_id,
                "position_ms": updated.position_ms,
                "playing": updated.playing,
                "volume": (updated.volume * 100.0) as u32,
                "resume_policy": updated.resume_policy,
            })))
        }
        None => Ok(Json(serde_json::json!({
            "restored": false,
            "message": "no saved playback session found",
        }))),
    }
}

pub fn auto_restore_playback_state(db: sqlx::SqlitePool, playback_state: std::sync::Arc<tokio::sync::RwLock<michi_sync::PlaybackState>>) {
    tokio::spawn(async move {
        match michi_db::get_latest_playback_session(&db).await {
            Ok(Some(session)) => {
                let mut state = playback_state.write().await;
                state.track_id = session.current_track_id;
                state.position_ms = session.position_ms;
                state.playing = false; // never auto-play
                state.volume = session.volume;
                state.updated_at = chrono::Utc::now();
                drop(state);

                // Also restore queue items from DB
                if let Some(qid) = session.queue_id {
                    if let Ok(items) = michi_db::get_queue_items(&db, &qid).await {
                        if !items.is_empty() {
                            info!("restored {} queue items from session {}", items.len(), session.id);
                        }
                    }
                }
            }
            Ok(None) => {
                info!("no saved playback session to restore");
            }
            Err(e) => {
                tracing::warn!("failed to restore playback state: {} (server will start fresh)", e);
            }
        }
    });
}
