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
        self,
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
        let _ = michi_db::update_mount_state(&self.db, &path_string, "online", "").await;
        let previous = snapshots.insert(root.to_path_buf(), Some(current.clone()));
        let Some(previous) = previous else {
            return;
        };
        let Some(previous) = previous else {
            info!(path = %root.display(), "library mount restored; reconciling root");
            let tracks =
                crate::scan_directories_cancellable(&[root.to_path_buf()], 1, cancel.clone()).await;
            if let Err(error) = crate::reconcile_root(&self.db, root, &tracks, cancel).await {
                warn!(path = %root.display(), %error, "failed to reconcile restored mount");
            }
            return;
        };

        for path in current
            .iter()
            .filter(|(path, fingerprint)| previous.get(*path) != Some(*fingerprint))
            .map(|(path, _)| path)
        {
            if cancel.is_cancelled() {
                return;
            }
            if let Some(track) = crate::scan_file(root.to_path_buf(), path.clone()).await {
                if let Err(error) = michi_db::upsert_track(&self.db, &track).await {
                    warn!(path = %path.display(), %error, "failed to persist changed track");
                }
            }
        }

        for path in previous.keys().filter(|path| !current.contains_key(*path)) {
            if cancel.is_cancelled() {
                return;
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

        let detached = parent.path().join("detached");
        std::fs::rename(&root, &detached).unwrap();
        watcher.poll_root(&root, &mut snapshots, &cancel).await;
        assert!(michi_db::get_track(&db, &track.id).await.unwrap().is_some());
        assert_eq!(
            michi_db::get_mount_states(&db).await.unwrap()[0].1,
            "unavailable"
        );

        std::fs::create_dir(&root).unwrap();
        watcher.poll_root(&root, &mut snapshots, &cancel).await;
        assert!(michi_db::get_track(&db, &track.id).await.unwrap().is_none());
        assert_eq!(
            michi_db::get_mount_states(&db).await.unwrap()[0].1,
            "online"
        );
    }
}
