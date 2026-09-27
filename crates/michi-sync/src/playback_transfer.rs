use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

/// Maximum payload size allowed for TailSync request (4 MiB).
pub const MAX_TAILSYNC_PAYLOAD_BYTES: usize = 4 * 1024 * 1024;
/// Maximum number of tracks in TailSync transfer queue.
pub const MAX_QUEUE_ENTRIES: usize = 10_000;
/// Default expiration duration for pending transfers (15 seconds).
pub const DEFAULT_TRANSFER_TTL: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContentRefV1 {
    #[serde(default)]
    pub track_id: Option<String>,
    pub title: String,
    pub artist: String,
    #[serde(default)]
    pub album: Option<String>,
    pub duration_ms: u64,
    #[serde(default)]
    pub content_sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TailSyncV1 {
    pub transfer_id: String,
    pub source_michi_id: String,
    pub target_michi_id: String,
    pub receiver_michi_id: String,
    /// Playback state: "playing" or "paused"
    pub status: String,
    pub position_ms: u64,
    pub current_track: ContentRefV1,
    #[serde(default)]
    pub queue: Vec<ContentRefV1>,
    #[serde(default)]
    pub queue_index: usize,
    #[serde(default = "default_repeat")]
    pub repeat_mode: String,
    #[serde(default)]
    pub shuffle: bool,
    pub timestamp_ms: i64,
    #[serde(default)]
    pub source_revision: u64,
}

fn default_repeat() -> String {
    "off".to_string()
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum TransferValidationError {
    #[error("Queue exceeds maximum limit of {0} items")]
    QueueTooLarge(usize),
    #[error("String field {field} exceeds maximum length {max}")]
    StringTooLong { field: &'static str, max: usize },
    #[error("Invalid playback status: {0} (must be 'playing' or 'paused')")]
    InvalidStatus(String),
    #[error("Empty title or artist in track reference")]
    MissingTrackMetadata,
    #[error("Queue index out of bounds")]
    QueueIndexOutOfBounds,
}

impl TailSyncV1 {
    pub fn validate_bounds(&self) -> Result<(), TransferValidationError> {
        if self.queue.len() > MAX_QUEUE_ENTRIES {
            return Err(TransferValidationError::QueueTooLarge(MAX_QUEUE_ENTRIES));
        }

        if self.status != "playing" && self.status != "paused" {
            return Err(TransferValidationError::InvalidStatus(self.status.clone()));
        }

        if !self.queue.is_empty() && self.queue_index >= self.queue.len() {
            return Err(TransferValidationError::QueueIndexOutOfBounds);
        }

        if self.transfer_id.len() > 128 {
            return Err(TransferValidationError::StringTooLong {
                field: "transfer_id",
                max: 128,
            });
        }
        if self.source_michi_id.len() > 128 {
            return Err(TransferValidationError::StringTooLong {
                field: "source_michi_id",
                max: 128,
            });
        }
        if self.target_michi_id.len() > 128 {
            return Err(TransferValidationError::StringTooLong {
                field: "target_michi_id",
                max: 128,
            });
        }
        if self.receiver_michi_id.len() > 128 {
            return Err(TransferValidationError::StringTooLong {
                field: "receiver_michi_id",
                max: 128,
            });
        }

        Self::validate_content_ref(&self.current_track)?;
        for track in &self.queue {
            Self::validate_content_ref(track)?;
        }

        Ok(())
    }

    fn validate_content_ref(track: &ContentRefV1) -> Result<(), TransferValidationError> {
        if track.title.trim().is_empty() || track.artist.trim().is_empty() {
            return Err(TransferValidationError::MissingTrackMetadata);
        }
        if track.title.len() > 512 {
            return Err(TransferValidationError::StringTooLong {
                field: "title",
                max: 512,
            });
        }
        if track.artist.len() > 512 {
            return Err(TransferValidationError::StringTooLong {
                field: "artist",
                max: 512,
            });
        }
        if let Some(ref alb) = track.album {
            if alb.len() > 512 {
                return Err(TransferValidationError::StringTooLong {
                    field: "album",
                    max: 512,
                });
            }
        }
        Ok(())
    }
}

/// State kept in RAM while a PawPass transfer is being coordinated.
#[derive(Debug, Clone)]
pub struct PendingPlaybackTransfer {
    pub transfer_id: String,
    pub source_michi_id: String,
    pub target_michi_id: String,
    pub receiver_michi_id: String,
    pub tailsync: TailSyncV1,
    pub prepared_current_track: michi_core::Track,
    pub prepared_queue: Vec<michi_core::Track>,
    pub created_at: DateTime<Utc>,
    pub expires_at: Instant,
}

impl PendingPlaybackTransfer {
    pub fn is_expired(&self) -> bool {
        Instant::now() >= self.expires_at
    }
}

/// In-memory store for pending playback transfers awaiting Stream authority handoff commit.
#[derive(Debug, Clone, Default)]
pub struct PendingTransferStore {
    transfers: Arc<RwLock<HashMap<String, PendingPlaybackTransfer>>>,
}

impl PendingTransferStore {
    pub fn new() -> Self {
        Self {
            transfers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn insert(&self, transfer: PendingPlaybackTransfer) {
        self.transfers
            .write()
            .await
            .insert(transfer.transfer_id.clone(), transfer);
    }

    pub async fn get(&self, transfer_id: &str) -> Option<PendingPlaybackTransfer> {
        let store = self.transfers.read().await;
        store.get(transfer_id).filter(|t| !t.is_expired()).cloned()
    }

    pub async fn remove(&self, transfer_id: &str) -> Option<PendingPlaybackTransfer> {
        self.transfers.write().await.remove(transfer_id)
    }

    pub async fn check_expirations(&self) -> Vec<String> {
        let mut expired = Vec::new();
        let mut store = self.transfers.write().await;
        let now = Instant::now();

        store.retain(|id, t| {
            if now >= t.expires_at {
                expired.push(id.clone());
                false
            } else {
                true
            }
        });

        expired
    }
}

/// Sign a canonical PawPass target ready proof.
pub fn sign_target_ready_proof(
    identity: &michi_identity::IdentityManager,
    transfer_id: &str,
    source_michi_id: &str,
    target_michi_id: &str,
    receiver_michi_id: &str,
    expected_epoch: u64,
    nonce: &str,
) -> String {
    let payload = format!(
        "{transfer_id}:{source_michi_id}:{target_michi_id}:{receiver_michi_id}:{expected_epoch}:{nonce}"
    );
    let (sig, _) = identity.sign_base64url(payload.as_bytes());
    sig
}

/// Verify a canonical PawPass target ready proof.
#[allow(clippy::too_many_arguments)]
pub fn verify_target_ready_proof(
    transfer_id: &str,
    source_michi_id: &str,
    target_michi_id: &str,
    receiver_michi_id: &str,
    expected_epoch: u64,
    nonce: &str,
    signature: &str,
    target_public_key: &str,
) -> Result<bool, String> {
    let payload = format!(
        "{transfer_id}:{source_michi_id}:{target_michi_id}:{receiver_michi_id}:{expected_epoch}:{nonce}"
    );
    michi_identity::IdentityManager::verify(payload.as_bytes(), signature, target_public_key)
        .map_err(|e| format!("signature verify error: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tailsync_bounds_validation() {
        let ts = TailSyncV1 {
            transfer_id: "xfer-1".into(),
            source_michi_id: "src-1".into(),
            target_michi_id: "tgt-1".into(),
            receiver_michi_id: "rec-1".into(),
            status: "playing".into(),
            position_ms: 12345,
            current_track: ContentRefV1 {
                track_id: None,
                title: "Song A".into(),
                artist: "Artist B".into(),
                album: None,
                duration_ms: 240000,
                content_sha256: None,
            },
            queue: vec![],
            queue_index: 0,
            repeat_mode: "off".into(),
            shuffle: false,
            timestamp_ms: 1000,
            source_revision: 1,
        };
        assert!(ts.validate_bounds().is_ok());

        // Invalid status
        let mut bad_status = ts.clone();
        bad_status.status = "stopped".into();
        assert!(bad_status.validate_bounds().is_err());

        // Empty title
        let mut empty_title = ts.clone();
        empty_title.current_track.title = "".into();
        assert!(empty_title.validate_bounds().is_err());
    }

    #[tokio::test]
    async fn test_pending_transfer_store_ttl() {
        let store = PendingTransferStore::new();
        let track = michi_core::Track {
            id: uuid::Uuid::new_v4(),
            title: Some("Test".into()),
            artist: Some("Artist".into()),
            album: None,
            album_artist: None,
            duration_ms: Some(180_000),
            file_path: "/tmp/test.flac".into(),
            format: michi_core::AudioFormat::Flac,
            sample_rate: Some(48000),
            bit_depth: Some(16),
            channels: Some(2),
            artwork_id: None,
            genre: None,
            year: None,
            track_number: None,
            disc_number: None,
            content_hash: None,
            file_size: None,
            file_mtime_ns: None,
            starred: false,
            rating: 0,
            starred_at: None,
            replaygain_track_gain: None,
            replaygain_track_peak: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        let ts = TailSyncV1 {
            transfer_id: "xfer-ttl".into(),
            source_michi_id: "src-1".into(),
            target_michi_id: "tgt-1".into(),
            receiver_michi_id: "rec-1".into(),
            status: "playing".into(),
            position_ms: 0,
            current_track: ContentRefV1 {
                track_id: None,
                title: "Test".into(),
                artist: "Artist".into(),
                album: None,
                duration_ms: 180000,
                content_sha256: None,
            },
            queue: vec![],
            queue_index: 0,
            repeat_mode: "off".into(),
            shuffle: false,
            timestamp_ms: 1000,
            source_revision: 1,
        };

        let pending = PendingPlaybackTransfer {
            transfer_id: "xfer-ttl".into(),
            source_michi_id: "src-1".into(),
            target_michi_id: "tgt-1".into(),
            receiver_michi_id: "rec-1".into(),
            tailsync: ts,
            prepared_current_track: track.clone(),
            prepared_queue: vec![track],
            created_at: Utc::now(),
            expires_at: Instant::now() + Duration::from_millis(50),
        };

        store.insert(pending).await;
        assert!(store.get("xfer-ttl").await.is_some());

        tokio::time::sleep(Duration::from_millis(60)).await;
        assert!(store.get("xfer-ttl").await.is_none());
    }
}
