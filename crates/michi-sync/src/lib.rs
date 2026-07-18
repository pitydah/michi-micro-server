use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{SqlitePool, FromRow};
use tokio::fs::{self, File};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tracing::{info, warn, error};
use utoipa::ToSchema;
use uuid::Uuid;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SyncMessage {
    #[serde(rename = "identify")]
    Identify { name: String, version: String, device_type: DeviceType },
    #[serde(rename = "state")]
    State {
        track_id: Option<Uuid>,
        position_ms: u64,
        playing: bool,
        volume: f64,
        updated_at: DateTime<Utc>,
        playlist_id: Option<Uuid>,
        queue_position: Option<u32>,
    },
    #[serde(rename = "handoff_request")]
    HandoffRequest { from_device: String, to_device: String },
    #[serde(rename = "handoff_accept")]
    HandoffAccept { session_data: SessionData },
    #[serde(rename = "ping")]
    Ping,
    #[serde(rename = "pong")]
    Pong,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub enum DeviceType {
    #[serde(rename = "desktop")]
    Desktop,
    #[serde(rename = "mobile")]
    Mobile,
    #[serde(rename = "server")]
    Server,
    #[serde(rename = "stream")]
    Stream,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SessionData {
    pub track_id: Option<Uuid>,
    pub position_ms: u64,
    pub playing: bool,
    pub volume: f64,
    pub playlist_id: Option<Uuid>,
    pub queue_position: Option<u32>,
    pub transferred_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, FromRow)]
pub struct SyncedFile {
    pub id: Uuid,
    pub filename: String,
    pub original_path: String,
    pub server_path: String,
    pub file_hash: String,
    pub file_size: i64,
    pub uploaded_at: DateTime<Utc>,
    pub uploaded_by: String,
    pub checksum_verified: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UploadChunk {
    pub file_id: Uuid,
    pub chunk_index: u32,
    pub total_chunks: u32,
    pub data: Vec<u8>,
    pub chunk_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UploadProgress {
    pub file_id: Uuid,
    pub uploaded_chunks: u32,
    pub total_chunks: u32,
    pub percentage: f64,
    pub completed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PlaybackState {
    pub track_id: Option<Uuid>,
    pub position_ms: u64,
    pub playing: bool,
    pub volume: f64,
    pub updated_at: DateTime<Utc>,
    pub playlist_id: Option<Uuid>,
    pub queue_position: Option<u32>,
    pub device_id: Option<String>,
}

impl Default for PlaybackState {
    fn default() -> Self {
        Self {
            track_id: None,
            position_ms: 0,
            playing: false,
            volume: 0.8,
            updated_at: Utc::now(),
            playlist_id: None,
            queue_position: None,
            device_id: None,
        }
    }
}

impl From<PlaybackState> for SyncMessage {
    fn from(state: PlaybackState) -> Self {
        SyncMessage::State {
            track_id: state.track_id,
            position_ms: state.position_ms,
            playing: state.playing,
            volume: state.volume,
            updated_at: state.updated_at,
            playlist_id: state.playlist_id,
            queue_position: state.queue_position,
        }
    }
}

#[derive(Error, Debug)]
pub enum SyncError {
    #[error("File not found: {0}")]
    FileNotFound(String),
    #[error("Hash mismatch: expected {expected}, got {actual}")]
    HashMismatch { expected: String, actual: String },
    #[error("Upload failed: {0}")]
    UploadFailed(String),
    #[error("Database error: {0}")]
    DatabaseError(#[from] sqlx::Error),
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
}

impl SyncMessage {
    pub fn serialize(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    pub fn deserialize(data: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(data)
    }

    pub fn identify(name: String, version: String, device_type: DeviceType) -> Self {
        SyncMessage::Identify { name, version, device_type }
    }

    pub fn handoff_request(from: String, to: String) -> Self {
        SyncMessage::HandoffRequest { from_device: from, to_device: to }
    }

    pub fn handoff_accept(session: SessionData) -> Self {
        SyncMessage::HandoffAccept { session_data: session }
    }
}

pub struct SyncManager {
    db_pool: SqlitePool,
    upload_dir: PathBuf,
    chunk_size: usize,
}

impl SyncManager {
    pub fn new(db_pool: SqlitePool, upload_dir: PathBuf) -> Self {
        Self {
            db_pool,
            upload_dir,
            chunk_size: 1024 * 1024, // 1MB chunks
        }
    }

    pub async fn calculate_file_hash<P: AsRef<Path>>(&self, path: P) -> Result<String, SyncError> {
        let mut file = File::open(path.as_ref()).await?;
        let mut hasher = Sha256::new();
        let mut buffer = vec![0u8; self.chunk_size];

        loop {
            let bytes_read = file.read(&mut buffer).await?;
            if bytes_read == 0 {
                break;
            }
            hasher.update(&buffer[..bytes_read]);
        }

        Ok(format!("{:x}", hasher.finalize()))
    }

    pub async fn upload_chunk(
        &self,
        chunk: UploadChunk,
    ) -> Result<UploadProgress, SyncError> {
        let file_path = self.upload_dir.join(chunk.file_id.to_string());
        let mut file = File::options()
            .create(true)
            .append(true)
            .open(&file_path)
            .await?;

        file.write_all(&chunk.data).await?;
        file.sync_all().await?;

        let progress = UploadProgress {
            file_id: chunk.file_id,
            uploaded_chunks: chunk.chunk_index + 1,
            total_chunks: chunk.total_chunks,
            percentage: ((chunk.chunk_index + 1) as f64 / chunk.total_chunks as f64) * 100.0,
            completed: chunk.chunk_index + 1 >= chunk.total_chunks,
        };

        if progress.completed {
            self.verify_and_finalize_upload(chunk.file_id).await?;
        }

        Ok(progress)
    }

    async fn verify_and_finalize_upload(&self, file_id: Uuid) -> Result<(), SyncError> {
        let file_path = self.upload_dir.join(file_id.to_string());
        let computed_hash = self.calculate_file_hash(&file_path).await?;
        
        // TODO: Compare with expected hash from metadata
        info!("Upload finalized for file {} with hash {}", file_id, computed_hash);
        
        Ok(())
    }

    pub async fn check_file_exists(&self, file_hash: &str) -> Result<Option<SyncedFile>, SyncError> {
        let file = sqlx::query_as::<_, SyncedFile>(
            "SELECT * FROM synced_files WHERE file_hash = ? AND checksum_verified = TRUE"
        )
        .bind(file_hash)
        .fetch_optional(&self.db_pool)
        .await?;

        Ok(file)
    }

    pub async fn register_uploaded_file(
        &self,
        filename: String,
        original_path: String,
        server_path: String,
        file_hash: String,
        file_size: i64,
        uploaded_by: String,
    ) -> Result<Uuid, SyncError> {
        let id = Uuid::new_v4();
        
        sqlx::query(
            "INSERT INTO synced_files (id, filename, original_path, server_path, file_hash, file_size, uploaded_by, checksum_verified)
             VALUES (?, ?, ?, ?, ?, ?, ?, TRUE)"
        )
        .bind(id)
        .bind(&filename)
        .bind(&original_path)
        .bind(&server_path)
        .bind(&file_hash)
        .bind(file_size)
        .bind(&uploaded_by)
        .execute(&self.db_pool)
        .await?;

        Ok(id)
    }

    pub async fn get_playback_state(&self) -> Result<PlaybackState, SyncError> {
        // TODO: Implement persistent playback state retrieval
        Ok(PlaybackState::default())
    }

    pub async fn update_playback_state(&self, state: PlaybackState) -> Result<(), SyncError> {
        // TODO: Implement persistent playback state update
        info!("Playback state updated: {:?}", state);
        Ok(())
    }

    pub async fn initiate_handoff(
        &self,
        from_device: String,
        to_device: String,
    ) -> Result<SessionData, SyncError> {
        let current_state = self.get_playback_state().await?;
        
        let session = SessionData {
            track_id: current_state.track_id,
            position_ms: current_state.position_ms,
            playing: current_state.playing,
            volume: current_state.volume,
            playlist_id: current_state.playlist_id,
            queue_position: current_state.queue_position,
            transferred_at: Utc::now(),
        };

        info!("Handoff initiated from {} to {}", from_device, to_device);
        Ok(session)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serialize_deserialize_state() {
        let msg = SyncMessage::State {
            track_id: Some(Uuid::nil()),
            position_ms: 12345,
            playing: true,
            volume: 0.8,
            updated_at: Utc::now(),
            playlist_id: None,
            queue_position: None,
        };
        let json = msg.serialize().unwrap();
        let deserialized = SyncMessage::deserialize(&json).unwrap();
        match deserialized {
            SyncMessage::State {
                track_id,
                position_ms,
                playing,
                volume,
                ..
            } => {
                assert_eq!(track_id, Some(Uuid::nil()));
                assert_eq!(position_ms, 12345);
                assert!(playing);
                assert!((volume - 0.8).abs() < 0.001);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_serialize_deserialize_identify() {
        let msg = SyncMessage::Identify {
            name: "Living Room".into(),
            version: "0.1.0".into(),
            device_type: DeviceType::Stream,
        };
        let json = msg.serialize().unwrap();
        let deserialized = SyncMessage::deserialize(&json).unwrap();
        match deserialized {
            SyncMessage::Identify { name, version, device_type } => {
                assert_eq!(name, "Living Room");
                assert_eq!(version, "0.1.0");
                assert!(matches!(device_type, DeviceType::Stream));
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_handoff_messages() {
        let request = SyncMessage::handoff_request("Desktop".into(), "Server".into());
        let json = request.serialize().unwrap();
        let deserialized = SyncMessage::deserialize(&json).unwrap();
        assert!(matches!(deserialized, SyncMessage::HandoffRequest { .. }));

        let session = SessionData {
            track_id: Some(Uuid::new_v4()),
            position_ms: 45000,
            playing: true,
            volume: 0.75,
            playlist_id: None,
            queue_position: Some(5),
            transferred_at: Utc::now(),
        };
        let accept = SyncMessage::handoff_accept(session);
        assert!(matches!(accept, SyncMessage::HandoffAccept { .. }));
    }

    #[test]
    fn test_playback_state_default() {
        let state = PlaybackState::default();
        assert!(state.track_id.is_none());
        assert!(!state.playing);
        assert_eq!(state.position_ms, 0);
        assert!(state.playlist_id.is_none());
        assert!(state.queue_position.is_none());
    }

    #[test]
    fn test_upload_chunk_structure() {
        let chunk = UploadChunk {
            file_id: Uuid::new_v4(),
            chunk_index: 0,
            total_chunks: 10,
            data: vec![0u8; 1024],
            chunk_hash: "abc123".into(),
        };
        assert_eq!(chunk.chunk_index, 0);
        assert_eq!(chunk.total_chunks, 10);
        assert_eq!(chunk.data.len(), 1024);
    }
}
