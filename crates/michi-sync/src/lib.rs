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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum UploadStatus {
    Uploading,
    Finalizing,
    Completed,
    Failed,
    Cancelled,
}

impl UploadStatus {
    pub fn as_db_str(self) -> &'static str {
        match self {
            Self::Uploading => "uploading",
            Self::Finalizing => "finalizing",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn from_db(value: &str) -> Result<Self, SyncError> {
        match value {
            "uploading" => Ok(Self::Uploading),
            "finalizing" => Ok(Self::Finalizing),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            other => Err(SyncError::InvalidPersistedState(format!(
                "unknown upload status in database: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UploadProgress {
    pub file_id: Uuid,
    pub uploaded_chunks: u32,
    pub total_chunks: u32,
    pub percentage: f64,
    pub status: UploadStatus,
    pub completed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UploadFailureClass {
    Recoverable,
    Terminal,
}

impl UploadFailureClass {
    pub fn as_db_str(self) -> &'static str {
        match self {
            Self::Recoverable => "recoverable",
            Self::Terminal => "terminal",
        }
    }

    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "recoverable" => Some(Self::Recoverable),
            "terminal" => Some(Self::Terminal),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncRecoveryReport {
    pub candidates: usize,
    pub completed: usize,
    pub deferred: usize,
    pub terminal_failures: usize,
    pub transient_failures: usize,
    pub invalid_rows: usize,
}

#[derive(Debug, Clone)]
pub struct SyncRuntimeConfig {
    pub chunk_size: usize,
    pub finalize_lease_duration: std::time::Duration,
}

impl Default for SyncRuntimeConfig {
    fn default() -> Self {
        Self {
            chunk_size: 1024 * 1024,
            finalize_lease_duration: std::time::Duration::from_secs(30),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UploadFailureCode {
    HashMismatch,
    SizeMismatch,
    MissingChunk,
    ArtifactMissing,
    InvalidPersistedState,
    IoTemporary,
    DatabaseTemporary,
}

impl UploadFailureCode {
    pub fn as_db_str(self) -> &'static str {
        match self {
            Self::HashMismatch => "hash_mismatch",
            Self::SizeMismatch => "size_mismatch",
            Self::MissingChunk => "missing_chunk",
            Self::ArtifactMissing => "artifact_missing",
            Self::InvalidPersistedState => "invalid_persisted_state",
            Self::IoTemporary => "io_temporary",
            Self::DatabaseTemporary => "database_temporary",
        }
    }

    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "hash_mismatch" | "HASH_MISMATCH" => Some(Self::HashMismatch),
            "size_mismatch" | "SIZE_MISMATCH" => Some(Self::SizeMismatch),
            "missing_chunk" | "MISSING_CHUNK" => Some(Self::MissingChunk),
            "artifact_missing" | "ARTIFACT_MISSING" => Some(Self::ArtifactMissing),
            "invalid_persisted_state" | "INVALID_PERSISTED_STATE" => {
                Some(Self::InvalidPersistedState)
            }
            "io_temporary" | "IO_TEMPORARY" => Some(Self::IoTemporary),
            "database_temporary" | "DATABASE_TEMPORARY" => Some(Self::DatabaseTemporary),
            _ => None,
        }
    }
}

impl std::fmt::Display for UploadFailureCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_db_str())
    }
}

#[derive(Error, Debug)]
pub enum SyncError {
    #[error("File not found: {0}")]
    FileNotFound(String),
    #[error("Session not found: {0}")]
    SessionNotFound(Uuid),
    #[error("Finalization ownership lost for upload {0}")]
    FinalizationOwnershipLost(Uuid),
    #[error("Finalization lease expired for upload {0}")]
    FinalizationLeaseExpired(Uuid),
    #[error("Finalization lease active for upload {file_id}")]
    FinalizationLeaseActive {
        file_id: Uuid,
        owner_epoch: Option<String>,
        lease_until: Option<DateTime<Utc>>,
    },
    #[error("Terminal upload failure for upload {file_id}: {code}")]
    TerminalUploadFailure {
        file_id: Uuid,
        code: UploadFailureCode,
    },
    #[error("Recoverable upload failure for upload {file_id}: {code}")]
    RecoverableUploadFailure {
        file_id: Uuid,
        code: UploadFailureCode,
    },
    #[error("Chunk conflict on index {index}: {message}")]
    ChunkConflict { index: u32, message: String },
    #[error("Invalid chunk parameter: {0}")]
    InvalidChunkParameter(String),
    #[error("Upload session already completed: {0}")]
    UploadAlreadyCompleted(Uuid),
    #[error("Upload session cancelled: {0}")]
    UploadCancelled(Uuid),
    #[error("Hash mismatch: expected {expected}, got {actual}")]
    HashMismatch { expected: String, actual: String },
    #[error("Upload failed: {0}")]
    UploadFailed(String),
    #[error("Invalid persisted state: {0}")]
    InvalidPersistedState(String),
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

pub const FINALIZATION_STALE_AFTER: std::time::Duration = std::time::Duration::from_secs(30);

fn is_valid_sha256_hex(h: &str) -> bool {
    h.len() == 64 && h.chars().all(|c| c.is_ascii_hexdigit())
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
    status: UploadStatus,
    finalize_token: Option<String>,
    finalize_owner_epoch: Option<String>,
    finalize_started_at: Option<String>,
    finalize_attempts: i64,
    finalize_lease_until: Option<String>,
    failure_class: Option<UploadFailureClass>,
    failure_code: Option<String>,
    last_error: Option<String>,
    received_chunks: HashSet<u32>,
}

impl UploadMeta {
    pub fn is_completed(&self) -> bool {
        self.status == UploadStatus::Completed
    }
}

#[derive(sqlx::FromRow)]
#[allow(dead_code)]
struct SyncUploadRow {
    id: String,
    filename: String,
    original_path: String,
    file_size: i64,
    expected_hash: String,
    uploaded_by: String,
    total_chunks: i64,
    chunk_size: i64,
    status: String,
    finalize_token: Option<String>,
    finalize_owner_epoch: Option<String>,
    finalize_started_at: Option<String>,
    finalize_attempts: i64,
    finalize_lease_until: Option<String>,
    failure_class: Option<String>,
    failure_code: Option<String>,
    last_error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SyncManager {
    db_pool: SqlitePool,
    upload_dir: PathBuf,
    runtime_config: SyncRuntimeConfig,
    process_epoch: Uuid,
    uploads: Arc<RwLock<HashMap<Uuid, UploadMeta>>>,
}

impl SyncManager {
    pub fn new(db_pool: SqlitePool, upload_dir: PathBuf) -> Self {
        Self::new_with_config(db_pool, upload_dir, SyncRuntimeConfig::default())
    }

    pub fn new_with_chunk_size(
        db_pool: SqlitePool,
        upload_dir: PathBuf,
        chunk_size: usize,
    ) -> Self {
        Self::new_with_config(
            db_pool,
            upload_dir,
            SyncRuntimeConfig {
                chunk_size: chunk_size.max(1),
                finalize_lease_duration: std::time::Duration::from_secs(30),
            },
        )
    }

    pub fn new_with_config(
        db_pool: SqlitePool,
        upload_dir: PathBuf,
        runtime_config: SyncRuntimeConfig,
    ) -> Self {
        Self::new_with_config_and_epoch(db_pool, upload_dir, runtime_config, Uuid::new_v4())
    }

    pub fn new_with_config_and_epoch(
        db_pool: SqlitePool,
        upload_dir: PathBuf,
        runtime_config: SyncRuntimeConfig,
        process_epoch: Uuid,
    ) -> Self {
        Self {
            db_pool,
            upload_dir,
            runtime_config,
            process_epoch,
            uploads: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn process_epoch(&self) -> Uuid {
        self.process_epoch
    }

    pub async fn calculate_file_hash<P: AsRef<Path>>(&self, path: P) -> Result<String, SyncError> {
        self.calculate_file_hash_with_heartbeat(path, None).await
    }

    pub async fn calculate_file_hash_with_heartbeat<P: AsRef<Path>>(
        &self,
        path: P,
        heartbeat: Option<(Uuid, &str)>,
    ) -> Result<String, SyncError> {
        let mut file = File::open(path.as_ref()).await?;
        let mut hasher = Sha256::new();
        let mut buffer = vec![0u8; self.runtime_config.chunk_size.min(1024 * 1024)];
        let mut bytes_since_heartbeat: usize = 0;
        const HEARTBEAT_INTERVAL_BYTES: usize = 4 * 1024 * 1024; // 4MB

        loop {
            let bytes_read = file.read(&mut buffer).await?;
            if bytes_read == 0 {
                break;
            }
            hasher.update(&buffer[..bytes_read]);
            bytes_since_heartbeat = bytes_since_heartbeat.saturating_add(bytes_read);

            if let Some((fid, tok)) = heartbeat {
                if bytes_since_heartbeat >= HEARTBEAT_INTERVAL_BYTES {
                    self.renew_finalization_lease(fid, tok).await?;
                    bytes_since_heartbeat = 0;
                }
            }
        }

        if let Some((fid, tok)) = heartbeat {
            self.renew_finalization_lease(fid, tok).await?;
        }

        Ok(format!("{:x}", hasher.finalize()))
    }

    pub async fn init_upload(&self, init: UploadInit) -> Result<Uuid, SyncError> {
        if init.file_size <= 0 {
            return Err(SyncError::InvalidChunkParameter(
                "file_size must be positive and greater than 0".into(),
            ));
        }

        if !is_valid_sha256_hex(&init.expected_hash) {
            return Err(SyncError::InvalidChunkParameter(
                "expected_hash must be a 64-character hexadecimal SHA-256 string".into(),
            ));
        }

        let file_size_usize = usize::try_from(init.file_size).map_err(|_| {
            SyncError::InvalidChunkParameter("file_size exceeds memory address space".into())
        })?;

        let file_id = Uuid::new_v4();
        let filename = init.filename.clone();
        let chunks = file_size_usize.div_ceil(self.runtime_config.chunk_size);
        let total_chunks = u32::try_from(chunks).map_err(|_| {
            SyncError::InvalidChunkParameter(
                "upload requires more chunks than protocol can represent".into(),
            )
        })?;

        let meta = UploadMeta {
            id: file_id,
            filename: init.filename.clone(),
            original_path: init.original_path.clone(),
            file_size: init.file_size,
            expected_hash: init.expected_hash.clone(),
            uploaded_by: init.uploaded_by.clone(),
            total_chunks,
            chunk_size: self.runtime_config.chunk_size,
            status: UploadStatus::Uploading,
            finalize_token: None,
            finalize_owner_epoch: None,
            finalize_started_at: None,
            finalize_attempts: 0,
            finalize_lease_until: None,
            failure_class: None,
            failure_code: None,
            last_error: None,
            received_chunks: HashSet::new(),
        };

        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO sync_uploads (id, filename, original_path, file_size, expected_hash, uploaded_by, total_chunks, chunk_size, status, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'uploading', ?, ?)"
        )
        .bind(file_id.to_string())
        .bind(&init.filename)
        .bind(&init.original_path)
        .bind(init.file_size)
        .bind(&init.expected_hash)
        .bind(&init.uploaded_by)
        .bind(total_chunks as i64)
        .bind(self.runtime_config.chunk_size as i64)
        .bind(&now)
        .bind(&now)
        .execute(&self.db_pool)
        .await?;

        self.uploads.write().await.insert(file_id, meta);
        info!(
            "Upload initialized: {} -> {} (total_chunks: {}, chunk_size: {})",
            file_id, filename, total_chunks, self.runtime_config.chunk_size
        );
        Ok(file_id)
    }

    pub async fn upload_chunk(&self, chunk: UploadChunk) -> Result<UploadProgress, SyncError> {
        if chunk.data.is_empty() {
            return Err(SyncError::InvalidChunkParameter(
                "chunk data cannot be empty".into(),
            ));
        }
        if chunk.total_chunks == 0 {
            return Err(SyncError::InvalidChunkParameter(
                "total_chunks must be greater than 0".into(),
            ));
        }
        if !is_valid_sha256_hex(&chunk.chunk_hash) {
            return Err(SyncError::InvalidChunkParameter(
                "chunk_hash must be a 64-character hexadecimal SHA-256 string".into(),
            ));
        }

        // 1. Verify individual chunk hash
        let mut hasher = Sha256::new();
        hasher.update(&chunk.data);
        let computed_chunk_hash = format!("{:x}", hasher.finalize());
        if computed_chunk_hash != chunk.chunk_hash {
            return Err(SyncError::HashMismatch {
                expected: chunk.chunk_hash,
                actual: computed_chunk_hash,
            });
        }

        // 2. Ensure session is loaded (from memory or SQLite on restart)
        let meta = self.get_or_load_session(chunk.file_id).await?;

        // 3. Validate session invariants and chunk parameters
        if meta.status == UploadStatus::Completed {
            return Err(SyncError::UploadAlreadyCompleted(chunk.file_id));
        }
        if meta.status == UploadStatus::Cancelled {
            return Err(SyncError::UploadCancelled(chunk.file_id));
        }

        if chunk.total_chunks != meta.total_chunks {
            return Err(SyncError::ChunkConflict {
                index: chunk.chunk_index,
                message: format!(
                    "request total_chunks {} != session total_chunks {}",
                    chunk.total_chunks, meta.total_chunks
                ),
            });
        }

        if chunk.chunk_index >= meta.total_chunks {
            return Err(SyncError::InvalidChunkParameter(format!(
                "chunk_index {} >= total_chunks {}",
                chunk.chunk_index, meta.total_chunks
            )));
        }

        let is_last_chunk = chunk.chunk_index == meta.total_chunks - 1;
        let expected_size = if is_last_chunk {
            let rem = meta.file_size as usize % meta.chunk_size;
            if rem == 0 {
                meta.chunk_size
            } else {
                rem
            }
        } else {
            meta.chunk_size
        };

        if chunk.data.len() != expected_size {
            return Err(SyncError::InvalidChunkParameter(format!(
                "invalid chunk size on index {}: expected {} bytes, got {} bytes",
                chunk.chunk_index,
                expected_size,
                chunk.data.len()
            )));
        }

        // 4. Check existing chunk
        let existing_chunk = sqlx::query_as::<_, (String, i64)>(
            "SELECT chunk_hash, size FROM sync_upload_chunks WHERE file_id = ? AND chunk_index = ?",
        )
        .bind(chunk.file_id.to_string())
        .bind(chunk.chunk_index as i64)
        .fetch_optional(&self.db_pool)
        .await?;

        if let Some((existing_hash, existing_size)) = existing_chunk {
            if existing_hash == chunk.chunk_hash && existing_size == chunk.data.len() as i64 {
                let current_meta = self.get_or_load_session(chunk.file_id).await?;
                if current_meta.status == UploadStatus::Completed {
                    let count = current_meta.received_chunks.len() as u32;
                    let total = current_meta.total_chunks.max(1);
                    return Ok(UploadProgress {
                        file_id: chunk.file_id,
                        uploaded_chunks: count,
                        total_chunks: total,
                        percentage: 100.0,
                        status: UploadStatus::Completed,
                        completed: true,
                        error: None,
                    });
                }

                let all_received = current_meta.total_chunks > 0
                    && (0..current_meta.total_chunks)
                        .all(|i| current_meta.received_chunks.contains(&i));

                if all_received {
                    let finalize_res = self.verify_and_finalize_upload(chunk.file_id).await;
                    let refreshed = self.get_or_load_session(chunk.file_id).await?;
                    let is_completed = refreshed.is_completed();
                    let count = refreshed.received_chunks.len() as u32;
                    let total = refreshed.total_chunks.max(1);

                    if let Err(e) = finalize_res {
                        if !is_completed {
                            return Err(e);
                        }
                    }

                    return Ok(UploadProgress {
                        file_id: chunk.file_id,
                        uploaded_chunks: count,
                        total_chunks: total,
                        percentage: (count as f64 / total as f64) * 100.0,
                        status: refreshed.status,
                        completed: is_completed,
                        error: refreshed.last_error,
                    });
                }

                let count = current_meta.received_chunks.len() as u32;
                let total = current_meta.total_chunks.max(1);
                return Ok(UploadProgress {
                    file_id: chunk.file_id,
                    uploaded_chunks: count,
                    total_chunks: total,
                    percentage: (count as f64 / total as f64) * 100.0,
                    status: current_meta.status,
                    completed: false,
                    error: current_meta.last_error,
                });
            } else {
                return Err(SyncError::ChunkConflict {
                    index: chunk.chunk_index,
                    message: format!(
                        "chunk {} already exists with different hash or size (existing hash: {}, request hash: {})",
                        chunk.chunk_index, existing_hash, chunk.chunk_hash
                    ),
                });
            }
        }

        // 5. Deterministic offset write to staging path `<uuid>.part`
        let part_file_path = self.upload_dir.join(format!("{}.part", chunk.file_id));
        let offset = (chunk.chunk_index as u64)
            .checked_mul(meta.chunk_size as u64)
            .ok_or_else(|| {
                SyncError::InvalidChunkParameter(
                    "arithmetic overflow calculating chunk offset".into(),
                )
            })?;

        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&part_file_path)
            .await?;

        file.seek(std::io::SeekFrom::Start(offset)).await?;
        file.write_all(&chunk.data).await?;
        file.sync_all().await?;

        // 6. Commit persistent chunk receipt in SQLite
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT OR IGNORE INTO sync_upload_chunks (file_id, chunk_index, chunk_hash, size, created_at)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(chunk.file_id.to_string())
        .bind(chunk.chunk_index as i64)
        .bind(&chunk.chunk_hash)
        .bind(chunk.data.len() as i64)
        .bind(&now)
        .execute(&self.db_pool)
        .await?;

        // Update in-memory state
        {
            let mut uploads = self.uploads.write().await;
            if let Some(m) = uploads.get_mut(&chunk.file_id) {
                m.received_chunks.insert(chunk.chunk_index);
            }
        }

        // 7. Check if all chunks have been received and finalize
        let (uploaded_chunks, total_chunks, all_chunks_present) = {
            let uploads = self.uploads.read().await;
            let current_meta = uploads.get(&chunk.file_id).unwrap();
            let count = current_meta.received_chunks.len() as u32;
            let total = current_meta.total_chunks.max(1);
            let is_all = current_meta.total_chunks > 0
                && (0..current_meta.total_chunks)
                    .all(|i| current_meta.received_chunks.contains(&i));
            (count, total, is_all)
        };

        if all_chunks_present {
            self.verify_and_finalize_upload(chunk.file_id).await?;
        }

        // Functional truth: query the latest status from memory or DB
        let meta_refreshed = self
            .get_or_load_session(chunk.file_id)
            .await
            .unwrap_or(meta);
        let is_completed = meta_refreshed.is_completed();

        let progress = UploadProgress {
            file_id: chunk.file_id,
            uploaded_chunks,
            total_chunks,
            percentage: (uploaded_chunks as f64 / total_chunks as f64) * 100.0,
            status: meta_refreshed.status,
            completed: is_completed,
            error: meta_refreshed.last_error,
        };

        Ok(progress)
    }

    pub(crate) async fn reload_session_from_db(
        &self,
        file_id: Uuid,
    ) -> Result<UploadMeta, SyncError> {
        let meta = self.load_session_from_db_internal(file_id).await?;
        let mut uploads = self.uploads.write().await;
        uploads.insert(file_id, meta.clone());
        Ok(meta)
    }

    async fn load_session_from_db_internal(&self, file_id: Uuid) -> Result<UploadMeta, SyncError> {
        let row = sqlx::query_as::<_, SyncUploadRow>(
            "SELECT id, filename, original_path, file_size, expected_hash, uploaded_by, total_chunks, chunk_size, status, finalize_token, finalize_owner_epoch, finalize_started_at, finalize_attempts, finalize_lease_until, failure_class, failure_code, last_error
             FROM sync_uploads WHERE id = ?"
        )
        .bind(file_id.to_string())
        .fetch_optional(&self.db_pool)
        .await?;

        let Some(row) = row else {
            return Err(SyncError::SessionNotFound(file_id));
        };

        let chunk_rows = sqlx::query_as::<_, (i64,)>(
            "SELECT chunk_index FROM sync_upload_chunks WHERE file_id = ?",
        )
        .bind(file_id.to_string())
        .fetch_all(&self.db_pool)
        .await?;

        let mut received = HashSet::new();
        for (idx,) in chunk_rows {
            received.insert(idx as u32);
        }

        let status = UploadStatus::from_db(&row.status)?;
        let failure_class = row
            .failure_class
            .as_deref()
            .and_then(UploadFailureClass::from_db);

        Ok(UploadMeta {
            id: file_id,
            filename: row.filename,
            original_path: row.original_path,
            file_size: row.file_size,
            expected_hash: row.expected_hash,
            uploaded_by: row.uploaded_by,
            total_chunks: row.total_chunks as u32,
            chunk_size: row.chunk_size as usize,
            status,
            finalize_token: row.finalize_token,
            finalize_owner_epoch: row.finalize_owner_epoch,
            finalize_started_at: row.finalize_started_at,
            finalize_attempts: row.finalize_attempts,
            finalize_lease_until: row.finalize_lease_until,
            failure_class,
            failure_code: row.failure_code,
            last_error: row.last_error,
            received_chunks: received,
        })
    }

    async fn get_or_load_session(&self, file_id: Uuid) -> Result<UploadMeta, SyncError> {
        // 1. Fast path: check in-memory cache with read lock
        {
            let uploads = self.uploads.read().await;
            if let Some(meta) = uploads.get(&file_id) {
                return Ok(meta.clone());
            }
        }

        // 2. Load from DB without holding RwLock (prevents head-of-line blocking)
        let meta = self.load_session_from_db_internal(file_id).await?;

        // 3. Insert with write lock (double check)
        let mut uploads = self.uploads.write().await;
        if let Some(existing) = uploads.get(&file_id) {
            return Ok(existing.clone());
        }
        uploads.insert(file_id, meta.clone());
        Ok(meta)
    }

    pub async fn assert_finalization_owner(
        &self,
        file_id: Uuid,
        token: &str,
    ) -> Result<(), SyncError> {
        let row = sqlx::query_as::<_, (String, Option<String>, Option<String>, Option<String>)>(
            "SELECT status, finalize_token, finalize_owner_epoch, finalize_lease_until FROM sync_uploads WHERE id = ?",
        )
        .bind(file_id.to_string())
        .fetch_optional(&self.db_pool)
        .await?;

        let Some((status, db_token, db_epoch, lease_until)) = row else {
            return Err(SyncError::SessionNotFound(file_id));
        };

        if status != "finalizing"
            || db_token.as_deref() != Some(token)
            || db_epoch.as_deref() != Some(&self.process_epoch.to_string())
        {
            return Err(SyncError::FinalizationOwnershipLost(file_id));
        }

        if let Some(l_str) = lease_until {
            if let Ok(l_dt) = DateTime::parse_from_rfc3339(&l_str) {
                if Utc::now() >= l_dt.with_timezone(&Utc) {
                    return Err(SyncError::FinalizationLeaseExpired(file_id));
                }
            }
        }

        Ok(())
    }

    pub async fn renew_finalization_lease(
        &self,
        file_id: Uuid,
        token: &str,
    ) -> Result<(), SyncError> {
        let now_dt = Utc::now();
        let lease_until = (now_dt
            + chrono::Duration::from_std(self.runtime_config.finalize_lease_duration)
                .unwrap_or_else(|_| chrono::Duration::seconds(30)))
        .to_rfc3339();
        let now = now_dt.to_rfc3339();

        let rows = sqlx::query(
            "UPDATE sync_uploads SET finalize_lease_until = ?, updated_at = ?
             WHERE id = ? AND status = 'finalizing' AND finalize_token = ? AND finalize_owner_epoch = ?",
        )
        .bind(&lease_until)
        .bind(&now)
        .bind(file_id.to_string())
        .bind(token)
        .bind(self.process_epoch.to_string())
        .execute(&self.db_pool)
        .await?
        .rows_affected();

        if rows == 0 {
            return Err(SyncError::FinalizationOwnershipLost(file_id));
        }

        let mut uploads = self.uploads.write().await;
        if let Some(m) = uploads.get_mut(&file_id) {
            m.finalize_lease_until = Some(lease_until);
        }

        Ok(())
    }

    pub async fn mark_upload_failed_as_owner(
        &self,
        file_id: Uuid,
        token: &str,
        failure_class: UploadFailureClass,
        failure_code: &str,
        reason: &str,
    ) -> Result<(), SyncError> {
        let now = Utc::now().to_rfc3339();
        let rows = sqlx::query(
            "UPDATE sync_uploads
             SET status = 'failed', failure_class = ?, failure_code = ?, last_error = ?, finalize_token = NULL, finalize_owner_epoch = NULL, finalize_lease_until = NULL, updated_at = ?
             WHERE id = ? AND status = 'finalizing' AND finalize_token = ? AND finalize_owner_epoch = ?",
        )
        .bind(failure_class.as_db_str())
        .bind(failure_code)
        .bind(reason)
        .bind(&now)
        .bind(file_id.to_string())
        .bind(token)
        .bind(self.process_epoch.to_string())
        .execute(&self.db_pool)
        .await?
        .rows_affected();

        if rows == 0 {
            let _ = self.reload_session_from_db(file_id).await;
            return Err(SyncError::FinalizationOwnershipLost(file_id));
        }

        let mut uploads = self.uploads.write().await;
        if let Some(m) = uploads.get_mut(&file_id) {
            m.status = UploadStatus::Failed;
            m.failure_class = Some(failure_class);
            m.failure_code = Some(failure_code.to_string());
            m.last_error = Some(reason.to_string());
            m.finalize_token = None;
            m.finalize_owner_epoch = None;
            m.finalize_lease_until = None;
        }

        Ok(())
    }

    pub async fn mark_upload_failed_unowned(
        &self,
        file_id: Uuid,
        failure_class: UploadFailureClass,
        failure_code: &str,
        reason: &str,
    ) -> Result<(), SyncError> {
        let now = Utc::now().to_rfc3339();
        let rows = sqlx::query(
            "UPDATE sync_uploads
             SET status = 'failed', failure_class = ?, failure_code = ?, last_error = ?, updated_at = ?
             WHERE id = ? AND status IN ('uploading', 'failed')",
        )
        .bind(failure_class.as_db_str())
        .bind(failure_code)
        .bind(reason)
        .bind(&now)
        .bind(file_id.to_string())
        .execute(&self.db_pool)
        .await?
        .rows_affected();

        if rows > 0 {
            let mut uploads = self.uploads.write().await;
            if let Some(m) = uploads.get_mut(&file_id) {
                m.status = UploadStatus::Failed;
                m.failure_class = Some(failure_class);
                m.failure_code = Some(failure_code.to_string());
                m.last_error = Some(reason.to_string());
            }
        }

        Ok(())
    }

    pub async fn mark_upload_completed(&self, file_id: Uuid, token: &str) -> Result<(), SyncError> {
        let now = Utc::now().to_rfc3339();
        let rows = sqlx::query(
            "UPDATE sync_uploads
             SET status = 'completed', finalize_token = NULL, finalize_owner_epoch = NULL, finalize_started_at = NULL, finalize_lease_until = NULL, failure_class = NULL, failure_code = NULL, last_error = NULL, updated_at = ?
             WHERE id = ? AND status = 'finalizing' AND finalize_token = ? AND finalize_owner_epoch = ?",
        )
        .bind(&now)
        .bind(file_id.to_string())
        .bind(token)
        .bind(self.process_epoch.to_string())
        .execute(&self.db_pool)
        .await?
        .rows_affected();

        if rows == 0 {
            let _ = self.reload_session_from_db(file_id).await;
            return Err(SyncError::FinalizationOwnershipLost(file_id));
        }

        let mut uploads = self.uploads.write().await;
        if let Some(m) = uploads.get_mut(&file_id) {
            m.status = UploadStatus::Completed;
            m.finalize_token = None;
            m.finalize_owner_epoch = None;
            m.finalize_started_at = None;
            m.finalize_lease_until = None;
            m.failure_class = None;
            m.failure_code = None;
            m.last_error = None;
        }

        Ok(())
    }

    pub async fn verify_and_finalize_upload(&self, file_id: Uuid) -> Result<(), SyncError> {
        let meta = self.get_or_load_session(file_id).await?;

        if meta.status == UploadStatus::Completed {
            let final_file_path = self.upload_dir.join(file_id.to_string());
            if tokio::fs::metadata(&final_file_path).await.is_ok() {
                return Ok(());
            }
        }
        if meta.status == UploadStatus::Cancelled {
            return Err(SyncError::UploadCancelled(file_id));
        }

        let token = Uuid::new_v4().to_string();
        let now_dt = Utc::now();
        let now = now_dt.to_rfc3339();
        let lease_until = (now_dt
            + chrono::Duration::from_std(self.runtime_config.finalize_lease_duration)
                .unwrap_or_else(|_| chrono::Duration::seconds(30)))
        .to_rfc3339();

        // 1. Attempt atomic initial acquisition from uploading or recoverable failed
        let rows_affected = sqlx::query(
            "UPDATE sync_uploads
             SET status = 'finalizing', finalize_token = ?, finalize_owner_epoch = ?, finalize_started_at = ?, finalize_lease_until = ?, finalize_attempts = finalize_attempts + 1, last_error = NULL, updated_at = ?
             WHERE id = ? AND (status = 'uploading' OR (status = 'failed' AND (failure_class IS NULL OR failure_class = 'recoverable')))",
        )
        .bind(&token)
        .bind(self.process_epoch.to_string())
        .bind(&now)
        .bind(&lease_until)
        .bind(&now)
        .bind(file_id.to_string())
        .execute(&self.db_pool)
        .await?
        .rows_affected();

        let owner_token = if rows_affected == 1 {
            token
        } else {
            let row = sqlx::query_as::<
                _,
                (
                    String,
                    Option<String>,
                    Option<String>,
                    Option<String>,
                    Option<String>,
                    Option<String>,
                ),
            >(
                "SELECT status, finalize_token, finalize_owner_epoch, finalize_started_at, finalize_lease_until, failure_class FROM sync_uploads WHERE id = ?",
            )
            .bind(file_id.to_string())
            .fetch_optional(&self.db_pool)
            .await?;

            match row {
                Some((
                    st_str,
                    existing_token,
                    existing_epoch,
                    started_at_str,
                    lease_until_str,
                    failure_class_str,
                )) => {
                    let st = UploadStatus::from_db(&st_str)?;
                    if st == UploadStatus::Completed {
                        let mut uploads = self.uploads.write().await;
                        if let Some(m) = uploads.get_mut(&file_id) {
                            m.status = UploadStatus::Completed;
                        }
                        return Ok(());
                    }

                    if st == UploadStatus::Failed
                        && failure_class_str.as_deref() == Some("terminal")
                    {
                        return Err(SyncError::TerminalUploadFailure {
                            file_id,
                            code: UploadFailureCode::InvalidPersistedState,
                        });
                    }

                    if st == UploadStatus::Finalizing {
                        let is_from_previous_process =
                            existing_epoch.as_deref() != Some(&self.process_epoch.to_string());

                        let is_stale = if is_from_previous_process {
                            // Crash recovery rule: an owner from a prior dead process is dead immediately
                            true
                        } else if let Some(l_str) = lease_until_str.as_ref() {
                            if let Ok(l_dt) = DateTime::parse_from_rfc3339(l_str) {
                                Utc::now() >= l_dt.with_timezone(&Utc)
                            } else {
                                true
                            }
                        } else if let Some(s_str) = started_at_str.as_ref() {
                            if let Ok(started_dt) = DateTime::parse_from_rfc3339(s_str) {
                                (Utc::now() - started_dt.with_timezone(&Utc))
                                    .to_std()
                                    .unwrap_or_default()
                                    >= self.runtime_config.finalize_lease_duration
                            } else {
                                true
                            }
                        } else {
                            true
                        };

                        if !is_stale {
                            // Active worker in current process holds unexpired lease: poll briefly
                            for _ in 0..20 {
                                tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
                                let poll_st = sqlx::query_scalar::<_, String>(
                                    "SELECT status FROM sync_uploads WHERE id = ?",
                                )
                                .bind(file_id.to_string())
                                .fetch_optional(&self.db_pool)
                                .await?;

                                if let Some(s) = poll_st {
                                    if s == "completed" {
                                        let mut uploads = self.uploads.write().await;
                                        if let Some(m) = uploads.get_mut(&file_id) {
                                            m.status = UploadStatus::Completed;
                                        }
                                        return Ok(());
                                    }
                                }
                            }
                            let lease_dt = lease_until_str.as_deref().and_then(|s| {
                                DateTime::parse_from_rfc3339(s)
                                    .ok()
                                    .map(|d| d.with_timezone(&Utc))
                            });
                            return Err(SyncError::FinalizationLeaseActive {
                                file_id,
                                owner_epoch: existing_epoch,
                                lease_until: lease_dt,
                            });
                        } else {
                            // Takeover CAS: reclaim from dead process epoch or expired lease
                            let takeover_token = Uuid::new_v4().to_string();
                            let takeover_now_dt = Utc::now();
                            let takeover_now = takeover_now_dt.to_rfc3339();
                            let takeover_lease_until = (takeover_now_dt
                                + chrono::Duration::from_std(
                                    self.runtime_config.finalize_lease_duration,
                                )
                                .unwrap_or_else(|_| chrono::Duration::seconds(30)))
                            .to_rfc3339();

                            let takeover_rows = if is_from_previous_process {
                                sqlx::query(
                                    "UPDATE sync_uploads
                                     SET finalize_token = ?, finalize_owner_epoch = ?, finalize_started_at = ?, finalize_lease_until = ?, finalize_attempts = finalize_attempts + 1, updated_at = ?
                                     WHERE id = ? AND status = 'finalizing' AND (finalize_owner_epoch IS NULL OR finalize_owner_epoch IS NOT ?)",
                                )
                                .bind(&takeover_token)
                                .bind(self.process_epoch.to_string())
                                .bind(&takeover_now)
                                .bind(&takeover_lease_until)
                                .bind(&takeover_now)
                                .bind(file_id.to_string())
                                .bind(self.process_epoch.to_string())
                                .execute(&self.db_pool)
                                .await?
                                .rows_affected()
                            } else {
                                sqlx::query(
                                    "UPDATE sync_uploads
                                     SET finalize_token = ?, finalize_owner_epoch = ?, finalize_started_at = ?, finalize_lease_until = ?, finalize_attempts = finalize_attempts + 1, updated_at = ?
                                     WHERE id = ? AND status = 'finalizing' AND finalize_token IS ? AND finalize_lease_until IS ?",
                                )
                                .bind(&takeover_token)
                                .bind(self.process_epoch.to_string())
                                .bind(&takeover_now)
                                .bind(&takeover_lease_until)
                                .bind(&takeover_now)
                                .bind(file_id.to_string())
                                .bind(&existing_token)
                                .bind(&lease_until_str)
                                .execute(&self.db_pool)
                                .await?
                                .rows_affected()
                            };

                            if takeover_rows == 1 {
                                takeover_token
                            } else {
                                return Err(SyncError::FinalizationOwnershipLost(file_id));
                            }
                        }
                    } else {
                        return Err(SyncError::UploadFailed(format!(
                            "cannot finalize upload in status {st_str}"
                        )));
                    }
                }
                None => return Err(SyncError::SessionNotFound(file_id)),
            }
        };

        // Update in-memory status
        {
            let mut uploads = self.uploads.write().await;
            if let Some(m) = uploads.get_mut(&file_id) {
                m.status = UploadStatus::Finalizing;
                m.finalize_token = Some(owner_token.clone());
                m.finalize_owner_epoch = Some(self.process_epoch.to_string());
                m.finalize_lease_until = Some(lease_until.clone());
            }
        }

        // 2. Verify DB contains all chunks
        let db_chunks = sqlx::query_as::<_, (i64,)>(
            "SELECT chunk_index FROM sync_upload_chunks WHERE file_id = ?",
        )
        .bind(file_id.to_string())
        .fetch_all(&self.db_pool)
        .await?;

        let db_chunk_set: HashSet<u32> = db_chunks.into_iter().map(|(idx,)| idx as u32).collect();
        for expected_idx in 0..meta.total_chunks {
            if !db_chunk_set.contains(&expected_idx) {
                let err_msg = format!(
                    "cannot finalize: missing chunk index {expected_idx} in persistent storage"
                );
                let _ = self
                    .mark_upload_failed_as_owner(
                        file_id,
                        &owner_token,
                        UploadFailureClass::Terminal,
                        UploadFailureCode::MissingChunk.as_db_str(),
                        &err_msg,
                    )
                    .await;
                return Err(SyncError::TerminalUploadFailure {
                    file_id,
                    code: UploadFailureCode::MissingChunk,
                });
            }
        }

        // Renew lease before disk operations
        self.renew_finalization_lease(file_id, &owner_token).await?;

        // 3. Verify physical staging file `<uuid>.part` or recovered `<uuid>`
        let part_file_path = self.upload_dir.join(format!("{file_id}.part"));
        let final_file_path = self.upload_dir.join(file_id.to_string());

        let target_file_to_verify = if tokio::fs::metadata(&part_file_path).await.is_ok() {
            part_file_path.clone()
        } else if tokio::fs::metadata(&final_file_path).await.is_ok() {
            final_file_path.clone()
        } else {
            let err_msg = format!(
                "neither staging file {part_file_path:?} nor final file {final_file_path:?} exists on disk"
            );
            let _ = self
                .mark_upload_failed_as_owner(
                    file_id,
                    &owner_token,
                    UploadFailureClass::Terminal,
                    UploadFailureCode::ArtifactMissing.as_db_str(),
                    &err_msg,
                )
                .await;
            return Err(SyncError::FileNotFound(err_msg));
        };

        let actual_size = tokio::fs::metadata(&target_file_to_verify).await?.len() as i64;

        if actual_size != meta.file_size {
            warn!(
                "File size mismatch for {}: expected {}, got {}",
                file_id, meta.file_size, actual_size
            );
            let err_msg = format!(
                "file size mismatch: expected {}, got {}",
                meta.file_size, actual_size
            );
            let _ = self
                .mark_upload_failed_as_owner(
                    file_id,
                    &owner_token,
                    UploadFailureClass::Terminal,
                    UploadFailureCode::SizeMismatch.as_db_str(),
                    &err_msg,
                )
                .await;
            return Err(SyncError::TerminalUploadFailure {
                file_id,
                code: UploadFailureCode::SizeMismatch,
            });
        }

        // 4. Verify whole-file SHA-256 with periodic lease renewal
        let computed_hash = self
            .calculate_file_hash_with_heartbeat(
                &target_file_to_verify,
                Some((file_id, &owner_token)),
            )
            .await?;
        if computed_hash != meta.expected_hash {
            warn!(
                "Hash mismatch for {}: expected {}, got {}",
                file_id, meta.expected_hash, computed_hash
            );
            let _ = self
                .mark_upload_failed_as_owner(
                    file_id,
                    &owner_token,
                    UploadFailureClass::Terminal,
                    UploadFailureCode::HashMismatch.as_db_str(),
                    &format!(
                        "hash mismatch: expected {}, got {}",
                        meta.expected_hash, computed_hash
                    ),
                )
                .await;
            return Err(SyncError::HashMismatch {
                expected: meta.expected_hash.clone(),
                actual: computed_hash,
            });
        }

        // Fencing check before promote
        self.assert_finalization_owner(file_id, &owner_token)
            .await?;

        // 5. Atomic rename staging file to final file if still in .part staging
        if target_file_to_verify == part_file_path {
            tokio::fs::rename(&part_file_path, &final_file_path).await?;
        }

        // Fencing check before register
        self.assert_finalization_owner(file_id, &owner_token)
            .await?;

        // 6. Idempotent registration in synced_files
        let server_path = final_file_path.to_string_lossy().to_string();
        self.register_uploaded_file(
            meta.filename.clone(),
            meta.original_path.clone(),
            server_path,
            meta.expected_hash.clone(),
            meta.file_size,
            meta.uploaded_by.clone(),
        )
        .await?;

        // 7. Update upload session status to completed using fencing token
        self.mark_upload_completed(file_id, &owner_token).await?;

        info!(
            "Upload finalized and verified for {} ({})",
            meta.filename, file_id
        );
        Ok(())
    }

    pub async fn recover_incomplete_uploads(&self) -> Result<SyncRecoveryReport, SyncError> {
        let candidate_rows = sqlx::query_as::<_, (String, String, Option<String>)>(
            "SELECT id, status, failure_class FROM sync_uploads WHERE status = 'finalizing' OR (status = 'failed' AND (failure_class IS NULL OR failure_class = 'recoverable'))",
        )
        .fetch_all(&self.db_pool)
        .await?;

        let mut report = SyncRecoveryReport {
            candidates: candidate_rows.len(),
            ..Default::default()
        };

        for (id_str, _, _) in candidate_rows {
            let file_id = match Uuid::parse_str(&id_str) {
                Ok(id) => id,
                Err(e) => {
                    warn!(
                        upload_id = %id_str,
                        error = %e,
                        "persisted sync upload contains invalid UUID; recording corrupt row"
                    );
                    report.invalid_rows += 1;
                    continue;
                }
            };

            match self.verify_and_finalize_upload(file_id).await {
                Ok(()) => {
                    info!(upload_id = %file_id, "successfully recovered incomplete sync upload");
                    report.completed += 1;
                }
                Err(SyncError::FinalizationLeaseActive { .. }) => {
                    report.deferred += 1;
                }
                Err(SyncError::TerminalUploadFailure { .. })
                | Err(SyncError::HashMismatch { .. })
                | Err(SyncError::FileNotFound(_)) => {
                    report.terminal_failures += 1;
                }
                Err(e) => {
                    warn!(
                        upload_id = %file_id,
                        error = %e,
                        "sync recovery encountered transient error"
                    );
                    report.transient_failures += 1;
                }
            }
        }

        info!(
            candidates = report.candidates,
            completed = report.completed,
            deferred = report.deferred,
            terminal = report.terminal_failures,
            transient = report.transient_failures,
            invalid = report.invalid_rows,
            "sync startup recovery scan finished"
        );

        Ok(report)
    }

    pub async fn get_upload_progress(
        &self,
        file_id: &Uuid,
    ) -> Result<Option<UploadProgress>, SyncError> {
        let meta = match self.get_or_load_session(*file_id).await {
            Ok(m) => m,
            Err(SyncError::SessionNotFound(_)) => return Ok(None),
            Err(e) => return Err(e),
        };

        let uploaded = meta.received_chunks.len() as u32;
        let total = meta.total_chunks.max(1);
        let completed = meta.is_completed();

        Ok(Some(UploadProgress {
            file_id: *file_id,
            uploaded_chunks: uploaded,
            total_chunks: total,
            percentage: (uploaded as f64 / total as f64) * 100.0,
            status: meta.status,
            completed,
            error: meta.last_error,
        }))
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
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, 1)
             ON CONFLICT(file_hash, server_path) DO NOTHING",
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

        let canonical_id = sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM synced_files WHERE file_hash = ? AND server_path = ?",
        )
        .bind(&file_hash)
        .bind(&server_path)
        .fetch_one(&self.db_pool)
        .await?;

        Ok(canonical_id)
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
    use sqlx::sqlite::SqlitePoolOptions;

    async fn create_test_db() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .connect("sqlite::memory:")
            .await
            .unwrap();

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
                checksum_verified INTEGER NOT NULL DEFAULT 1,
                UNIQUE(file_hash, server_path)
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
                finalize_token TEXT,
                finalize_owner_epoch TEXT,
                finalize_started_at TEXT,
                finalize_attempts INTEGER NOT NULL DEFAULT 0,
                finalize_lease_until TEXT,
                failure_class TEXT,
                failure_code TEXT,
                last_error TEXT,
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

        pool
    }

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

    // ── Test A: Non-divisible size uploaded out-of-order ────────────
    #[tokio::test]
    async fn test_falsification_a_non_divisible_out_of_order() {
        let pool = create_test_db().await;
        let temp_dir = tempfile::tempdir().unwrap();
        let chunk_size = 10;
        let sync_mgr =
            SyncManager::new_with_chunk_size(pool, temp_dir.path().to_path_buf(), chunk_size);

        // File size = 25 bytes => 3 chunks: chunk 0 (10), chunk 1 (10), chunk 2 (5)
        let full_content = b"0123456789ABCDEFGHIJ99999";
        assert_eq!(full_content.len(), 25);

        let mut h_total = Sha256::new();
        h_total.update(full_content);
        let expected_hash = format!("{:x}", h_total.finalize());

        let chunk0_data = full_content[0..10].to_vec();
        let chunk1_data = full_content[10..20].to_vec();
        let chunk2_data = full_content[20..25].to_vec();

        let hash0 = format!("{:x}", Sha256::digest(&chunk0_data));
        let hash1 = format!("{:x}", Sha256::digest(&chunk1_data));
        let hash2 = format!("{:x}", Sha256::digest(&chunk2_data));

        let file_id = sync_mgr
            .init_upload(UploadInit {
                filename: "nondiv.bin".into(),
                original_path: "/tmp/nondiv.bin".into(),
                file_size: 25,
                expected_hash: expected_hash.clone(),
                uploaded_by: "tester".into(),
            })
            .await
            .unwrap();

        // Send in order: 2, 0, 1
        let p2 = sync_mgr
            .upload_chunk(UploadChunk {
                file_id,
                chunk_index: 2,
                total_chunks: 3,
                data: chunk2_data,
                chunk_hash: hash2,
            })
            .await
            .unwrap();
        assert_eq!(p2.uploaded_chunks, 1);
        assert!(!p2.completed);

        let p0 = sync_mgr
            .upload_chunk(UploadChunk {
                file_id,
                chunk_index: 0,
                total_chunks: 3,
                data: chunk0_data,
                chunk_hash: hash0,
            })
            .await
            .unwrap();
        assert_eq!(p0.uploaded_chunks, 2);
        assert!(!p0.completed);

        let p1 = sync_mgr
            .upload_chunk(UploadChunk {
                file_id,
                chunk_index: 1,
                total_chunks: 3,
                data: chunk1_data,
                chunk_hash: hash1,
            })
            .await
            .unwrap();
        assert_eq!(p1.uploaded_chunks, 3);
        assert!(p1.completed);

        // Verify final file is intact
        let written = std::fs::read(temp_dir.path().join(file_id.to_string())).unwrap();
        assert_eq!(written, full_content);

        // Verify registered
        let registered = sync_mgr.check_file_exists(&expected_hash).await.unwrap();
        assert!(registered.is_some());
    }

    // ── Test B: Identical retransmission is idempotent ───────────────
    #[tokio::test]
    async fn test_falsification_b_identical_retransmission() {
        let pool = create_test_db().await;
        let temp_dir = tempfile::tempdir().unwrap();
        let sync_mgr = SyncManager::new_with_chunk_size(pool, temp_dir.path().to_path_buf(), 4);

        let chunk0_data = b"ABCD".to_vec();
        let hash0 = format!("{:x}", Sha256::digest(&chunk0_data));

        let mut h_total = Sha256::new();
        h_total.update(b"ABCD1234");
        let expected_hash = format!("{:x}", h_total.finalize());

        let file_id = sync_mgr
            .init_upload(UploadInit {
                filename: "dup.bin".into(),
                original_path: "/tmp/dup.bin".into(),
                file_size: 8,
                expected_hash,
                uploaded_by: "tester".into(),
            })
            .await
            .unwrap();

        let p1 = sync_mgr
            .upload_chunk(UploadChunk {
                file_id,
                chunk_index: 0,
                total_chunks: 2,
                data: chunk0_data.clone(),
                chunk_hash: hash0.clone(),
            })
            .await
            .unwrap();
        assert_eq!(p1.uploaded_chunks, 1);

        // Resend identical chunk 0
        let p2 = sync_mgr
            .upload_chunk(UploadChunk {
                file_id,
                chunk_index: 0,
                total_chunks: 2,
                data: chunk0_data,
                chunk_hash: hash0,
            })
            .await
            .unwrap();
        assert_eq!(p2.uploaded_chunks, 1);
    }

    // ── Test C: Conflicting chunk rejected & logged ──────────────────
    #[tokio::test]
    async fn test_falsification_c_chunk_conflict() {
        let pool = create_test_db().await;
        let temp_dir = tempfile::tempdir().unwrap();
        let sync_mgr = SyncManager::new_with_chunk_size(pool, temp_dir.path().to_path_buf(), 4);

        let chunk0_a = b"ABCD".to_vec();
        let hash0_a = format!("{:x}", Sha256::digest(&chunk0_a));

        let chunk0_b = b"WXYZ".to_vec();
        let hash0_b = format!("{:x}", Sha256::digest(&chunk0_b));

        let file_id = sync_mgr
            .init_upload(UploadInit {
                filename: "conflict.bin".into(),
                original_path: "/tmp/conflict.bin".into(),
                file_size: 8,
                expected_hash: format!("{:x}", Sha256::digest(b"conflict_full")),
                uploaded_by: "tester".into(),
            })
            .await
            .unwrap();

        sync_mgr
            .upload_chunk(UploadChunk {
                file_id,
                chunk_index: 0,
                total_chunks: 2,
                data: chunk0_a,
                chunk_hash: hash0_a,
            })
            .await
            .unwrap();

        // Send conflicting chunk 0
        let err = sync_mgr
            .upload_chunk(UploadChunk {
                file_id,
                chunk_index: 0,
                total_chunks: 2,
                data: chunk0_b,
                chunk_hash: hash0_b,
            })
            .await
            .unwrap_err();

        assert!(matches!(err, SyncError::ChunkConflict { .. }));
    }

    // ── Test D: Real restart reconstruction from SQLite ──────────────
    #[tokio::test]
    async fn test_falsification_d_restart_reconstruction() {
        let pool = create_test_db().await;
        let temp_dir = tempfile::tempdir().unwrap();
        let upload_path = temp_dir.path().to_path_buf();

        let chunk0_data = b"HEAD".to_vec();
        let hash0 = format!("{:x}", Sha256::digest(&chunk0_data));
        let chunk1_data = b"TAIL".to_vec();
        let hash1 = format!("{:x}", Sha256::digest(&chunk1_data));
        let full_data = b"HEADTAIL".to_vec();
        let expected_hash = format!("{:x}", Sha256::digest(&full_data));

        let file_id = {
            let sync_mgr = SyncManager::new_with_chunk_size(pool.clone(), upload_path.clone(), 4);
            let fid = sync_mgr
                .init_upload(UploadInit {
                    filename: "restart.bin".into(),
                    original_path: "/tmp/restart.bin".into(),
                    file_size: 8,
                    expected_hash: expected_hash.clone(),
                    uploaded_by: "tester".into(),
                })
                .await
                .unwrap();

            // Upload chunk 0
            sync_mgr
                .upload_chunk(UploadChunk {
                    file_id: fid,
                    chunk_index: 0,
                    total_chunks: 2,
                    data: chunk0_data,
                    chunk_hash: hash0,
                })
                .await
                .unwrap();

            fid
            // Instance sync_mgr dropped here
        };

        // Fresh SyncManager instance
        let sync_mgr2 = SyncManager::new_with_chunk_size(pool, upload_path, 4);

        // Upload chunk 1 into new manager
        let prog = sync_mgr2
            .upload_chunk(UploadChunk {
                file_id,
                chunk_index: 1,
                total_chunks: 2,
                data: chunk1_data,
                chunk_hash: hash1,
            })
            .await
            .unwrap();

        assert!(prog.completed);
        assert_eq!(prog.uploaded_chunks, 2);

        let final_bytes = std::fs::read(temp_dir.path().join(file_id.to_string())).unwrap();
        assert_eq!(final_bytes, b"HEADTAIL");
    }

    // ── Test E: SQLite failure propagates DatabaseError ──────────────
    #[tokio::test]
    async fn test_falsification_e_db_failure_propagates() {
        let pool = SqlitePoolOptions::new()
            .connect("sqlite::memory:")
            .await
            .unwrap();
        // Do not create tables to force error
        let temp_dir = tempfile::tempdir().unwrap();
        let sync_mgr = SyncManager::new(pool, temp_dir.path().to_path_buf());

        let res = sync_mgr
            .init_upload(UploadInit {
                filename: "err.bin".into(),
                original_path: "/tmp/err.bin".into(),
                file_size: 10,
                expected_hash: format!("{:x}", Sha256::digest(b"err")),
                uploaded_by: "tester".into(),
            })
            .await;

        assert!(matches!(res, Err(SyncError::DatabaseError(_))));
    }

    // ── Test F: Whole-file hash mismatch rejected on finalize ────────
    #[tokio::test]
    async fn test_falsification_f_final_hash_mismatch() {
        let pool = create_test_db().await;
        let temp_dir = tempfile::tempdir().unwrap();
        let sync_mgr = SyncManager::new_with_chunk_size(pool, temp_dir.path().to_path_buf(), 4);

        let chunk0_data = b"REAL".to_vec();
        let hash0 = format!("{:x}", Sha256::digest(&chunk0_data));

        let file_id = sync_mgr
            .init_upload(UploadInit {
                filename: "fakehash.bin".into(),
                original_path: "/tmp/fakehash.bin".into(),
                file_size: 4,
                expected_hash: format!("{:x}", Sha256::digest(b"DIFFERENT")),
                uploaded_by: "tester".into(),
            })
            .await
            .unwrap();

        let err = sync_mgr
            .upload_chunk(UploadChunk {
                file_id,
                chunk_index: 0,
                total_chunks: 1,
                data: chunk0_data,
                chunk_hash: hash0,
            })
            .await
            .unwrap_err();

        assert!(matches!(err, SyncError::HashMismatch { .. }));
    }

    // ── Test G: File size mismatch rejected ──────────────────────────
    #[tokio::test]
    async fn test_falsification_g_size_mismatch() {
        let pool = create_test_db().await;
        let temp_dir = tempfile::tempdir().unwrap();
        let sync_mgr = SyncManager::new_with_chunk_size(pool, temp_dir.path().to_path_buf(), 4);

        let err = sync_mgr
            .init_upload(UploadInit {
                filename: "badsize.bin".into(),
                original_path: "/tmp/badsize.bin".into(),
                file_size: 0, // invalid <= 0
                expected_hash: format!("{:x}", Sha256::digest(b"badsize")),
                uploaded_by: "tester".into(),
            })
            .await
            .unwrap_err();

        assert!(matches!(err, SyncError::InvalidChunkParameter(_)));
    }

    // ── Test H: Total chunks mutation on subsequent chunks rejected ──
    #[tokio::test]
    async fn test_falsification_h_total_chunks_mutation() {
        let pool = create_test_db().await;
        let temp_dir = tempfile::tempdir().unwrap();
        let sync_mgr = SyncManager::new_with_chunk_size(pool, temp_dir.path().to_path_buf(), 4);

        let file_id = sync_mgr
            .init_upload(UploadInit {
                filename: "mut.bin".into(),
                original_path: "/tmp/mut.bin".into(),
                file_size: 8, // expects 2 chunks
                expected_hash: format!("{:x}", Sha256::digest(b"mut_full")),
                uploaded_by: "tester".into(),
            })
            .await
            .unwrap();

        let chunk_data = b"ABCD".to_vec();
        let chunk_hash = format!("{:x}", Sha256::digest(&chunk_data));

        let err = sync_mgr
            .upload_chunk(UploadChunk {
                file_id,
                chunk_index: 0,
                total_chunks: 5, // mismatch from session total_chunks = 2
                data: chunk_data,
                chunk_hash,
            })
            .await
            .unwrap_err();

        assert!(matches!(err, SyncError::ChunkConflict { .. }));
    }

    // ── Test I: Finalization failure marks failed & allows safe retry ─
    #[tokio::test]
    async fn test_falsification_i_finalization_failure_marks_failed() {
        let pool = create_test_db().await;
        let temp_dir = tempfile::tempdir().unwrap();
        let upload_path = temp_dir.path().to_path_buf();
        let sync_mgr = SyncManager::new_with_chunk_size(pool.clone(), upload_path, 4);

        let file_id = sync_mgr
            .init_upload(UploadInit {
                filename: "fail_finalize.bin".into(),
                original_path: "/tmp/fail_finalize.bin".into(),
                file_size: 4,
                expected_hash: format!("{:x}", Sha256::digest(b"wrong_full_hash")),
                uploaded_by: "tester".into(),
            })
            .await
            .unwrap();

        let chunk_data = b"ABCD".to_vec();
        let chunk_hash = format!("{:x}", Sha256::digest(&chunk_data));

        let err = sync_mgr
            .upload_chunk(UploadChunk {
                file_id,
                chunk_index: 0,
                total_chunks: 1,
                data: chunk_data,
                chunk_hash,
            })
            .await
            .unwrap_err();

        assert!(matches!(err, SyncError::HashMismatch { .. }));

        // Verify status in DB is marked as failed and not completed
        let row = sqlx::query_as::<_, (String,)>("SELECT status FROM sync_uploads WHERE id = ?")
            .bind(file_id.to_string())
            .fetch_one(&pool)
            .await
            .unwrap();

        assert_eq!(row.0, "failed");

        let prog = sync_mgr
            .get_upload_progress(&file_id)
            .await
            .unwrap()
            .unwrap();
        assert!(!prog.completed);
    }

    // ── Test J: Concurrent finalization owner ─────────────────────────
    #[tokio::test]
    async fn test_falsification_j_concurrent_finalization() {
        let pool = create_test_db().await;
        let temp_dir = tempfile::tempdir().unwrap();
        let upload_path = temp_dir.path().to_path_buf();
        let sync_mgr = Arc::new(SyncManager::new_with_chunk_size(
            pool.clone(),
            upload_path,
            4,
        ));

        let chunk_data = b"CONC".to_vec();
        let expected_hash = format!("{:x}", Sha256::digest(&chunk_data));

        let file_id = sync_mgr
            .init_upload(UploadInit {
                filename: "conc.bin".into(),
                original_path: "/tmp/conc.bin".into(),
                file_size: 4,
                expected_hash: expected_hash.clone(),
                uploaded_by: "tester".into(),
            })
            .await
            .unwrap();

        let barrier = Arc::new(tokio::sync::Barrier::new(2));
        let b1 = barrier.clone();
        let b2 = barrier.clone();

        let mgr1 = sync_mgr.clone();
        let mgr2 = sync_mgr.clone();
        let h1 = expected_hash.clone();
        let h2 = expected_hash.clone();

        let t1 = tokio::spawn(async move {
            b1.wait().await;
            mgr1.upload_chunk(UploadChunk {
                file_id,
                chunk_index: 0,
                total_chunks: 1,
                data: b"CONC".to_vec(),
                chunk_hash: h1,
            })
            .await
        });

        let t2 = tokio::spawn(async move {
            b2.wait().await;
            mgr2.upload_chunk(UploadChunk {
                file_id,
                chunk_index: 0,
                total_chunks: 1,
                data: b"CONC".to_vec(),
                chunk_hash: h2,
            })
            .await
        });

        let (r1, r2) = tokio::join!(t1, t2);
        let prog1 = r1.unwrap();
        let prog2 = r2.unwrap();

        // Both concurrent calls must resolve successfully with completed state
        assert!(prog1.is_ok());
        assert!(prog2.is_ok());
        assert!(prog1.unwrap().completed);
        assert!(prog2.unwrap().completed);

        // Check DB has exactly one registered synced_file
        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM synced_files WHERE file_hash = ?")
            .bind(&expected_hash)
            .fetch_one(&pool)
            .await
            .unwrap();

        assert_eq!(count.0, 1);

        // Verify exactly one final file on disk
        let final_file = temp_dir.path().join(file_id.to_string());
        assert!(tokio::fs::metadata(&final_file).await.is_ok());
    }

    // ── Test K: Crash recovery after rename before status updated ─────
    #[tokio::test]
    async fn test_falsification_k_crash_after_rename() {
        let pool = create_test_db().await;
        let temp_dir = tempfile::tempdir().unwrap();
        let upload_path = temp_dir.path().to_path_buf();

        let chunk_data = b"CRASH1".to_vec();
        let expected_hash = format!("{:x}", Sha256::digest(&chunk_data));

        let file_id = {
            let sync_mgr = SyncManager::new_with_chunk_size(pool.clone(), upload_path.clone(), 6);
            let fid = sync_mgr
                .init_upload(UploadInit {
                    filename: "crash1.bin".into(),
                    original_path: "/tmp/crash1.bin".into(),
                    file_size: 6,
                    expected_hash: expected_hash.clone(),
                    uploaded_by: "tester".into(),
                })
                .await
                .unwrap();

            // Insert chunk to DB directly and create final renamed file directly (simulating crash after rename)
            sqlx::query(
                "INSERT INTO sync_upload_chunks (file_id, chunk_index, chunk_hash, size, created_at)
                 VALUES (?, 0, ?, 6, ?)"
            )
            .bind(fid.to_string())
            .bind(&expected_hash)
            .bind(Utc::now().to_rfc3339())
            .execute(&pool)
            .await
            .unwrap();

            let final_path = upload_path.join(fid.to_string());
            tokio::fs::write(&final_path, &chunk_data).await.unwrap();

            fid
        };

        // Fresh SyncManager restart reconciles final file
        let sync_mgr = SyncManager::new_with_chunk_size(pool.clone(), upload_path, 6);
        let res = sync_mgr.verify_and_finalize_upload(file_id).await;
        assert!(res.is_ok());

        let prog = sync_mgr
            .get_upload_progress(&file_id)
            .await
            .unwrap()
            .unwrap();
        assert!(prog.completed);
    }

    // ── Test L: Crash recovery after register before status completed ─
    #[tokio::test]
    async fn test_falsification_l_crash_after_register() {
        let pool = create_test_db().await;
        let temp_dir = tempfile::tempdir().unwrap();
        let upload_path = temp_dir.path().to_path_buf();

        let chunk_data = b"CRASH2".to_vec();
        let expected_hash = format!("{:x}", Sha256::digest(&chunk_data));

        let file_id = {
            let sync_mgr = SyncManager::new_with_chunk_size(pool.clone(), upload_path.clone(), 6);
            let fid = sync_mgr
                .init_upload(UploadInit {
                    filename: "crash2.bin".into(),
                    original_path: "/tmp/crash2.bin".into(),
                    file_size: 6,
                    expected_hash: expected_hash.clone(),
                    uploaded_by: "tester".into(),
                })
                .await
                .unwrap();

            sqlx::query(
                "INSERT INTO sync_upload_chunks (file_id, chunk_index, chunk_hash, size, created_at)
                 VALUES (?, 0, ?, 6, ?)"
            )
            .bind(fid.to_string())
            .bind(&expected_hash)
            .bind(Utc::now().to_rfc3339())
            .execute(&pool)
            .await
            .unwrap();

            let final_path = upload_path.join(fid.to_string());
            tokio::fs::write(&final_path, &chunk_data).await.unwrap();

            // Register in synced_files directly before crash
            sync_mgr
                .register_uploaded_file(
                    "crash2.bin".into(),
                    "/tmp/crash2.bin".into(),
                    final_path.to_string_lossy().to_string(),
                    expected_hash.clone(),
                    6,
                    "tester".into(),
                )
                .await
                .unwrap();

            fid
        };

        let sync_mgr = SyncManager::new_with_chunk_size(pool.clone(), upload_path, 6);
        let res = sync_mgr.verify_and_finalize_upload(file_id).await;
        assert!(res.is_ok());

        let prog = sync_mgr
            .get_upload_progress(&file_id)
            .await
            .unwrap()
            .unwrap();
        assert!(prog.completed);

        // Verify exactly one synced_file exists
        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM synced_files WHERE file_hash = ?")
            .bind(&expected_hash)
            .fetch_one(&pool)
            .await
            .unwrap();

        assert_eq!(count.0, 1);
    }

    // ── Test M: Status completed iff final artifact valid ─────────────
    #[tokio::test]
    async fn test_falsification_m_status_completed_iff_artifact_valid() {
        let pool = create_test_db().await;
        let temp_dir = tempfile::tempdir().unwrap();
        let upload_path = temp_dir.path().to_path_buf();
        let sync_mgr = SyncManager::new_with_chunk_size(pool.clone(), upload_path.clone(), 4);

        let file_id = sync_mgr
            .init_upload(UploadInit {
                filename: "valid.bin".into(),
                original_path: "/tmp/valid.bin".into(),
                file_size: 4,
                expected_hash: format!("{:x}", Sha256::digest(b"OKAY")),
                uploaded_by: "tester".into(),
            })
            .await
            .unwrap();

        let prog = sync_mgr
            .get_upload_progress(&file_id)
            .await
            .unwrap()
            .unwrap();
        assert!(!prog.completed);

        sync_mgr
            .upload_chunk(UploadChunk {
                file_id,
                chunk_index: 0,
                total_chunks: 1,
                data: b"OKAY".to_vec(),
                chunk_hash: format!("{:x}", Sha256::digest(b"OKAY")),
            })
            .await
            .unwrap();

        let final_path = upload_path.join(file_id.to_string());
        assert!(tokio::fs::metadata(&final_path).await.is_ok());

        let prog_done = sync_mgr
            .get_upload_progress(&file_id)
            .await
            .unwrap()
            .unwrap();
        assert!(prog_done.completed);
        assert_eq!(prog_done.uploaded_chunks, 1);
    }

    // ── Test N: Stale finalization lease is recovered by another worker ──
    #[tokio::test]
    async fn test_falsification_n_stale_finalization_is_recovered() {
        let pool = create_test_db().await;
        let temp_dir = tempfile::tempdir().unwrap();
        let upload_path = temp_dir.path().to_path_buf();

        let chunk_data = b"STALE".to_vec();
        let expected_hash = format!("{:x}", Sha256::digest(&chunk_data));
        let file_id = Uuid::new_v4();

        // Stale timestamp 60 seconds ago
        let stale_time = (Utc::now() - chrono::Duration::seconds(60)).to_rfc3339();
        let stale_token = Uuid::new_v4().to_string();

        sqlx::query(
            "INSERT INTO sync_uploads (id, filename, original_path, file_size, expected_hash, uploaded_by, total_chunks, chunk_size, status, finalize_token, finalize_started_at, finalize_attempts, created_at, updated_at)
             VALUES (?, 'stale.bin', '/tmp/stale.bin', 5, ?, 'tester', 1, 5, 'finalizing', ?, ?, 1, ?, ?)"
        )
        .bind(file_id.to_string())
        .bind(&expected_hash)
        .bind(&stale_token)
        .bind(&stale_time)
        .bind(&stale_time)
        .bind(&stale_time)
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO sync_upload_chunks (file_id, chunk_index, chunk_hash, size, created_at)
             VALUES (?, 0, ?, 5, ?)",
        )
        .bind(file_id.to_string())
        .bind(&expected_hash)
        .bind(&stale_time)
        .execute(&pool)
        .await
        .unwrap();

        // Write .part file to disk
        let part_path = upload_path.join(format!("{file_id}.part"));
        tokio::fs::write(&part_path, &chunk_data).await.unwrap();

        // Fresh sync manager recovers stale upload
        let sync_mgr = SyncManager::new_with_chunk_size(pool.clone(), upload_path, 5);
        let res = sync_mgr.verify_and_finalize_upload(file_id).await;
        assert!(res.is_ok());

        let prog = sync_mgr
            .get_upload_progress(&file_id)
            .await
            .unwrap()
            .unwrap();
        assert!(prog.completed);
        assert_eq!(prog.status, UploadStatus::Completed);
    }

    // ── Test O: Old owner is fenced after lease takeover ──────────────
    #[tokio::test]
    async fn test_falsification_o_old_owner_is_fenced_after_takeover() {
        let pool = create_test_db().await;
        let temp_dir = tempfile::tempdir().unwrap();
        let upload_path = temp_dir.path().to_path_buf();
        let sync_mgr = SyncManager::new_with_chunk_size(pool.clone(), upload_path, 5);

        let file_id = Uuid::new_v4();
        let token_a = "token-worker-a";
        let token_b = "token-worker-b";
        let now = Utc::now().to_rfc3339();

        sqlx::query(
            "INSERT INTO sync_uploads (id, filename, original_path, file_size, expected_hash, uploaded_by, total_chunks, chunk_size, status, finalize_token, finalize_started_at, finalize_attempts, created_at, updated_at)
             VALUES (?, 'fence.bin', '/tmp/fence.bin', 5, 'hash', 'tester', 1, 5, 'finalizing', ?, ?, 1, ?, ?)"
        )
        .bind(file_id.to_string())
        .bind(token_b) // Worker B is current owner
        .bind(&now)
        .bind(&now)
        .bind(&now)
        .execute(&pool)
        .await
        .unwrap();

        // Worker A attempts to mark upload completed with stale token A -> fenced!
        let err = sync_mgr
            .mark_upload_completed(file_id, token_a)
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            SyncError::FinalizationOwnershipLost(_) | SyncError::UploadFailed(_)
        ));

        // Verify DB row remains in finalizing status under token B
        let row = sqlx::query_as::<_, (String, Option<String>)>(
            "SELECT status, finalize_token FROM sync_uploads WHERE id = ?",
        )
        .bind(file_id.to_string())
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(row.0, "finalizing");
        assert_eq!(row.1, Some(token_b.to_string()));
    }

    // ── Test P: Corrupt status in DB fails closed ─────────────────────
    #[tokio::test]
    async fn test_falsification_p_status_corrupt_fails_closed() {
        let pool = create_test_db().await;
        let temp_dir = tempfile::tempdir().unwrap();
        let sync_mgr =
            SyncManager::new_with_chunk_size(pool.clone(), temp_dir.path().to_path_buf(), 4);

        let file_id = Uuid::new_v4();
        let now = Utc::now().to_rfc3339();

        sqlx::query(
            "INSERT INTO sync_uploads (id, filename, original_path, file_size, expected_hash, uploaded_by, total_chunks, chunk_size, status, created_at, updated_at)
             VALUES (?, 'corrupt.bin', '/tmp/corrupt.bin', 4, 'hash', 'tester', 1, 4, 'CORRUPT_STATE_XYZ', ?, ?)"
        )
        .bind(file_id.to_string())
        .bind(&now)
        .bind(&now)
        .execute(&pool)
        .await
        .unwrap();

        let err = sync_mgr.get_upload_progress(&file_id).await.unwrap_err();
        assert!(matches!(err, SyncError::InvalidPersistedState(_)));
    }

    // ── Test Q: recover_incomplete_uploads processes all candidates ───
    #[tokio::test]
    async fn test_falsification_q_recover_incomplete_uploads() {
        let pool = create_test_db().await;
        let temp_dir = tempfile::tempdir().unwrap();
        let upload_path = temp_dir.path().to_path_buf();

        let chunk_data = b"BATCH".to_vec();
        let expected_hash = format!("{:x}", Sha256::digest(&chunk_data));

        let file_id = Uuid::new_v4();
        let stale_time = (Utc::now() - chrono::Duration::seconds(60)).to_rfc3339();

        sqlx::query(
            "INSERT INTO sync_uploads (id, filename, original_path, file_size, expected_hash, uploaded_by, total_chunks, chunk_size, status, finalize_started_at, created_at, updated_at)
             VALUES (?, 'batch.bin', '/tmp/batch.bin', 5, ?, 'tester', 1, 5, 'finalizing', ?, ?, ?)"
        )
        .bind(file_id.to_string())
        .bind(&expected_hash)
        .bind(&stale_time)
        .bind(&stale_time)
        .bind(&stale_time)
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO sync_upload_chunks (file_id, chunk_index, chunk_hash, size, created_at)
             VALUES (?, 0, ?, 5, ?)",
        )
        .bind(file_id.to_string())
        .bind(&expected_hash)
        .bind(&stale_time)
        .execute(&pool)
        .await
        .unwrap();

        let part_path = upload_path.join(format!("{file_id}.part"));
        tokio::fs::write(&part_path, &chunk_data).await.unwrap();

        let sync_mgr = SyncManager::new_with_chunk_size(pool.clone(), upload_path, 5);
        let report = sync_mgr.recover_incomplete_uploads().await.unwrap();
        assert_eq!(report.candidates, 1);
        assert_eq!(report.completed, 1);

        let prog = sync_mgr
            .get_upload_progress(&file_id)
            .await
            .unwrap()
            .unwrap();
        assert!(prog.completed);
        assert_eq!(prog.status, UploadStatus::Completed);
    }

    // ── Test R: Old owner cannot mark failed after takeover ──────────
    #[tokio::test]
    async fn test_falsification_r_old_owner_cannot_mark_failed_after_takeover() {
        let pool = create_test_db().await;
        let temp_dir = tempfile::tempdir().unwrap();
        let upload_path = temp_dir.path().to_path_buf();
        let sync_mgr = SyncManager::new_with_chunk_size(pool.clone(), upload_path, 5);

        let file_id = Uuid::new_v4();
        let token_a = "token-worker-a";
        let token_b = "token-worker-b";
        let now = Utc::now().to_rfc3339();

        sqlx::query(
            "INSERT INTO sync_uploads (id, filename, original_path, file_size, expected_hash, uploaded_by, total_chunks, chunk_size, status, finalize_token, finalize_started_at, finalize_attempts, created_at, updated_at)
             VALUES (?, 'fence_fail.bin', '/tmp/fence_fail.bin', 5, 'hash', 'tester', 1, 5, 'finalizing', ?, ?, 1, ?, ?)"
        )
        .bind(file_id.to_string())
        .bind(token_b) // Worker B is current owner
        .bind(&now)
        .bind(&now)
        .bind(&now)
        .execute(&pool)
        .await
        .unwrap();

        // Worker A tries to mark failed with old token -> must be fenced and rejected!
        let err = sync_mgr
            .mark_upload_failed_as_owner(
                file_id,
                token_a,
                UploadFailureClass::Terminal,
                "LATE_ERROR",
                "worker A failed late",
            )
            .await
            .unwrap_err();
        assert!(matches!(err, SyncError::FinalizationOwnershipLost(_)));

        // Verify DB row remains finalizing under token B
        let row = sqlx::query_as::<_, (String, Option<String>)>(
            "SELECT status, finalize_token FROM sync_uploads WHERE id = ?",
        )
        .bind(file_id.to_string())
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(row.0, "finalizing");
        assert_eq!(row.1, Some(token_b.to_string()));
    }

    // ── Test S: Live owner lease renewal prevents takeover ───────────
    #[tokio::test]
    async fn test_falsification_s_live_owner_heartbeat_prevents_takeover() {
        let pool = create_test_db().await;
        let temp_dir = tempfile::tempdir().unwrap();
        let upload_path = temp_dir.path().to_path_buf();
        let sync_mgr = SyncManager::new_with_config(
            pool.clone(),
            upload_path,
            SyncRuntimeConfig {
                chunk_size: 5,
                finalize_lease_duration: std::time::Duration::from_millis(200),
            },
        );

        let file_id = Uuid::new_v4();
        let token_a = "token-worker-a";
        let now_dt = Utc::now();
        let now = now_dt.to_rfc3339();
        let lease_until = (now_dt + chrono::Duration::milliseconds(200)).to_rfc3339();

        sqlx::query(
            "INSERT INTO sync_uploads (id, filename, original_path, file_size, expected_hash, uploaded_by, total_chunks, chunk_size, status, finalize_token, finalize_owner_epoch, finalize_started_at, finalize_lease_until, finalize_attempts, created_at, updated_at)
             VALUES (?, 'live_lease.bin', '/tmp/live_lease.bin', 5, 'hash', 'tester', 1, 5, 'finalizing', ?, ?, ?, ?, 1, ?, ?)"
        )
        .bind(file_id.to_string())
        .bind(token_a)
        .bind(sync_mgr.process_epoch().to_string())
        .bind(&now)
        .bind(&lease_until)
        .bind(&now)
        .bind(&now)
        .execute(&pool)
        .await
        .unwrap();

        // Worker A renews lease
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        sync_mgr
            .renew_finalization_lease(file_id, token_a)
            .await
            .unwrap();

        // Worker B attempts takeover immediately -> lease still active, must not steal
        let err = sync_mgr
            .verify_and_finalize_upload(file_id)
            .await
            .unwrap_err();
        assert!(matches!(err, SyncError::FinalizationLeaseActive { .. }));

        let row = sqlx::query_as::<_, (String, Option<String>)>(
            "SELECT status, finalize_token FROM sync_uploads WHERE id = ?",
        )
        .bind(file_id.to_string())
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(row.0, "finalizing");
        assert_eq!(row.1, Some(token_a.to_string()));
    }
}
