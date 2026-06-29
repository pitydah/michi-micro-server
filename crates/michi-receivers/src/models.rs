use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReceiverInfo {
    pub id: Uuid,
    pub name: String,
    pub device_type: String,
    pub host: String,
    pub port: u16,
    pub capabilities: Vec<String>,
    pub online: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairStartResponse {
    pub pairing_id: Uuid,
    pub code: String,
    pub expires_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairConfirmRequest {
    pub code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairConfirmResponse {
    pub device_token: String,
    pub refresh_token: String,
    pub device_id: Uuid,
    pub alias: String,
    pub permissions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatRequest {
    pub device_id: Uuid,
    pub status: String,
    pub uptime_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStartRequest {
    pub track_id: Uuid,
    pub stream_url: String,
    pub position_ms: u64,
    pub volume: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStopRequest {
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeRequest {
    pub volume: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiResponse {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ApiError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiError {
    pub code: String,
    pub message: String,
}
