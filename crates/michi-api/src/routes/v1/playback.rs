use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use chrono::Utc;
use michi_playback::{PlaybackLifecycle, RepeatMode};
use serde::{Deserialize, Serialize};
use tracing::info;
use uuid::Uuid;

use crate::output::{resolve_output, PlaybackOutputSelection};
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

pub async fn playback_state_handler(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let snap = state.playback_engine.snapshot().await.map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "ENGINE_ERROR",
            &e.to_string(),
        )
    })?;

    // Functional truth: playing is STRICTLY derived from engine lifecycle
    let is_playing = matches!(
        snap.lifecycle,
        PlaybackLifecycle::AudioFlowing | PlaybackLifecycle::Playing
    );
    let state_str = if is_playing { "playing" } else { "paused" };

    let ps_read = state.playback_state.read().await;
    let track_id = snap.track_id.or(ps_read.track_id);
    let position_ms = if is_playing || snap.position_ms > 0 {
        snap.position_ms
    } else {
        ps_read.position_ms
    };
    drop(ps_read);

    // Sync projection to keep legacy subscriptions updated
    {
        let mut ps = state.playback_state.write().await;
        ps.track_id = track_id;
        ps.position_ms = position_ms;
        ps.playing = is_playing;
        ps.volume = (snap.volume as f64) / 100.0;
        ps.shuffle = snap.shuffle;
        ps.repeat = snap.repeat.as_str().to_string();
        ps.updated_at = snap.updated_at;
    }

    let current_track = if let Some(tid) = track_id {
        michi_db::get_track(&state.db, &tid)
            .await
            .ok()
            .flatten()
            .map(|t| {
                serde_json::json!({
                    "id": t.id,
                    "title": t.title,
                    "artist": t.artist,
                    "album": t.album,
                    "duration_ms": t.duration_ms,
                    "format": t.format,
                })
            })
    } else {
        None
    };

    Ok(Json(serde_json::json!({
        "state": state_str,
        "lifecycle": snap.lifecycle,
        "playing": is_playing,
        "track_id": track_id,
        "current_track": current_track,
        "position_ms": position_ms,
        "duration_ms": snap.duration_ms.or_else(|| current_track.as_ref().and_then(|t| t.get("duration_ms")).and_then(|v| v.as_u64())),
        "volume": snap.volume,
        "shuffle": snap.shuffle,
        "repeat": snap.repeat.as_str(),
        "output": snap.output,
        "sinks": snap.sinks,
        "bytes_decoded": snap.bytes_decoded,
        "bytes_delivered": snap.bytes_delivered,
        "output_health": snap.output_health,
        "last_error": snap.last_error,
        "restored": false,
        "updated_at": snap.updated_at,
    })))
}

pub async fn get_playback_output_handler(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let selection = state.playback_output_selection.read().await.clone();
    Ok(Json(serde_json::json!({
        "output": selection,
    })))
}

pub async fn set_playback_output_handler(
    State(state): State<AppState>,
    Json(selection): Json<PlaybackOutputSelection>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    match resolve_output(&selection, &state).await {
        Ok(plan) => {
            *state.playback_output_selection.write().await = Some(selection.clone());
            Ok(Json(serde_json::json!({
                "status": "output_selected",
                "output": selection,
                "description": plan.description,
            })))
        }
        Err(e) => Err(v1_error(
            StatusCode::BAD_REQUEST,
            e.error_code(),
            &e.to_string(),
        )),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaybackRepeatMode {
    Off,
    One,
    All,
}

impl PlaybackRepeatMode {
    pub fn to_engine_repeat(&self) -> RepeatMode {
        match self {
            Self::Off => RepeatMode::Off,
            Self::One => RepeatMode::One,
            Self::All => RepeatMode::All,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PlaybackControlValue {
    Integer(u64),
    Repeat(PlaybackRepeatMode),
    Boolean(bool),
    Null,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlaybackControlBody {
    pub command: String,
    pub track_id: Option<Uuid>,
    pub position_ms: Option<u64>,
    pub volume: Option<u32>,
    pub value: Option<PlaybackControlValue>,
}

pub async fn playback_control_handler(
    State(state): State<AppState>,
    Json(body): Json<PlaybackControlBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let cmd = body.command.as_str();
    const VALID_COMMANDS: &[&str] = &[
        "play",
        "pause",
        "toggle",
        "stop",
        "next",
        "previous",
        "seek",
        "set_volume",
        "mute",
        "unmute",
        "shuffle",
        "repeat",
    ];

    if !VALID_COMMANDS.contains(&cmd) {
        return Err(v1_error_code(
            StatusCode::BAD_REQUEST,
            michi_link::MichiLinkErrorCode::InvalidRequest,
            &format!("unknown playback command '{cmd}'"),
        ));
    }

    match cmd {
        "play" => {
            let snap = state.playback_engine.snapshot().await.map_err(|e| {
                v1_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "ENGINE_ERROR",
                    &e.to_string(),
                )
            })?;

            if body.track_id.is_none()
                && (snap.lifecycle == PlaybackLifecycle::Paused
                    || (state.playback_state.read().await.track_id.is_some()
                        && !state.playback_state.read().await.playing))
            {
                let _ = state.playback_engine.resume().await;
                let mut current = state.playback_state.write().await;
                current.playing = true;
                current.updated_at = Utc::now();
                return Ok(Json(serde_json::json!({
                    "status": "accepted",
                    "lifecycle": "preparing",
                })));
            }

            let track_id_opt = body.track_id.or(snap.track_id).or({
                let ps = state.playback_state.read().await;
                ps.track_id
            });
            let pos_ms = body
                .position_ms
                .or(match body.value {
                    Some(PlaybackControlValue::Integer(ms)) => Some(ms),
                    _ => None,
                })
                .unwrap_or(0);

            let track = match track_id_opt {
                Some(tid) => {
                    let track_opt = michi_db::get_track(&state.db, &tid).await.map_err(|e| {
                        v1_error_code(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            michi_link::MichiLinkErrorCode::InternalError,
                            &e.to_string(),
                        )
                    })?;
                    track_opt.ok_or_else(|| {
                        v1_error_code(
                            StatusCode::NOT_FOUND,
                            michi_link::MichiLinkErrorCode::NotFound,
                            &format!("track not found: {tid}"),
                        )
                    })?
                }
                None => {
                    let active_qid =
                        crate::routes::v1::queue::get_or_create_active_queue(&state.db)
                            .await
                            .ok();
                    let first_tid = if let Some(qid) = active_qid {
                        let items = michi_db::get_queue_items(&state.db, &qid)
                            .await
                            .unwrap_or_default();
                        items.first().map(|(id, _)| *id)
                    } else {
                        None
                    };

                    if let Some(tid) = first_tid {
                        let track_opt = michi_db::get_track(&state.db, &tid).await.ok().flatten();
                        if let Some(t) = track_opt {
                            t
                        } else if let Some(ref t) = snap.current_track {
                            t.clone()
                        } else {
                            return Err(v1_error_code(
                                StatusCode::BAD_REQUEST,
                                michi_link::MichiLinkErrorCode::InvalidRequest,
                                "no track selected for playback",
                            ));
                        }
                    } else if let Some(ref t) = snap.current_track {
                        t.clone()
                    } else {
                        return Err(v1_error_code(
                            StatusCode::BAD_REQUEST,
                            michi_link::MichiLinkErrorCode::InvalidRequest,
                            "no track selected for playback",
                        ));
                    }
                }
            };

            // Fail-closed output resolution with automatic receiver discovery fallback
            let selection_opt = {
                let sel = state.playback_output_selection.read().await.clone();
                if sel.is_none() {
                    let reg_arc = state.receiver_manager.registry().await;
                    let reg = reg_arc.read().await;
                    let paired_rec = reg
                        .list()
                        .into_iter()
                        .find(|r| r.paired)
                        .map(|r| r.receiver_id.clone());
                    paired_rec.map(|id| PlaybackOutputSelection::Receiver { id })
                } else {
                    sel
                }
            };

            let selection = selection_opt.ok_or_else(|| {
                v1_error(
                    StatusCode::BAD_REQUEST,
                    "NO_OUTPUT_SELECTED",
                    "no playback output target configured. Select an output target first.",
                )
            })?;

            let plan = resolve_output(&selection, &state)
                .await
                .map_err(|e| v1_error(StatusCode::BAD_GATEWAY, e.error_code(), &e.to_string()))?;

            let track_id = track.id;
            let play_res = state
                .playback_engine
                .play(track, plan.sinks, plan.description.clone(), pos_ms)
                .await;

            match play_res {
                Ok(()) => {
                    {
                        let mut current = state.playback_state.write().await;
                        current.track_id = Some(track_id);
                        current.playing = true;
                        current.position_ms = pos_ms;
                        current.updated_at = Utc::now();
                    }

                    Ok(Json(serde_json::json!({
                        "status": "accepted",
                        "lifecycle": "preparing",
                        "track_id": track_id,
                        "output": plan.description,
                    })))
                }
                Err(e) => Err(v1_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    e.error_code(),
                    &e.to_string(),
                )),
            }
        }
        "pause" => {
            state.playback_engine.pause().await.map_err(|e| {
                v1_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    e.error_code(),
                    &e.to_string(),
                )
            })?;

            {
                let mut current = state.playback_state.write().await;
                current.playing = false;
                current.updated_at = Utc::now();
            }

            Ok(Json(serde_json::json!({
                "status": "accepted",
                "lifecycle": "paused",
            })))
        }
        "resume" => {
            state.playback_engine.resume().await.map_err(|e| {
                v1_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    e.error_code(),
                    &e.to_string(),
                )
            })?;

            {
                let mut current = state.playback_state.write().await;
                current.playing = true;
                current.updated_at = Utc::now();
            }

            Ok(Json(serde_json::json!({
                "status": "accepted",
                "lifecycle": "preparing",
            })))
        }
        "toggle" => {
            let snap = state.playback_engine.snapshot().await.map_err(|e| {
                v1_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "ENGINE_ERROR",
                    &e.to_string(),
                )
            })?;

            if snap.is_playing() {
                state.playback_engine.pause().await.map_err(|e| {
                    v1_error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        e.error_code(),
                        &e.to_string(),
                    )
                })?;
                let mut current = state.playback_state.write().await;
                current.playing = false;
            } else {
                state.playback_engine.resume().await.map_err(|e| {
                    v1_error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        e.error_code(),
                        &e.to_string(),
                    )
                })?;
                let mut current = state.playback_state.write().await;
                current.playing = true;
            }

            Ok(Json(serde_json::json!({
                "status": "accepted",
            })))
        }
        "stop" => {
            state.playback_engine.stop().await.map_err(|e| {
                v1_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    e.error_code(),
                    &e.to_string(),
                )
            })?;

            {
                let mut current = state.playback_state.write().await;
                current.playing = false;
                current.position_ms = 0;
            }

            Ok(Json(serde_json::json!({
                "status": "accepted",
                "lifecycle": "stopped",
            })))
        }
        "seek" => {
            let pos_ms = body
                .position_ms
                .or(match body.value {
                    Some(PlaybackControlValue::Integer(ms)) => Some(ms),
                    _ => None,
                })
                .unwrap_or(0);

            state.playback_engine.seek(pos_ms).await.map_err(|e| {
                v1_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    e.error_code(),
                    &e.to_string(),
                )
            })?;

            {
                let mut current = state.playback_state.write().await;
                current.position_ms = pos_ms;
            }

            Ok(Json(serde_json::json!({
                "status": "accepted",
                "position_ms": pos_ms,
            })))
        }
        "next" => {
            {
                let ps = state.playback_state.read().await;
                let rep_mode: RepeatMode = ps.repeat.parse().unwrap_or(RepeatMode::Off);
                let _ = state.playback_engine.set_repeat(rep_mode).await;
                let _ = state.playback_engine.set_shuffle(ps.shuffle).await;
            }

            let snap = state.playback_engine.snapshot().await.unwrap_or_default();
            if snap.current_track.is_none() {
                let active_qid = crate::routes::v1::queue::get_or_create_active_queue(&state.db)
                    .await
                    .ok();
                if let Some(qid) = active_qid {
                    let items = michi_db::get_queue_items(&state.db, &qid)
                        .await
                        .unwrap_or_default();
                    let mut tracks = Vec::new();
                    for (tid, _) in &items {
                        if let Ok(Some(t)) = michi_db::get_track(&state.db, tid).await {
                            tracks.push(t);
                        }
                    }
                    if !tracks.is_empty() {
                        let ps = state.playback_state.read().await;
                        let cur_idx = if let Some(ref cur_tid) = ps.track_id {
                            tracks.iter().position(|t| t.id == *cur_tid).unwrap_or(0)
                        } else {
                            0
                        };
                        drop(ps);
                        let _ = state.playback_engine.set_queue(tracks, cur_idx).await;
                    }
                }
            }

            let _ = state.playback_engine.next().await;
            let snap = state.playback_engine.snapshot().await.unwrap_or_default();

            {
                let mut current = state.playback_state.write().await;
                if let Some(tid) = snap.track_id {
                    current.track_id = Some(tid);
                } else {
                    let active_qid =
                        crate::routes::v1::queue::get_or_create_active_queue(&state.db)
                            .await
                            .ok();
                    if let Some(qid) = active_qid {
                        if let Ok(items) = michi_db::get_queue_items(&state.db, &qid).await {
                            if !items.is_empty() {
                                let cur_idx = if let Some(ref cur_tid) = current.track_id {
                                    items.iter().position(|(it, _)| it == cur_tid).unwrap_or(0)
                                } else {
                                    0
                                };
                                let next_idx = if cur_idx + 1 < items.len() {
                                    cur_idx + 1
                                } else if current.repeat == "all" {
                                    0
                                } else {
                                    cur_idx
                                };
                                current.track_id = Some(items[next_idx].0);
                            }
                        }
                    }
                }
                current.position_ms = 0;
                current.playing = snap.is_playing();
                current.updated_at = Utc::now();
            }

            Ok(Json(serde_json::json!({
                "status": "accepted",
            })))
        }
        "previous" => {
            let pos_threshold = {
                let ps = state.playback_state.read().await;
                ps.position_ms > 3000
            };

            let _ = state.playback_engine.previous().await;
            let snap = state.playback_engine.snapshot().await.unwrap_or_default();

            {
                let mut current = state.playback_state.write().await;
                if pos_threshold {
                    current.position_ms = 0;
                } else if let Some(tid) = snap.track_id {
                    current.track_id = Some(tid);
                    current.position_ms = 0;
                } else {
                    let active_qid =
                        crate::routes::v1::queue::get_or_create_active_queue(&state.db)
                            .await
                            .ok();
                    if let Some(qid) = active_qid {
                        if let Ok(items) = michi_db::get_queue_items(&state.db, &qid).await {
                            if !items.is_empty() {
                                let cur_idx = if let Some(ref cur_tid) = current.track_id {
                                    items.iter().position(|(it, _)| it == cur_tid).unwrap_or(0)
                                } else {
                                    0
                                };
                                let prev_idx = if cur_idx > 0 { cur_idx - 1 } else { 0 };
                                current.track_id = Some(items[prev_idx].0);
                            }
                        }
                    }
                    current.position_ms = 0;
                }
                current.playing = snap.is_playing();
                current.updated_at = Utc::now();
            }

            Ok(Json(serde_json::json!({
                "status": "accepted",
            })))
        }
        "set_volume" => {
            let vol_val = body.volume.or(match body.value {
                Some(PlaybackControlValue::Integer(v)) => Some(v as u32),
                _ => None,
            });

            let vol_u32 = match vol_val {
                Some(v) => {
                    if v > 100 {
                        return Err(v1_error_code(
                            StatusCode::BAD_REQUEST,
                            michi_link::MichiLinkErrorCode::InvalidRequest,
                            "volume must be between 0 and 100",
                        ));
                    }
                    v
                }
                None => 80,
            };

            let vol = vol_u32 as u8;
            state.playback_engine.set_volume(vol).await.map_err(|e| {
                v1_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    e.error_code(),
                    &e.to_string(),
                )
            })?;

            {
                let mut current = state.playback_state.write().await;
                current.volume = (vol as f64) / 100.0;
            }

            Ok(Json(serde_json::json!({
                "status": "accepted",
                "volume": vol,
            })))
        }
        "mute" => {
            state.playback_engine.set_volume(0).await.map_err(|e| {
                v1_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    e.error_code(),
                    &e.to_string(),
                )
            })?;

            {
                let mut current = state.playback_state.write().await;
                current.volume = 0.0;
            }

            Ok(Json(serde_json::json!({
                "status": "accepted",
                "volume": 0,
            })))
        }
        "unmute" => {
            state.playback_engine.set_volume(80).await.map_err(|e| {
                v1_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    e.error_code(),
                    &e.to_string(),
                )
            })?;

            {
                let mut current = state.playback_state.write().await;
                current.volume = 0.8;
            }

            Ok(Json(serde_json::json!({
                "status": "accepted",
                "volume": 80,
            })))
        }
        "shuffle" => {
            let shuf = match body.value {
                Some(PlaybackControlValue::Boolean(b)) => b,
                _ => true,
            };

            state.playback_engine.set_shuffle(shuf).await.map_err(|e| {
                v1_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    e.error_code(),
                    &e.to_string(),
                )
            })?;

            {
                let mut current = state.playback_state.write().await;
                current.shuffle = shuf;
            }

            Ok(Json(serde_json::json!({
                "status": "accepted",
                "shuffle": shuf,
            })))
        }
        "repeat" => {
            let rep_mode = match body.value {
                Some(PlaybackControlValue::Repeat(PlaybackRepeatMode::One)) => RepeatMode::One,
                Some(PlaybackControlValue::Repeat(PlaybackRepeatMode::All)) => RepeatMode::All,
                _ => RepeatMode::Off,
            };

            state
                .playback_engine
                .set_repeat(rep_mode)
                .await
                .map_err(|e| {
                    v1_error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        e.error_code(),
                        &e.to_string(),
                    )
                })?;

            {
                let mut current = state.playback_state.write().await;
                current.repeat = rep_mode.as_str().to_string();
            }

            Ok(Json(serde_json::json!({
                "status": "accepted",
                "repeat": rep_mode.as_str(),
            })))
        }
        _ => unreachable!(),
    }
}

#[derive(Debug, Deserialize)]
pub struct PlaybackSessionBody {
    #[serde(default, alias = "queue_state")]
    pub queue: Vec<Uuid>,
    pub current_track_id: Option<Uuid>,
    #[serde(default)]
    pub position_ms: u64,
    #[serde(default)]
    pub playing: bool,
    pub volume: Option<f64>,
    pub source: Option<String>,
    pub resume_policy: Option<String>,
    pub device_id: Option<String>,
    pub repeat_mode: Option<String>,
    pub shuffle: Option<bool>,
}

pub async fn playback_session_handler(
    State(state): State<AppState>,
    Json(body): Json<PlaybackSessionBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let session_id = Uuid::new_v4();
    let queue_id = Uuid::new_v4();
    let now = chrono::Utc::now().to_rfc3339();
    let queue_json = serde_json::to_string(&body.queue).unwrap_or_default();

    // Validate and load tracks
    let mut runtime_tracks = Vec::new();
    for track_id in &body.queue {
        if let Ok(Some(track)) = michi_db::get_track(&state.db, track_id).await {
            runtime_tracks.push(track);
        }
    }

    let current_track_id = body
        .current_track_id
        .or_else(|| body.queue.first().copied());
    let current_index = if let Some(ref cur_id) = current_track_id {
        runtime_tracks
            .iter()
            .position(|t| t.id == *cur_id)
            .unwrap_or(0)
    } else {
        0
    };

    // Create queue in DB
    let _ =
        sqlx::query("INSERT INTO queues (id, name, created_at, updated_at) VALUES (?, ?, ?, ?)")
            .bind(queue_id.to_string())
            .bind("playback-session")
            .bind(&now)
            .bind(&now)
            .execute(&state.db)
            .await;

    for (i, track_id) in body.queue.iter().enumerate() {
        let item_id = Uuid::new_v4();
        let _ = sqlx::query(
            "INSERT INTO queue_items (id, queue_id, track_id, position, added_at) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(item_id.to_string())
        .bind(queue_id.to_string())
        .bind(track_id.to_string())
        .bind(i as i64)
        .bind(&now)
        .execute(&state.db)
        .await;
    }

    let db_session = michi_core::PlaybackSessionDb {
        id: session_id,
        device_id: body
            .device_id
            .as_deref()
            .and_then(|d| Uuid::parse_str(d).ok())
            .unwrap_or_else(Uuid::nil),
        queue_id: Some(queue_id),
        queue_state_json: queue_json,
        current_index: current_index as i32,
        current_track_id,
        position_ms: body.position_ms,
        playing: body.playing,
        repeat_mode: body.repeat_mode.clone().unwrap_or_else(|| "off".into()),
        shuffle: body.shuffle.unwrap_or(false),
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

    // Load queue into PlaybackEngine
    if !runtime_tracks.is_empty() {
        state
            .playback_engine
            .set_queue(runtime_tracks.clone(), current_index)
            .await
            .map_err(|e| {
                v1_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    e.error_code(),
                    &e.to_string(),
                )
            })?;
    }

    if let Some(ref rep_str) = body.repeat_mode {
        let rep_mode: RepeatMode = rep_str.parse().unwrap_or(RepeatMode::Off);
        let _ = state.playback_engine.set_repeat(rep_mode).await;
    }

    if let Some(shuf) = body.shuffle {
        let _ = state.playback_engine.set_shuffle(shuf).await;
    }

    let vol = (body.volume.unwrap_or(0.8) * 100.0) as u8;
    let _ = state.playback_engine.set_volume(vol).await;

    if body.playing {
        let selection_opt = {
            let sel = state.playback_output_selection.read().await.clone();
            if sel.is_none() {
                let reg_arc = state.receiver_manager.registry().await;
                let reg = reg_arc.read().await;
                let paired_rec = reg
                    .list()
                    .into_iter()
                    .find(|r| r.paired)
                    .map(|r| r.receiver_id.clone());
                paired_rec.map(|id| PlaybackOutputSelection::Receiver { id })
            } else {
                sel
            }
        };

        if let Some(selection) = selection_opt {
            let plan = resolve_output(&selection, &state)
                .await
                .map_err(|e| v1_error(StatusCode::BAD_GATEWAY, e.error_code(), &e.to_string()))?;

            if let Some(cur_track) = runtime_tracks.get(current_index) {
                state
                    .playback_engine
                    .play(
                        cur_track.clone(),
                        plan.sinks,
                        plan.description.clone(),
                        body.position_ms,
                    )
                    .await
                    .map_err(|e| {
                        v1_error(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            e.error_code(),
                            &e.to_string(),
                        )
                    })?;
            }
        }
    }

    {
        let mut current = state.playback_state.write().await;
        current.track_id = current_track_id;
        current.position_ms = body.position_ms;
        current.playing = body.playing;
        current.volume = body.volume.unwrap_or(0.8);
        current.updated_at = Utc::now();
    }

    let _ = state.tx.send(
        serde_json::json!({
            "type": "playback_session_created", "session_id": session_id,
        })
        .to_string(),
    );

    Ok(Json(serde_json::json!({
        "session_id": session_id,
        "queue_id": queue_id,
        "accepted": true,
        "lifecycle": if body.playing { "preparing" } else { "paused" },
    })))
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
                current.updated_at = Utc::now();
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
                state.playing = false; // never auto-play on restart
                state.volume = session.volume;
                state.updated_at = Utc::now();
                drop(state);

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_playback_control_schema_valid_shapes() {
        let json_repeat_off = r#"{"command": "repeat", "value": "off"}"#;
        let parsed: PlaybackControlBody = serde_json::from_str(json_repeat_off).unwrap();
        assert_eq!(parsed.command, "repeat");
        assert_eq!(
            parsed.value,
            Some(PlaybackControlValue::Repeat(PlaybackRepeatMode::Off))
        );

        let json_repeat_all = r#"{"command": "repeat", "value": "all"}"#;
        let parsed: PlaybackControlBody = serde_json::from_str(json_repeat_all).unwrap();
        assert_eq!(
            parsed.value,
            Some(PlaybackControlValue::Repeat(PlaybackRepeatMode::All))
        );

        let json_seek = r#"{"command": "seek", "value": 12345}"#;
        let parsed_seek: PlaybackControlBody = serde_json::from_str(json_seek).unwrap();
        assert_eq!(parsed_seek.command, "seek");
        assert_eq!(
            parsed_seek.value,
            Some(PlaybackControlValue::Integer(12345))
        );
    }
}
