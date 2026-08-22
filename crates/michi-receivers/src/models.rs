use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReceiverInfo {
    pub service: Option<String>,
    pub name: Option<String>,
    pub device_id: Option<String>,
    #[serde(default)]
    pub server_id: Option<String>,
    #[serde(default)]
    pub id: Option<String>,
    pub version: Option<String>,
    pub api_version: Option<String>,
    pub firmware: Option<String>,
    #[serde(rename = "type")]
    pub device_type: Option<String>,
    pub roles: Option<Vec<String>>,
    pub identity_scheme: Option<String>,
    pub michi_id: Option<String>,
    pub public_key: Option<String>,
    pub auth: Option<serde_json::Value>,
    pub output: Option<serde_json::Value>,
    pub audio: Option<serde_json::Value>,
    pub supported_codecs: Option<Vec<String>>,
    pub features: Option<serde_json::Value>,
}

impl ReceiverInfo {
    pub fn get_codecs(&self) -> Vec<String> {
        if let Some(ref sc) = self.supported_codecs {
            if !sc.is_empty() {
                return sc.clone();
            }
        }
        if let Some(ref audio) = self.audio {
            if let Some(arr) = audio.get("codecs").and_then(|v| v.as_array()) {
                return arr
                    .iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect();
            }
        }
        // No fake fallback to pcm_s16le. Unknown != pcm_s16le.
        Vec::new()
    }
}

/// Discrete, structured audio capabilities negotiated with a receiver.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct DiscreteAudioCapabilities {
    pub sample_rates: Vec<u32>,
    pub bit_depths: Vec<u32>,
    pub channels: Vec<u8>,
    pub codecs: Vec<String>,
    pub transports: Vec<String>,
}

/// Pending pairing state held in memory between `/pair/start` and `/pair/confirm`.
#[derive(Debug, Clone)]
pub struct PendingReceiverPairing {
    pub pairing_id: String,
    pub receiver_base_url: String,
    pub receiver_info: ReceiverInfo,
    pub receiver_pair_session_id: String,
    pub initiator_id: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub expires_at: chrono::DateTime<chrono::Utc>,
}

/// RAM-only active session authority for an active receiver stream.
#[derive(Debug, Clone)]
pub struct ReceiverActiveSession {
    pub receiver_id: String,
    pub playback_session_id: String,
    pub receiver_session_id: String,
    pub session_token: Option<String>,
    pub device_token: Option<String>,
    pub stream_port: u16,
    pub lease_seconds: u64,
    pub heartbeat_sequence: u64,
    pub negotiated_codec: String,
    pub negotiated_sample_rate: u32,
    pub negotiated_bit_depth: u32,
    pub negotiated_channels: u32,
    pub payload_type: u8,
    pub ssrc: u32,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub last_heartbeat: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairStartResponse {
    pub status: Option<String>,
    pub session_id: Option<String>,
    pub expires_at: Option<String>,
    pub attempts_remaining: Option<u32>,
    pub server_michi_id: Option<String>,
    pub server_public_key: Option<String>,
    pub device_id: Option<String>,
    pub pairing_window_seconds: Option<u64>,
    pub expires_in: Option<u64>,
    pub nonce: Option<String>,
    pub error: Option<ErrorBody>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairConfirmResponse {
    pub status: Option<String>,
    pub token: Option<String>,
    pub refresh_token: Option<String>,
    pub expires_in: Option<u64>,
    pub device_id: Option<String>,
    pub server_id: Option<String>,
    pub controller_id: Option<String>,
    pub error: Option<ErrorBody>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStartResponse {
    #[serde(default)]
    pub status: Option<String>,
    pub session_id: Option<String>,
    pub session_token: Option<String>,
    pub lease_seconds: Option<u64>,
    pub effective: Option<serde_json::Value>,
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
    pub lease_seconds: Option<u64>,
    pub receiver_uptime_ms: Option<u64>,
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
    pub supported_sample_rates: Vec<u32>,
    pub supported_bit_depths: Vec<u32>,
    pub supported_channels: Vec<u8>,
    pub maximum_safe_volume: Option<u32>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaybackControlResponse {
    pub status: Option<String>,
    pub command: Option<String>,
    pub playing: Option<bool>,
    pub position_ms: Option<u64>,
    pub error: Option<ErrorBody>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReceiverPlaybackState {
    pub status: Option<String>,
    pub session_id: Option<String>,
    pub playing: Option<bool>,
    pub position_ms: Option<u64>,
    pub volume: Option<u32>,
    pub error: Option<ErrorBody>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRecoverResponse {
    pub status: Option<String>,
    pub session_id: Option<String>,
    pub position_ms: Option<u64>,
    pub volume: Option<u32>,
    pub playing: Option<bool>,
    pub error: Option<ErrorBody>,
}
