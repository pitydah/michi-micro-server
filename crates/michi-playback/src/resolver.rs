use async_trait::async_trait;
use michi_core::Track;
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::error::PlaybackError;

#[async_trait]
pub trait TrackResolver: Send + Sync {
    async fn get_track(&self, id: Uuid) -> Result<Track, PlaybackError>;
}

pub struct SqliteTrackResolver {
    pool: SqlitePool,
}

impl SqliteTrackResolver {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl TrackResolver for SqliteTrackResolver {
    async fn get_track(&self, id: Uuid) -> Result<Track, PlaybackError> {
        michi_db::get_track(&self.pool, &id)
            .await
            .map_err(PlaybackError::Database)?
            .ok_or(PlaybackError::TrackNotFound(id))
    }
}
