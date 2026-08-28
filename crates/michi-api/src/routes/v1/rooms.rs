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
        })?
        .ok_or_else(|| {
            v1_error(
                StatusCode::NOT_FOUND,
                "TRACK_NOT_FOUND",
                "track not found in library",
            )
        })?;

    // Find room group
    let groups = michi_db::list_room_groups_db(&state.db)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;

    let found = groups
        .into_iter()
        .find(|(gid, name, _, _, _, _)| gid.to_string() == id || name == &id);

    let (gid, group_name, _mode, _recv_ids, _vols, _created_at) = found.ok_or_else(|| {
        v1_error(
            StatusCode::NOT_FOUND,
            "ROOM_NOT_FOUND",
            &format!("room {id} not found"),
        )
    })?;

    let selection = crate::output::PlaybackOutputSelection::RoomGroup { id: gid };
    let plan = crate::output::resolve_output(&selection, &state)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::BAD_GATEWAY,
                e.error_code(),
                &e.to_string(),
            )
        })?;

    let pos_ms = body.position_ms.unwrap_or(0);
    state
        .playback_engine
        .play(track, plan.sinks, plan.description.clone(), pos_ms)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                e.error_code(),
                &e.to_string(),
            )
        })?;

    *state.playback_output_selection.write().await = Some(selection);

    Ok(Json(serde_json::json!({
        "status": "accepted",
        "lifecycle": "preparing",
        "room_id": gid,
        "room_name": group_name,
        "output": plan.description,
    })))
}
