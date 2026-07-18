use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

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

    let mut discovered = Vec::new();
    let timeout = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if let Ok(event) = receiver.recv_async().await {
                match event {
                    ServiceEvent::ServiceResolved(info) => {
                        let host = info.get_hostname();
                        let port = info.get_port();
                        let addr = format!("http://{}:{}", host.trim_end_matches('.'), port);
                        discovered.push(serde_json::json!({
                            "name": info.get_fullname(),
                            "host": addr,
                            "port": port,
                            "addresses": info.get_addresses().iter().map(|a| a.to_string()).collect::<Vec<_>>(),
                        }));
                    }
                    ServiceEvent::ServiceRemoved(_, _) => {}
                    _ => {}
                }
            }
        }
    });

    let _ = timeout.await;
    let _ = daemon.shutdown();
    Ok(discovered)
}
