use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::AppState;

pub async fn receivers_handler(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let receivers = michi_db::list_receivers(&state.db).await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
            "error": "database_error",
            "message": e.to_string()
        })))
    })?;

    let result: Vec<serde_json::Value> = receivers
        .into_iter()
        .map(|r| {
            let capabilities: Vec<String> =
                serde_json::from_str(&r.capabilities_json).unwrap_or_default();
            serde_json::json!({
                "id": r.id,
                "name": r.name,
                "device_type": r.device_type,
                "host": r.host,
                "port": r.port,
                "capabilities": capabilities,
                "online": r.online,
                "last_seen": r.last_seen,
            })
        })
        .collect();

    Ok(Json(serde_json::json!({ "receivers": result })))
}

pub async fn get_receiver_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let receiver = michi_db::get_receiver(&state.db, &id).await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
            "error": "database_error",
            "message": e.to_string()
        })))
    })?
    .ok_or_else(|| {
        (StatusCode::NOT_FOUND, Json(serde_json::json!({
            "error": "not_found",
            "message": format!("receiver not found: {}", id)
        })))
    })?;

    let capabilities: Vec<String> =
        serde_json::from_str(&receiver.capabilities_json).unwrap_or_default();

    Ok(Json(serde_json::json!({
        "id": receiver.id,
        "name": receiver.name,
        "device_type": receiver.device_type,
        "host": receiver.host,
        "port": receiver.port,
        "capabilities": capabilities,
        "online": receiver.online,
        "last_seen": receiver.last_seen,
    })))
}

#[derive(Debug, Deserialize)]
pub struct RegisterReceiverBody {
    pub name: String,
    pub device_type: String,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub capabilities: Option<Vec<String>>,
}

pub async fn register_receiver_handler(
    State(state): State<AppState>,
    Json(body): Json<RegisterReceiverBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let id = Uuid::new_v4();
    let capabilities_json = serde_json::to_string(&body.capabilities.unwrap_or_default())
        .unwrap_or_default();

    let receiver = michi_core::ReceiverDb {
        id,
        name: body.name,
        device_type: body.device_type,
        host: body.host,
        port: body.port,
        capabilities_json,
        online: true,
        last_seen: Some(chrono::Utc::now().to_rfc3339()),
    };

    michi_db::upsert_receiver(&state.db, &receiver).await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
            "error": "database_error",
            "message": e.to_string()
        })))
    })?;

    Ok(Json(serde_json::json!({
        "id": id,
        "status": "registered",
    })))
}
