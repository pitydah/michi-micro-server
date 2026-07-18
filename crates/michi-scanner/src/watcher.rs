use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tracing::info;

pub struct LibraryWatcher {
    paths: Vec<PathBuf>,
}

impl LibraryWatcher {
    pub fn new(paths: Vec<PathBuf>) -> Self {
        Self { paths }
    }

    pub async fn start(self) {
        let pending: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));
        let paths = self.paths.clone();

        // Scanner task
        let p = pending.clone();
        let scan_paths = paths.clone();
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
                    info!("library watcher: scan complete, {} tracks", tracks.len());
                }
            }
        });

        // Polling task
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
                                    info!("library watcher: change in {:?}", item.0);
                                }
                            }
                        }
                    }
                }
            }
        });
    }
}
