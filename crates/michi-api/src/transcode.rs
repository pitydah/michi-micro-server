use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get},
    Json, Router,
};
use serde::{Deserialize, Serialize};

use crate::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscodeProfile {
    pub id: String,
    pub name: String,
    pub codec: String,
    pub bitrate: u32,
    pub sample_rate: u32,
    pub channels: u8,
    pub description: String,
    pub is_default: bool,
}

#[derive(Debug, Deserialize)]
pub struct CreateProfileRequest {
    pub name: String,
    pub codec: String,
    pub bitrate: u32,
    #[serde(default = "default_sample_rate")]
    pub sample_rate: u32,
    #[serde(default = "default_channels")]
    pub channels: u8,
    pub description: Option<String>,
}

fn default_sample_rate() -> u32 {
    44100
}
fn default_channels() -> u8 {
    2
}

pub fn default_profiles() -> Vec<TranscodeProfile> {
    vec![
        TranscodeProfile {
            id: "direct".into(),
            name: "Direct (original)".into(),
            codec: "copy".into(),
            bitrate: 0,
            sample_rate: 0,
            channels: 0,
            description: "Stream original file without transcoding".into(),
            is_default: true,
        },
        TranscodeProfile {
            id: "lan_high".into(),
            name: "LAN High Quality".into(),
            codec: "flac".into(),
            bitrate: 0,
            sample_rate: 96000,
            channels: 2,
            description: "High quality for local network".into(),
            is_default: false,
        },
        TranscodeProfile {
            id: "remote_balanced".into(),
            name: "Remote Balanced".into(),
            codec: "mp3".into(),
            bitrate: 256,
            sample_rate: 44100,
            channels: 2,
            description: "Balanced quality for remote access".into(),
            is_default: false,
        },
        TranscodeProfile {
            id: "mobile_low".into(),
            name: "Mobile Low".into(),
            codec: "aac".into(),
            bitrate: 128,
            sample_rate: 44100,
            channels: 2,
            description: "Low bandwidth for mobile".into(),
            is_default: false,
        },
    ]
}

pub async fn profiles_handler(State(state): State<AppState>) -> Json<Vec<TranscodeProfile>> {
    let profiles = state.transcode_profiles.read().await;
    Json(profiles.clone())
}

pub async fn create_profile_handler(
    State(state): State<AppState>,
    Json(body): Json<CreateProfileRequest>,
) -> Result<Json<TranscodeProfile>, (StatusCode, Json<serde_json::Value>)> {
    let id = uuid::Uuid::new_v4().to_string();
    let profile = TranscodeProfile {
        id: id.clone(),
        name: body.name,
        codec: body.codec,
        bitrate: body.bitrate,
        sample_rate: body.sample_rate,
        channels: body.channels,
        description: body.description.unwrap_or_default(),
        is_default: false,
    };
    let mut profiles = state.transcode_profiles.write().await;
    profiles.push(profile.clone());
    Ok(Json(profile))
}

pub async fn delete_profile_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let mut profiles = state.transcode_profiles.write().await;
    profiles.retain(|p| p.id != id);
    Ok(Json(serde_json::json!({"status": "deleted"})))
}

pub fn transcode_router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/transcode/profiles",
            get(profiles_handler).post(create_profile_handler),
        )
        .route(
            "/api/v1/transcode/profiles/:id",
            delete(delete_profile_handler),
        )
}
