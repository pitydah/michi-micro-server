use std::path::Path;

use michi_core::{track_id_from_path, AudioFormat, Track};
use michi_metadata::read_metadata_safe;
use tracing::{info, warn};

const SUPPORTED_EXTENSIONS: &[&str] = &[
    "mp3", "flac", "ogg", "opus", "aac", "m4a", "wav", "aiff", "aif", "dsf", "dff",
];

fn is_audio_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| SUPPORTED_EXTENSIONS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}

fn scan_directory_sync(path: &Path) -> Vec<Track> {
    let mut tracks = Vec::new();

    if !path.exists() || !path.is_dir() {
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
            let sub_tracks = scan_directory_sync(&entry_path);
            tracks.extend(sub_tracks);
        } else if entry_path.is_file() && is_audio_file(&entry_path) {
            let metadata = read_metadata_safe(&entry_path);

            let file_path = entry_path.to_string_lossy().to_string();
            let track_id = track_id_from_path(&file_path);

            let track = Track {
                id: track_id,
                title: metadata.title.clone(),
                artist: metadata.artist.clone(),
                album: metadata.album.clone(),
                album_artist: metadata.album_artist.clone(),
                duration_ms: metadata.duration_ms,
                file_path,
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

    tracks
}

pub async fn scan_directory(path: &Path) -> Vec<Track> {
    let path_buf = path.to_path_buf();
    info!("scanning directory: {}", path_buf.display());

    if !path_buf.exists() || !path_buf.is_dir() {
        warn!("directory does not exist: {}", path_buf.display());
        return Vec::new();
    }

    let tracks = tokio::task::spawn_blocking({
        let path_buf = path_buf.clone();
        move || scan_directory_sync(&path_buf)
    })
    .await
    .unwrap_or_default();

    info!(
        "scanned {} tracks from {}",
        tracks.len(),
        path_buf.display()
    );
    tracks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_audio_file() {
        assert!(is_audio_file(Path::new("song.mp3")));
        assert!(is_audio_file(Path::new("song.flac")));
        assert!(is_audio_file(Path::new("song.wav")));
        assert!(is_audio_file(Path::new("song.aiff")));
        assert!(is_audio_file(Path::new("song.aif")));
        assert!(is_audio_file(Path::new("song.dsf")));
        assert!(is_audio_file(Path::new("song.dff")));
        assert!(is_audio_file(Path::new("song.ogg")));
        assert!(is_audio_file(Path::new("song.opus")));
        assert!(is_audio_file(Path::new("song.aac")));
        assert!(is_audio_file(Path::new("song.m4a")));
        assert!(!is_audio_file(Path::new("song.txt")));
        assert!(!is_audio_file(Path::new("song")));
    }
}
