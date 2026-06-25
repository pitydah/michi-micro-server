use std::path::Path;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use michi_core::{AudioFormat, LibraryStats, Track, TrackUpdate};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Row, SqlitePool};
use thiserror::Error;
use tracing::info;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum DbError {
    #[error("database error: {0}")]
    Sqlx(#[from] sqlx::Error),

    #[error("migration error: {0}")]
    Migration(String),
}

fn ensure_db_parent_dir(database_url: &str) -> Result<(), DbError> {
    // Strip sqlite:// or sqlite: prefix and any query params to get the file path.
    // Works for: sqlite:///path/to/db.db, sqlite:/path, sqlite:./path, :memory:
    let no_scheme = database_url
        .trim_start_matches("sqlite://")
        .trim_start_matches("sqlite:")
        .trim_start_matches("sqlite");

    let path = no_scheme.split('?').next().unwrap_or(no_scheme);

    // Skip in-memory databases and purely relative-looking paths that don't need parent dirs
    if path.is_empty() || path == ":memory:" {
        return Ok(());
    }

    let db_path = Path::new(path);
    if let Some(parent) = db_path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            std::fs::create_dir_all(parent)
                .map_err(|e| DbError::Migration(format!("failed to create db dir: {}", e)))?;
        }
    }
    Ok(())
}

pub async fn init_pool(database_url: &str) -> Result<SqlitePool, DbError> {
    info!("initializing database at {}", database_url);

    ensure_db_parent_dir(database_url)?;

    let opts = SqliteConnectOptions::from_str(database_url)
        .map_err(|e| DbError::Migration(format!("invalid database URL: {}", e)))?
        .create_if_missing(true);

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(opts)
        .await?;

    run_migrations(&pool).await?;

    Ok(pool)
}

async fn run_migrations(pool: &SqlitePool) -> Result<(), DbError> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS _migrations (
            version INTEGER PRIMARY KEY,
            applied_at TEXT NOT NULL
        )",
    )
    .execute(pool)
    .await?;

    let current: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(version), 0) FROM _migrations")
        .fetch_one(pool)
        .await?;

    if current < 1 {
        info!("applying migration 1: initial schema");
        migration_001(pool).await?;
        sqlx::query("INSERT INTO _migrations (version, applied_at) VALUES (1, ?)")
            .bind(Utc::now().to_rfc3339())
            .execute(pool)
            .await?;
        info!("migration 1 applied");
    }

    info!("database schema at version {}", current.max(1));
    Ok(())
}

async fn migration_001(pool: &SqlitePool) -> Result<(), DbError> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS tracks (
            id TEXT PRIMARY KEY,
            title TEXT,
            artist TEXT,
            album TEXT,
            album_artist TEXT,
            duration_ms INTEGER,
            file_path TEXT NOT NULL UNIQUE,
            format TEXT NOT NULL DEFAULT 'unknown',
            sample_rate INTEGER,
            bit_depth INTEGER,
            channels INTEGER,
            artwork_id TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )",
    )
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn upsert_track(pool: &SqlitePool, track: &Track) -> Result<(), DbError> {
    let id = track.id.to_string();
    let format = track.format.as_str();
    let now = track.updated_at.to_rfc3339();
    let created = track.created_at.to_rfc3339();
    let artwork_id = track.artwork_id.map(|u| u.to_string());

    sqlx::query(
        "INSERT INTO tracks (id, title, artist, album, album_artist, duration_ms, file_path, format, sample_rate, bit_depth, channels, artwork_id, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(file_path) DO UPDATE SET
            id = excluded.id,
            title = excluded.title,
            artist = excluded.artist,
            album = excluded.album,
            album_artist = excluded.album_artist,
            duration_ms = excluded.duration_ms,
            format = excluded.format,
            sample_rate = excluded.sample_rate,
            bit_depth = excluded.bit_depth,
            channels = excluded.channels,
            artwork_id = excluded.artwork_id,
            updated_at = excluded.updated_at",
    )
    .bind(&id)
    .bind(&track.title)
    .bind(&track.artist)
    .bind(&track.album)
    .bind(&track.album_artist)
    .bind(track.duration_ms.map(|v| v as i64))
    .bind(&track.file_path)
    .bind(format)
    .bind(track.sample_rate.map(|v| v as i64))
    .bind(track.bit_depth.map(|v| v as i64))
    .bind(track.channels.map(|v| v as i64))
    .bind(&artwork_id)
    .bind(&created)
    .bind(&now)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn upsert_tracks(pool: &SqlitePool, tracks: &[Track]) -> Result<usize, DbError> {
    let mut saved = 0usize;
    for track in tracks {
        upsert_track(pool, track).await?;
        saved += 1;
    }
    Ok(saved)
}

pub async fn list_tracks(pool: &SqlitePool) -> Result<Vec<Track>, DbError> {
    let rows = sqlx::query(
        "SELECT id, title, artist, album, album_artist, duration_ms, file_path, format, sample_rate, bit_depth, channels, artwork_id, created_at, updated_at FROM tracks ORDER BY title ASC",
    )
    .fetch_all(pool)
    .await?;

    let tracks = rows.iter().map(row_to_track).collect();

    Ok(tracks)
}

pub async fn library_stats(pool: &SqlitePool) -> Result<LibraryStats, DbError> {
    let row = sqlx::query(
        "SELECT COUNT(*) as tracks, COUNT(DISTINCT album) as albums, COUNT(DISTINCT artist) as artists FROM tracks",
    )
    .fetch_one(pool)
    .await?;

    Ok(LibraryStats {
        tracks: row.get("tracks"),
        albums: row.get("albums"),
        artists: row.get("artists"),
    })
}

pub async fn get_track(pool: &SqlitePool, id: &Uuid) -> Result<Option<Track>, DbError> {
    let id_str = id.to_string();
    let rows = sqlx::query(
        "SELECT id, title, artist, album, album_artist, duration_ms, file_path, format, sample_rate, bit_depth, channels, artwork_id, created_at, updated_at FROM tracks WHERE id = ?",
    )
    .bind(&id_str)
    .fetch_all(pool)
    .await?;

    Ok(rows.first().map(row_to_track))
}

pub async fn delete_track(pool: &SqlitePool, id: &Uuid) -> Result<bool, DbError> {
    let id_str = id.to_string();
    let result = sqlx::query("DELETE FROM tracks WHERE id = ?")
        .bind(&id_str)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn delete_all_tracks(pool: &SqlitePool) -> Result<u64, DbError> {
    let result = sqlx::query("DELETE FROM tracks").execute(pool).await?;
    Ok(result.rows_affected())
}

pub async fn update_track(
    pool: &SqlitePool,
    id: &Uuid,
    update: &TrackUpdate,
) -> Result<bool, DbError> {
    let id_str = id.to_string();
    let now = Utc::now().to_rfc3339();

    let rows_affected = sqlx::query(
        "UPDATE tracks SET
            title = COALESCE(?, title),
            artist = COALESCE(?, artist),
            album = COALESCE(?, album),
            album_artist = COALESCE(?, album_artist),
            duration_ms = COALESCE(?, duration_ms),
            sample_rate = COALESCE(?, sample_rate),
            bit_depth = COALESCE(?, bit_depth),
            channels = COALESCE(?, channels),
            updated_at = ?
         WHERE id = ?",
    )
    .bind(&update.title)
    .bind(&update.artist)
    .bind(&update.album)
    .bind(&update.album_artist)
    .bind(update.duration_ms.map(|v| v as i64))
    .bind(update.sample_rate.map(|v| v as i64))
    .bind(update.bit_depth.map(|v| v as i64))
    .bind(update.channels.map(|v| v as i64))
    .bind(&now)
    .bind(&id_str)
    .execute(pool)
    .await?
    .rows_affected();

    Ok(rows_affected > 0)
}

fn row_to_track(row: &sqlx::sqlite::SqliteRow) -> Track {
    let id_str: &str = row.get("id");
    let format_str: &str = row.get("format");
    let artwork_id_str: Option<&str> = row.get("artwork_id");
    let created_str: &str = row.get("created_at");
    let updated_str: &str = row.get("updated_at");

    Track {
        id: Uuid::parse_str(id_str).unwrap_or_else(|_| Uuid::nil()),
        title: row.get("title"),
        artist: row.get("artist"),
        album: row.get("album"),
        album_artist: row.get("album_artist"),
        duration_ms: row.get::<Option<i64>, _>("duration_ms").map(|v| v as u64),
        file_path: row.get("file_path"),
        format: parse_format(format_str),
        sample_rate: row.get::<Option<i64>, _>("sample_rate").map(|v| v as u32),
        bit_depth: row.get::<Option<i64>, _>("bit_depth").map(|v| v as u8),
        channels: row.get::<Option<i64>, _>("channels").map(|v| v as u8),
        artwork_id: artwork_id_str.and_then(|s| Uuid::parse_str(s).ok()),
        created_at: DateTime::parse_from_rfc3339(created_str)
            .map(|d| d.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now()),
        updated_at: DateTime::parse_from_rfc3339(updated_str)
            .map(|d| d.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now()),
    }
}

fn parse_format(s: &str) -> AudioFormat {
    match s.to_lowercase().as_str() {
        "mp3" => AudioFormat::Mp3,
        "flac" => AudioFormat::Flac,
        "ogg" => AudioFormat::Ogg,
        "opus" => AudioFormat::Opus,
        "aac" => AudioFormat::Aac,
        "m4a" => AudioFormat::M4a,
        "wav" => AudioFormat::Wav,
        "aiff" => AudioFormat::Aiff,
        "dsf" => AudioFormat::Dsf,
        "dff" => AudioFormat::Dff,
        _ => AudioFormat::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use michi_core::track_id_from_path;
    use sqlx::sqlite::SqliteConnectOptions;
    use std::str::FromStr;

    async fn test_pool() -> SqlitePool {
        let opts = SqliteConnectOptions::from_str("sqlite::memory:")
            .unwrap()
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(opts)
            .await
            .unwrap();
        run_migrations(&pool).await.unwrap();
        pool
    }

    fn sample_track() -> Track {
        Track {
            id: track_id_from_path("/music/test.flac"),
            title: Some("Test Song".into()),
            artist: Some("Test Artist".into()),
            album: Some("Test Album".into()),
            album_artist: Some("Test Album Artist".into()),
            duration_ms: Some(240000),
            file_path: "/music/test.flac".into(),
            format: AudioFormat::Flac,
            sample_rate: Some(44100),
            bit_depth: Some(16),
            channels: Some(2),
            artwork_id: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn test_upsert_and_get_track() {
        let pool = test_pool().await;
        let track = sample_track();

        upsert_track(&pool, &track).await.unwrap();

        let fetched = get_track(&pool, &track.id).await.unwrap();
        assert!(fetched.is_some());
        let fetched = fetched.unwrap();
        assert_eq!(fetched.title, track.title);
        assert_eq!(fetched.artist, track.artist);
        assert_eq!(fetched.album, track.album);
        assert_eq!(fetched.file_path, track.file_path);
        assert_eq!(fetched.format, track.format);
    }

    #[tokio::test]
    async fn test_upsert_idempotent() {
        let pool = test_pool().await;
        let mut track = sample_track();

        upsert_track(&pool, &track).await.unwrap();
        track.title = Some("Updated Title".into());
        upsert_track(&pool, &track).await.unwrap();

        let fetched = get_track(&pool, &track.id).await.unwrap().unwrap();
        assert_eq!(fetched.title, Some("Updated Title".into()));
    }

    #[tokio::test]
    async fn test_upsert_updates_by_file_path() {
        let pool = test_pool().await;
        let track1 = sample_track();
        upsert_track(&pool, &track1).await.unwrap();

        let mut track2 = sample_track();
        track2.title = Some("Updated via path".into());
        upsert_track(&pool, &track2).await.unwrap();

        let tracks = list_tracks(&pool).await.unwrap();
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].title, Some("Updated via path".into()));
    }

    #[tokio::test]
    async fn test_list_tracks() {
        let pool = test_pool().await;
        assert!(list_tracks(&pool).await.unwrap().is_empty());

        upsert_track(&pool, &sample_track()).await.unwrap();
        let tracks = list_tracks(&pool).await.unwrap();
        assert_eq!(tracks.len(), 1);
    }

    #[tokio::test]
    async fn test_update_track() {
        let pool = test_pool().await;
        let track = sample_track();
        upsert_track(&pool, &track).await.unwrap();

        let update = TrackUpdate {
            title: Some("New Title".into()),
            artist: Some("New Artist".into()),
            ..Default::default()
        };
        let updated = update_track(&pool, &track.id, &update).await.unwrap();
        assert!(updated);

        let fetched = get_track(&pool, &track.id).await.unwrap().unwrap();
        assert_eq!(fetched.title, Some("New Title".into()));
        assert_eq!(fetched.artist, Some("New Artist".into()));
        assert_eq!(fetched.album, track.album);
    }

    #[tokio::test]
    async fn test_delete_track() {
        let pool = test_pool().await;
        let track = sample_track();
        upsert_track(&pool, &track).await.unwrap();
        assert!(get_track(&pool, &track.id).await.unwrap().is_some());

        let deleted = delete_track(&pool, &track.id).await.unwrap();
        assert!(deleted);
        assert!(get_track(&pool, &track.id).await.unwrap().is_none());

        let not_found = delete_track(&pool, &track.id).await.unwrap();
        assert!(!not_found);
    }

    #[tokio::test]
    async fn test_delete_all_tracks() {
        let pool = test_pool().await;
        upsert_track(&pool, &sample_track()).await.unwrap();
        let mut t2 = sample_track();
        t2.file_path = "/music/other.flac".into();
        t2.id = track_id_from_path("/music/other.flac");
        upsert_track(&pool, &t2).await.unwrap();
        assert_eq!(list_tracks(&pool).await.unwrap().len(), 2);

        let deleted = delete_all_tracks(&pool).await.unwrap();
        assert_eq!(deleted, 2);
        assert!(list_tracks(&pool).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_library_stats() {
        let pool = test_pool().await;
        let stats = library_stats(&pool).await.unwrap();
        assert_eq!(stats.tracks, 0);

        upsert_track(&pool, &sample_track()).await.unwrap();
        let stats = library_stats(&pool).await.unwrap();
        assert_eq!(stats.tracks, 1);
    }

    #[tokio::test]
    async fn test_get_track_not_found() {
        let pool = test_pool().await;
        let result = get_track(&pool, &uuid::Uuid::nil()).await.unwrap();
        assert!(result.is_none());
    }
}
