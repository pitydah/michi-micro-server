use sqlx::SqlitePool;
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Fingerprint {
    modified: Option<SystemTime>,
    len: u64,
}

pub struct LibraryWatcher {
    paths: Vec<PathBuf>,
    db: SqlitePool,
}

impl LibraryWatcher {
    pub fn new(paths: Vec<PathBuf>, db: SqlitePool) -> Self {
        Self { paths, db }
    }

    pub async fn run(
        &self,
        module_cancel: CancellationToken,
        shutdown: CancellationToken,
        poll_interval: Duration,
    ) {
        let mut snapshots: HashMap<PathBuf, Option<HashMap<PathBuf, Fingerprint>>> = HashMap::new();
        let mut interval = tokio::time::interval(poll_interval);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                _ = module_cancel.cancelled() => break,
                _ = shutdown.cancelled() => break,
                _ = interval.tick() => {
                    for root in &self.paths {
                        if module_cancel.is_cancelled() || shutdown.is_cancelled() {
                            break;
                        }
                        self.poll_root(root, &mut snapshots, &module_cancel).await;
                    }
                }
            }
        }
        info!("library watcher stopped");
    }

    async fn poll_root(
        &self,
        root: &Path,
        snapshots: &mut HashMap<PathBuf, Option<HashMap<PathBuf, Fingerprint>>>,
        cancel: &CancellationToken,
    ) {
        let scan_root = root.to_path_buf();
        let result = tokio::task::spawn_blocking(move || snapshot(&scan_root)).await;
        let current = match result {
            Ok(Ok(snapshot)) => snapshot,
            Ok(Err(error)) => {
                warn!(path = %root.display(), %error, "library mount unavailable");
                let _ = michi_db::update_mount_state(
                    &self.db,
                    &root.display().to_string(),
                    "unavailable",
                    &error.to_string(),
                )
                .await;
                snapshots.insert(root.to_path_buf(), None);
                return;
            }
            Err(error) => {
                warn!(path = %root.display(), %error, "library snapshot task failed");
                snapshots.insert(root.to_path_buf(), None);
                return;
            }
        };

        let path_string = root.display().to_string();
        let current_dev_id = crate::get_path_device_id(root);
        let recorded_dev_id = michi_db::get_mount_device_id(&self.db, &path_string)
            .await
            .ok()
            .flatten();

        // 1. Device identity check: if filesystem device ID changed, mount is lost or replaced!
        if let (Some(expected), Some(current_dev)) = (recorded_dev_id, current_dev_id) {
            if expected != current_dev {
                warn!(
                    path = %root.display(),
                    expected_dev = expected,
                    current_dev = current_dev,
                    "watcher detected mount device identity mismatch; mount lost or replaced; preserving DB"
                );
                let _ = michi_db::update_mount_state_with_device(
                    &self.db,
                    &path_string,
                    "unavailable",
                    "mount device identity mismatch: filesystem lost or replaced",
                    None,
                )
                .await;

                snapshots.insert(root.to_path_buf(), None);
                return;
            }
        }

        let previous = snapshots.insert(root.to_path_buf(), Some(current.clone()));
        let Some(previous) = previous else {
            // First poll for this root:
            // If device_id is not yet recorded, watcher MUST NOT establish the first trusted device
            // when DB contains tracks! Delegate entirely to reconcile_root to verify continuity and bootstrap.
            if recorded_dev_id.is_none() {
                let scan_res = crate::scan_root_cancellable(root, cancel.clone()).await;
                if let Err(error) = crate::reconcile_root(&self.db, root, &scan_res, cancel).await {
                    warn!(path = %root.display(), %error, "reconcile_root failed during initial watcher bootstrap");
                    snapshots.insert(root.to_path_buf(), None);
                    return;
                }
                let mount_states = michi_db::get_mount_states(&self.db)
                    .await
                    .unwrap_or_default();
                if mount_states
                    .iter()
                    .any(|(p, s, ..)| p == &path_string && s == "unavailable")
                {
                    snapshots.insert(root.to_path_buf(), None);
                    return;
                }
            } else {
                let _ = michi_db::update_mount_state_with_device(
                    &self.db,
                    &path_string,
                    "online",
                    "",
                    current_dev_id,
                )
                .await;
            }
            return;
        };
        let Some(previous) = previous else {
            info!(path = %root.display(), "library mount restored; reconciling root");
            let scan_res = crate::scan_root_cancellable(root, cancel.clone()).await;
            if let Err(error) = crate::reconcile_root(&self.db, root, &scan_res, cancel).await {
                warn!(path = %root.display(), %error, "failed to reconcile restored mount");
                snapshots.insert(root.to_path_buf(), None);
                return;
            }
            let mount_states = michi_db::get_mount_states(&self.db)
                .await
                .unwrap_or_default();
            if mount_states
                .iter()
                .any(|(p, s, ..)| p == &path_string && s == "unavailable")
            {
                snapshots.insert(root.to_path_buf(), None);
                return;
            }
            return;
        };

        let _ = michi_db::update_mount_state_with_device(
            &self.db,
            &path_string,
            "online",
            "",
            current_dev_id,
        )
        .await;

        // If current is empty but previous was non-empty:
        // Double check device identity before treating as legitimate empty library!
        if current.is_empty() && !previous.is_empty() && current_dev_id.is_none() {
            warn!(
                path = %root.display(),
                "watcher skipped deletion: unable to confirm device identity on empty snapshot"
            );
            return;
        }

        for path in current
            .iter()
            .filter(|(path, fingerprint)| previous.get(*path) != Some(*fingerprint))
            .map(|(path, _)| path)
        {
            if cancel.is_cancelled() {
                return;
            }
            if let Some(mut track) = crate::scan_file(root.to_path_buf(), path.clone()).await {
                // If the file was modified, compute its new content hash in a blocking task
                if previous.contains_key(path) {
                    let path_for_hash = path.clone();
                    if let Ok(Some(new_hash)) = tokio::task::spawn_blocking(move || {
                        crate::compute_file_content_hash(&path_for_hash)
                    })
                    .await
                    {
                        track.content_hash = Some(new_hash);
                    }
                }
                if let Err(error) = michi_db::upsert_track(&self.db, &track).await {
                    warn!(path = %path.display(), %error, "failed to persist changed track");
                }
            }
        }

        for path in previous.keys().filter(|path| !current.contains_key(*path)) {
            if cancel.is_cancelled() {
                return;
            }
            // Double check that the individual file actually disappeared on disk before deleting from DB
            if path.exists() {
                continue;
            }
            match michi_db::find_track_by_path(&self.db, &path.display().to_string()).await {
                Ok(Some(track)) => {
                    if let Err(error) = michi_db::delete_track(&self.db, &track.id).await {
                        warn!(path = %path.display(), %error, "failed to remove deleted track");
                    }
                }
                Ok(None) => {}
                Err(error) => {
                    warn!(path = %path.display(), %error, "failed to look up deleted track")
                }
            }
        }
    }
}

fn snapshot(root: &Path) -> io::Result<HashMap<PathBuf, Fingerprint>> {
    let metadata = std::fs::metadata(root)?;
    if !metadata.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotADirectory,
            "music root is not a directory",
        ));
    }
    let mut files = HashMap::new();
    snapshot_directory(root, &mut files)?;
    Ok(files)
}

fn snapshot_directory(
    directory: &Path,
    files: &mut HashMap<PathBuf, Fingerprint>,
) -> io::Result<()> {
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let file_name = entry.file_name();
        let name_str = file_name.to_string_lossy();
        if name_str.starts_with('.') {
            continue;
        }
        let metadata = std::fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            snapshot_directory(&path, files)?;
        } else if metadata.is_file() && crate::is_audio_file(&path) {
            files.insert(
                path,
                Fingerprint {
                    modified: metadata.modified().ok(),
                    len: metadata.len(),
                },
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_pool(directory: &tempfile::TempDir) -> SqlitePool {
        let url = format!("sqlite://{}", directory.path().join("test.db").display());
        michi_db::init_pool(&url).await.unwrap()
    }

    #[tokio::test]
    async fn unavailable_mount_preserves_tracks_until_successful_reconnect() {
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("music");
        std::fs::create_dir(&root).unwrap();
        let file = root.join("song.mp3");
        std::fs::write(&file, b"not real audio").unwrap();

        let db_directory = tempfile::tempdir().unwrap();
        let db = test_pool(&db_directory).await;
        let track = crate::scan_file(root.clone(), file).await.unwrap();
        michi_db::upsert_track(&db, &track).await.unwrap();
        let watcher = LibraryWatcher::new(vec![root.clone()], db.clone());
        let cancel = CancellationToken::new();
        let mut snapshots = HashMap::new();
        watcher.poll_root(&root, &mut snapshots, &cancel).await;

        // 1. Mount detached (directory renamed/missing) -> tracks preserved, mount unavailable
        let detached = parent.path().join("detached");
        std::fs::rename(&root, &detached).unwrap();
        watcher.poll_root(&root, &mut snapshots, &cancel).await;
        assert!(michi_db::get_track(&db, &track.id).await.unwrap().is_some());
        assert_eq!(
            michi_db::get_mount_states(&db).await.unwrap()[0].1,
            "unavailable"
        );

        // 2. Underlying mountpoint directory is recreated empty (mount lost) -> tracks MUST BE PRESERVED
        std::fs::create_dir(&root).unwrap();
        watcher.poll_root(&root, &mut snapshots, &cancel).await;
        assert!(
            michi_db::get_track(&db, &track.id).await.unwrap().is_some(),
            "empty mountpoint directory must NOT delete existing tracks"
        );

        // 3. Mount restored with new active file (e.g. song2.mp3) -> online, song.mp3 reconciled
        let song2 = root.join("song2.mp3");
        std::fs::write(&song2, b"new audio").unwrap();
        // Reset snapshots to simulate restored mount
        snapshots.insert(root.clone(), None);
        watcher.poll_root(&root, &mut snapshots, &cancel).await;
        assert_eq!(
            michi_db::get_mount_states(&db).await.unwrap()[0].1,
            "online"
        );
        // song.mp3 is now legitimately removed because the mount is online with song2.mp3
        assert!(michi_db::get_track(&db, &track.id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_watcher_first_poll_with_historical_db_and_unverified_device_fails_closed() {
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("music");
        std::fs::create_dir(&root).unwrap();

        // 1. DB contains historical track from before upgrade, with device_id = NULL
        let db_directory = tempfile::tempdir().unwrap();
        let db = test_pool(&db_directory).await;
        let track_id = uuid::Uuid::new_v4();
        let historical_track = michi_core::Track {
            id: track_id,
            title: Some("Historical Track".into()),
            artist: None,
            album: None,
            album_artist: None,
            duration_ms: None,
            file_path: root.join("historical.flac").to_string_lossy().to_string(),
            format: michi_core::AudioFormat::Flac,
            sample_rate: None,
            bit_depth: None,
            channels: None,
            artwork_id: None,
            genre: None,
            year: None,
            track_number: None,
            disc_number: None,
            content_hash: Some("hash456".into()),
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
        michi_db::upsert_track(&db, &historical_track)
            .await
            .unwrap();

        let path_string = root.display().to_string();
        assert_eq!(
            michi_db::get_mount_device_id(&db, &path_string)
                .await
                .unwrap(),
            None
        );

        // 2. Filesystem on disk is disconnected/incorrect (e.g. empty or containing an unmatching file)
        let dummy_file = root.join("unrelated.mp3");
        std::fs::write(&dummy_file, b"unrelated track").unwrap();

        // 3. First poll of watcher
        let watcher = LibraryWatcher::new(vec![root.clone()], db.clone());
        let cancel = CancellationToken::new();
        let mut snapshots = HashMap::new();
        watcher.poll_root(&root, &mut snapshots, &cancel).await;

        // 4. Verify historical track was NOT destroyed
        assert!(
            michi_db::get_track(&db, &track_id).await.unwrap().is_some(),
            "Historical track MUST be preserved on first watcher poll when device is unverified"
        );

        // Verify trusted device was NOT established to the incorrect device
        let stored_dev = michi_db::get_mount_device_id(&db, &path_string)
            .await
            .unwrap();
        assert_eq!(
            stored_dev, None,
            "Watcher must NOT establish trusted device on unverified filesystem"
        );

        // Verify mount state is marked unavailable
        let states = michi_db::get_mount_states(&db).await.unwrap();
        let state = states.iter().find(|(p, ..)| p == &path_string).unwrap();
        assert_eq!(state.1, "unavailable");
        assert!(state.4.contains("bootstrap pending"));
    }
}
