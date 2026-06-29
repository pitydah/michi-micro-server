use std::fmt;
use std::path::Path;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum PathError {
    #[error("cannot canonicalize library root '{0}'")]
    CannotCanonicalizeRoot(String),

    #[error("cannot canonicalize file path '{0}'")]
    CannotCanonicalizeFile(String),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, ToSchema)]
#[non_exhaustive]
pub enum AudioFormat {
    Mp3,
    Flac,
    Ogg,
    Opus,
    Aac,
    M4a,
    Wav,
    Aiff,
    Dsf,
    Dff,
    Unknown,
}

impl AudioFormat {
    pub fn from_extension(ext: &str) -> Self {
        match ext.to_lowercase().as_str() {
            "mp3" => Self::Mp3,
            "flac" => Self::Flac,
            "ogg" => Self::Ogg,
            "opus" => Self::Opus,
            "aac" => Self::Aac,
            "m4a" => Self::M4a,
            "wav" => Self::Wav,
            "aiff" | "aif" => Self::Aiff,
            "dsf" => Self::Dsf,
            "dff" => Self::Dff,
            _ => Self::Unknown,
        }
    }

    pub fn mime_type(&self) -> &'static str {
        match self {
            Self::Mp3 => "audio/mpeg",
            Self::Flac => "audio/flac",
            Self::Ogg => "audio/ogg",
            Self::Opus => "audio/ogg",
            Self::Aac => "audio/aac",
            Self::M4a => "audio/mp4",
            Self::Wav => "audio/wav",
            Self::Aiff => "audio/aiff",
            Self::Dsf => "audio/dsf",
            Self::Dff => "audio/dff",
            Self::Unknown => "application/octet-stream",
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Mp3 => "mp3",
            Self::Flac => "flac",
            Self::Ogg => "ogg",
            Self::Opus => "opus",
            Self::Aac => "aac",
            Self::M4a => "m4a",
            Self::Wav => "wav",
            Self::Aiff => "aiff",
            Self::Dsf => "dsf",
            Self::Dff => "dff",
            Self::Unknown => "unknown",
        }
    }
}

impl fmt::Display for AudioFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for AudioFormat {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::from_extension(s))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AudioMetadata {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub genre: Option<String>,
    pub year: Option<i32>,
    pub track_number: Option<u32>,
    pub disc_number: Option<u32>,
    pub duration_ms: Option<u64>,
    pub sample_rate: Option<u32>,
    pub bit_depth: Option<u8>,
    pub channels: Option<u8>,
    pub format: AudioFormat,
    pub has_artwork: bool,
}

impl Default for AudioMetadata {
    fn default() -> Self {
        Self {
            title: None,
            artist: None,
            album: None,
            album_artist: None,
            genre: None,
            year: None,
            track_number: None,
            disc_number: None,
            duration_ms: None,
            sample_rate: None,
            bit_depth: None,
            channels: None,
            format: AudioFormat::Unknown,
            has_artwork: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Track {
    pub id: Uuid,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub duration_ms: Option<u64>,
    pub file_path: String,
    pub format: AudioFormat,
    pub sample_rate: Option<u32>,
    pub bit_depth: Option<u8>,
    pub channels: Option<u8>,
    pub artwork_id: Option<Uuid>,
    pub genre: Option<String>,
    pub year: Option<i32>,
    pub track_number: Option<u32>,
    pub disc_number: Option<u32>,
    pub content_hash: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackIdentity {
    pub content_hash: String,
    pub file_size: u64,
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportPreflightTrack {
    pub local_track_id: Option<Uuid>,
    pub quick_hash: Option<String>,
    pub content_hash: Option<String>,
    pub sha256_prefix: Option<String>,
    pub file_size: Option<u64>,
    pub duration_ms: Option<u64>,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportPreflightRequest {
    pub tracks: Vec<ImportPreflightTrack>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportPreflightItem {
    pub local_track_id: Option<Uuid>,
    pub status: String,
    pub remote_track_id: Option<Uuid>,
    #[serde(rename = "match")]
    pub match_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitMapping {
    pub local_track_id: Uuid,
    pub status: String,
    pub remote_track_id: Uuid,
    pub checksum: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct LibraryStats {
    pub tracks: i64,
    pub albums: i64,
    pub artists: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AlbumSummary {
    pub album: String,
    pub album_artist: Option<String>,
    pub track_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ArtistSummary {
    pub artist: Option<String>,
    pub track_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Playlist {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub track_count: i64,
    #[serde(default)]
    pub share_code: Option<String>,
    #[serde(default)]
    pub is_public: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PlaylistCreate {
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PlaylistTrack {
    pub id: Uuid,
    pub playlist_id: Uuid,
    pub track_id: Uuid,
    pub position: i64,
    pub added_at: DateTime<Utc>,
}

pub fn track_id_from_path(path: &str) -> Uuid {
    let normalized = path.replace('\\', "/");
    Uuid::new_v5(&Uuid::NAMESPACE_URL, normalized.as_bytes())
}

pub fn track_id_from_library_path(library_root: &Path, file_path: &Path) -> Uuid {
    if let Ok(relative) = file_path.strip_prefix(library_root) {
        let rel_str = relative.to_string_lossy().replace('\\', "/");
        if !rel_str.is_empty() {
            return Uuid::new_v5(&Uuid::NAMESPACE_URL, rel_str.as_bytes());
        }
    }
    let full = file_path.to_string_lossy().replace('\\', "/");
    Uuid::new_v5(&Uuid::NAMESPACE_URL, full.as_bytes())
}

pub fn is_path_inside_library(library_root: &Path, file_path: &Path) -> Result<bool, PathError> {
    let canonical_root = library_root.canonicalize().map_err(|e| {
        PathError::CannotCanonicalizeRoot(format!("{}: {}", library_root.display(), e))
    })?;
    let canonical_file = file_path.canonicalize().map_err(|e| {
        PathError::CannotCanonicalizeFile(format!("{}: {}", file_path.display(), e))
    })?;
    Ok(canonical_file.starts_with(&canonical_root))
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PlayHistory {
    pub id: Uuid,
    pub track_id: Uuid,
    pub played_at: DateTime<Utc>,
    pub duration_ms: Option<u64>,
    pub scrobbled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, ToSchema)]
pub struct TrackUpdate {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artist: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub album: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub album_artist: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample_rate: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bit_depth: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channels: Option<u8>,
}

// --- Link Device ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkDevice {
    pub device_id: Uuid,
    pub alias: String,
    pub device_type: String,
    pub device_model: Option<String>,
    pub token_hash: String,
    pub permissions_json: String,
    pub created_at: DateTime<Utc>,
    pub last_seen: Option<String>,
    pub revoked: bool,
}

// --- Pairing Session DB ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairingSessionDb {
    pub pairing_id: Uuid,
    pub code: String,
    pub device_name: String,
    pub device_type: String,
    pub expires_at: String,
    pub confirmed: bool,
}

// --- Import Session DB ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportSessionDb {
    pub session_id: Uuid,
    pub device_id: Uuid,
    pub total_tracks: u32,
    pub total_playlists: u32,
    pub imported_tracks: u32,
    pub imported_playlists: u32,
    pub total_size_bytes: u64,
    pub status: String,
    pub expires_at: String,
    pub created_at: String,
}

// --- Receiver DB ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReceiverDb {
    pub id: Uuid,
    pub name: String,
    pub device_type: String,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub capabilities_json: String,
    pub online: bool,
    pub last_seen: Option<String>,
}

// --- Playback Session DB ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaybackSessionDb {
    pub id: Uuid,
    pub device_id: Uuid,
    pub queue_id: Option<Uuid>,
    pub queue_state_json: String,
    pub current_index: i32,
    pub current_track_id: Option<Uuid>,
    pub position_ms: u64,
    pub playing: bool,
    pub repeat_mode: String,
    pub shuffle: bool,
    pub volume: f64,
    pub source: String,
    pub resume_policy: String,
    pub restored: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ImportState {
    Created,
    Uploading,
    Uploaded,
    Verifying,
    Committing,
    Committed,
    Failed,
    RolledBack,
    Expired,
}

impl ImportState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Uploading => "uploading",
            Self::Uploaded => "uploaded",
            Self::Verifying => "verifying",
            Self::Committing => "committing",
            Self::Committed => "committed",
            Self::Failed => "failed",
            Self::RolledBack => "rolled_back",
            Self::Expired => "expired",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "created" => Self::Created,
            "uploading" => Self::Uploading,
            "uploaded" => Self::Uploaded,
            "verifying" => Self::Verifying,
            "committing" => Self::Committing,
            "committed" => Self::Committed,
            "failed" => Self::Failed,
            "rolled_back" => Self::RolledBack,
            "expired" => Self::Expired,
            _ => Self::Created,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audio_format_from_extension() {
        assert_eq!(AudioFormat::from_extension("mp3"), AudioFormat::Mp3);
        assert_eq!(AudioFormat::from_extension("FLAC"), AudioFormat::Flac);
        assert_eq!(AudioFormat::from_extension("Wav"), AudioFormat::Wav);
        assert_eq!(AudioFormat::from_extension("aiff"), AudioFormat::Aiff);
        assert_eq!(AudioFormat::from_extension("aif"), AudioFormat::Aiff);
        assert_eq!(AudioFormat::from_extension("dsf"), AudioFormat::Dsf);
        assert_eq!(AudioFormat::from_extension("dff"), AudioFormat::Dff);
        assert_eq!(AudioFormat::from_extension("ogg"), AudioFormat::Ogg);
        assert_eq!(AudioFormat::from_extension("opus"), AudioFormat::Opus);
        assert_eq!(AudioFormat::from_extension("aac"), AudioFormat::Aac);
        assert_eq!(AudioFormat::from_extension("m4a"), AudioFormat::M4a);
        assert_eq!(AudioFormat::from_extension("txt"), AudioFormat::Unknown);
    }

    #[test]
    fn test_audio_format_as_str() {
        assert_eq!(AudioFormat::Mp3.as_str(), "mp3");
        assert_eq!(AudioFormat::Flac.as_str(), "flac");
        assert_eq!(AudioFormat::Wav.as_str(), "wav");
        assert_eq!(AudioFormat::Aiff.as_str(), "aiff");
        assert_eq!(AudioFormat::Dsf.as_str(), "dsf");
        assert_eq!(AudioFormat::Dff.as_str(), "dff");
        assert_eq!(AudioFormat::Ogg.as_str(), "ogg");
        assert_eq!(AudioFormat::Opus.as_str(), "opus");
        assert_eq!(AudioFormat::Aac.as_str(), "aac");
        assert_eq!(AudioFormat::M4a.as_str(), "m4a");
        assert_eq!(AudioFormat::Unknown.as_str(), "unknown");
    }

    #[test]
    fn test_track_id_from_path() {
        let id1 = track_id_from_path("/music/test.flac");
        let id2 = track_id_from_path("/music/test.flac");
        let id3 = track_id_from_path("/music/other.flac");
        assert_eq!(id1, id2);
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_track_id_path_normalization() {
        let unix = track_id_from_path("/music/test.flac");
        let win = track_id_from_path("\\music\\test.flac");
        assert_eq!(unix, win);
    }

    #[test]
    fn test_audio_metadata_default() {
        let m = AudioMetadata::default();
        assert_eq!(m.format, AudioFormat::Unknown);
        assert!(m.title.is_none());
        assert!(!m.has_artwork);
    }

    #[test]
    fn test_audio_format_mime_type() {
        assert_eq!(AudioFormat::Mp3.mime_type(), "audio/mpeg");
        assert_eq!(AudioFormat::Flac.mime_type(), "audio/flac");
        assert_eq!(AudioFormat::Ogg.mime_type(), "audio/ogg");
        assert_eq!(AudioFormat::Opus.mime_type(), "audio/ogg");
        assert_eq!(AudioFormat::M4a.mime_type(), "audio/mp4");
        assert_eq!(AudioFormat::Aac.mime_type(), "audio/aac");
        assert_eq!(AudioFormat::Wav.mime_type(), "audio/wav");
        assert_eq!(AudioFormat::Aiff.mime_type(), "audio/aiff");
        assert_eq!(AudioFormat::Dsf.mime_type(), "audio/dsf");
        assert_eq!(AudioFormat::Dff.mime_type(), "audio/dff");
        assert_eq!(AudioFormat::Unknown.mime_type(), "application/octet-stream");
    }

    #[test]
    fn test_track_id_from_library_path_relative() {
        let root = Path::new("/music");
        let file = Path::new("/music/Pink Floyd/Time.flac");
        let id1 = track_id_from_library_path(root, file);
        let id2 = track_id_from_library_path(root, file);
        assert_eq!(id1, id2);
    }

    #[test]
    fn test_track_id_from_library_path_same_relative_different_root() {
        let id1 = track_id_from_library_path(
            Path::new("/music"),
            Path::new("/music/Pink Floyd/Time.flac"),
        );
        let id2 = track_id_from_library_path(
            Path::new("/mnt/music"),
            Path::new("/mnt/music/Pink Floyd/Time.flac"),
        );
        assert_eq!(
            id1, id2,
            "same relative path under different roots must produce same ID"
        );
    }

    #[test]
    fn test_track_id_from_library_path_different_files() {
        let root = Path::new("/music");
        let id1 = track_id_from_library_path(root, Path::new("/music/song1.flac"));
        let id2 = track_id_from_library_path(root, Path::new("/music/song2.flac"));
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_track_id_from_library_path_file_outside_root() {
        let root = Path::new("/music");
        let outside = Path::new("/other/file.flac");
        let id = track_id_from_library_path(root, outside);
        assert_ne!(id, Uuid::nil());
    }

    #[test]
    fn test_is_path_inside_library_valid() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        let file_path = sub.join("test.flac");
        std::fs::write(&file_path, b"data").unwrap();
        let result = is_path_inside_library(dir.path(), &file_path);
        assert!(result.unwrap());
    }

    #[test]
    fn test_is_path_inside_library_outside() {
        let dir1 = tempfile::tempdir().unwrap();
        let dir2 = tempfile::tempdir().unwrap();
        let outside = dir2.path().join("secret.txt");
        std::fs::write(&outside, b"data").unwrap();
        let result = is_path_inside_library(dir1.path(), &outside);
        assert!(result.is_ok());
        assert!(!result.unwrap());
    }

    #[test]
    fn test_is_path_inside_library_nonexistent_root() {
        let root = Path::new("/nonexistent_path_xyz");
        let file = Path::new("/some/file.flac");
        let result = is_path_inside_library(root, file);
        assert!(matches!(result, Err(PathError::CannotCanonicalizeRoot(_))));
    }

    #[test]
    fn test_is_path_inside_library_traversal_attempt() {
        let dir = tempfile::tempdir().unwrap();
        let inside = dir.path().join("real_file.flac");
        std::fs::write(&inside, b"data").unwrap();
        let traversal = dir.path().join("../../etc/passwd");
        let result = is_path_inside_library(dir.path(), &traversal);
        assert!(result.is_err() || !result.unwrap());
    }

    #[test]
    fn test_track_update_default() {
        let u = TrackUpdate::default();
        assert!(u.title.is_none());
        assert!(u.artist.is_none());
        assert!(u.album.is_none());
        assert!(u.album_artist.is_none());
        assert!(u.duration_ms.is_none());
    }
}
