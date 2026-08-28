use async_trait::async_trait;
use michi_core::Track;
use sqlx::SqlitePool;
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::error::PlaybackError;

#[async_trait]
pub trait TrackResolver: Send + Sync {
    async fn get_track(&self, id: Uuid) -> Result<Track, PlaybackError>;
}

pub struct SqliteTrackResolver {
    pool: SqlitePool,
    music_paths: Vec<PathBuf>,
}

impl SqliteTrackResolver {
    pub fn new(pool: SqlitePool, music_paths: Vec<PathBuf>) -> Self {
        Self { pool, music_paths }
    }

    pub fn with_pool_only(pool: SqlitePool) -> Self {
        Self {
            pool,
            music_paths: Vec::new(),
        }
    }
}

#[async_trait]
impl TrackResolver for SqliteTrackResolver {
    async fn get_track(&self, id: Uuid) -> Result<Track, PlaybackError> {
        let mut track = michi_db::get_track(&self.pool, &id)
            .await
            .map_err(PlaybackError::Database)?
            .ok_or(PlaybackError::TrackNotFound(id))?;

        let path = Path::new(&track.file_path);
        if !path.exists() {
            return Err(PlaybackError::TrackFileMissing(track.file_path.clone()));
        }

        let canonical = match path.canonicalize() {
            Ok(c) => c,
            Err(e) => {
                return Err(PlaybackError::TrackFileMissing(format!(
                    "path invalid or inaccessible: {e}"
                )))
            }
        };

        if !canonical.is_file() {
            return Err(PlaybackError::InvalidMedia(
                "track path is not a regular file".to_string(),
            ));
        }

        if !self.music_paths.is_empty() {
            let inside = self.music_paths.iter().any(|root| {
                if let Ok(canon_root) = root.canonicalize() {
                    canonical.starts_with(canon_root)
                } else {
                    false
                }
            });
            if !inside {
                return Err(PlaybackError::TrackOutsideLibrary(track.file_path.clone()));
            }
        }

        track.file_path = canonical.to_string_lossy().to_string();
        Ok(track)
    }
}
