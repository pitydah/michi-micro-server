use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{SqlitePool, FromRow};
use tokio::fs::{self, File};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tracing::{info, warn, error};
use utoipa::ToSchema;
use uuid::Uuid;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;

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
pub struct UploadInit {
    pub filename: String,
    pub original_path: String,
    pub file_size: i64,
    pub expected_hash: String,
    pub uploaded_by: String,
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
    #[error(transparent)]
    DatabaseError(#[from] sqlx::Error),
    #[error(transparent)]
    IoError(#[from] std::io::Error),
    #[error(transparent)]
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

#[derive(Debug, Clone)]
struct UploadMeta {
    filename: String,
    original_path: String,
    file_size: i64,
    expected_hash: String,
    uploaded_by: String,
    total_chunks: u32,
}

#[derive(Debug, Clone)]
pub struct SyncManager {
    db_pool: SqlitePool,
    upload_dir: PathBuf,
    chunk_size: usize,
    uploads: Arc<RwLock<HashMap<Uuid, UploadMeta>>>,
}

impl SyncManager {
    pub fn new(db_pool: SqlitePool, upload_dir: PathBuf) -> Self {
        Self {
            db_pool,
            upload_dir,
            chunk_size: 1024 * 1024,
            uploads: Arc::new(RwLock::new(HashMap::new())),
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

    pub async fn init_upload(&self, init: UploadInit) -> Result<Uuid, SyncError> {
        let file_id = Uuid::new_v4();
        let filename = init.filename.clone();
        let meta = UploadMeta {
            filename: init.filename,
            original_path: init.original_path,
            file_size: init.file_size,
            expected_hash: init.expected_hash,
            uploaded_by: init.uploaded_by,
            total_chunks: 0,
        };
        self.uploads.write().await.insert(file_id, meta);
        info!("Upload initialized: {} -> {}", file_id, filename);
        Ok(file_id)
    }

    pub async fn upload_chunk(
        &self,
        chunk: UploadChunk,
    ) -> Result<UploadProgress, SyncError> {
        let file_path = self.upload_dir.join(chunk.file_id.to_string());

        // Track total_chunks from the first chunk
        {
            let mut uploads = self.uploads.write().await;
            if let Some(meta) = uploads.get_mut(&chunk.file_id) {
                if chunk.chunk_index == 0 {
                    meta.total_chunks = chunk.total_chunks;
                }
            }
        }

        // Verify individual chunk hash (checksum of chunk data)
        let mut hasher = Sha256::new();
        hasher.update(&chunk.data);
        let computed_chunk_hash = format!("{:x}", hasher.finalize());
        if computed_chunk_hash != chunk.chunk_hash {
            return Err(SyncError::HashMismatch {
                expected: chunk.chunk_hash,
                actual: computed_chunk_hash,
            });
        }

        let mut file = File::options()
            .create(true)
            .append(true)
            .open(&file_path)
            .await?;

        file.write_all(&chunk.data).await?;
        file.sync_all().await?;

        let completed = chunk.chunk_index + 1 >= chunk.total_chunks;
        let progress = UploadProgress {
            file_id: chunk.file_id,
            uploaded_chunks: chunk.chunk_index + 1,
            total_chunks: chunk.total_chunks,
            percentage: ((chunk.chunk_index + 1) as f64 / chunk.total_chunks as f64) * 100.0,
            completed,
        };

        if completed {
            self.verify_and_finalize_upload(chunk.file_id).await?;
        }

        Ok(progress)
    }

    async fn verify_and_finalize_upload(&self, file_id: Uuid) -> Result<(), SyncError> {
        let meta = self.uploads.read().await.get(&file_id).cloned();
        let meta = meta.ok_or_else(|| SyncError::UploadFailed("upload not initialized".into()))?;

        let file_path = self.upload_dir.join(file_id.to_string());
        let computed_hash = self.calculate_file_hash(&file_path).await?;

        if computed_hash != meta.expected_hash {
            warn!(
                "Hash mismatch for {}: expected {}, got {}",
                file_id, meta.expected_hash, computed_hash
            );
            return Err(SyncError::HashMismatch {
                expected: meta.expected_hash.clone(),
                actual: computed_hash,
            });
        }

        // Register in DB
        let server_path = file_path.to_string_lossy().to_string();
        self.register_uploaded_file(
            meta.filename.clone(),
            meta.original_path.clone(),
            server_path,
            meta.expected_hash.clone(),
            meta.file_size,
            meta.uploaded_by.clone(),
        )
        .await?;

        // Cleanup metadata
        self.uploads.write().await.remove(&file_id);

        info!(
            "Upload finalized and verified for {} ({})",
            meta.filename, file_id
        );
        Ok(())
    }

    pub async fn get_upload_progress(&self, file_id: &Uuid) -> Result<Option<UploadProgress>, SyncError> {
        let meta = self.uploads.read().await.get(file_id).cloned();
        Ok(meta.map(|m| {
            let file_path = self.upload_dir.join(file_id.to_string());
            let uploaded_chunks = if file_path.exists() {
                // Estimate from file size
                (std::fs::metadata(&file_path).map(|m| m.len()).unwrap_or(0) as f64 / self.chunk_size as f64).ceil() as u32
            } else {
                0
            };
            let total = if m.total_chunks > 0 { m.total_chunks } else { 1 };
            UploadProgress {
                file_id: *file_id,
                uploaded_chunks,
                total_chunks: total,
                percentage: (uploaded_chunks as f64 / total as f64) * 100.0,
                completed: uploaded_chunks >= total,
            }
        }))
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
        let row = sqlx::query_as::<_, (String, String, bool, f64, String)>(
            "SELECT current_track_id, position_ms, playing, volume, updated_at
             FROM playback_sessions ORDER BY updated_at DESC LIMIT 1"
        )
        .fetch_optional(&self.db_pool)
        .await?;

        match row {
            Some((tid, pos, playing, vol, updated)) => Ok(PlaybackState {
                track_id: Some(Uuid::parse_str(&tid).unwrap_or_default()),
                position_ms: pos.parse().unwrap_or(0),
                playing,
                volume: vol,
                updated_at: DateTime::parse_from_rfc3339(&updated)
                    .map(|d| d.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
                playlist_id: None,
                queue_position: None,
                device_id: Some("server".into()),
            }),
            None => Ok(PlaybackState::default()),
        }
    }

    pub async fn update_playback_state(&self, state: PlaybackState) -> Result<(), SyncError> {
        let tid_str = state.track_id.map(|id| id.to_string());
        sqlx::query(
            "UPDATE playback_sessions SET current_track_id = ?, position_ms = ?, playing = ?, volume = ?, updated_at = ? WHERE id IN (SELECT id FROM playback_sessions ORDER BY updated_at DESC LIMIT 1)"
        )
        .bind(&tid_str)
        .bind(state.position_ms.to_string())
        .bind(state.playing)
        .bind(state.volume)
        .bind(state.updated_at.to_rfc3339())
        .execute(&self.db_pool)
        .await?;
        Ok(())
    }

    pub async fn initiate_handoff(&self, from_device: String, to_device: String) -> Result<SessionData, SyncError> {
        let current = self.get_playback_state().await?;
        let session = SessionData {
            track_id: current.track_id,
            position_ms: current.position_ms,
            playing: current.playing,
            volume: current.volume,
            playlist_id: current.playlist_id,
            queue_position: current.queue_position,
            transferred_at: Utc::now(),
        };
        info!("Handoff initiated: {} -> {}", from_device, to_device);
        Ok(session)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serialize_deserialize_state() {
        let msg = SyncMessage::State {
            track_id: Some(Uuid::new_v4()),
            position_ms: 120000,
            playing: true,
            volume: 0.75,
            updated_at: Utc::now(),
            playlist_id: None,
            queue_position: None,
        };
        let json = msg.serialize().unwrap();
        let deserialized = SyncMessage::deserialize(&json).unwrap();
        assert!(matches!(deserialized, SyncMessage::State { .. }));
    }

    #[test]
    fn test_serialize_deserialize_identify() {
        let msg = SyncMessage::Identify {
            name: "test-device".into(),
            version: "1.0".into(),
            device_type: DeviceType::Desktop,
        };
        let json = msg.serialize().unwrap();
        let deserialized = SyncMessage::deserialize(&json).unwrap();
        assert!(matches!(deserialized, SyncMessage::Identify { .. }));
    }

    #[test]
    fn test_handoff_messages() {
        let req = SyncMessage::handoff_request("pc".into(), "server".into());
        let json = req.serialize().unwrap();
        let deserialized = SyncMessage::deserialize(&json).unwrap();
        assert!(matches!(deserialized, SyncMessage::HandoffRequest { .. }));

        let accept = SyncMessage::handoff_accept(SessionData {
            track_id: None,
            position_ms: 0,
            playing: false,
            volume: 0.5,
            playlist_id: None,
            queue_position: None,
            transferred_at: Utc::now(),
        });
        let json = accept.serialize().unwrap();
        let deserialized = SyncMessage::deserialize(&json).unwrap();
        assert!(matches!(deserialized, SyncMessage::HandoffAccept { .. }));
    }

    #[test]
    fn test_playback_state_default() {
        let state = PlaybackState::default();
        assert!(!state.playing);
        assert!((state.volume - 0.8).abs() < 0.01);
    }

    #[test]
    fn test_upload_chunk_structure() {
        let chunk = UploadChunk {
            file_id: Uuid::new_v4(),
            chunk_index: 0,
            total_chunks: 5,
            data: vec![1, 2, 3],
            chunk_hash: "abc".into(),
        };
        assert_eq!(chunk.chunk_index, 0);
        assert_eq!(chunk.total_chunks, 5);
    }
}
