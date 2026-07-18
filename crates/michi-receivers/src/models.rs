use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReceiverInfo {
    pub service: Option<String>,
    pub name: Option<String>,
    pub device_id: Option<String>,
    #[serde(default)]
    pub id: Option<Uuid>,
    pub api_version: Option<String>,
    pub michi_link_version: Option<String>,
    pub firmware: Option<String>,
    #[serde(rename = "type")]
    pub device_type: Option<String>,
    pub roles: Option<Vec<String>>,
    pub auth: Option<serde_json::Value>,
    pub output: Option<serde_json::Value>,
    pub supported_codecs: Option<Vec<String>>,
    pub features: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairStartResponse {
    pub status: Option<String>,
    pub device_id: Option<String>,
    pub pairing_window_seconds: Option<u64>,
    pub nonce: Option<String>,
    pub error: Option<ErrorBody>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairConfirmResponse {
    pub status: Option<String>,
    pub device_id: Option<String>,
    pub controller_id: Option<String>,
    pub token: Option<String>,
    pub error: Option<ErrorBody>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStartResponse {
    pub status: Option<String>,
    pub session_id: Option<String>,
    pub device_id: Option<String>,
    pub stream_port: Option<u16>,
    pub buffer_ms: Option<u64>,
    pub error: Option<ErrorBody>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStopResponse {
    pub status: Option<String>,
    pub session_id: Option<String>,
    pub error: Option<ErrorBody>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatResponse {
    pub status: Option<String>,
    pub session_id: Option<String>,
    pub uptime_seconds: Option<u64>,
    pub error: Option<ErrorBody>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeResponse {
    pub status: Option<String>,
    pub volume: Option<u32>,
    pub error: Option<ErrorBody>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorBody {
    pub code: String,
    pub message: String,
    pub details: Option<serde_json::Value>,
}

// Registry

#[derive(Debug, Clone)]
pub struct ReceiverRegistryEntry {
    pub receiver_id: String,
    pub name: String,
    pub device_type: String,
    pub base_url: String,
    pub paired: bool,
    pub token: Option<String>,
    pub last_seen: Option<chrono::DateTime<chrono::Utc>>,
    pub capabilities: Vec<String>,
    pub active_session_id: Option<String>,
    pub max_sample_rate: u32,
    pub max_bit_depth: u32,
    pub supported_codecs: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ReceiverRegistry {
    pub receivers: HashMap<String, ReceiverRegistryEntry>,
}

impl ReceiverRegistry {
    pub fn new() -> Self {
        Self {
            receivers: HashMap::new(),
        }
    }

    pub fn add(&mut self, entry: ReceiverRegistryEntry) {
        self.receivers.insert(entry.receiver_id.clone(), entry);
    }

    pub fn get(&self, id: &str) -> Option<&ReceiverRegistryEntry> {
        self.receivers.get(id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut ReceiverRegistryEntry> {
        self.receivers.get_mut(id)
    }

    pub fn list(&self) -> Vec<&ReceiverRegistryEntry> {
        self.receivers.values().collect()
    }

    pub fn remove(&mut self, id: &str) {
        self.receivers.remove(id);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReceiverCapabilities {
    pub device_type: String,
    pub supported_codecs: Vec<String>,
    pub max_sample_rate: u32,
    pub max_bit_depth: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayRequest {
    pub track_id: String,
    pub stream_url: String,
    pub codec: String,
    pub sample_rate: u32,
    pub bit_depth: u32,
    pub volume: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaybackPosition {
    pub position_ms: u64,
    pub duration_ms: u64,
    pub playing: bool,
}
