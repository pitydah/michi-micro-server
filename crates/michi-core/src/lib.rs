use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PlaybackState {
    Idle,
    Playing,
    Paused,
    Buffering,
    Stopped,
    Error,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Album {
    pub id: Uuid,
    pub title: String,
    pub artist: Option<String>,
    pub year: Option<i32>,
    pub artwork_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artist {
    pub id: Uuid,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Playlist {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub tracks: Vec<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryStats {
    pub tracks: i64,
    pub albums: i64,
    pub artists: i64,
}

pub fn track_id_from_path(path: &str) -> Uuid {
    let normalized = path.replace('\\', "/");
    Uuid::new_v5(&Uuid::NAMESPACE_URL, normalized.as_bytes())
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
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
    fn test_track_update_default() {
        let u = TrackUpdate::default();
        assert!(u.title.is_none());
        assert!(u.artist.is_none());
        assert!(u.album.is_none());
        assert!(u.album_artist.is_none());
        assert!(u.duration_ms.is_none());
    }
}
