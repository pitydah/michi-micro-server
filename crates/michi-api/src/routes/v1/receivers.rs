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
            &format!("group {group_id} not found"),
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
            let online = e
                .last_seen
                .map(|ls| (chrono::Utc::now() - ls).num_seconds() < 180)
                .unwrap_or(false);
            serde_json::json!({
                "id": e.receiver_id,
                "receiver_id": e.receiver_id,
                "name": e.name,
                "device_type": e.device_type,
                "host": e.base_url,
                "paired": e.paired,
                "online": online,
                "session_active": e.active_session_id.is_some(),
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
            &format!("receiver not found: {id}"),
        )
    })?;
    let online = entry
        .last_seen
        .map(|ls| (chrono::Utc::now() - ls).num_seconds() < 180)
        .unwrap_or(false);
    Ok(Json(serde_json::json!({
        "id": entry.receiver_id,
        "name": entry.name,
        "device_type": entry.device_type,
        "host": entry.base_url,
        "paired": entry.paired,
        "online": online,
        "session_active": entry.active_session_id.is_some(),
        "capabilities": entry.capabilities,
        "max_sample_rate": entry.max_sample_rate,
        "max_bit_depth": entry.max_bit_depth,
        "supported_codecs": entry.supported_codecs,
        "active_session_id": entry.active_session_id,
        "last_seen": entry.last_seen,
    })))
}

#[derive(Debug, Deserialize)]
pub struct ReceiverPairStartBody {
    pub base_url: String,
    pub initiator_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ReceiverPairConfirmBody {
    pub pairing_id: String,
    pub pin: String,
}

pub async fn receiver_pair_start_handler(
    State(state): State<AppState>,
    Json(body): Json<ReceiverPairStartBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let initiator_id = body
        .initiator_id
        .unwrap_or_else(|| "michi-micro-server".into());

    match state
        .receiver_manager
        .start_pairing(&body.base_url, &initiator_id)
        .await
    {
        Ok(pending) => Ok(Json(serde_json::json!({
            "status": "pending_confirmation",
            "pairing_id": pending.pairing_id,
            "receiver_base_url": pending.receiver_base_url,
            "receiver_pair_session_id": pending.receiver_pair_session_id,
            "expires_at": pending.expires_at.to_rfc3339(),
        }))),
        Err(e) => Err(v1_error(StatusCode::BAD_REQUEST, "PAIR_START_FAILED", &e)),
    }
}

pub async fn receiver_pair_confirm_handler(
    State(state): State<AppState>,
    Json(body): Json<ReceiverPairConfirmBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if body.pin.trim().is_empty() {
        return Err(v1_error_code(
            StatusCode::BAD_REQUEST,
            michi_link::MichiLinkErrorCode::InvalidRequest,
            "PIN is required to confirm pairing",
        ));
    }

    match state
        .receiver_manager
        .confirm_pairing(&body.pairing_id, &body.pin)
        .await
    {
        Ok(device_id) => Ok(Json(serde_json::json!({
            "status": "paired",
            "device_id": device_id,
        }))),
        Err(e) => Err(v1_error(StatusCode::BAD_REQUEST, "PAIR_CONFIRM_FAILED", &e)),
    }
}

#[derive(Debug, Deserialize)]
pub struct DiscoverReceiverBody {
    pub base_url: String,
    pub initiator_id: Option<String>,
    pub pin: Option<String>,
}

pub async fn discover_receiver_handler(
    State(state): State<AppState>,
    Json(body): Json<DiscoverReceiverBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let initiator_id = body
        .initiator_id
        .unwrap_or_else(|| "michi-micro-server".into());
    let pin = match body.pin {
        Some(p) if !p.trim().is_empty() => p,
        _ => {
            return Err(v1_error_code(
                StatusCode::BAD_REQUEST,
                michi_link::MichiLinkErrorCode::InvalidRequest,
                "PIN is required to pair with receiver",
            ));
        }
    };
    match state
        .receiver_manager
        .discover_and_pair(&body.base_url, &initiator_id, &pin)
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
        Ok(resp) => {
            let rtp_local_port = state.receiver_manager.get_rtp_local_port(&id).await;
            Ok(Json(serde_json::json!({
                "status": "session_started",
                "session_id": resp.session_id,
                "stream_port": resp.stream_port,
                "buffer_ms": resp.buffer_ms,
                "ssrc": resp.ssrc,
                "transport": resp.transport,
                "codec": resp.codec,
                "sample_rate": resp.sample_rate,
                "bit_depth": resp.bit_depth,
                "channels": resp.channels,
                "rtp_local_port": rtp_local_port,
            })))
        }
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

#[derive(Debug, Deserialize)]
pub struct ReceiverTestPcmBody {
    pub pcm_base64: Option<String>,
    pub frequency_hz: Option<f32>,
    pub duration_ms: Option<usize>,
}

pub async fn receiver_stream_test_pcm_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<ReceiverTestPcmBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let pcm_bytes = if let Some(ref b64) = body.pcm_base64 {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD
            .decode(b64)
            .map_err(|e| v1_error(StatusCode::BAD_REQUEST, "INVALID_BASE64", &e.to_string()))?
    } else {
        let freq = body.frequency_hz.unwrap_or(440.0);
        let ms = body.duration_ms.unwrap_or(20);
        let frames = (48000 * ms) / 1000;
        let mut bytes = Vec::with_capacity(frames * 4);
        for i in 0..frames {
            let t = (i as f32) / 48000.0;
            let sample_f = (2.0 * std::f32::consts::PI * freq * t).sin();
            let sample_i = (sample_f * 16384.0) as i16;
            let le = sample_i.to_le_bytes();
            // Stereo
            bytes.extend_from_slice(&le);
            bytes.extend_from_slice(&le);
        }
        bytes
    };

    match state.receiver_manager.send_test_pcm(&id, &pcm_bytes).await {
        Ok(bytes_sent) => Ok(Json(serde_json::json!({
            "status": "pcm_streamed",
            "bytes_sent": bytes_sent,
            "receiver_id": id,
        }))),
        Err(e) => Err(v1_error(StatusCode::BAD_REQUEST, "STREAM_PCM_FAILED", &e)),
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

    let daemon = ServiceDaemon::new().map_err(|e| format!("mDNS daemon: {e}"))?;
    let service_type = "_michi-link._tcp.local.";
    let receiver = daemon
        .browse(service_type)
        .map_err(|e| format!("mDNS browse: {e}"))?;

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
                        let addresses: Vec<String> =
                            info.get_addresses().iter().map(|a| a.to_string()).collect();
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
    })
    .await;

    let _ = daemon.shutdown();
    Ok(discovered.into_inner().unwrap())
}

// ── Room Groups (Persistent) ─────────────────────────────────────

pub async fn list_room_groups_handler(
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

    let mut groups = Vec::new();
    for (id, name, mode_str, receiver_ids, volumes, created_at_str) in rows {
        let mode = michi_core::RoomMode::from_config_str(&mode_str);
        let created_at = chrono::DateTime::parse_from_rfc3339(&created_at_str)
            .map(|dt| dt.with_timezone(&chrono::Utc))
            .unwrap_or_else(|_| chrono::Utc::now());

        // Check if any receiver in this room has an active session
        let has_active_session = receiver_ids.iter().any(|rid| {
            reg_read
                .get(rid)
                .and_then(|e| e.active_session_id.as_ref())
                .is_some()
        });

        groups.push(michi_core::RoomGroup {
            id,
            name,
            mode,
            receiver_ids,
            volumes,
            active: has_active_session,
            chain_id: None,
            created_at,
        });
    }

    Ok(Json(serde_json::json!({ "groups": groups })))
}

#[derive(Debug, Deserialize)]
pub struct CreateRoomGroupBody {
    pub name: String,
    pub mode: Option<String>,
    pub receiver_ids: Vec<String>,
}

pub async fn create_room_group_handler(
    State(state): State<AppState>,
    Json(body): Json<CreateRoomGroupBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if body.name.trim().is_empty() {
        return Err(v1_error(
            StatusCode::BAD_REQUEST,
            "VALIDATION_ERROR",
            "name is required",
        ));
    }
    let mode = michi_core::RoomMode::from_config_str(body.mode.as_deref().unwrap_or("party"));
    let default_vol = match mode {
        michi_core::RoomMode::Party => 80,
        michi_core::RoomMode::Relax => 40,
        michi_core::RoomMode::Custom => 60,
    };
    let volumes: HashMap<String, u32> = body
        .receiver_ids
        .iter()
        .map(|id| (id.clone(), default_vol))
        .collect();

    let new_id = Uuid::new_v4();
    let mode_str = match mode {
        michi_core::RoomMode::Party => "party",
        michi_core::RoomMode::Relax => "relax",
        michi_core::RoomMode::Custom => "custom",
    };

    michi_db::save_room_group_db(
        &state.db,
        &new_id,
        body.name.trim(),
        mode_str,
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

    let group = michi_core::RoomGroup {
        id: new_id,
        name: body.name.trim().to_string(),
        mode,
        receiver_ids: body.receiver_ids,
        volumes,
        active: false,
        chain_id: None,
        created_at: chrono::Utc::now(),
    };
    Ok(Json(serde_json::json!({ "group": group })))
}

pub async fn get_room_group_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
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

    let found = rows.into_iter().find(|(gid, _, _, _, _, _)| *gid == id);
    match found {
        Some((gid, name, mode_str, receiver_ids, volumes, created_at_str)) => {
            let mode = michi_core::RoomMode::from_config_str(&mode_str);
            let created_at = chrono::DateTime::parse_from_rfc3339(&created_at_str)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .unwrap_or_else(|_| chrono::Utc::now());

            let reg = state.receiver_manager.registry().await;
            let reg_read = reg.read().await;
            let active = receiver_ids.iter().any(|rid| {
                reg_read
                    .get(rid)
                    .and_then(|e| e.active_session_id.as_ref())
                    .is_some()
            });

            let group = michi_core::RoomGroup {
                id: gid,
                name,
                mode,
                receiver_ids,
                volumes,
                active,
                chain_id: None,
                created_at,
            };
            Ok(Json(serde_json::json!({ "group": group })))
        }
        None => Err(v1_error(
            StatusCode::NOT_FOUND,
            "NOT_FOUND",
            "group not found",
        )),
    }
}

#[derive(Debug, Deserialize)]
pub struct UpdateRoomGroupBody {
    pub name: Option<String>,
    pub mode: Option<String>,
    pub receiver_ids: Option<Vec<String>>,
}

pub async fn update_room_group_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateRoomGroupBody>,
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

    let found = rows.into_iter().find(|(gid, _, _, _, _, _)| *gid == id);
    match found {
        Some((gid, mut name, mut mode_str, mut receiver_ids, mut volumes, _)) => {
            if let Some(n) = body.name {
                name = n;
            }
            if let Some(m) = body.mode {
                mode_str = m;
            }
            if let Some(rids) = body.receiver_ids {
                receiver_ids = rids;
                let default_vol = match mode_str.as_str() {
                    "party" => 80,
                    "relax" => 40,
                    _ => 60,
                };
                volumes = receiver_ids
                    .iter()
                    .map(|rid| (rid.clone(), default_vol))
                    .collect();
            }

            michi_db::save_room_group_db(
                &state.db,
                &gid,
                &name,
                &mode_str,
                &receiver_ids,
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

            let group = michi_core::RoomGroup {
                id: gid,
                name,
                mode: michi_core::RoomMode::from_config_str(&mode_str),
                receiver_ids,
                volumes,
                active: false,
                chain_id: None,
                created_at: chrono::Utc::now(),
            };
            Ok(Json(serde_json::json!({ "group": group })))
        }
        None => Err(v1_error(
            StatusCode::NOT_FOUND,
            "NOT_FOUND",
            "group not found",
        )),
    }
}

pub async fn delete_room_group_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let deleted = michi_db::delete_room_group_db(&state.db, &id)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;

    if !deleted {
        return Err(v1_error(
            StatusCode::NOT_FOUND,
            "NOT_FOUND",
            "group not found",
        ));
    }
    Ok(Json(serde_json::json!({ "status": "deleted" })))
}

pub async fn activate_room_group_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
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

    let found = rows.into_iter().find(|(gid, _, _, _, _, _)| *gid == id);
    let (_gid, name, mode_str, recv_ids, vols, _) =
        found.ok_or_else(|| v1_error(StatusCode::NOT_FOUND, "NOT_FOUND", "group not found"))?;

    if recv_ids.is_empty() {
        return Err(v1_error(
            StatusCode::BAD_REQUEST,
            "INVALID_ROOM",
            "cannot activate an empty room group with 0 receivers",
        ));
    }

    let mode = michi_core::RoomMode::from_config_str(&mode_str);
    let mut receiver_results = Vec::new();
    let mut success_count = 0usize;

    for recv_id in &recv_ids {
        let vol = vols.get(recv_id).copied().unwrap_or(match mode {
            michi_core::RoomMode::Party => 80,
            michi_core::RoomMode::Relax => 40,
            michi_core::RoomMode::Custom => 60,
        });

        let capped_vol = {
            let reg_a = state.receiver_manager.registry().await;
            let reg_read_a = reg_a.read().await;
            let max_safe = reg_read_a.get(recv_id).and_then(|e| e.maximum_safe_volume);
            max_safe.map(|max| vol.min(max)).unwrap_or(vol)
        };

        let entry_info = {
            let reg_b = state.receiver_manager.registry().await;
            let reg_read_b = reg_b.read().await;
            reg_read_b
                .get(recv_id)
                .map(|e| (e.paired, e.active_session_id.is_none()))
        };

        if let Some((paired, session_is_none)) = entry_info {
            if !paired {
                receiver_results.push(serde_json::json!({
                    "receiver_id": recv_id,
                    "status": "failed",
                    "error": { "code": "NOT_PAIRED", "message": "receiver is not paired" }
                }));
                continue;
            }

            let session_ok = if session_is_none {
                state
                    .receiver_manager
                    .start_session(
                        recv_id,
                        &id.to_string(),
                        "pcm_s16le",
                        48000,
                        16,
                        2,
                        0,
                        200,
                        capped_vol,
                    )
                    .await
                    .is_ok()
            } else {
                true
            };

            if session_ok {
                let _ = state.receiver_manager.set_volume(recv_id, capped_vol).await;
                success_count += 1;
                receiver_results.push(serde_json::json!({
                    "receiver_id": recv_id,
                    "status": "active",
                    "volume": capped_vol,
                }));
            } else {
                receiver_results.push(serde_json::json!({
                    "receiver_id": recv_id,
                    "status": "failed",
                    "error": { "code": "SESSION_FAILED", "message": "Failed to start receiver session" }
                }));
            }
        } else {
            receiver_results.push(serde_json::json!({
                "receiver_id": recv_id,
                "status": "failed",
                "error": { "code": "NOT_FOUND", "message": "receiver not found in registry" }
            }));
        }
    }

    if success_count == 0 {
        return Err(v1_error(
            StatusCode::BAD_GATEWAY,
            "ROOM_ACTIVATION_FAILED",
            "failed to activate any receivers in room group",
        ));
    }

    let overall_status = if success_count == recv_ids.len() {
        "active"
    } else {
        "partial"
    };

    Ok(Json(serde_json::json!({
        "status": overall_status,
        "group_id": id,
        "group_name": name,
        "active": true,
        "successful_receivers": success_count,
        "total_receivers": recv_ids.len(),
        "receivers": receiver_results,
    })))
}

pub async fn deactivate_room_group_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
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

    let found = rows.into_iter().find(|(gid, _, _, _, _, _)| *gid == id);
    let (_gid, _name, _mode_str, recv_ids, _, _) =
        found.ok_or_else(|| v1_error(StatusCode::NOT_FOUND, "NOT_FOUND", "group not found"))?;

    let mut stopped_count = 0usize;
    let mut failed_count = 0usize;
    let mut per_link = Vec::new();

    for recv_id in &recv_ids {
        let reg = state.receiver_manager.registry().await;
        let reg_read = reg.read().await;
        if let Some(entry) = reg_read.get(recv_id) {
            if entry.active_session_id.is_some() {
                drop(reg_read);
                drop(reg);
                match state.receiver_manager.stop_session(recv_id).await {
                    Ok(_) => {
                        stopped_count += 1;
                        per_link.push(
                            serde_json::json!({ "receiver_id": recv_id, "status": "stopped" }),
                        );
                    }
                    Err(e) => {
                        failed_count += 1;
                        per_link.push(serde_json::json!({ "receiver_id": recv_id, "status": "failed", "error": e.to_string() }));
                    }
                }
            } else {
                stopped_count += 1;
                per_link.push(
                    serde_json::json!({ "receiver_id": recv_id, "status": "already_inactive" }),
                );
            }
        }
    }

    let status = if failed_count == 0 {
        "deactivated"
    } else if stopped_count > 0 {
        "partial"
    } else {
        "failed"
    };

    Ok(Json(serde_json::json!({
        "status": status,
        "group_id": id,
        "stopped_count": stopped_count,
        "failed_count": failed_count,
        "receivers": per_link,
    })))
}

#[derive(Debug, Deserialize)]
pub struct SetRoomModeBody {
    pub mode: String,
}

pub async fn set_room_mode_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<SetRoomModeBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let new_mode = michi_core::RoomMode::from_config_str(&body.mode);
    let mode_str = match new_mode {
        michi_core::RoomMode::Party => "party",
        michi_core::RoomMode::Relax => "relax",
        michi_core::RoomMode::Custom => "custom",
    };

    let rows = michi_db::list_room_groups_db(&state.db)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;

    let found = rows.into_iter().find(|(gid, _, _, _, _, _)| *gid == id);
    match found {
        Some((gid, name, _, receiver_ids, mut volumes, _)) => {
            let default_vol = match new_mode {
                michi_core::RoomMode::Party => 80,
                michi_core::RoomMode::Relax => 40,
                michi_core::RoomMode::Custom => 60,
            };
            for vol in volumes.values_mut() {
                *vol = default_vol;
            }

            michi_db::save_room_group_db(&state.db, &gid, &name, mode_str, &receiver_ids, &volumes)
                .await
                .map_err(|e| {
                    v1_error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "DATABASE_ERROR",
                        &e.to_string(),
                    )
                })?;

            let group = michi_core::RoomGroup {
                id: gid,
                name,
                mode: new_mode,
                receiver_ids,
                volumes,
                active: false,
                chain_id: None,
                created_at: chrono::Utc::now(),
            };
            Ok(Json(
                serde_json::json!({ "status": "mode_updated", "group": group }),
            ))
        }
        None => Err(v1_error(
            StatusCode::NOT_FOUND,
            "NOT_FOUND",
            "group not found",
        )),
    }
}
