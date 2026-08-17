use std::collections::HashMap;

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

pub async fn rooms_handler(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let rows = michi_db::list_room_groups_db(&state.db)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;

    let reg = state.receiver_manager.registry().await;
    let reg_read = reg.read().await;

    let mut rooms = Vec::new();
    for (id, name, mode, receiver_ids, volumes, created_at) in rows {
        let mut member_receivers = Vec::new();
        let mut active_count = 0usize;

        for rid in &receiver_ids {
            if let Some(entry) = reg_read.get(rid) {
                if entry.active_session_id.is_some() {
                    active_count += 1;
                }
                member_receivers.push(serde_json::json!({
                    "receiver_id": entry.receiver_id,
                    "name": entry.name,
                    "device_type": entry.device_type,
                    "active_session_id": entry.active_session_id,
                    "paired": entry.paired,
                }));
            }
        }

        rooms.push(serde_json::json!({
            "id": id,
            "name": name,
            "mode": mode,
            "receiver_ids": receiver_ids,
            "volumes": volumes,
            "created_at": created_at,
            "active": active_count > 0,
            "receivers": member_receivers,
            "snapcast_available": false,
        }));
    }

    Ok(Json(serde_json::json!({ "rooms": rooms })))
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
    if body.name.trim().is_empty() {
        return Err(v1_error(
            StatusCode::BAD_REQUEST,
            "VALIDATION_ERROR",
            "name is required",
        ));
    }

    let reg = state.receiver_manager.registry().await;
    let reg_read = reg.read().await;
    let mut unknown = Vec::new();
    for rid in &body.receiver_ids {
        if reg_read.get(rid).is_none() {
            unknown.push(rid.clone());
        }
    }
    drop(reg_read);
    drop(reg);

    if !unknown.is_empty() {
        return Err(v1_error(
            StatusCode::BAD_REQUEST,
            "UNKNOWN_RECEIVERS",
            &format!("receivers not found: {unknown:?}"),
        ));
    }

    let room_id = Uuid::new_v4();
    let volumes: HashMap<String, u32> = body
        .receiver_ids
        .iter()
        .map(|id| (id.clone(), 60))
        .collect();

    michi_db::save_room_group_db(
        &state.db,
        &room_id,
        body.name.trim(),
        "custom",
        &body.receiver_ids,
        &volumes,
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
        "status": "created",
        "room_id": room_id,
        "name": body.name,
        "receiver_ids": body.receiver_ids,
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
    // Validate track exists
    let track = michi_db::get_track(&state.db, &body.track_id)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;

    if track.is_none() {
        return Err(v1_error(
            StatusCode::NOT_FOUND,
            "TRACK_NOT_FOUND",
            "track not found in library",
        ));
    }

    // Find room group
    let rows = michi_db::list_room_groups_db(&state.db)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;

    let found = rows
        .into_iter()
        .find(|(gid, name, _, _, _, _)| gid.to_string() == id || name == &id);

    let (room_id, _, _, receiver_ids, volumes, _) = found.ok_or_else(|| {
        v1_error(
            StatusCode::NOT_FOUND,
            "ROOM_NOT_FOUND",
            &format!("room {id} not found"),
        )
    })?;

    if receiver_ids.is_empty() {
        return Err(v1_error(
            StatusCode::BAD_REQUEST,
            "INVALID_ROOM",
            "room has no receivers configured",
        ));
    }

    let mut active_count = 0usize;
    for recv_id in &receiver_ids {
        let vol = volumes.get(recv_id).copied().unwrap_or(60);
        let reg = state.receiver_manager.registry().await;
        let reg_read = reg.read().await;
        if let Some(entry) = reg_read.get(recv_id) {
            if entry.paired {
                drop(reg_read);
                drop(reg);
                let session_res = state
                    .receiver_manager
                    .start_session(
                        recv_id,
                        &room_id.to_string(),
                        "pcm_s16le",
                        48000,
                        16,
                        2,
                        0,
                        200,
                        vol,
                    )
                    .await;
                if session_res.is_ok() {
                    active_count += 1;
                }
            }
        }
    }

    if active_count == 0 {
        return Err(v1_error(
            StatusCode::BAD_GATEWAY,
            "PLAYBACK_FAILED",
            "failed to start audio output on any receiver in the room",
        ));
    }

    let mut current = state.playback_state.write().await;
    current.track_id = Some(body.track_id);
    current.position_ms = body.position_ms.unwrap_or(0);
    current.playing = true;
    current.updated_at = chrono::Utc::now();
    let state_clone = current.clone();
    drop(current);

    let _ = state.sync_tx.send(state_clone.into());
    let _ = state.tx.send(
        serde_json::json!({
            "type": "room_play", "room_id": room_id, "track_id": body.track_id,
        })
        .to_string(),
    );

    Ok(Json(serde_json::json!({
        "status": "playing",
        "room_id": room_id,
        "active_receivers": active_count,
        "total_receivers": receiver_ids.len(),
    })))
}
