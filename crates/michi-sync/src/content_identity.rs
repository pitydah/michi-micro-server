use crate::playback_transfer::ContentRefV1;
use michi_core::Track;
use sqlx::SqlitePool;
use uuid::Uuid;

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum ContentResolutionError {
    #[error("Content not found: '{title}' by '{artist}'")]
    NotFound { title: String, artist: String },
    #[error("Ambiguous content match for '{title}': found {count} candidates")]
    AmbiguousMatch { title: String, count: usize },
    #[error("Database error: {0}")]
    Database(String),
}

pub struct ContentIdentityResolver {
    pool: SqlitePool,
}

impl ContentIdentityResolver {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Resolve a ContentRefV1 to a concrete database Track.
    pub async fn resolve(&self, content: &ContentRefV1) -> Result<Track, ContentResolutionError> {
        // 1. Direct UUID resolution if provided
        if let Some(ref tid_str) = content.track_id {
            if let Ok(uid) = Uuid::parse_str(tid_str) {
                if let Ok(Some(track)) = self.get_track_by_uuid(&uid).await {
                    return Ok(track);
                }
            }
        }

        // 2. Whole-file content_sha256 match from synced_files
        if let Some(ref sha) = content.content_sha256 {
            if !sha.trim().is_empty() {
                let matched = self.find_by_sha256(sha).await?;
                if matched.len() == 1 {
                    return Ok(matched.into_iter().next().unwrap());
                } else if matched.len() > 1 {
                    return Err(ContentResolutionError::AmbiguousMatch {
                        title: content.title.clone(),
                        count: matched.len(),
                    });
                }
            }
        }

        // 3. Strict Title + Artist + Duration within ±3000ms
        let candidates = self
            .find_by_metadata(&content.title, &content.artist, content.duration_ms)
            .await?;

        match candidates.len() {
            1 => Ok(candidates.into_iter().next().unwrap()),
            0 => Err(ContentResolutionError::NotFound {
                title: content.title.clone(),
                artist: content.artist.clone(),
            }),
            count => {
                // Disambiguate by album if provided
                if let Some(ref target_album) = content.album {
                    let album_filtered: Vec<Track> = candidates
                        .into_iter()
                        .filter(|t| {
                            t.album
                                .as_ref()
                                .map(|a| a.eq_ignore_ascii_case(target_album))
                                .unwrap_or(false)
                        })
                        .collect();
                    if album_filtered.len() == 1 {
                        return Ok(album_filtered.into_iter().next().unwrap());
                    }
                }
                Err(ContentResolutionError::AmbiguousMatch {
                    title: content.title.clone(),
                    count,
                })
            }
        }
    }

    async fn get_track_by_uuid(&self, id: &Uuid) -> Result<Option<Track>, ContentResolutionError> {
        let row = sqlx::query_as::<_, TrackDbRow>(
            "SELECT id, title, artist, album, album_artist, duration_ms, file_path, format, sample_rate, bit_depth, channels, created_at, updated_at
             FROM tracks WHERE id = ?"
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| ContentResolutionError::Database(e.to_string()))?;

        Ok(row.map(Into::into))
    }

    async fn find_by_sha256(&self, sha: &str) -> Result<Vec<Track>, ContentResolutionError> {
        let rows = sqlx::query_as::<_, TrackDbRow>(
            "SELECT t.id, t.title, t.artist, t.album, t.album_artist, t.duration_ms, t.file_path, t.format, t.sample_rate, t.bit_depth, t.channels, t.created_at, t.updated_at
             FROM tracks t
             JOIN synced_files s ON s.server_path = t.file_path
             WHERE s.file_hash = ?"
        )
        .bind(sha)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| ContentResolutionError::Database(e.to_string()))?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn find_by_metadata(
        &self,
        title: &str,
        artist: &str,
        duration_ms: u64,
    ) -> Result<Vec<Track>, ContentResolutionError> {
        let min_dur = duration_ms.saturating_sub(3000) as i64;
        let max_dur = (duration_ms + 3000) as i64;

        let rows = sqlx::query_as::<_, TrackDbRow>(
            "SELECT id, title, artist, album, album_artist, duration_ms, file_path, format, sample_rate, bit_depth, channels, created_at, updated_at
             FROM tracks
             WHERE LOWER(TRIM(title)) = LOWER(TRIM(?))
               AND LOWER(TRIM(artist)) = LOWER(TRIM(?))
               AND duration_ms >= ?
               AND duration_ms <= ?"
        )
        .bind(title)
        .bind(artist)
        .bind(min_dur)
        .bind(max_dur)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| ContentResolutionError::Database(e.to_string()))?;

        Ok(rows.into_iter().map(Into::into).collect())
    }
}

#[derive(sqlx::FromRow)]
struct TrackDbRow {
    id: String,
    title: Option<String>,
    artist: Option<String>,
    album: Option<String>,
    album_artist: Option<String>,
    duration_ms: Option<i64>,
    file_path: String,
    format: String,
    sample_rate: Option<i64>,
    bit_depth: Option<i64>,
    channels: Option<i64>,
    created_at: String,
    updated_at: String,
}

impl From<TrackDbRow> for Track {
    fn from(r: TrackDbRow) -> Self {
        let uid = Uuid::parse_str(&r.id).unwrap_or_else(|_| Uuid::new_v4());
        let fmt = match r.format.to_lowercase().as_str() {
            "flac" => michi_core::AudioFormat::Flac,
            "mp3" => michi_core::AudioFormat::Mp3,
            "ogg" => michi_core::AudioFormat::Ogg,
            "opus" => michi_core::AudioFormat::Opus,
            "aac" => michi_core::AudioFormat::Aac,
            "m4a" => michi_core::AudioFormat::M4a,
            "wav" => michi_core::AudioFormat::Wav,
            _ => michi_core::AudioFormat::Unknown,
        };

        Track {
            id: uid,
            title: r.title,
            artist: r.artist,
            album: r.album,
            album_artist: r.album_artist,
            duration_ms: r.duration_ms.map(|d| d as u64),
            file_path: r.file_path,
            format: fmt,
            sample_rate: r.sample_rate.map(|s| s as u32),
            bit_depth: r.bit_depth.map(|b| b as u8),
            channels: r.channels.map(|c| c as u8),
            artwork_id: None,
            genre: None,
            year: None,
            track_number: None,
            disc_number: None,
            content_hash: None,
            file_size: None,
            file_mtime_ns: None,
            starred: false,
            rating: 0,
            starred_at: None,
            replaygain_track_gain: None,
            replaygain_track_peak: None,
            created_at: chrono::DateTime::parse_from_rfc3339(&r.created_at)
                .map(|d| d.with_timezone(&chrono::Utc))
                .unwrap_or_else(|_| chrono::Utc::now()),
            updated_at: chrono::DateTime::parse_from_rfc3339(&r.updated_at)
                .map(|d| d.with_timezone(&chrono::Utc))
                .unwrap_or_else(|_| chrono::Utc::now()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn create_test_tracks_db() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .connect("sqlite::memory:")
            .await
            .unwrap();

        sqlx::query(
            "CREATE TABLE tracks (
                id TEXT PRIMARY KEY,
                title TEXT,
                artist TEXT,
                album TEXT,
                album_artist TEXT,
                duration_ms INTEGER,
                file_path TEXT NOT NULL,
                format TEXT NOT NULL,
                sample_rate INTEGER,
                bit_depth INTEGER,
                channels INTEGER,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE synced_files (
                id TEXT PRIMARY KEY,
                filename TEXT NOT NULL,
                original_path TEXT NOT NULL,
                server_path TEXT NOT NULL,
                file_hash TEXT NOT NULL,
                file_size INTEGER NOT NULL,
                uploaded_at TEXT NOT NULL,
                uploaded_by TEXT NOT NULL,
                checksum_verified INTEGER NOT NULL DEFAULT 1
            );",
        )
        .execute(&pool)
        .await
        .unwrap();

        pool
    }

    #[tokio::test]
    async fn test_resolve_by_uuid() {
        let pool = create_test_tracks_db().await;
        let track_id = Uuid::new_v4();
        let now = chrono::Utc::now().to_rfc3339();

        sqlx::query(
            "INSERT INTO tracks (id, title, artist, album, duration_ms, file_path, format, created_at, updated_at)
             VALUES (?, 'Song A', 'Artist B', 'Album C', 200000, '/music/song_a.flac', 'flac', ?, ?)"
        )
        .bind(track_id.to_string())
        .bind(&now)
        .bind(&now)
        .execute(&pool)
        .await
        .unwrap();

        let resolver = ContentIdentityResolver::new(pool);
        let cref = ContentRefV1 {
            track_id: Some(track_id.to_string()),
            title: "Different Title".into(),
            artist: "Different Artist".into(),
            album: None,
            duration_ms: 1000,
            content_sha256: None,
        };

        let resolved = resolver.resolve(&cref).await.unwrap();
        assert_eq!(resolved.id, track_id);
    }

    #[tokio::test]
    async fn test_resolve_by_metadata_within_tolerance() {
        let pool = create_test_tracks_db().await;
        let track_id = Uuid::new_v4();
        let now = chrono::Utc::now().to_rfc3339();

        sqlx::query(
            "INSERT INTO tracks (id, title, artist, album, duration_ms, file_path, format, created_at, updated_at)
             VALUES (?, 'Midnight Sun', 'Elena', 'Nordic Tales', 241500, '/music/sun.flac', 'flac', ?, ?)"
        )
        .bind(track_id.to_string())
        .bind(&now)
        .bind(&now)
        .execute(&pool)
        .await
        .unwrap();

        let resolver = ContentIdentityResolver::new(pool);
        // Duration differs by 2000ms, which is within the ±3000ms window
        let cref = ContentRefV1 {
            track_id: None,
            title: "midnight sun".into(),
            artist: "ELENA".into(),
            album: None,
            duration_ms: 243500,
            content_sha256: None,
        };

        let resolved = resolver.resolve(&cref).await.unwrap();
        assert_eq!(resolved.id, track_id);

        // Outside window: 4000ms difference -> NotFound
        let cref_outside = ContentRefV1 {
            track_id: None,
            title: "midnight sun".into(),
            artist: "ELENA".into(),
            album: None,
            duration_ms: 246000,
            content_sha256: None,
        };
        assert!(matches!(
            resolver.resolve(&cref_outside).await,
            Err(ContentResolutionError::NotFound { .. })
        ));
    }

    #[tokio::test]
    async fn test_resolve_ambiguous_and_disambiguate_by_album() {
        let pool = create_test_tracks_db().await;
        let track1 = Uuid::new_v4();
        let track2 = Uuid::new_v4();
        let now = chrono::Utc::now().to_rfc3339();

        sqlx::query(
            "INSERT INTO tracks (id, title, artist, album, duration_ms, file_path, format, created_at, updated_at)
             VALUES (?, 'Echoes', 'Band', 'Live In Berlin', 300000, '/music/1.flac', 'flac', ?, ?)"
        )
        .bind(track1.to_string())
        .bind(&now)
        .bind(&now)
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO tracks (id, title, artist, album, duration_ms, file_path, format, created_at, updated_at)
             VALUES (?, 'Echoes', 'Band', 'Studio Album', 301000, '/music/2.flac', 'flac', ?, ?)"
        )
        .bind(track2.to_string())
        .bind(&now)
        .bind(&now)
        .execute(&pool)
        .await
        .unwrap();

        let resolver = ContentIdentityResolver::new(pool);
        // Without album, match is ambiguous
        let cref_ambiguous = ContentRefV1 {
            track_id: None,
            title: "Echoes".into(),
            artist: "Band".into(),
            album: None,
            duration_ms: 300500,
            content_sha256: None,
        };
        assert!(matches!(
            resolver.resolve(&cref_ambiguous).await,
            Err(ContentResolutionError::AmbiguousMatch { count: 2, .. })
        ));

        // With album, disambiguation succeeds
        let cref_disambiguated = ContentRefV1 {
            track_id: None,
            title: "Echoes".into(),
            artist: "Band".into(),
            album: Some("Studio Album".into()),
            duration_ms: 300500,
            content_sha256: None,
        };
        let resolved = resolver.resolve(&cref_disambiguated).await.unwrap();
        assert_eq!(resolved.id, track2);
    }
}
