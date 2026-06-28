use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::AppState;

#[derive(Debug, Serialize)]
pub struct Capabilities {
    pub version: u32,
    pub features: Vec<String>,
    pub max_manifest_items: u32,
    pub pairing_ttl_seconds: u32,
}

#[derive(Debug, Serialize)]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    pub revoked: bool,
    pub last_seen: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ManifestTrackInfo {
    pub track_id: Uuid,
    pub file_path: String,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration_ms: Option<i64>,
    pub artwork_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct PairStartRequest {
    pub device_name: String,
}

#[derive(Debug, Serialize)]
pub struct PairStartResponse {
    pub code: String,
    pub expires_at: String,
}

#[derive(Debug, Deserialize)]
pub struct PairConfirmRequest {
    pub code: String,
}
#[derive(Debug, Serialize)]
pub struct SyncManifestResponse {
    pub tracks: Vec<ManifestTrackInfo>,
    pub total: usize,
}

fn ok<T: Serialize>(data: T) -> (StatusCode, Json<T>) {
    (StatusCode::OK, Json(data))
}

pub async fn capabilities_handler(
    State(_state): State<AppState>,
) -> (StatusCode, Json<Capabilities>) {
    ok(Capabilities {
        version: 1,
        features: vec![
            "pairing".into(),
            "manifest".into(),
            "sync_plan".into(),
            "discovery".into(),
        ],
        max_manifest_items: 100000,
        pairing_ttl_seconds: 300,
    })
}

pub async fn devices_handler(
    State(state): State<AppState>,
) -> Result<(StatusCode, Json<Vec<DeviceInfo>>), (StatusCode, Json<serde_json::Value>)> {
    let devices = michi_db::list_sync_devices(&state.db).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(
                serde_json::json!({"error": {"code": "INTERNAL_ERROR", "message": e.to_string()}}),
            ),
        )
    })?;
    let result: Vec<DeviceInfo> = devices
        .into_iter()
        .map(|(id, name, _device_type, revoked, last_seen)| DeviceInfo {
            id,
            name,
            revoked,
            last_seen,
        })
        .collect();
    Ok(ok(result))
}

pub async fn revoke_device_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<serde_json::Value>)> {
    michi_db::revoke_sync_device(&state.db, &id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
        })?;
    Ok(ok(serde_json::json!({"status": "revoked"})))
}

pub async fn pair_start_handler(
    State(state): State<AppState>,
    Json(body): Json<PairStartRequest>,
) -> Result<(StatusCode, Json<PairStartResponse>), (StatusCode, Json<serde_json::Value>)> {
    let code = Uuid::new_v4().to_string()[..8].to_string();
    let token_id = Uuid::new_v4();
    let expires_at = (chrono::Utc::now() + chrono::Duration::seconds(300)).to_rfc3339();

    michi_db::create_pairing_token(&state.db, &token_id, &code, &body.device_name, &expires_at)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
        })?;

    Ok(ok(PairStartResponse { code, expires_at }))
}

pub async fn pair_confirm_handler(
    State(state): State<AppState>,
    Json(body): Json<PairConfirmRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<serde_json::Value>)> {
    let result = michi_db::consume_pairing_token(&state.db, &body.code)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
        })?;

    match result {
        Some((_token_id, device_name)) => {
            let device_id = Uuid::new_v4();
            michi_db::create_sync_device(&state.db, &device_id, &device_name, "desktop", None)
                .await
                .map_err(|e| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({"error": e.to_string()})),
                    )
                })?;
            Ok(ok(serde_json::json!({
                "status": "paired",
                "device_id": device_id.to_string(),
                "device_name": device_name,
            })))
        }
        None => Err((
            StatusCode::NOT_FOUND,
            Json(
                serde_json::json!({"error": {"code": "NOT_FOUND", "message": "invalid or expired code"}}),
            ),
        )),
    }
}

pub async fn manifest_handler(
    State(state): State<AppState>,
) -> Result<(StatusCode, Json<SyncManifestResponse>), (StatusCode, Json<serde_json::Value>)> {
    let tracks = michi_db::get_all_tracks_manifest(&state.db)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
        })?;

    let manifest: Vec<ManifestTrackInfo> = tracks
        .into_iter()
        .map(
            |(track_id, file_path, title, artist, album, duration_ms, artwork_id)| {
                ManifestTrackInfo {
                    track_id,
                    file_path,
                    title,
                    artist,
                    album,
                    duration_ms,
                    artwork_id: if artwork_id.is_empty() {
                        None
                    } else {
                        Some(artwork_id)
                    },
                }
            },
        )
        .collect();

    let total = manifest.len();
    Ok(ok(SyncManifestResponse {
        tracks: manifest,
        total,
    }))
}

pub fn sync_router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/sync/capabilities", get(capabilities_handler))
        .route("/api/v1/sync/devices", get(devices_handler))
        .route("/api/v1/sync/devices/:id", delete(revoke_device_handler))
        .route("/api/v1/sync/pair/start", post(pair_start_handler))
        .route("/api/v1/sync/pair/confirm", post(pair_confirm_handler))
        .route("/api/v1/sync/manifest", get(manifest_handler))
}
