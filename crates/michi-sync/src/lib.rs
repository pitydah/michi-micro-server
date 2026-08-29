use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{FromRow, SqlitePool};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use thiserror::Error;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::sync::RwLock;
use tracing::{info, warn};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SyncMessage {
    #[serde(rename = "identify")]
    Identify {
        name: String,
        version: String,
        device_type: DeviceType,
    },
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
    HandoffRequest {
        from_device: String,
        to_device: String,
    },
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
    #[serde(default)]
    pub shuffle: bool,
    #[serde(
        default = "default_repeat_mode",
        deserialize_with = "deserialize_repeat_mode"
    )]
    pub repeat: String,
}

fn default_repeat_mode() -> String {
    "off".to_string()
}

fn deserialize_repeat_mode<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    if s == "none" {
        Ok("off".to_string())
    } else {
        Ok(s)
    }
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
            shuffle: false,
            repeat: "off".into(),
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
        SyncMessage::Identify {
            name,
            version,
            device_type,
        }
    }

    pub fn handoff_request(from: String, to: String) -> Self {
        SyncMessage::HandoffRequest {
            from_device: from,
            to_device: to,
        }
    }

    pub fn handoff_accept(session: SessionData) -> Self {
        SyncMessage::HandoffAccept {
            session_data: session,
        }
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
struct UploadMeta {
    id: Uuid,
    filename: String,
    original_path: String,
    file_size: i64,
    expected_hash: String,
    uploaded_by: String,
    total_chunks: u32,
    chunk_size: usize,
    received_chunks: HashSet<u32>,
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
            id: file_id,
            filename: init.filename.clone(),
            original_path: init.original_path.clone(),
            file_size: init.file_size,
            expected_hash: init.expected_hash.clone(),
            uploaded_by: init.uploaded_by.clone(),
            total_chunks: 0,
            chunk_size: self.chunk_size,
            received_chunks: HashSet::new(),
        };

        let now = Utc::now().to_rfc3339();
        let _ = sqlx::query(
            "INSERT INTO sync_uploads (id, filename, original_path, file_size, expected_hash, uploaded_by, total_chunks, chunk_size, status, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, 0, ?, 'uploading', ?, ?)"
        )
        .bind(file_id.to_string())
        .bind(&init.filename)
        .bind(&init.original_path)
        .bind(init.file_size)
        .bind(&init.expected_hash)
        .bind(&init.uploaded_by)
        .bind(self.chunk_size as i64)
        .bind(&now)
        .bind(&now)
        .execute(&self.db_pool)
        .await;

        self.uploads.write().await.insert(file_id, meta);
        info!("Upload initialized: {} -> {}", file_id, filename);
        Ok(file_id)
    }

    pub async fn upload_chunk(&self, chunk: UploadChunk) -> Result<UploadProgress, SyncError> {
        let file_path = self.upload_dir.join(chunk.file_id.to_string());

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

        // Ensure session is loaded (from memory or fallback to DB on restart)
        {
            let mut uploads = self.uploads.write().await;
            if let std::collections::hash_map::Entry::Vacant(e) = uploads.entry(chunk.file_id) {
                if let Ok(Some(row)) = sqlx::query_as::<
                    _,
                    (
                        String,
                        String,
                        String,
                        i64,
                        String,
                        String,
                        i64,
                        i64,
                        String,
                    ),
                >(
                    "SELECT id, filename, original_path, file_size, expected_hash, uploaded_by, total_chunks, chunk_size, status
                     FROM sync_uploads WHERE id = ?"
                )
                .bind(chunk.file_id.to_string())
                .fetch_optional(&self.db_pool)
                .await {
                    let mut received = HashSet::new();
                    if let Ok(chunk_rows) = sqlx::query_as::<_, (i64,)>(
                        "SELECT chunk_index FROM sync_upload_chunks WHERE file_id = ?"
                    )
                    .bind(chunk.file_id.to_string())
                    .fetch_all(&self.db_pool)
                    .await {
                        for (idx,) in chunk_rows {
                            received.insert(idx as u32);
                        }
                    }

                    e.insert(UploadMeta {
                        id: chunk.file_id,
                        filename: row.1,
                        original_path: row.2,
                        file_size: row.3,
                        expected_hash: row.4,
                        uploaded_by: row.5,
                        total_chunks: row.6 as u32,
                        chunk_size: row.7 as usize,
                        received_chunks: received,
                    });
                }
            }

            if let Some(meta) = uploads.get_mut(&chunk.file_id) {
                if meta.total_chunks == 0 || meta.total_chunks != chunk.total_chunks {
                    meta.total_chunks = chunk.total_chunks;
                    let _ = sqlx::query("UPDATE sync_uploads SET total_chunks = ? WHERE id = ?")
                        .bind(chunk.total_chunks as i64)
                        .bind(chunk.file_id.to_string())
                        .execute(&self.db_pool)
                        .await;
                }
            } else {
                return Err(SyncError::UploadFailed("upload session not found".into()));
            }
        }

        let (chunk_size, is_already_received) = {
            let mut uploads = self.uploads.write().await;
            let meta = uploads.get_mut(&chunk.file_id).unwrap();
            let effective_chunk_size = if meta.total_chunks > 1 && meta.file_size > 0 {
                (meta.file_size as usize).div_ceil(meta.total_chunks as usize).max(1)
            } else {
                meta.chunk_size
            };
            meta.chunk_size = effective_chunk_size;
            (
                effective_chunk_size,
                meta.received_chunks.contains(&chunk.chunk_index),
            )
        };

        if !is_already_received {
            // Write at offset: offset = chunk_index * chunk_size
            let offset = (chunk.chunk_index as u64) * (chunk_size as u64);
            let mut file = tokio::fs::OpenOptions::new()
                .create(true)
                .truncate(false)
                .write(true)
                .open(&file_path)
                .await?;

            file.seek(std::io::SeekFrom::Start(offset)).await?;
            file.write_all(&chunk.data).await?;
            file.sync_all().await?;

            // Persist chunk receipt in DB
            let now = Utc::now().to_rfc3339();
            let _ = sqlx::query(
                "INSERT OR REPLACE INTO sync_upload_chunks (file_id, chunk_index, chunk_hash, size, created_at)
                 VALUES (?, ?, ?, ?, ?)"
            )
            .bind(chunk.file_id.to_string())
            .bind(chunk.chunk_index as i64)
            .bind(&chunk.chunk_hash)
            .bind(chunk.data.len() as i64)
            .bind(&now)
            .execute(&self.db_pool)
            .await;

            let mut uploads = self.uploads.write().await;
            if let Some(meta) = uploads.get_mut(&chunk.file_id) {
                meta.received_chunks.insert(chunk.chunk_index);
            }
        }

        let (uploaded_chunks, total_chunks, completed) = {
            let uploads = self.uploads.read().await;
            let meta = uploads.get(&chunk.file_id).unwrap();
            let received_count = meta.received_chunks.len() as u32;
            let total = meta.total_chunks.max(1);
            let is_complete = meta.total_chunks > 0
                && received_count >= meta.total_chunks
                && (0..meta.total_chunks).all(|i| meta.received_chunks.contains(&i));
            (received_count, total, is_complete)
        };

        let progress = UploadProgress {
            file_id: chunk.file_id,
            uploaded_chunks,
            total_chunks,
            percentage: (uploaded_chunks as f64 / total_chunks as f64) * 100.0,
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

        let actual_size = std::fs::metadata(&file_path)
            .map(|m| m.len() as i64)
            .unwrap_or(0);
        if meta.file_size > 0 && actual_size != meta.file_size {
            warn!(
                "File size mismatch for {}: expected {}, got {}",
                file_id, meta.file_size, actual_size
            );
            return Err(SyncError::UploadFailed(format!(
                "file size mismatch: expected {}, got {}",
                meta.file_size, actual_size
            )));
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

        // Update upload session status in DB
        let now = Utc::now().to_rfc3339();
        let _ = sqlx::query(
            "UPDATE sync_uploads SET status = 'completed', updated_at = ? WHERE id = ?",
        )
        .bind(&now)
        .bind(file_id.to_string())
        .execute(&self.db_pool)
        .await;

        // Cleanup metadata
        self.uploads.write().await.remove(&file_id);

        info!(
            "Upload finalized and verified for {} ({})",
            meta.filename, file_id
        );
        Ok(())
    }

    pub async fn get_upload_progress(
        &self,
        file_id: &Uuid,
    ) -> Result<Option<UploadProgress>, SyncError> {
        let meta = {
            let uploads = self.uploads.read().await;
            uploads.get(file_id).cloned()
        };

        if let Some(m) = meta {
            let uploaded = m.received_chunks.len() as u32;
            let total = m.total_chunks.max(1);
            let completed = m.total_chunks > 0
                && uploaded >= m.total_chunks
                && (0..m.total_chunks).all(|i| m.received_chunks.contains(&i));
            return Ok(Some(UploadProgress {
                file_id: *file_id,
                uploaded_chunks: uploaded,
                total_chunks: total,
                percentage: (uploaded as f64 / total as f64) * 100.0,
                completed,
            }));
        }

        // Fallback to DB check
        if let Ok(Some(row)) = sqlx::query_as::<_, (i64, String)>(
            "SELECT total_chunks, status FROM sync_uploads WHERE id = ?",
        )
        .bind(file_id.to_string())
        .fetch_optional(&self.db_pool)
        .await
        {
            let (total, status) = row;
            let chunk_count: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM sync_upload_chunks WHERE file_id = ?")
                    .bind(file_id.to_string())
                    .fetch_one(&self.db_pool)
                    .await
                    .unwrap_or(0);

            let total_u32 = (total as u32).max(1);
            let uploaded_u32 = chunk_count as u32;
            return Ok(Some(UploadProgress {
                file_id: *file_id,
                uploaded_chunks: uploaded_u32,
                total_chunks: total_u32,
                percentage: (uploaded_u32 as f64 / total_u32 as f64) * 100.0,
                completed: status == "completed",
            }));
        }

        Ok(None)
    }

    pub async fn check_file_exists(
        &self,
        file_hash: &str,
    ) -> Result<Option<SyncedFile>, SyncError> {
        let file = sqlx::query_as::<_, SyncedFile>(
            "SELECT * FROM synced_files WHERE file_hash = ? AND checksum_verified = TRUE",
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
        let now = Utc::now();

        sqlx::query(
            "INSERT INTO synced_files (id, filename, original_path, server_path, file_hash, file_size, uploaded_at, uploaded_by, checksum_verified)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, TRUE)"
        )
        .bind(id)
        .bind(&filename)
        .bind(&original_path)
        .bind(&server_path)
        .bind(&file_hash)
        .bind(file_size)
        .bind(now)
        .bind(&uploaded_by)
        .execute(&self.db_pool)
        .await?;

        Ok(id)
    }

    pub async fn get_playback_state(&self) -> Result<PlaybackState, SyncError> {
        let row = sqlx::query_as::<_, (String, String, bool, f64, String)>(
            "SELECT current_track_id, position_ms, playing, volume, updated_at
             FROM playback_sessions ORDER BY updated_at DESC LIMIT 1",
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
                shuffle: false,
                repeat: "none".into(),
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

    pub async fn initiate_handoff(
        &self,
        from_device: String,
        to_device: String,
    ) -> Result<SessionData, SyncError> {
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

    #[tokio::test]
    async fn test_resumable_upload_out_of_order_and_idempotent() {
        use sqlx::sqlite::SqlitePoolOptions;
        let pool = SqlitePoolOptions::new()
            .connect("sqlite::memory:")
            .await
            .unwrap();

        // Run migrations/tables
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS synced_files (
                id TEXT PRIMARY KEY,
                filename TEXT NOT NULL,
                original_path TEXT NOT NULL,
                server_path TEXT NOT NULL,
                file_hash TEXT NOT NULL,
                file_size INTEGER NOT NULL,
                uploaded_at TEXT NOT NULL,
                uploaded_by TEXT NOT NULL,
                checksum_verified INTEGER NOT NULL DEFAULT 1
            )",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS sync_uploads (
                id TEXT PRIMARY KEY,
                filename TEXT NOT NULL,
                original_path TEXT NOT NULL,
                file_size INTEGER NOT NULL,
                expected_hash TEXT NOT NULL,
                uploaded_by TEXT NOT NULL,
                total_chunks INTEGER NOT NULL,
                chunk_size INTEGER NOT NULL,
                status TEXT NOT NULL DEFAULT 'uploading',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS sync_upload_chunks (
                file_id TEXT NOT NULL,
                chunk_index INTEGER NOT NULL,
                chunk_hash TEXT NOT NULL,
                size INTEGER NOT NULL,
                created_at TEXT NOT NULL,
                PRIMARY KEY (file_id, chunk_index)
            )",
        )
        .execute(&pool)
        .await
        .unwrap();

        let temp_dir = tempfile::tempdir().unwrap();
        let sync_mgr = SyncManager {
            db_pool: pool.clone(),
            upload_dir: temp_dir.path().to_path_buf(),
            chunk_size: 4,
            uploads: Arc::new(RwLock::new(HashMap::new())),
        };

        // File: "ABCDEFGH" (8 bytes, chunk_size=4 => 2 chunks)
        // chunk 0: "ABCD"
        // chunk 1: "EFGH"
        let data_chunk0 = b"ABCD".to_vec();
        let data_chunk1 = b"EFGH".to_vec();

        let mut h0 = Sha256::new();
        h0.update(&data_chunk0);
        let hash0 = format!("{:x}", h0.finalize());

        let mut h1 = Sha256::new();
        h1.update(&data_chunk1);
        let hash1 = format!("{:x}", h1.finalize());

        let mut h_total = Sha256::new();
        h_total.update(b"ABCDEFGH");
        let expected_total_hash = format!("{:x}", h_total.finalize());

        let file_id = sync_mgr
            .init_upload(UploadInit {
                filename: "test.bin".into(),
                original_path: "/tmp/test.bin".into(),
                file_size: 8,
                expected_hash: expected_total_hash.clone(),
                uploaded_by: "tester".into(),
            })
            .await
            .unwrap();

        // Send Chunk 1 FIRST (out of order)
        let prog1 = sync_mgr
            .upload_chunk(UploadChunk {
                file_id,
                chunk_index: 1,
                total_chunks: 2,
                data: data_chunk1.clone(),
                chunk_hash: hash1.clone(),
            })
            .await
            .unwrap();
        assert_eq!(prog1.uploaded_chunks, 1);
        assert!(!prog1.completed);

        // Resend Chunk 1 (idempotency)
        let prog1_dup = sync_mgr
            .upload_chunk(UploadChunk {
                file_id,
                chunk_index: 1,
                total_chunks: 2,
                data: data_chunk1.clone(),
                chunk_hash: hash1.clone(),
            })
            .await
            .unwrap();
        assert_eq!(prog1_dup.uploaded_chunks, 1);
        assert!(!prog1_dup.completed);

        // Send Chunk 0 (completes upload)
        let prog0 = sync_mgr
            .upload_chunk(UploadChunk {
                file_id,
                chunk_index: 0,
                total_chunks: 2,
                data: data_chunk0.clone(),
                chunk_hash: hash0.clone(),
            })
            .await
            .unwrap();
        assert_eq!(prog0.uploaded_chunks, 2);
        assert!(prog0.completed);

        // Verify final file content on disk
        let written = std::fs::read(temp_dir.path().join(file_id.to_string())).unwrap();
        assert_eq!(written, b"ABCDEFGH");

        // Verify registered in DB
        let exists = sync_mgr
            .check_file_exists(&expected_total_hash)
            .await
            .unwrap();
        assert!(exists.is_some());
    }
}
