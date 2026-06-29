use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkServerInfo {
    pub service: String,
    pub name: String,
    pub server_id: Uuid,
    pub version: String,
    pub api_version: String,
    pub michi_link_version: String,
    pub roles: Vec<String>,
    pub features: LinkFeatures,
    pub auth: LinkAuthInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkAuthInfo {
    pub required: bool,
    pub strategy: String,
    pub token_refresh: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkFeatures {
    pub library: bool,
    pub search: bool,
    pub streaming: bool,
    pub download: bool,
    pub artwork: bool,
    pub playlists: bool,
    pub sync_manifest: bool,
    pub import: bool,
    pub playback: bool,
    pub queue: bool,
    pub receivers: bool,
    pub rooms: bool,
    pub events: bool,
    pub transcoding: bool,
    pub token_refresh: bool,
}

impl LinkFeatures {
    pub fn all_enabled() -> Self {
        Self {
            library: true,
            search: true,
            streaming: true,
            download: true,
            artwork: true,
            playlists: true,
            sync_manifest: true,
            import: true,
            playback: true,
            queue: true,
            receivers: false,
            rooms: false,
            events: true,
            transcoding: false,
            token_refresh: true,
        }
    }
}

impl LinkAuthInfo {
    pub fn server_code() -> Self {
        Self {
            required: true,
            strategy: "SERVER_CODE".into(),
            token_refresh: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairStartRequest {
    pub device_name: Option<String>,
    pub alias: Option<String>,
    pub device_type: Option<String>,
    pub device_model: Option<String>,
    pub client_device_id: Option<String>,
}

impl PairStartRequest {
    pub fn device_name(&self) -> &str {
        self.alias.as_deref().or(self.device_name.as_deref()).unwrap_or("unknown")
    }

    pub fn client_device_id(&self) -> Option<String> {
        self.client_device_id.clone().or(self.device_name.clone())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairStartResponse {
    pub pairing_id: Uuid,
    pub code: String,
    pub expires_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairConfirmRequest {
    pub pairing_id: Option<Uuid>,
    pub code: String,
    pub client_device_id: Option<String>,
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
pub struct TokenRefreshRequest {
    pub refresh_token: String,
    pub device_id: Option<Uuid>,
    pub client_device_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenRefreshResponse {
    pub device_token: String,
    pub refresh_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportSessionRequest {
    pub device_id: Option<Uuid>,
    pub total_tracks: u32,
    pub total_playlists: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportSessionResponse {
    pub session_id: Uuid,
    pub expires_at: String,
    pub max_chunk_size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportUploadRequest {
    pub session_id: Uuid,
    pub track_index: u32,
    pub filename: String,
    pub hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportUploadResponse {
    pub accepted: bool,
    pub is_duplicate: bool,
    pub track_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportCommitRequest {
    pub session_id: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportCommitResponse {
    pub tracks_imported: u32,
    pub playlists_imported: u32,
    pub total_size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaybackSessionRequest {
    pub queue: Vec<Uuid>,
    pub current_track_id: Option<Uuid>,
    pub position_ms: u64,
    pub playing: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaybackSessionResponse {
    pub session_id: Uuid,
    pub accepted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaybackControlRequest {
    pub command: PlaybackCommand,
    pub parameter: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PlaybackCommand {
    Play,
    Pause,
    Next,
    Previous,
    Seek,
    SetVolume,
    Stop,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncManifestEntry {
    pub track_id: Uuid,
    #[serde(skip_serializing)]
    pub internal_file_path: String,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration_ms: Option<i64>,
    pub artwork_id: Option<String>,
    pub hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncManifestDeltaRequest {
    pub known_track_ids: Vec<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncManifestDeltaResponse {
    pub added: Vec<SyncManifestEntry>,
    pub removed: Vec<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncStateUpload {
    pub track_id: Option<Uuid>,
    pub position_ms: u64,
    pub playing: bool,
    pub volume: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReceiverInfo {
    pub id: Uuid,
    pub name: String,
    pub device_type: String,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub capabilities: Vec<String>,
    pub online: bool,
    pub last_seen: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomInfo {
    pub id: String,
    pub name: String,
    pub receivers: Vec<Uuid>,
    pub volume: u32,
    pub muted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomPlayRequest {
    pub track_id: Uuid,
    pub position_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueItem {
    pub id: Uuid,
    pub track_id: Uuid,
    pub position: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueState {
    pub id: Uuid,
    pub items: Vec<QueueItem>,
    pub current_index: i32,
    pub repeat_mode: String,
    pub shuffle: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueJumpRequest {
    pub index: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueReorderRequest {
    pub item_ids: Vec<Uuid>,
}
