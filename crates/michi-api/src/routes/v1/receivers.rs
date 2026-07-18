use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
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

// ── Speaker group management ─────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeakerGroup {
    pub id: String,
    pub name: String,
    pub receiver_ids: Vec<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

lazy_static::lazy_static! {
    static ref SPEAKER_GROUPS: Arc<RwLock<Vec<SpeakerGroup>>> = Arc::new(RwLock::new(Vec::new()));
}

pub async fn list_groups_handler() -> Json<serde_json::Value> {
    let groups = SPEAKER_GROUPS.read().await;
    Json(serde_json::json!({ "groups": groups.clone() }))
}

#[derive(Debug, Deserialize)]
pub struct CreateGroupBody {
    pub name: String,
    pub receiver_ids: Vec<String>,
}

pub async fn create_group_handler(
    Json(body): Json<CreateGroupBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if body.name.trim().is_empty() {
        return Err(v1_error(
            StatusCode::BAD_REQUEST,
            "VALIDATION_ERROR",
            "group name is required",
        ));
    }
    let mut groups = SPEAKER_GROUPS.write().await;
    let group = SpeakerGroup {
        id: uuid::Uuid::new_v4().to_string(),
        name: body.name,
        receiver_ids: body.receiver_ids,
        created_at: chrono::Utc::now(),
    };
    groups.push(group.clone());
    Ok(Json(serde_json::json!({ "group": group })))
}

#[derive(Debug, Deserialize)]
pub struct SyncGroupBody {
    pub track_id: String,
    pub position_ms: u64,
    pub playing: bool,
}

pub async fn sync_group_handler(
    Path(group_id): Path<String>,
    Json(body): Json<SyncGroupBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let groups = SPEAKER_GROUPS.read().await;
    let group = groups.iter().find(|g| g.id == group_id).cloned();
    match group {
        Some(g) => Ok(Json(serde_json::json!({
            "status": "sync_initiated",
            "group": g.name,
            "receivers": g.receiver_ids,
            "track_id": body.track_id,
            "position_ms": body.position_ms,
            "playing": body.playing,
        }))),
        None => Err(v1_error(
            StatusCode::NOT_FOUND,
            "GROUP_NOT_FOUND",
            &format!("group {} not found", group_id),
        )),
    }
}

// ── Existing receivers CRUD ─────────────────────────────────────

pub async fn receivers_handler(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let reg = state.receiver_manager.registry().await;
    let reg_read = reg.read().await;
    let receivers: Vec<serde_json::Value> = reg_read
        .list()
        .iter()
        .map(|e| {
            serde_json::json!({
                "id": e.receiver_id,
                "name": e.name,
                "device_type": e.device_type,
                "host": e.base_url,
                "paired": e.paired,
                "online": e.active_session_id.is_none() && e.last_seen.map(|ls| {
                    (chrono::Utc::now() - ls).num_seconds() < 180
                }).unwrap_or(false),
                "capabilities": e.capabilities,
                "active_session_id": e.active_session_id,
                "last_seen": e.last_seen,
            })
        })
        .collect();
    Ok(Json(serde_json::json!({ "receivers": receivers })))
}

pub async fn get_receiver_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let reg = state.receiver_manager.registry().await;
    let reg_read = reg.read().await;
    let entry = reg_read.get(&id).ok_or_else(|| {
        v1_error(
            StatusCode::NOT_FOUND,
            "NOT_FOUND",
            &format!("receiver not found: {}", id),
        )
    })?;
    Ok(Json(serde_json::json!({
        "id": entry.receiver_id,
        "name": entry.name,
        "device_type": entry.device_type,
        "host": entry.base_url,
        "paired": entry.paired,
        "capabilities": entry.capabilities,
        "max_sample_rate": entry.max_sample_rate,
        "max_bit_depth": entry.max_bit_depth,
        "supported_codecs": entry.supported_codecs,
        "active_session_id": entry.active_session_id,
        "last_seen": entry.last_seen,
    })))
}

#[derive(Debug, Deserialize)]
pub struct DiscoverReceiverBody {
    pub base_url: String,
    pub initiator_id: Option<String>,
}

pub async fn discover_receiver_handler(
    State(state): State<AppState>,
    Json(body): Json<DiscoverReceiverBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let initiator_id = body
        .initiator_id
        .unwrap_or_else(|| "michi-micro-server".into());
    match state
        .receiver_manager
        .discover_and_pair(&body.base_url, &initiator_id)
        .await
    {
        Ok(device_id) => Ok(Json(serde_json::json!({
            "status": "paired",
            "device_id": device_id,
        }))),
        Err(e) => Err(v1_error(StatusCode::BAD_REQUEST, "DISCOVERY_FAILED", &e)),
    }
}

#[derive(Debug, Deserialize)]
pub struct ReceiverSessionStartBody {
    pub session_id: String,
    pub codec: String,
    pub sample_rate: u32,
    pub bit_depth: u32,
    pub channels: u32,
    pub stream_port: u16,
    pub buffer_ms: u64,
    pub volume: u32,
}

pub async fn receiver_session_start_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<ReceiverSessionStartBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    match state
        .receiver_manager
        .start_session(
            &id,
            &body.session_id,
            &body.codec,
            body.sample_rate,
            body.bit_depth,
            body.channels,
            body.stream_port,
            body.buffer_ms,
            body.volume,
        )
        .await
    {
        Ok(resp) => Ok(Json(serde_json::json!({
            "status": resp.status,
            "session_id": resp.session_id,
            "stream_port": resp.stream_port,
            "buffer_ms": resp.buffer_ms,
        }))),
        Err(e) => Err(v1_error(
            StatusCode::BAD_REQUEST,
            "SESSION_START_FAILED",
            &e,
        )),
    }
}

pub async fn receiver_session_stop_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    match state.receiver_manager.stop_session(&id).await {
        Ok(resp) => Ok(Json(
            serde_json::json!({ "status": resp.status, "session_id": resp.session_id }),
        )),
        Err(e) => Err(v1_error(StatusCode::BAD_REQUEST, "SESSION_STOP_FAILED", &e)),
    }
}

#[derive(Debug, Deserialize)]
pub struct ReceiverVolumeBody {
    pub volume: u32,
}

pub async fn receiver_volume_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<ReceiverVolumeBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    match state.receiver_manager.set_volume(&id, body.volume).await {
        Ok(resp) => Ok(Json(
            serde_json::json!({ "status": "ok", "volume": resp.volume }),
        )),
        Err(e) => Err(v1_error(StatusCode::BAD_REQUEST, "VOLUME_FAILED", &e)),
    }
}

pub async fn receiver_heartbeat_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    match state.receiver_manager.heartbeat(&id).await {
        Ok(resp) => Ok(Json(
            serde_json::json!({ "status": resp.status, "uptime_seconds": resp.uptime_seconds }),
        )),
        Err(e) => Err(v1_error(StatusCode::BAD_REQUEST, "HEARTBEAT_FAILED", &e)),
    }
}

pub async fn discover_mdns_handler(
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    match discover_mdns_receivers().await {
        Ok(receivers) => Ok(Json(serde_json::json!({ "receivers": receivers }))),
        Err(e) => Err(v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DISCOVERY_FAILED",
            &e,
        )),
    }
}

async fn discover_mdns_receivers() -> Result<Vec<serde_json::Value>, String> {
    use mdns_sd::{ServiceDaemon, ServiceEvent};
    use std::time::Duration;

    let daemon = ServiceDaemon::new().map_err(|e| format!("mDNS daemon: {}", e))?;
    let service_type = "_michi-receiver._tcp.local.";
    let receiver = daemon
        .browse(service_type)
        .map_err(|e| format!("mDNS browse: {}", e))?;

    let result: Vec<serde_json::Value> = Vec::new();
    let discovered = std::sync::Mutex::new(result);
    let _ = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if let Ok(event) = receiver.recv_async().await {
                match event {
                    ServiceEvent::ServiceResolved(info) => {
                        let host = info.get_hostname().to_string();
                        let port = info.get_port();
                        let fullname = info.get_fullname().to_string();
                        let addresses: Vec<String> = info.get_addresses().iter().map(|a| a.to_string()).collect();
                        let addr = format!("http://{}:{}", host.trim_end_matches('.'), port);
                        discovered.lock().unwrap().push(serde_json::json!({
                            "name": fullname,
                            "host": addr,
                            "port": port,
                            "addresses": addresses,
                        }));
                    }
                    ServiceEvent::ServiceRemoved(_, _) => {}
                    _ => {}
                }
            }
        }
    }).await;

    let _ = daemon.shutdown();
    Ok(discovered.into_inner().unwrap())
}

// ── Room Groups ──────────────────────────────────────────────────

lazy_static::lazy_static! {
    static ref ROOM_GROUPS: Arc<RwLock<Vec<michi_core::RoomGroup>>> = Arc::new(RwLock::new(Vec::new()));
}

pub async fn list_room_groups_handler() -> Json<serde_json::Value> {
    let groups = ROOM_GROUPS.read().await;
    Json(serde_json::json!({ "groups": groups.clone() }))
}

#[derive(Debug, Deserialize)]
pub struct CreateRoomGroupBody {
    pub name: String,
    pub mode: Option<String>,
    pub receiver_ids: Vec<String>,
}

pub async fn create_room_group_handler(
    Json(body): Json<CreateRoomGroupBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if body.name.trim().is_empty() {
        return Err(v1_error(StatusCode::BAD_REQUEST, "VALIDATION_ERROR", "name is required"));
    }
    let mode = michi_core::RoomMode::from_str(body.mode.as_deref().unwrap_or("party"));
    let default_vol = match mode {
        michi_core::RoomMode::Party => 80,
        michi_core::RoomMode::Relax => 40,
        michi_core::RoomMode::Custom => 60,
    };
    let volumes: HashMap<String, u32> = body.receiver_ids.iter().map(|id| (id.clone(), default_vol)).collect();

    let group = michi_core::RoomGroup {
        id: Uuid::new_v4(),
        name: body.name.trim().to_string(),
        mode,
        receiver_ids: body.receiver_ids,
        volumes,
        active: false,
        chain_id: None,
        created_at: chrono::Utc::now(),
    };
    ROOM_GROUPS.write().await.push(group.clone());
    Ok(Json(serde_json::json!({ "group": group })))
}

pub async fn get_room_group_handler(
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let groups = ROOM_GROUPS.read().await;
    let group = groups.iter().find(|g| g.id == id).cloned();
    match group {
        Some(g) => Ok(Json(serde_json::json!({ "group": g }))),
        None => Err(v1_error(StatusCode::NOT_FOUND, "NOT_FOUND", "group not found")),
    }
}

#[derive(Debug, Deserialize)]
pub struct UpdateRoomGroupBody {
    pub name: Option<String>,
    pub mode: Option<String>,
    pub receiver_ids: Option<Vec<String>>,
}

pub async fn update_room_group_handler(
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateRoomGroupBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let mut groups = ROOM_GROUPS.write().await;
    let group = groups.iter_mut().find(|g| g.id == id);
    match group {
        Some(g) => {
            if let Some(name) = body.name { g.name = name; }
            if let Some(mode_str) = body.mode {
                let new_mode = michi_core::RoomMode::from_str(&mode_str);
                g.mode = new_mode;
            }
            if let Some(ids) = body.receiver_ids { g.receiver_ids = ids; }
            Ok(Json(serde_json::json!({ "group": g.clone() })))
        }
        None => Err(v1_error(StatusCode::NOT_FOUND, "NOT_FOUND", "group not found")),
    }
}

pub async fn delete_room_group_handler(
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let mut groups = ROOM_GROUPS.write().await;
    let len_before = groups.len();
    groups.retain(|g| g.id != id);
    if groups.len() == len_before {
        return Err(v1_error(StatusCode::NOT_FOUND, "NOT_FOUND", "group not found"));
    }
    Ok(Json(serde_json::json!({ "status": "deleted" })))
}

pub async fn activate_room_group_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let (recv_ids, vols, mode, group_clone, was_active) = {
        let mut groups = ROOM_GROUPS.write().await;
        let group = groups.iter_mut().find(|g| g.id == id);
        match group {
            Some(g) => {
                if g.active {
                    return Ok(Json(serde_json::json!({ "status": "already_active", "group": g.clone() })));
                }
                g.active = true;
                (g.receiver_ids.clone(), g.volumes.clone(), g.mode, g.clone(), false)
            }
            None => return Err(v1_error(StatusCode::NOT_FOUND, "NOT_FOUND", "group not found")),
        }
    };

    for recv_id in &recv_ids {
        let vol = vols.get(recv_id).copied().unwrap_or(match mode {
            michi_core::RoomMode::Party => 80,
            michi_core::RoomMode::Relax => 40,
            michi_core::RoomMode::Custom => 60,
        });
        let reg = state.receiver_manager.registry().await;
        let reg_read = reg.read().await;
        if let Some(entry) = reg_read.get(recv_id) {
            if entry.paired && entry.active_session_id.is_none() {
                let _ = state.receiver_manager
                    .start_session(recv_id, &id.to_string(), "pcm", 48000, 24, 2, 0, 200, vol)
                    .await;
            }
            let _ = state.receiver_manager.set_volume(recv_id, vol).await;
        }
    }

    Ok(Json(serde_json::json!({ "status": "activated", "group": group_clone })))
}

pub async fn deactivate_room_group_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let mut groups = ROOM_GROUPS.write().await;
    let group = groups.iter_mut().find(|g| g.id == id);
    match group {
        Some(g) => {
            if !g.active {
                return Ok(Json(serde_json::json!({ "status": "already_inactive" })));
            }
            let recv_ids = g.receiver_ids.clone();
            g.active = false;
            drop(groups);

            for recv_id in &recv_ids {
                let reg = state.receiver_manager.registry().await;
                let reg_read = reg.read().await;
                if let Some(entry) = reg_read.get(recv_id) {
                    if entry.active_session_id.is_some() {
                        let _ = state.receiver_manager.stop_session(recv_id).await;
                    }
                }
            }
            Ok(Json(serde_json::json!({ "status": "deactivated" })))
        }
        None => Err(v1_error(StatusCode::NOT_FOUND, "NOT_FOUND", "group not found")),
    }
}

#[derive(Debug, Deserialize)]
pub struct SetRoomModeBody {
    pub mode: String,
}

pub async fn set_room_mode_handler(
    Path(id): Path<Uuid>,
    Json(body): Json<SetRoomModeBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let new_mode = michi_core::RoomMode::from_str(&body.mode);
    let mut groups = ROOM_GROUPS.write().await;
    let group = groups.iter_mut().find(|g| g.id == id);
    match group {
        Some(g) => {
            g.mode = new_mode;
            let default_vol = match g.mode {
                michi_core::RoomMode::Party => 80,
                michi_core::RoomMode::Relax => 40,
                michi_core::RoomMode::Custom => 60,
            };
            for (_, vol) in g.volumes.iter_mut() {
                *vol = default_vol;
            }
            Ok(Json(serde_json::json!({ "status": "mode_updated", "group": g.clone() })))
        }
        None => Err(v1_error(StatusCode::NOT_FOUND, "NOT_FOUND", "group not found")),
    }
}
