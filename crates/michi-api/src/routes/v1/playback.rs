use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use tracing::info;
use uuid::Uuid;

use crate::AppState;

fn v1_error_code(
    status: StatusCode,
    code: michi_link::MichiLinkErrorCode,
    message: &str,
) -> (StatusCode, Json<serde_json::Value>) {
    (
        status,
        Json(serde_json::json!({
            "error": { "code": code.as_str(), "message": message, "details": {} }
        })),
    )
}

fn state_string(playing: bool) -> &'static str {
    if playing {
        "playing"
    } else {
        "paused"
    }
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
        michi_db::get_track(&state.db, &tid)
            .await
            .ok()
            .flatten()
            .map(|t| {
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
        "shuffle": current.shuffle,
        "repeat": current.repeat,
        "playing": current.playing,
        "restored": false,
    })))
}

#[derive(Debug, Deserialize)]
pub struct PlaybackControlBody {
    pub command: String,
    pub value: Option<serde_json::Value>,
    pub position_ms: Option<u64>,
    pub volume: Option<u32>,
}

pub async fn playback_control_handler(
    State(state): State<AppState>,
    Json(body): Json<PlaybackControlBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let cmd = body.command.as_str();

    let mut current = state.playback_state.write().await;

    match cmd {
        "play" => {
            if let Some(val) = &body.value {
                let tid_str = val
                    .get("track_id")
                    .and_then(|v| v.as_str())
                    .or_else(|| val.as_str());
                if let Some(track_id) = tid_str {
                    let uid = Uuid::parse_str(track_id).map_err(|_| {
                        v1_error_code(
                            StatusCode::BAD_REQUEST,
                            michi_link::MichiLinkErrorCode::InvalidRequest,
                            "invalid track UUID format",
                        )
                    })?;
                    let track = michi_db::get_track(&state.db, &uid).await.map_err(|e| {
                        v1_error_code(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            michi_link::MichiLinkErrorCode::InternalError,
                            &e.to_string(),
                        )
                    })?;
                    if track.is_none() {
                        return Err(v1_error_code(
                            StatusCode::NOT_FOUND,
                            michi_link::MichiLinkErrorCode::TrackNotFound,
                            "track not found in library",
                        ));
                    }
                    current.track_id = Some(uid);
                }
            }
            current.playing = true;
            if let Some(pos) = body.position_ms.or_else(|| {
                body.value
                    .as_ref()
                    .and_then(|v| v.get("position_ms").and_then(|p| p.as_u64()))
            }) {
                current.position_ms = pos;
            }
        }
        "pause" => {
            current.playing = false;
        }
        "toggle" => {
            current.playing = !current.playing;
        }
        "next" => {
            let active_queue_id = crate::routes::v1::queue::get_or_create_active_queue(&state.db)
                .await
                .ok();
            let items = if let Some(qid) = active_queue_id {
                sqlx::query_as::<_, (String, String, i64)>(
                    "SELECT id, track_id, position FROM queue_items WHERE queue_id = ? ORDER BY position ASC"
                )
                .bind(qid.to_string())
                .fetch_all(&state.db)
                .await
                .unwrap_or_default()
            } else {
                Vec::new()
            };

            if items.is_empty() {
                current.track_id = None;
                current.position_ms = 0;
                current.playing = false;
            } else if current.repeat == "one" {
                current.position_ms = 0;
            } else {
                let cur_idx = if let Some(cur_id) = current.track_id {
                    items.iter().position(|it| it.1 == cur_id.to_string())
                } else {
                    None
                };

                let next_idx = match cur_idx {
                    Some(idx) => {
                        if current.shuffle {
                            if items.len() > 1 {
                                use rand::Rng;
                                let mut rng = rand::thread_rng();
                                let offset = rng.gen_range(1..items.len());
                                (idx + offset) % items.len()
                            } else {
                                0
                            }
                        } else if idx + 1 < items.len() {
                            idx + 1
                        } else if current.repeat == "all" {
                            0
                        } else {
                            items.len()
                        }
                    }
                    None => 0,
                };

                if next_idx < items.len() {
                    if let Ok(tid) = Uuid::parse_str(&items[next_idx].1) {
                        current.track_id = Some(tid);
                        current.position_ms = 0;
                    }
                } else {
                    current.playing = false;
                    current.position_ms = 0;
                }
            }
        }
        "previous" => {
            if current.position_ms > 3000 {
                current.position_ms = 0;
            } else {
                let active_queue_id =
                    crate::routes::v1::queue::get_or_create_active_queue(&state.db)
                        .await
                        .ok();
                let items = if let Some(qid) = active_queue_id {
                    sqlx::query_as::<_, (String, String, i64)>(
                        "SELECT id, track_id, position FROM queue_items WHERE queue_id = ? ORDER BY position ASC"
                    )
                    .bind(qid.to_string())
                    .fetch_all(&state.db)
                    .await
                    .unwrap_or_default()
                } else {
                    Vec::new()
                };

                if !items.is_empty() {
                    let cur_idx = if let Some(cur_id) = current.track_id {
                        items.iter().position(|it| it.1 == cur_id.to_string())
                    } else {
                        None
                    };

                    let prev_idx = match cur_idx {
                        Some(idx) if idx > 0 => idx - 1,
                        _ => 0,
                    };

                    if let Ok(tid) = Uuid::parse_str(&items[prev_idx].1) {
                        current.track_id = Some(tid);
                        current.position_ms = 0;
                    }
                } else {
                    current.position_ms = 0;
                }
            }
        }
        "stop" => {
            current.playing = false;
            current.position_ms = 0;
        }
        "seek" => {
            if let Some(p) = body.position_ms.or_else(|| {
                body.value
                    .as_ref()
                    .and_then(|v| v.get("position_ms").and_then(|p| p.as_u64()))
            }) {
                current.position_ms = p;
            }
        }
        "set_volume" => {
            let vol = body.volume.or_else(|| {
                body.value.as_ref().and_then(|v| {
                    v.get("volume")
                        .and_then(|p| p.as_i64().or_else(|| p.as_f64().map(|f| f as i64)))
                        .map(|v| v as u32)
                })
            });
            match vol {
                Some(v) if v <= 100 => {
                    current.volume = (v as f64) / 100.0;
                }
                Some(_) => {
                    return Err(v1_error_code(
                        StatusCode::BAD_REQUEST,
                        michi_link::MichiLinkErrorCode::InvalidRequest,
                        "volume must be between 0 and 100",
                    ));
                }
                None => {
                    return Err(v1_error_code(
                        StatusCode::BAD_REQUEST,
                        michi_link::MichiLinkErrorCode::InvalidRequest,
                        "volume is required",
                    ));
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
        "shuffle" => {
            if let Some(val) = &body.value {
                if let Some(shuf) = val.get("shuffle").and_then(|v| v.as_bool()) {
                    current.shuffle = shuf;
                } else if let Some(shuf) = val.as_bool() {
                    current.shuffle = shuf;
                } else {
                    current.shuffle = !current.shuffle;
                }
            } else {
                current.shuffle = !current.shuffle;
            }
        }
        "repeat" => {
            if let Some(val) = &body.value {
                let rep_str = val
                    .get("repeat")
                    .and_then(|v| v.as_str())
                    .or_else(|| val.as_str());
                if let Some(rep) = rep_str {
                    match rep {
                        "off" => {
                            current.repeat = "off".to_string();
                        }
                        "all" | "one" => {
                            current.repeat = rep.to_string();
                        }
                        _ => {
                            return Err(v1_error_code(
                                StatusCode::BAD_REQUEST,
                                michi_link::MichiLinkErrorCode::InvalidRequest,
                                "repeat mode must be 'off', 'one', or 'all'",
                            ));
                        }
                    }
                } else {
                    current.repeat = match current.repeat.as_str() {
                        "off" => "all".into(),
                        "all" => "one".into(),
                        _ => "off".into(),
                    };
                }
            } else {
                current.repeat = match current.repeat.as_str() {
                    "off" => "all".into(),
                    "all" => "one".into(),
                    _ => "off".into(),
                };
            }
        }
        _ => {
            return Err(v1_error_code(
                StatusCode::BAD_REQUEST,
                michi_link::MichiLinkErrorCode::InvalidRequest,
                &format!("unknown command: {cmd}"),
            ));
        }
    }

    current.updated_at = chrono::Utc::now();
    let state_clone = current.clone();
    drop(current);

    let _ = state.sync_tx.send(state_clone.clone().into());
    let _ = state.tx.send(
        serde_json::json!({
            "type": "playback_state_changed", "command": cmd,
        })
        .to_string(),
    );

    Ok(Json(serde_json::json!({
        "status": "ok",
        "state": if state_clone.playing { "playing" } else { "paused" },
        "position_ms": state_clone.position_ms,
        "shuffle": state_clone.shuffle,
        "repeat": state_clone.repeat,
    })))
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
        .bind(queue_id.to_string())
        .bind("playback-session")
        .bind(&now)
        .bind(&now)
        .execute(&state.db)
        .await
        .ok();

    for (i, track_id) in body.queue.iter().enumerate() {
        let item_id = Uuid::new_v4();
        let _ = sqlx::query(
            "INSERT INTO queue_items (id, queue_id, track_id, position, added_at) VALUES (?, ?, ?, ?, ?)",
        ).bind(item_id.to_string()).bind(queue_id.to_string())
         .bind(track_id.to_string()).bind(i as i64).bind(&now)
         .execute(&state.db).await;
    }

    let db_session = michi_core::PlaybackSessionDb {
        id: session_id,
        device_id: Uuid::nil(),
        queue_id: Some(queue_id),
        queue_state_json: queue_json,
        current_index: 0,
        current_track_id: body.current_track_id,
        position_ms: body.position_ms,
        playing: body.playing,
        repeat_mode: "none".into(),
        shuffle: false,
        volume: body.volume.unwrap_or(0.8),
        source: body.source.unwrap_or_else(|| "player".into()),
        resume_policy: body.resume_policy.unwrap_or_else(|| "manual".into()),
        restored: false,
    };

    michi_db::create_playback_session(&state.db, &db_session)
        .await
        .map_err(|e| {
            v1_error_code(
                StatusCode::INTERNAL_SERVER_ERROR,
                michi_link::MichiLinkErrorCode::InternalError,
                &e.to_string(),
            )
        })?;

    {
        let mut current = state.playback_state.write().await;
        current.track_id = body.current_track_id;
        current.position_ms = body.position_ms;
        current.playing = body.playing;
        current.volume = body.volume.unwrap_or(0.8);
        current.updated_at = chrono::Utc::now();
    }

    let _ = state.tx.send(
        serde_json::json!({
            "type": "playback_session_created", "session_id": session_id,
        })
        .to_string(),
    );

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
    let session = michi_db::get_playback_session(&state.db, &session_id)
        .await
        .map_err(|e| {
            v1_error_code(
                StatusCode::INTERNAL_SERVER_ERROR,
                michi_link::MichiLinkErrorCode::InternalError,
                &e.to_string(),
            )
        })?
        .ok_or_else(|| {
            v1_error_code(
                StatusCode::NOT_FOUND,
                michi_link::MichiLinkErrorCode::NotFound,
                "playback session not found",
            )
        })?;

    let queue_items = if let Some(qid) = session.queue_id {
        michi_db::get_queue_items(&state.db, &qid)
            .await
            .ok()
            .unwrap_or_default()
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
    let latest = michi_db::get_latest_playback_session(&state.db)
        .await
        .map_err(|e| {
            v1_error_code(
                StatusCode::INTERNAL_SERVER_ERROR,
                michi_link::MichiLinkErrorCode::InternalError,
                &e.to_string(),
            )
        })?;

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
            michi_db::update_playback_session(&state.db, &updated)
                .await
                .ok();

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

pub fn auto_restore_playback_state(
    db: sqlx::SqlitePool,
    playback_state: std::sync::Arc<tokio::sync::RwLock<michi_sync::PlaybackState>>,
) {
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
                            info!(
                                "restored {} queue items from session {}",
                                items.len(),
                                session.id
                            );
                        }
                    }
                }
            }
            Ok(None) => {
                info!("no saved playback session to restore");
            }
            Err(e) => {
                tracing::warn!(
                    "failed to restore playback state: {} (server will start fresh)",
                    e
                );
            }
        }
    });
}

#[derive(Debug, Deserialize)]
pub struct HandoffBody {
    pub track_id: Uuid,
    pub position_ms: u64,
    pub playing: bool,
    pub volume: Option<f64>,
    pub playlist_id: Option<Uuid>,
    pub queue_position: Option<u32>,
    pub from_device: Option<String>,
}

pub async fn handoff_handler(
    State(state): State<AppState>,
    Json(body): Json<HandoffBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let new_state = michi_sync::PlaybackState {
        track_id: Some(body.track_id),
        position_ms: body.position_ms,
        playing: body.playing,
        volume: body.volume.unwrap_or(0.8),
        updated_at: Utc::now(),
        playlist_id: body.playlist_id,
        queue_position: body.queue_position,
        device_id: Some("server".into()),
        shuffle: false,
        repeat: "off".into(),
    };

    {
        let mut current = state.playback_state.write().await;
        *current = new_state.clone();
    }

    let from = body.from_device.unwrap_or_else(|| "unknown".into());
    let handoff_msg = michi_sync::SyncMessage::handoff_request(from.clone(), "server".into());
    let _ = state.sync_tx.send(handoff_msg);

    info!(
        "handoff: track={} position={}ms from={}",
        body.track_id, body.position_ms, from
    );

    Ok(Json(serde_json::json!({
        "status": "handoff_accepted",
        "track_id": body.track_id,
        "position_ms": body.position_ms,
        "playing": body.playing,
    })))
}
