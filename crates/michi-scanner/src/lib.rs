use std::path::{Path, PathBuf};
use std::sync::Arc;

use michi_core::{track_id_from_library_path, AudioFormat, Track};
use michi_metadata::read_metadata_safe;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

pub mod watcher;

const SUPPORTED_EXTENSIONS: &[&str] = &["mp3", "flac", "ogg", "opus", "aac", "m4a", "wav"];

fn is_audio_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| SUPPORTED_EXTENSIONS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}

pub fn scan_single_file(library_root: &Path, entry_path: &Path) -> Option<Track> {
    track_from_file(library_root, entry_path)
}

fn track_from_file(library_root: &Path, entry_path: &Path) -> Option<Track> {
    if !entry_path.is_file() || !is_audio_file(entry_path) {
        return None;
    }
    let metadata = read_metadata_safe(entry_path);
    let file_path = entry_path.to_string_lossy().to_string();
    let track_id = track_id_from_library_path(library_root, entry_path);
    Some(Track {
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
        artwork_id: metadata.has_artwork.then_some(track_id),
        genre: metadata.genre.clone(),
        year: metadata.year,
        track_number: metadata.track_number,
        disc_number: metadata.disc_number,
        content_hash: None,
        starred: false,
        rating: 0,
        starred_at: None,
        replaygain_track_gain: None,
        replaygain_track_peak: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    })
}

#[derive(Debug, Clone)]
pub enum ScanResult {
    Success(Vec<Track>),
    Unavailable(String),
    Cancelled,
    Partial { tracks: Vec<Track>, error: String },
}

impl ScanResult {
    pub fn tracks(&self) -> &[Track] {
        match self {
            ScanResult::Success(tracks) | ScanResult::Partial { tracks, .. } => tracks,
            ScanResult::Unavailable(_) | ScanResult::Cancelled => &[],
        }
    }
}

pub fn compute_file_content_hash(path: &Path) -> Option<String> {
    use sha2::{Digest, Sha256};
    let mut file = std::fs::File::open(path).ok()?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    use std::io::Read;
    loop {
        let count = file.read(&mut buffer).ok()?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Some(hex::encode(hasher.finalize()))
}

fn scan_directory_sync_strict(
    library_root: &Path,
    path: &Path,
    cancel: &CancellationToken,
) -> Result<Vec<Track>, String> {
    if !path.exists() {
        return Err(format!("path does not exist: {}", path.display()));
    }
    if !path.is_dir() {
        return Err(format!("path is not a directory: {}", path.display()));
    }

    let entries = path
        .read_dir()
        .map_err(|e| format!("failed to read directory {}: {e}", path.display()))?;

    let mut tracks = Vec::new();
    for entry in entries {
        if cancel.is_cancelled() {
            break;
        }
        let entry = entry.map_err(|e| format!("failed to read entry: {e}"))?;
        let entry_path = entry.path();

        if entry_path.is_symlink() {
            warn!("skipping symlink: {}", entry_path.display());
            continue;
        }

        if entry_path.is_dir() {
            if entry_path
                .file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.starts_with('.'))
                .unwrap_or(false)
            {
                continue;
            }
            let sub_tracks = scan_directory_sync_strict(library_root, &entry_path, cancel)?;
            tracks.extend(sub_tracks);
        } else if let Some(track) = track_from_file(library_root, &entry_path) {
            if track.format != AudioFormat::Unknown {
                info!(
                    "found track: {} ({:?})",
                    track.title.as_deref().unwrap_or("unknown"),
                    track.format
                );
            }
            tracks.push(track);
        }
    }

    Ok(tracks)
}

pub async fn scan_root_cancellable(root: &Path, cancel: CancellationToken) -> ScanResult {
    if !root.exists() {
        return ScanResult::Unavailable(format!("root does not exist: {}", root.display()));
    }
    if !root.is_dir() {
        return ScanResult::Unavailable(format!("root is not a directory: {}", root.display()));
    }

    let root_buf = root.to_path_buf();
    let root_for_closure = root_buf.clone();
    let scan_cancel = cancel.clone();

    let res = tokio::task::spawn_blocking(move || {
        scan_directory_sync_strict(&root_for_closure, &root_for_closure, &scan_cancel)
    })
    .await;

    match res {
        Ok(Ok(mut tracks)) => {
            if cancel.is_cancelled() {
                ScanResult::Cancelled
            } else {
                let mut seen = std::collections::HashSet::new();
                tracks.retain(|t| seen.insert(t.id));
                ScanResult::Success(tracks)
            }
        }
        Ok(Err(e)) => ScanResult::Unavailable(e),
        Err(join_err) => ScanResult::Unavailable(join_err.to_string()),
    }
}

pub async fn scan_directories(paths: &[PathBuf]) -> Vec<Track> {
    scan_directories_cancellable(paths, 2, CancellationToken::new()).await
}

pub async fn scan_directories_with_concurrency(
    paths: &[PathBuf],
    concurrency: usize,
) -> Vec<Track> {
    scan_directories_cancellable(paths, concurrency, CancellationToken::new()).await
}

pub async fn scan_directories_cancellable(
    paths: &[PathBuf],
    concurrency: usize,
    cancel: CancellationToken,
) -> Vec<Track> {
    let mut all_tracks: Vec<Track> = Vec::new();
    let sem = Arc::new(Semaphore::new(concurrency));

    let mut handles = Vec::new();
    for path in paths {
        if cancel.is_cancelled() {
            break;
        }
        let path_buf = path.clone();
        let sem_clone = sem.clone();
        let scan_cancel = cancel.clone();

        let handle = tokio::spawn(async move {
            let _permit = sem_clone.acquire().await.expect("semaphore");
            scan_root_cancellable(&path_buf, scan_cancel).await
        });
        handles.push(handle);
    }

    for handle in handles {
        match handle.await {
            Ok(ScanResult::Success(tracks) | ScanResult::Partial { tracks, .. }) => {
                all_tracks.extend(tracks);
            }
            Ok(ScanResult::Unavailable(e)) => {
                warn!("scan directory unavailable: {}", e);
            }
            Ok(ScanResult::Cancelled) => {
                info!("scan cancelled");
            }
            Err(e) => {
                warn!("scan task failed: {}", e);
            }
        }
    }

    // Deduplicate by track ID (UUID v5 of relative path is unique per path)
    let mut seen = std::collections::HashSet::new();
    all_tracks.retain(|t| seen.insert(t.id));

    all_tracks
}

pub async fn scan_file(library_root: PathBuf, path: PathBuf) -> Option<Track> {
    tokio::task::spawn_blocking(move || track_from_file(&library_root, &path))
        .await
        .ok()
        .flatten()
}

pub async fn reconcile_root(
    db: &sqlx::SqlitePool,
    root: &Path,
    scan_result: &ScanResult,
    cancel: &CancellationToken,
) -> Result<(), michi_db::DbError> {
    if cancel.is_cancelled() {
        return Ok(());
    }

    let tracks = match scan_result {
        ScanResult::Success(tracks) => tracks,
        ScanResult::Unavailable(err) => {
            warn!(
                path = %root.display(),
                error = %err,
                "reconcile_root skipped: library mount is unavailable. Existing tracks preserved."
            );
            let _ =
                michi_db::update_mount_state(db, &root.display().to_string(), "unavailable", err)
                    .await;
            return Ok(());
        }
        ScanResult::Cancelled => {
            info!(path = %root.display(), "reconcile_root aborted due to cancellation");
            return Ok(());
        }
        ScanResult::Partial { tracks, error } => {
            warn!(
                path = %root.display(),
                error = %error,
                "reconcile_root skipped deletion: scan was only partial. Upserting found tracks without deleting missing ones."
            );
            if !tracks.is_empty() {
                michi_db::upsert_tracks(db, tracks).await?;
            }
            return Ok(());
        }
    };

    // Double-check that root directory is actually present on disk before deleting any tracks
    if !root.exists() || !root.is_dir() {
        warn!(
            path = %root.display(),
            "reconcile_root skipped deletion: root directory disappeared before reconciliation."
        );
        let _ = michi_db::update_mount_state(
            db,
            &root.display().to_string(),
            "unavailable",
            "mount disappeared",
        )
        .await;
        return Ok(());
    }

    let _ = michi_db::update_mount_state(db, &root.display().to_string(), "online", "").await;

    if !tracks.is_empty() {
        michi_db::upsert_tracks(db, tracks).await?;
    }
    if cancel.is_cancelled() {
        return Ok(());
    }

    let scanned: std::collections::HashSet<_> = tracks.iter().map(|track| track.id).collect();
    for existing in michi_db::list_tracks(db).await? {
        if Path::new(&existing.file_path).starts_with(root) && !scanned.contains(&existing.id) {
            // Confirm the individual file does not exist on disk before deletion
            if !Path::new(&existing.file_path).exists() {
                michi_db::delete_track(db, &existing.id).await?;
            }
        }
    }
    Ok(())
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
        assert!(!is_audio_file(Path::new("song.aiff")));
        assert!(!is_audio_file(Path::new("song.aif")));
        assert!(!is_audio_file(Path::new("song.dsf")));
        assert!(!is_audio_file(Path::new("song.dff")));
        assert!(is_audio_file(Path::new("song.mp3")));
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

        let tracks =
            scan_directory_sync_strict(dir.path(), dir.path(), &CancellationToken::new()).unwrap();
        assert_eq!(tracks.len(), 2, "should find flac and mp3, skip txt");
    }

    #[test]
    fn test_scan_directory_handles_unreadable_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("corrupt.flac");
        fs::write(&file, b"not a valid audio file").unwrap();

        let tracks =
            scan_directory_sync_strict(dir.path(), dir.path(), &CancellationToken::new()).unwrap();
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

        let tracks =
            scan_directory_sync_strict(dir.path(), dir.path(), &CancellationToken::new()).unwrap();
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

        let tracks =
            scan_directory_sync_strict(dir.path(), dir.path(), &CancellationToken::new()).unwrap();
        assert_eq!(tracks.len(), 0, "symlinks should be skipped");
    }

    #[tokio::test]
    async fn test_reconcile_root_preserves_tracks_when_mount_unavailable() {
        let pool = michi_db::init_pool("sqlite::memory:").await.unwrap();
        let fake_root = Path::new("/mnt/nas/music");
        let track_id = uuid::Uuid::new_v4();
        let track = Track {
            id: track_id,
            title: Some("Preserved Track".into()),
            artist: Some("Artist".into()),
            album: None,
            album_artist: None,
            duration_ms: Some(180_000),
            file_path: "/mnt/nas/music/song.flac".into(),
            format: AudioFormat::Flac,
            sample_rate: Some(44100),
            bit_depth: Some(16),
            channels: Some(2),
            artwork_id: None,
            genre: None,
            year: None,
            track_number: None,
            disc_number: None,
            content_hash: Some("sha256_hash_123".into()),
            starred: false,
            rating: 0,
            starred_at: None,
            replaygain_track_gain: None,
            replaygain_track_peak: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        michi_db::upsert_track(&pool, &track).await.unwrap();

        // 1. Mount becomes unavailable (returns ScanResult::Unavailable)
        let unavailable_scan = ScanResult::Unavailable("NFS stale file handle".into());
        reconcile_root(
            &pool,
            fake_root,
            &unavailable_scan,
            &CancellationToken::new(),
        )
        .await
        .unwrap();

        // Verify track is NOT deleted
        let in_db = michi_db::get_track(&pool, &track_id).await.unwrap();
        assert!(
            in_db.is_some(),
            "track MUST be preserved when mount is unavailable"
        );
        assert_eq!(
            in_db.unwrap().content_hash.as_deref(),
            Some("sha256_hash_123"),
            "content_hash MUST be preserved"
        );

        // 2. Normal metadata scan with content_hash = None (COALESCE preservation)
        let mut refreshed_track = track.clone();
        refreshed_track.content_hash = None;
        refreshed_track.title = Some("Updated Title".into());
        michi_db::upsert_track(&pool, &refreshed_track)
            .await
            .unwrap();

        let updated_in_db = michi_db::get_track(&pool, &track_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated_in_db.title.as_deref(), Some("Updated Title"));
        assert_eq!(
            updated_in_db.content_hash.as_deref(),
            Some("sha256_hash_123"),
            "COALESCE must preserve existing content_hash during metadata refreshes"
        );
    }
}
