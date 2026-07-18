use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use sqlx::SqlitePool;
use tokio::sync::Mutex;
use tracing::info;

pub struct LibraryWatcher {
    paths: Vec<PathBuf>,
    db: SqlitePool,
}

impl LibraryWatcher {
    pub fn new(paths: Vec<PathBuf>, db: SqlitePool) -> Self {
        Self { paths, db }
    }

    pub async fn start(self) {
        let pending: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));
        let db = self.db.clone();
        let paths = self.paths.clone();

        // Scanner task — detects change flag, does full scan, persists to DB
        let p = pending.clone();
        let scan_paths = paths.clone();
        let scan_db = db.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(10));
            interval.tick().await;
            loop {
                interval.tick().await;
                let mut flag = p.lock().await;
                if *flag {
                    *flag = false;
                    info!("library watcher: change detected, scanning");
                    let tracks = crate::scan_directories(&scan_paths).await;
                    // Persist each track
                    for track in &tracks {
                        let _ = michi_db::upsert_track(&scan_db, track).await;
                    }
                    // Detect deletions: tracks in DB but not in fresh scan
                    if let Ok(db_tracks) = michi_db::list_tracks(&scan_db).await {
                        let scanned_ids: std::collections::HashSet<_> =
                            tracks.iter().map(|t| t.id).collect();
                        for old in &db_tracks {
                            if !scanned_ids.contains(&old.id) {
                                let _ = michi_db::delete_track(&scan_db, &old.id).await;
                                info!("library watcher: removed missing track {}", old.id);
                            }
                        }
                    }
                    info!("library watcher: scan complete, {} tracks", tracks.len());
                }
            }
        });

        // Polling task — detect mtime changes at root level
        // This is a simplified approach; for deep changes, the watcher triggers
        // a full scan which re-reads all files and reconciles with DB
        tokio::spawn(async move {
            let mut last_mtimes: Vec<(PathBuf, u64)> = paths
                .iter()
                .map(|p| {
                    let mtime = std::fs::metadata(p)
                        .ok()
                        .and_then(|m| m.modified().ok())
                        .and_then(|t| t.elapsed().ok())
                        .map(|d| d.as_secs())
                        .unwrap_or(0);
                    (p.clone(), mtime)
                })
                .collect();

            loop {
                tokio::time::sleep(Duration::from_secs(5)).await;
                for item in last_mtimes.iter_mut() {
                    if let Ok(meta) = std::fs::metadata(&item.0) {
                        if let Ok(modified) = meta.modified() {
                            if let Ok(elapsed) = modified.elapsed() {
                                let current = elapsed.as_secs();
                                if current < item.1 || current > item.1 + 60 {
                                    item.1 = current;
                                    let mut flag = pending.lock().await;
                                    *flag = true;
                                    info!("library watcher: change detected in {:?}", item.0);
                                }
                            }
                        }
                    }
                }
            }
        });
    }
}
