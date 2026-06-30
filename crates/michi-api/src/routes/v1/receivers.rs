use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;

use crate::AppState;

fn v1_error(status: StatusCode, code: &str, message: &str) -> (StatusCode, Json<serde_json::Value>) {
    (status, Json(serde_json::json!({
        "error": { "code": code, "message": message, "details": {} }
    })))
}

pub async fn receivers_handler(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let reg = state.receiver_manager.registry().await;
    let reg_read = reg.read().await;
    let receivers: Vec<serde_json::Value> = reg_read.list().iter().map(|e| {
        serde_json::json!({
            "id": e.receiver_id,
            "name": e.name,
            "device_type": e.device_type,
            "host": e.base_url,
            "paired": e.paired,
            "online": !e.active_session_id.is_some() && e.last_seen.map(|ls| {
                (chrono::Utc::now() - ls).num_seconds() < 180
            }).unwrap_or(false),
            "capabilities": e.capabilities,
            "active_session_id": e.active_session_id,
            "last_seen": e.last_seen,
        })
    }).collect();
    Ok(Json(serde_json::json!({ "receivers": receivers })))
}

pub async fn get_receiver_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let reg = state.receiver_manager.registry().await;
    let reg_read = reg.read().await;
    let entry = reg_read.get(&id).ok_or_else(|| {
        v1_error(StatusCode::NOT_FOUND, "NOT_FOUND", &format!("receiver not found: {}", id))
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
    let initiator_id = body.initiator_id.unwrap_or_else(|| "michi-micro-server".into());
    match state.receiver_manager.discover_and_pair(&body.base_url, &initiator_id).await {
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
    match state.receiver_manager.start_session(
        &id, &body.session_id, &body.codec, body.sample_rate,
        body.bit_depth, body.channels, body.stream_port, body.buffer_ms, body.volume,
    ).await {
        Ok(resp) => Ok(Json(serde_json::json!({
            "status": resp.status,
            "session_id": resp.session_id,
            "stream_port": resp.stream_port,
            "buffer_ms": resp.buffer_ms,
        }))),
        Err(e) => Err(v1_error(StatusCode::BAD_REQUEST, "SESSION_START_FAILED", &e)),
    }
}

pub async fn receiver_session_stop_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    match state.receiver_manager.stop_session(&id).await {
        Ok(resp) => Ok(Json(serde_json::json!({ "status": resp.status, "session_id": resp.session_id }))),
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
        Ok(resp) => Ok(Json(serde_json::json!({ "status": "ok", "volume": resp.volume }))),
        Err(e) => Err(v1_error(StatusCode::BAD_REQUEST, "VOLUME_FAILED", &e)),
    }
}

pub async fn receiver_heartbeat_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    match state.receiver_manager.heartbeat(&id).await {
        Ok(resp) => Ok(Json(serde_json::json!({ "status": resp.status, "uptime_seconds": resp.uptime_seconds }))),
        Err(e) => Err(v1_error(StatusCode::BAD_REQUEST, "HEARTBEAT_FAILED", &e)),
    }
}
