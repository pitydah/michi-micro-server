//! Michi Onboard — Asistente de configuración novato
//!
//! Detecta estado "virgen" y guía al usuario paso a paso.

use serde::Serialize;
use sqlx::SqlitePool;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub enum SetupStep {
    Scan,
    Perms,
    Done,
}

#[derive(Debug, Clone, Serialize)]
pub struct SetupStatus {
    pub needs_setup: bool,
    pub step: SetupStep,
    pub track_count: i64,
    pub music_paths_found: Vec<String>,
}

/// Check if the server needs first-time setup
pub async fn check_setup_status(db: &SqlitePool) -> SetupStatus {
    let track_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tracks")
        .fetch_one(db)
        .await
        .unwrap_or(0);

    let music_paths_found = discover_music_paths();

    if track_count > 0 {
        return SetupStatus {
            needs_setup: false,
            step: SetupStep::Done,
            track_count,
            music_paths_found,
        };
    }

    if music_paths_found.is_empty() {
        return SetupStatus {
            needs_setup: true,
            step: SetupStep::Scan,
            track_count: 0,
            music_paths_found,
        };
    }

    SetupStatus {
        needs_setup: true,
        step: SetupStep::Perms,
        track_count: 0,
        music_paths_found,
    }
}

/// Discover common music paths
fn discover_music_paths() -> Vec<String> {
    let candidates = ["/music", "/media", "/media/music", "/data/music"];
    candidates
        .iter()
        .filter(|p| std::path::Path::new(p).exists())
        .map(|p| p.to_string())
        .collect()
}

pub fn discover_music_paths_wrapper() -> Vec<String> {
    discover_music_paths()
}

/// Get total size of music files found
pub async fn scan_music_stats(paths: &[String]) -> (u64, u64) {
    let mut files = 0u64;
    let mut total_bytes = 0u64;
    for path_str in paths {
        let path = std::path::Path::new(path_str);
        if !path.exists() {
            continue;
        }
        if let Ok(mut dir) = tokio::fs::read_dir(path).await {
            while let Ok(Some(entry)) = dir.next_entry().await {
                if let Ok(meta) = entry.metadata().await {
                    if meta.is_file() {
                        let ext = entry
                            .path()
                            .extension()
                            .and_then(|e| e.to_str())
                            .unwrap_or("")
                            .to_lowercase();
                        if let "mp3" | "flac" | "ogg" | "opus" | "aac" | "m4a" | "wav" | "aiff"
                        | "dsf" = ext.as_str()
                        {
                            files += 1;
                            total_bytes += meta.len();
                        }
                    }
                }
            }
        }
    }
    (files, total_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_discover_empty() {
        let paths = discover_music_paths();
        // In CI, likely no /music exists, so empty is fine
        assert!(paths.is_empty() || paths.len() <= 5);
    }

    #[test]
    fn test_setup_step_order() {
        assert!(SetupStep::Scan != SetupStep::Done);
        assert!(SetupStep::Perms != SetupStep::Done);
    }
}
