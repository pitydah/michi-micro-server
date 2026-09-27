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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReceiverActiveSessionState {
    Active,
    Closing,
    Failed,
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
    pub state: ReceiverActiveSessionState,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub last_heartbeat: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReceiverProtocolError {
    pub http_status: u16,
    pub code: String,
    pub message: String,
    #[serde(default)]
    pub details: serde_json::Value,
}

impl std::fmt::Display for ReceiverProtocolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ReceiverProtocolError(status={}, code={}, message={})",
            self.http_status, self.code, self.message
        )
    }
}

impl std::error::Error for ReceiverProtocolError {}

/// Strict DTO for successfully negotiated and verified session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NegotiatedReceiverSession {
    pub session_id: String,
    pub session_token: String,
    pub lease_seconds: u64,
    pub transport: String,
    pub codec: String,
    pub sample_rate: u32,
    pub bit_depth: u32,
    pub channels: u32,
    pub packet_ms: u32,
    pub buffer_ms: u32,
    pub payload_type: u8,
    pub ssrc: u32,
    pub stream_port: u16,
    pub volume: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairStartResponse {
    pub session_id: Option<String>,
    pub expires_at: Option<String>,
    pub attempts_remaining: Option<u32>,
    pub server_michi_id: Option<String>,
    pub server_public_key: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
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
#[serde(deny_unknown_fields)]
pub struct EffectiveSessionWire {
    pub transport: String,
    pub codec: String,
    pub sample_rate: u32,
    pub bit_depth: u32,
    pub channels: u32,
    pub packet_ms: u32,
    pub buffer_ms: u32,
    pub payload_type: u8,
    pub ssrc: u32,
    pub stream_port: u16,
    pub volume: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionStartResponse {
    pub session_id: String,
    pub session_token: String,
    pub lease_seconds: u64,
    pub effective: EffectiveSessionWire,
}

impl SessionStartResponse {
    pub fn validate_strict(&self) -> Result<NegotiatedReceiverSession, String> {
        // Enforce valid UUID format for session_id
        if uuid::Uuid::parse_str(&self.session_id).is_err() {
            return Err(format!(
                "CONTRACT_VIOLATION: session_id must be a valid UUID v4, got '{}'",
                self.session_id
            ));
        }

        // Enforce frozen lease_seconds == 30
        if self.lease_seconds != 30 {
            return Err(format!(
                "CONTRACT_VIOLATION: lease_seconds must be exactly 30, got {}",
                self.lease_seconds
            ));
        }

        // Validate session_token pattern ^[A-Za-z0-9_-]{43}$
        if self.session_token.len() != 43
            || !self
                .session_token
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err(
                "CONTRACT_VIOLATION: session_token must be 43 base64url characters".to_string(),
            );
        }

        let eff = &self.effective;
        if eff.transport != "rtp_udp" {
            return Err(format!(
                "CONTRACT_VIOLATION: effective transport must be 'rtp_udp', got '{}'",
                eff.transport
            ));
        }
        if eff.codec != "pcm_s16le" {
            return Err(format!(
                "CONTRACT_VIOLATION: effective codec must be 'pcm_s16le', got '{}'",
                eff.codec
            ));
        }
        if eff.sample_rate != 48000 {
            return Err(format!(
                "CONTRACT_VIOLATION: effective sample_rate must be 48000, got {}",
                eff.sample_rate
            ));
        }
        if eff.bit_depth != 16 {
            return Err(format!(
                "CONTRACT_VIOLATION: effective bit_depth must be 16, got {}",
                eff.bit_depth
            ));
        }
        if eff.channels != 2 {
            return Err(format!(
                "CONTRACT_VIOLATION: effective channels must be 2, got {}",
                eff.channels
            ));
        }
        if eff.packet_ms != 10 {
            return Err(format!(
                "CONTRACT_VIOLATION: effective packet_ms must be 10, got {}",
                eff.packet_ms
            ));
        }
        if !(50..=500).contains(&eff.buffer_ms) {
            return Err(format!(
                "CONTRACT_VIOLATION: effective buffer_ms must be 50..=500, got {}",
                eff.buffer_ms
            ));
        }
        if eff.payload_type != 97 {
            return Err(format!(
                "CONTRACT_VIOLATION: effective payload_type must be 97, got {}",
                eff.payload_type
            ));
        }
        if eff.ssrc == 0 {
            return Err("CONTRACT_VIOLATION: effective ssrc must be non-zero".to_string());
        }
        if !(49152..=65535).contains(&eff.stream_port) {
            return Err(format!(
                "CONTRACT_VIOLATION: effective stream_port must be 49152..=65535, got {}",
                eff.stream_port
            ));
        }
        if eff.volume > 100 {
            return Err(format!(
                "CONTRACT_VIOLATION: effective volume must be <= 100, got {}",
                eff.volume
            ));
        }

        Ok(NegotiatedReceiverSession {
            session_id: self.session_id.clone(),
            session_token: self.session_token.clone(),
            lease_seconds: self.lease_seconds,
            transport: eff.transport.clone(),
            codec: eff.codec.clone(),
            sample_rate: eff.sample_rate,
            bit_depth: eff.bit_depth,
            channels: eff.channels,
            packet_ms: eff.packet_ms,
            buffer_ms: eff.buffer_ms,
            payload_type: eff.payload_type,
            ssrc: eff.ssrc,
            stream_port: eff.stream_port,
            volume: eff.volume,
        })
    }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ReceiverPresence {
    #[default]
    Unknown,
    Offline,
    VerifiedOnline,
    Degraded,
}

#[derive(Debug, Clone)]
pub struct ReceiverRegistryEntry {
    pub receiver_id: String,
    pub michi_id: Option<String>,
    pub name: String,
    pub device_type: String,
    pub base_url: String,
    pub paired: bool,
    pub token: Option<String>,
    pub presence: ReceiverPresence,
    pub last_seen: Option<chrono::DateTime<chrono::Utc>>,
    pub capabilities: Vec<String>,
    pub capabilities_verified_at: Option<chrono::DateTime<chrono::Utc>>,
    pub capabilities_stale: bool,
    pub authority_supported: bool,
    pub owner_michi_id: Option<String>,
    pub owner_name: Option<String>,
    pub active_session_id: Option<String>,
    pub max_sample_rate: u32,
    pub max_bit_depth: u32,
    pub supported_transports: Vec<String>,
    pub supported_codecs: Vec<String>,
    pub supported_sample_rates: Vec<u32>,
    pub supported_bit_depths: Vec<u32>,
    pub supported_channels: Vec<u8>,
    pub maximum_safe_volume: Option<u32>,
    pub qualification: ReceiverQualification,
}

impl ReceiverRegistryEntry {
    pub fn michi_id(&self) -> &str {
        self.michi_id.as_deref().unwrap_or(&self.receiver_id)
    }

    pub fn is_online(&self) -> bool {
        self.presence == ReceiverPresence::VerifiedOnline
    }

    pub fn supports_authority_v1(&self) -> bool {
        self.authority_supported
    }

    pub fn compute_qualification(&self) -> ReceiverQualification {
        if self.qualification == ReceiverQualification::IdentityMismatch {
            return ReceiverQualification::IdentityMismatch;
        }
        if self.paired
            && self
                .token
                .as_ref()
                .map(|t| t.trim().is_empty())
                .unwrap_or(true)
        {
            return ReceiverQualification::MissingCredential;
        }
        if self.capabilities_stale || self.capabilities_verified_at.is_none() {
            return ReceiverQualification::NeedsCapabilityRefresh;
        }
        if !self.supported_transports.iter().any(|t| t == "rtp_udp") {
            return ReceiverQualification::UnsupportedTransport;
        }
        if !self.supported_codecs.iter().any(|c| c == "pcm_s16le") {
            return ReceiverQualification::UnsupportedCodec;
        }
        if !self.supported_sample_rates.contains(&48000)
            || !self.supported_bit_depths.contains(&16)
            || !self.supported_channels.contains(&2)
            || self.max_sample_rate < 48000
            || self.max_bit_depth < 16
        {
            return ReceiverQualification::NeedsCapabilityRefresh;
        }
        ReceiverQualification::Qualified
    }

    pub fn to_capabilities(&self) -> ReceiverCapabilities {
        ReceiverCapabilities {
            device_type: self.device_type.clone(),
            supported_codecs: self.supported_codecs.clone(),
            max_sample_rate: self.max_sample_rate,
            max_bit_depth: self.max_bit_depth,
            supported_transports: self.supported_transports.clone(),
            supported_sample_rates: self.supported_sample_rates.clone(),
            supported_bit_depths: self.supported_bit_depths.clone(),
            supported_channels: self.supported_channels.clone(),
            features: self.capabilities.clone(),
            authority_features: if self.authority_supported {
                vec!["authority-v1".to_string()]
            } else {
                Vec::new()
            },
        }
    }
}

/// Operational qualification state of a receiver.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ReceiverQualification {
    #[default]
    Unknown,
    Qualified,
    NeedsCapabilityRefresh,
    UnsupportedTransport,
    UnsupportedCodec,
    MissingCredential,
    IdentityMismatch,
}

impl Default for ReceiverRegistryEntry {
    fn default() -> Self {
        Self {
            receiver_id: String::new(),
            michi_id: None,
            name: String::new(),
            device_type: "standard".to_string(),
            base_url: String::new(),
            paired: false,
            token: None,
            presence: ReceiverPresence::Unknown,
            last_seen: None,
            capabilities: Vec::new(),
            capabilities_verified_at: None,
            capabilities_stale: true,
            authority_supported: false,
            owner_michi_id: None,
            owner_name: None,
            active_session_id: None,
            max_sample_rate: 0,
            max_bit_depth: 0,
            supported_transports: Vec::new(),
            supported_codecs: Vec::new(),
            supported_sample_rates: Vec::new(),
            supported_bit_depths: Vec::new(),
            supported_channels: Vec::new(),
            maximum_safe_volume: None,
            qualification: ReceiverQualification::Unknown,
        }
    }
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
        self.receivers.get(id).or_else(|| self.get_by_michi_id(id))
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut ReceiverRegistryEntry> {
        if self.receivers.contains_key(id) {
            self.receivers.get_mut(id)
        } else {
            self.receivers
                .values_mut()
                .find(|r| r.michi_id.as_deref() == Some(id))
        }
    }

    pub fn get_by_michi_id(&self, michi_id: &str) -> Option<&ReceiverRegistryEntry> {
        self.receivers
            .values()
            .find(|r| r.michi_id.as_deref() == Some(michi_id) || r.receiver_id == michi_id)
    }

    pub fn get_by_michi_id_mut(&mut self, michi_id: &str) -> Option<&mut ReceiverRegistryEntry> {
        self.receivers
            .values_mut()
            .find(|r| r.michi_id.as_deref() == Some(michi_id) || r.receiver_id == michi_id)
    }

    pub fn list(&self) -> Vec<&ReceiverRegistryEntry> {
        self.receivers.values().collect()
    }

    pub fn list_online(&self) -> Vec<&ReceiverRegistryEntry> {
        self.receivers
            .values()
            .filter(|r| r.presence == ReceiverPresence::VerifiedOnline)
            .collect()
    }

    pub fn remove(&mut self, id: &str) {
        self.receivers.remove(id);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ReceiverCapabilities {
    pub device_type: String,
    pub supported_codecs: Vec<String>,
    pub max_sample_rate: u32,
    pub max_bit_depth: u32,
    #[serde(default)]
    pub supported_transports: Vec<String>,
    #[serde(default)]
    pub supported_sample_rates: Vec<u32>,
    #[serde(default)]
    pub supported_bit_depths: Vec<u32>,
    #[serde(default)]
    pub supported_channels: Vec<u8>,
    #[serde(default)]
    pub features: Vec<String>,
    #[serde(default)]
    pub authority_features: Vec<String>,
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
