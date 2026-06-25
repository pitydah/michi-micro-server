use std::path::Path;

use michi_core::{track_id_from_library_path, AudioFormat, Track};
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

fn scan_directory_sync(library_root: &Path, path: &Path) -> Vec<Track> {
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

        if entry_path.is_symlink() {
            warn!("skipping symlink: {}", entry_path.display());
            continue;
        }

        if entry_path.is_dir() {
            let sub_tracks = scan_directory_sync(library_root, &entry_path);
            tracks.extend(sub_tracks);
        } else if entry_path.is_file() && is_audio_file(&entry_path) {
            let metadata = read_metadata_safe(&entry_path);

            let file_path = entry_path.to_string_lossy().to_string();
            let track_id = track_id_from_library_path(library_root, &entry_path);

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
        move || scan_directory_sync(&path_buf, &path_buf)
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
    use std::fs;

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

    #[test]
    fn test_scan_directory_skips_unsupported_files() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("song.flac"), b"not a real flac").unwrap();
        fs::write(dir.path().join("readme.txt"), b"hello").unwrap();
        fs::write(dir.path().join("song.mp3"), b"not a real mp3").unwrap();

        let tracks = scan_directory_sync(dir.path(), dir.path());
        assert_eq!(tracks.len(), 2, "should find flac and mp3, skip txt");
    }

    #[test]
    fn test_scan_directory_handles_unreadable_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("corrupt.flac");
        fs::write(&file, b"not a valid audio file").unwrap();

        let tracks = scan_directory_sync(dir.path(), dir.path());
        assert_eq!(tracks.len(), 1, "corrupt file should still be registered");
        assert_eq!(tracks[0].format, AudioFormat::Flac);
        assert!(
            tracks[0].title.is_none(),
            "metadata should be empty for corrupt file"
        );
    }

    #[test]
    fn test_scan_directory_uses_relative_ids() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("artist");
        fs::create_dir_all(&sub).unwrap();
        fs::write(sub.join("song.flac"), b"data").unwrap();

        let tracks = scan_directory_sync(dir.path(), dir.path());
        assert_eq!(tracks.len(), 1);

        let relative_id =
            michi_core::track_id_from_library_path(dir.path(), &sub.join("song.flac"));
        assert_eq!(
            tracks[0].id, relative_id,
            "ID should be based on relative path"
        );
    }

    #[test]
    fn test_scan_directory_skips_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("secret.flac"), b"data").unwrap();

        let symlink_path = dir.path().join("link_to_outside");
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(outside.path().join("secret.flac"), &symlink_path).ok();
        }

        let tracks = scan_directory_sync(dir.path(), dir.path());
        assert_eq!(tracks.len(), 0, "symlinks should be skipped");
    }
}
