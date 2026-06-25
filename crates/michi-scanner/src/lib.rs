use std::path::Path;

use michi_core::{AudioFormat, Track};
use michi_metadata::read_metadata_safe;
use tracing::{info, warn};
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("database error: {0}")]
    Db(#[from] michi_db::DbError),
}

const SUPPORTED_EXTENSIONS: &[&str] = &[
    "mp3", "flac", "ogg", "opus", "aac", "m4a", "wav", "aiff", "aif", "dsf", "dff",
];

fn is_audio_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| SUPPORTED_EXTENSIONS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}

pub fn scan_directory(path: &Path) -> Vec<Track> {
    info!("scanning directory: {}", path.display());

    let mut tracks = Vec::new();

    if !path.exists() || !path.is_dir() {
        warn!("directory does not exist: {}", path.display());
        return tracks;
    }

    let entries = match path.read_dir() {
        Ok(entries) => entries,
        Err(e) => {
            warn!("failed to read directory {}: {}", path.display(), e);
            return tracks;
        }
    };

    for entry in entries.flatten() {
        let entry_path = entry.path();

        if entry_path.is_dir() {
            let sub_tracks = scan_directory(&entry_path);
            tracks.extend(sub_tracks);
        } else if entry_path.is_file() && is_audio_file(&entry_path) {
            let metadata = read_metadata_safe(&entry_path);

            let track = Track {
                id: Uuid::new_v4(),
                title: metadata.title.clone(),
                artist: metadata.artist.clone(),
                album: metadata.album.clone(),
                album_artist: metadata.album_artist.clone(),
                duration_ms: metadata.duration_ms,
                file_path: entry_path.to_string_lossy().to_string(),
                format: metadata.format,
                sample_rate: metadata.sample_rate,
                bit_depth: metadata.bit_depth,
                channels: metadata.channels,
                artwork_id: None,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            };

            if metadata.format != AudioFormat::Unknown {
                info!(
                    "found track: {} ({:?})",
                    track.title.as_deref().unwrap_or("unknown"),
                    metadata.format
                );
            }

            tracks.push(track);
        }
    }

    info!("scanned {} tracks from {}", tracks.len(), path.display());
    tracks
}
