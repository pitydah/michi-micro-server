use std::path::Path;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use michi_core::{
    AlbumSummary, ArtistSummary, AudioFormat, LibraryStats, PlayHistory, Playlist, PlaylistCreate,
    PlaylistTrack, Track, TrackUpdate,
};
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
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .busy_timeout(std::time::Duration::from_secs(5));

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

    if current < 2 {
        info!("applying migration 2: playlists");
        migration_002(pool).await?;
        sqlx::query("INSERT INTO _migrations (version, applied_at) VALUES (2, ?)")
            .bind(Utc::now().to_rfc3339())
            .execute(pool)
            .await?;
        info!("migration 2 applied");
    }

    if current < 3 {
        info!("applying migration 3: indices");
        migration_003(pool).await?;
        sqlx::query("INSERT INTO _migrations (version, applied_at) VALUES (3, ?)")
            .bind(Utc::now().to_rfc3339())
            .execute(pool)
            .await?;
        info!("migration 3 applied");
    }

    if current < 4 {
        info!("applying migration 4: play history");
        migration_004(pool).await?;
        sqlx::query("INSERT INTO _migrations (version, applied_at) VALUES (4, ?)")
            .bind(Utc::now().to_rfc3339())
            .execute(pool)
            .await?;
        info!("migration 4 applied");
    }

    if current < 5 {
        info!("applying migration 5: users");
        migration_005(pool).await?;
        sqlx::query("INSERT INTO _migrations (version, applied_at) VALUES (5, ?)")
            .bind(Utc::now().to_rfc3339())
            .execute(pool)
            .await?;
        info!("migration 5 applied");
    }

    if current < 6 {
        info!("applying migration 6: user_id on playlists");
        migration_006(pool).await?;
        sqlx::query("INSERT INTO _migrations (version, applied_at) VALUES (6, ?)")
            .bind(Utc::now().to_rfc3339())
            .execute(pool)
            .await?;
        info!("migration 6 applied");
    }

    if current < 7 {
        info!("applying migration 7: user_id on play_history");
        migration_007(pool).await?;
        sqlx::query("INSERT INTO _migrations (version, applied_at) VALUES (7, ?)")
            .bind(Utc::now().to_rfc3339())
            .execute(pool)
            .await?;
        info!("migration 7 applied");
    }

    if current < 8 {
        info!("applying migration 8: playlist sharing");
        migration_008(pool).await?;
        sqlx::query("INSERT INTO _migrations (version, applied_at) VALUES (8, ?)")
            .bind(Utc::now().to_rfc3339())
            .execute(pool)
            .await?;
        info!("migration 8 applied");
    }

    info!(
        "database schema at version {}",
        current
            .max(1)
            .max(2)
            .max(3)
            .max(4)
            .max(5)
            .max(6)
            .max(7)
            .max(8)
    );
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

async fn migration_002(pool: &SqlitePool) -> Result<(), DbError> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS playlists (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            description TEXT,
            track_count INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS playlist_tracks (
            id TEXT PRIMARY KEY,
            playlist_id TEXT NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
            track_id TEXT NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
            position INTEGER NOT NULL,
            added_at TEXT NOT NULL
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_playlist_tracks_playlist_id ON playlist_tracks(playlist_id)",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_playlist_tracks_position ON playlist_tracks(playlist_id, position)",
    )
    .execute(pool)
    .await?;

    Ok(())
}

async fn migration_003(pool: &SqlitePool) -> Result<(), DbError> {
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_tracks_title ON tracks(title)")
        .execute(pool)
        .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_tracks_artist ON tracks(artist)")
        .execute(pool)
        .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_tracks_album ON tracks(album)")
        .execute(pool)
        .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_tracks_album_artist ON tracks(album_artist)")
        .execute(pool)
        .await?;
    Ok(())
}

async fn migration_004(pool: &SqlitePool) -> Result<(), DbError> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS play_history (
            id TEXT PRIMARY KEY,
            track_id TEXT NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
            played_at TEXT NOT NULL,
            duration_ms INTEGER,
            scrobbled INTEGER NOT NULL DEFAULT 0
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_play_history_played_at ON play_history(played_at)")
        .execute(pool)
        .await?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_play_history_track_id ON play_history(track_id)")
        .execute(pool)
        .await?;

    Ok(())
}

async fn migration_005(pool: &SqlitePool) -> Result<(), DbError> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS users (
            id BLOB PRIMARY KEY,
            username TEXT NOT NULL UNIQUE,
            password_hash TEXT NOT NULL,
            is_admin INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        )",
    )
    .execute(pool)
    .await?;

    Ok(())
}

async fn migration_006(pool: &SqlitePool) -> Result<(), DbError> {
    sqlx::query("ALTER TABLE playlists ADD COLUMN user_id BLOB REFERENCES users(id)")
        .execute(pool)
        .await?;

    Ok(())
}

async fn migration_007(pool: &SqlitePool) -> Result<(), DbError> {
    sqlx::query("ALTER TABLE play_history ADD COLUMN user_id BLOB REFERENCES users(id)")
        .execute(pool)
        .await?;

    Ok(())
}

async fn migration_008(pool: &SqlitePool) -> Result<(), DbError> {
    sqlx::query("ALTER TABLE playlists ADD COLUMN share_code TEXT")
        .execute(pool)
        .await?;

    sqlx::query("ALTER TABLE playlists ADD COLUMN is_public INTEGER NOT NULL DEFAULT 0")
        .execute(pool)
        .await?;

    Ok(())
}

pub async fn create_user(
    pool: &SqlitePool,
    id: &Uuid,
    username: &str,
    password_hash: &str,
    is_admin: bool,
) -> Result<(), DbError> {
    let id_str = id.to_string();
    sqlx::query("INSERT INTO users (id, username, password_hash, is_admin) VALUES (?, ?, ?, ?)")
        .bind(&id_str)
        .bind(username)
        .bind(password_hash)
        .bind(is_admin as i64)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn get_user_by_username(
    pool: &SqlitePool,
    username: &str,
) -> Result<Option<(Uuid, String, String, bool)>, DbError> {
    let rows =
        sqlx::query("SELECT id, username, password_hash, is_admin FROM users WHERE username = ?")
            .bind(username)
            .fetch_all(pool)
            .await?;

    Ok(rows.first().map(|r| {
        let id = Uuid::parse_str(r.get::<&str, _>("id")).unwrap_or(Uuid::nil());
        let is_admin: bool = r.get::<i64, _>("is_admin") != 0;
        (
            id,
            r.get::<&str, _>("username").to_string(),
            r.get::<&str, _>("password_hash").to_string(),
            is_admin,
        )
    }))
}

pub async fn get_user_by_id(
    pool: &SqlitePool,
    id: &Uuid,
) -> Result<Option<(Uuid, String, String, bool)>, DbError> {
    let id_str = id.to_string();
    let rows = sqlx::query("SELECT id, username, password_hash, is_admin FROM users WHERE id = ?")
        .bind(&id_str)
        .fetch_all(pool)
        .await?;

    Ok(rows.first().map(|r| {
        let id = Uuid::parse_str(r.get::<&str, _>("id")).unwrap_or(Uuid::nil());
        let is_admin: bool = r.get::<i64, _>("is_admin") != 0;
        (
            id,
            r.get::<&str, _>("username").to_string(),
            r.get::<&str, _>("password_hash").to_string(),
            is_admin,
        )
    }))
}

pub async fn record_play(
    pool: &SqlitePool,
    track_id: &Uuid,
    duration_ms: Option<u64>,
    played_at: &DateTime<Utc>,
    user_id: Option<&Uuid>,
) -> Result<PlayHistory, DbError> {
    let id = Uuid::new_v4();
    let id_str = id.to_string();
    let tid_str = track_id.to_string();
    let played_str = played_at.to_rfc3339();
    let uid_str = user_id.map(|u| u.to_string());

    sqlx::query(
        "INSERT INTO play_history (id, track_id, played_at, duration_ms, scrobbled, user_id) VALUES (?, ?, ?, ?, 0, ?)",
    )
    .bind(&id_str)
    .bind(&tid_str)
    .bind(&played_str)
    .bind(duration_ms.map(|v| v as i64))
    .bind(&uid_str)
    .execute(pool)
    .await?;

    Ok(PlayHistory {
        id,
        track_id: *track_id,
        played_at: *played_at,
        duration_ms,
        scrobbled: false,
    })
}

pub async fn get_play_history(
    pool: &SqlitePool,
    limit: i64,
    offset: i64,
    user_id: Option<&Uuid>,
) -> Result<Vec<(PlayHistory, Track)>, DbError> {
    let rows = if let Some(uid) = user_id {
        let uid_str = uid.to_string();
        sqlx::query(
            "SELECT ph.id, ph.track_id, ph.played_at, ph.duration_ms, ph.scrobbled,
                    t.id as t_id, t.title, t.artist, t.album, t.album_artist,
                    t.duration_ms as t_duration_ms, t.file_path, t.format,
                    t.sample_rate, t.bit_depth, t.channels, t.artwork_id,
                    t.created_at, t.updated_at
             FROM play_history ph
             JOIN tracks t ON t.id = ph.track_id
             WHERE ph.user_id = ?
             ORDER BY ph.played_at DESC
             LIMIT ? OFFSET ?",
        )
        .bind(&uid_str)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query(
            "SELECT ph.id, ph.track_id, ph.played_at, ph.duration_ms, ph.scrobbled,
                    t.id as t_id, t.title, t.artist, t.album, t.album_artist,
                    t.duration_ms as t_duration_ms, t.file_path, t.format,
                    t.sample_rate, t.bit_depth, t.channels, t.artwork_id,
                    t.created_at, t.updated_at
             FROM play_history ph
             JOIN tracks t ON t.id = ph.track_id
             ORDER BY ph.played_at DESC
             LIMIT ? OFFSET ?",
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await?
    };

    Ok(rows.iter().map(row_to_play_history_with_track).collect())
}

pub async fn get_unscrobbled(pool: &SqlitePool) -> Result<Vec<(PlayHistory, Track)>, DbError> {
    let rows = sqlx::query(
        "SELECT ph.id, ph.track_id, ph.played_at, ph.duration_ms, ph.scrobbled,
                t.id as t_id, t.title, t.artist, t.album, t.album_artist,
                t.duration_ms as t_duration_ms, t.file_path, t.format,
                t.sample_rate, t.bit_depth, t.channels, t.artwork_id,
                t.created_at, t.updated_at
         FROM play_history ph
         JOIN tracks t ON t.id = ph.track_id
         WHERE ph.scrobbled = 0",
    )
    .fetch_all(pool)
    .await?;

    Ok(rows.iter().map(row_to_play_history_with_track).collect())
}

pub async fn mark_scrobbled(pool: &SqlitePool, id: &Uuid) -> Result<(), DbError> {
    let id_str = id.to_string();
    sqlx::query("UPDATE play_history SET scrobbled = 1 WHERE id = ?")
        .bind(&id_str)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn list_albums(pool: &SqlitePool) -> Result<Vec<AlbumSummary>, DbError> {
    let rows = sqlx::query(
        "SELECT album, album_artist, COUNT(*) as track_count \
         FROM tracks WHERE album IS NOT NULL \
         GROUP BY album ORDER BY album COLLATE NOCASE ASC",
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .iter()
        .map(|r| AlbumSummary {
            album: r.get("album"),
            album_artist: r.get("album_artist"),
            track_count: r.get("track_count"),
        })
        .collect())
}

pub async fn list_artists(pool: &SqlitePool) -> Result<Vec<ArtistSummary>, DbError> {
    let rows = sqlx::query(
        "SELECT artist, COUNT(*) as track_count \
         FROM tracks WHERE artist IS NOT NULL \
         GROUP BY artist ORDER BY artist COLLATE NOCASE ASC",
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .iter()
        .map(|r| ArtistSummary {
            artist: r.get("artist"),
            track_count: r.get("track_count"),
        })
        .collect())
}

pub async fn count_album_tracks(pool: &SqlitePool, album: &str) -> Result<i64, DbError> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tracks WHERE album = ?")
        .bind(album)
        .fetch_one(pool)
        .await?;
    Ok(count)
}

pub async fn get_album_tracks(pool: &SqlitePool, album: &str) -> Result<Vec<Track>, DbError> {
    let rows = sqlx::query(
        "SELECT id, title, artist, album, album_artist, duration_ms, file_path, \
         format, sample_rate, bit_depth, channels, artwork_id, created_at, updated_at \
         FROM tracks WHERE album = ? ORDER BY title ASC",
    )
    .bind(album)
    .fetch_all(pool)
    .await?;

    Ok(rows.iter().map(row_to_track).collect())
}

pub async fn get_artist_tracks(pool: &SqlitePool, artist: &str) -> Result<Vec<Track>, DbError> {
    let rows = sqlx::query(
        "SELECT id, title, artist, album, album_artist, duration_ms, file_path, \
         format, sample_rate, bit_depth, channels, artwork_id, created_at, updated_at \
         FROM tracks WHERE artist = ? ORDER BY album ASC, title ASC",
    )
    .bind(artist)
    .fetch_all(pool)
    .await?;

    Ok(rows.iter().map(row_to_track).collect())
}

pub async fn create_playlist(
    pool: &SqlitePool,
    input: &PlaylistCreate,
    user_id: Option<&Uuid>,
) -> Result<Playlist, DbError> {
    let id = Uuid::new_v4();
    let now = Utc::now().to_rfc3339();
    let uid_str = user_id.map(|u| u.to_string());
    sqlx::query(
        "INSERT INTO playlists (id, name, description, track_count, created_at, updated_at, user_id, share_code, is_public) VALUES (?, ?, ?, 0, ?, ?, ?, NULL, 0)",
    )
    .bind(id.to_string())
    .bind(&input.name)
    .bind(&input.description)
    .bind(&now)
    .bind(&now)
    .bind(&uid_str)
    .execute(pool)
    .await?;

    Ok(Playlist {
        id,
        name: input.name.clone(),
        description: input.description.clone(),
        track_count: 0,
        share_code: None,
        is_public: false,
        created_at: now.parse().unwrap_or_else(|_| Utc::now()),
        updated_at: Utc::now(),
    })
}

pub async fn list_playlists(
    pool: &SqlitePool,
    user_id: Option<&Uuid>,
) -> Result<Vec<Playlist>, DbError> {
    let rows = if let Some(uid) = user_id {
        let uid_str = uid.to_string();
        sqlx::query(
            "SELECT id, name, description, track_count, share_code, is_public, \
             created_at, updated_at FROM playlists \
             WHERE user_id = ? ORDER BY name COLLATE NOCASE ASC",
        )
        .bind(&uid_str)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query(
            "SELECT id, name, description, track_count, share_code, is_public, created_at, updated_at FROM playlists ORDER BY name COLLATE NOCASE ASC",
        )
        .fetch_all(pool)
        .await?
    };

    Ok(rows.iter().map(row_to_playlist).collect())
}

pub async fn get_playlist(pool: &SqlitePool, id: &Uuid) -> Result<Option<Playlist>, DbError> {
    let id_str = id.to_string();
    let rows = sqlx::query(
        "SELECT id, name, description, track_count, share_code, is_public, created_at, updated_at FROM playlists WHERE id = ?",
    )
    .bind(&id_str)
    .fetch_all(pool)
    .await?;

    Ok(rows.first().map(row_to_playlist))
}

pub async fn delete_playlist(pool: &SqlitePool, id: &Uuid) -> Result<bool, DbError> {
    let id_str = id.to_string();
    let result = sqlx::query("DELETE FROM playlists WHERE id = ?")
        .bind(&id_str)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn set_share_code(
    pool: &SqlitePool,
    playlist_id: &Uuid,
    share_code: Option<&str>,
) -> Result<(), DbError> {
    let id_str = playlist_id.to_string();
    sqlx::query("UPDATE playlists SET share_code = ?, is_public = ? WHERE id = ?")
        .bind(share_code)
        .bind(if share_code.is_some() { 1 } else { 0 })
        .bind(&id_str)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn find_playlist_by_share_code(
    pool: &SqlitePool,
    code: &str,
) -> Result<Option<(Playlist, Vec<(PlaylistTrack, Track)>)>, DbError> {
    let rows = sqlx::query(
        "SELECT id, name, description, track_count, share_code, is_public, created_at, updated_at FROM playlists WHERE share_code = ? AND is_public = 1",
    )
    .bind(code)
    .fetch_all(pool)
    .await?;

    match rows.first() {
        Some(r) => {
            let playlist = row_to_playlist(r);
            let tracks = get_playlist_tracks(pool, &playlist.id).await?;
            Ok(Some((playlist, tracks)))
        }
        None => Ok(None),
    }
}

pub async fn add_track_to_playlist(
    pool: &SqlitePool,
    playlist_id: &Uuid,
    track_id: &Uuid,
) -> Result<PlaylistTrack, DbError> {
    let id = Uuid::new_v4();
    let now = Utc::now().to_rfc3339();
    let p_id = playlist_id.to_string();
    let t_id = track_id.to_string();

    let max_pos: Option<i64> =
        sqlx::query_scalar("SELECT MAX(position) FROM playlist_tracks WHERE playlist_id = ?")
            .bind(&p_id)
            .fetch_one(pool)
            .await?;
    let position = max_pos.unwrap_or(-1) + 1;

    sqlx::query(
        "INSERT INTO playlist_tracks (id, playlist_id, track_id, position, added_at) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(id.to_string())
    .bind(&p_id)
    .bind(&t_id)
    .bind(position)
    .bind(&now)
    .execute(pool)
    .await?;

    sqlx::query(
        "UPDATE playlists SET track_count = (SELECT COUNT(*) FROM playlist_tracks WHERE playlist_id = ?), updated_at = ? WHERE id = ?",
    )
    .bind(&p_id)
    .bind(&now)
    .bind(&p_id)
    .execute(pool)
    .await?;

    Ok(PlaylistTrack {
        id,
        playlist_id: *playlist_id,
        track_id: *track_id,
        position,
        added_at: Utc::now(),
    })
}

pub async fn remove_track_from_playlist(
    pool: &SqlitePool,
    playlist_id: &Uuid,
    track_id: &Uuid,
) -> Result<bool, DbError> {
    let p_id = playlist_id.to_string();
    let t_id = track_id.to_string();
    let now = Utc::now().to_rfc3339();

    let result = sqlx::query("DELETE FROM playlist_tracks WHERE playlist_id = ? AND track_id = ?")
        .bind(&p_id)
        .bind(&t_id)
        .execute(pool)
        .await?;

    sqlx::query(
        "UPDATE playlists SET track_count = (SELECT COUNT(*) FROM playlist_tracks WHERE playlist_id = ?), updated_at = ? WHERE id = ?",
    )
    .bind(&p_id)
    .bind(&now)
    .bind(&p_id)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

pub async fn reorder_playlist_tracks(
    pool: &SqlitePool,
    playlist_id: &Uuid,
    track_ids: &[Uuid],
) -> Result<(), DbError> {
    let p_id = playlist_id.to_string();
    let now = Utc::now().to_rfc3339();
    let count = track_ids.len() as i64;

    let mut tx = pool.begin().await?;

    sqlx::query("DELETE FROM playlist_tracks WHERE playlist_id = ?")
        .bind(&p_id)
        .execute(&mut *tx)
        .await?;

    for (i, track_id) in track_ids.iter().enumerate() {
        let pt_id = Uuid::new_v4();
        let t_id = track_id.to_string();
        sqlx::query(
            "INSERT INTO playlist_tracks (id, playlist_id, track_id, position, added_at) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(pt_id.to_string())
        .bind(&p_id)
        .bind(&t_id)
        .bind(i as i64)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
    }

    sqlx::query("UPDATE playlists SET track_count = ?, updated_at = ? WHERE id = ?")
        .bind(count)
        .bind(&now)
        .bind(&p_id)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;

    Ok(())
}

pub async fn get_playlist_tracks(
    pool: &SqlitePool,
    playlist_id: &Uuid,
) -> Result<Vec<(PlaylistTrack, Track)>, DbError> {
    let p_id = playlist_id.to_string();
    let rows = sqlx::query(
        "SELECT pt.id, pt.playlist_id, pt.track_id, pt.position, pt.added_at,
                t.id as t_id, t.title, t.artist, t.album, t.album_artist,
                t.duration_ms, t.file_path, t.format, t.sample_rate, t.bit_depth, t.channels,
                t.artwork_id, t.created_at as t_created_at, t.updated_at as t_updated_at
         FROM playlist_tracks pt
         JOIN tracks t ON t.id = pt.track_id
         WHERE pt.playlist_id = ?
         ORDER BY pt.position ASC",
    )
    .bind(&p_id)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .iter()
        .map(|r| {
            let playlist_track = PlaylistTrack {
                id: Uuid::parse_str(r.get::<&str, _>("id")).unwrap_or(Uuid::nil()),
                playlist_id: Uuid::parse_str(r.get::<&str, _>("playlist_id"))
                    .unwrap_or(Uuid::nil()),
                track_id: Uuid::parse_str(r.get::<&str, _>("track_id")).unwrap_or(Uuid::nil()),
                position: r.get("position"),
                added_at: r
                    .get::<&str, _>("added_at")
                    .parse()
                    .unwrap_or_else(|_| Utc::now()),
            };
            let track = Track {
                id: playlist_track.track_id,
                title: r.get("title"),
                artist: r.get("artist"),
                album: r.get("album"),
                album_artist: r.get("album_artist"),
                duration_ms: r.get::<Option<i64>, _>("duration_ms").map(|v| v as u64),
                file_path: r.get("file_path"),
                format: parse_format(r.get::<&str, _>("format")),
                sample_rate: r.get::<Option<i64>, _>("sample_rate").map(|v| v as u32),
                bit_depth: r.get::<Option<i64>, _>("bit_depth").map(|v| v as u8),
                channels: r.get::<Option<i64>, _>("channels").map(|v| v as u8),
                artwork_id: r
                    .get::<Option<&str>, _>("artwork_id")
                    .and_then(|s| Uuid::parse_str(s).ok()),
                created_at: r
                    .get::<&str, _>("t_created_at")
                    .parse()
                    .unwrap_or_else(|_| Utc::now()),
                updated_at: r
                    .get::<&str, _>("t_updated_at")
                    .parse()
                    .unwrap_or_else(|_| Utc::now()),
            };
            (playlist_track, track)
        })
        .collect())
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

pub async fn search_tracks(pool: &SqlitePool, q: &str) -> Result<Vec<Track>, DbError> {
    let pattern = format!("%{}%", q);
    let rows = sqlx::query(
        "SELECT id, title, artist, album, album_artist, duration_ms, file_path, format, sample_rate, bit_depth, channels, artwork_id, created_at, updated_at FROM tracks WHERE title LIKE ? OR artist LIKE ? OR album LIKE ? OR album_artist LIKE ? OR format LIKE ? ORDER BY title ASC",
    )
    .bind(&pattern)
    .bind(&pattern)
    .bind(&pattern)
    .bind(&pattern)
    .bind(&pattern)
    .fetch_all(pool)
    .await?;

    let tracks = rows.iter().map(row_to_track).collect();

    Ok(tracks)
}

pub async fn list_tracks_paged(
    pool: &SqlitePool,
    limit: i64,
    offset: i64,
) -> Result<Vec<Track>, DbError> {
    let rows = sqlx::query(
        "SELECT id, title, artist, album, album_artist, duration_ms, file_path, format, sample_rate, bit_depth, channels, artwork_id, created_at, updated_at FROM tracks ORDER BY title ASC LIMIT ? OFFSET ?",
    )
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;

    let tracks = rows.iter().map(row_to_track).collect();

    Ok(tracks)
}

pub async fn count_tracks(pool: &SqlitePool) -> Result<i64, DbError> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tracks")
        .fetch_one(pool)
        .await?;
    Ok(count)
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

pub async fn find_track_by_path(
    pool: &SqlitePool,
    file_path: &str,
) -> Result<Option<Track>, DbError> {
    let rows = sqlx::query(
        "SELECT id, title, artist, album, album_artist, duration_ms, file_path, format, sample_rate, bit_depth, channels, artwork_id, created_at, updated_at FROM tracks WHERE file_path = ?",
    )
    .bind(file_path)
    .fetch_all(pool)
    .await?;

    Ok(rows.first().map(row_to_track))
}

pub async fn find_tracks_by_paths(
    pool: &SqlitePool,
    paths: &[String],
) -> Result<Vec<Track>, DbError> {
    if paths.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders: Vec<String> = (0..paths.len()).map(|_| "?".to_string()).collect();
    let sql = format!(
        "SELECT id, title, artist, album, album_artist, duration_ms, file_path, format, sample_rate, bit_depth, channels, artwork_id, created_at, updated_at FROM tracks WHERE file_path IN ({})",
        placeholders.join(",")
    );
    let mut query = sqlx::query(&sql);
    for path in paths {
        query = query.bind(path);
    }
    let rows = query.fetch_all(pool).await?;
    Ok(rows.iter().map(row_to_track).collect())
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

fn row_to_playlist(row: &sqlx::sqlite::SqliteRow) -> Playlist {
    Playlist {
        id: Uuid::parse_str(row.get::<&str, _>("id")).unwrap_or(Uuid::nil()),
        name: row.get("name"),
        description: row.get("description"),
        track_count: row.get("track_count"),
        share_code: row
            .get::<Option<&str>, _>("share_code")
            .map(|s| s.to_string()),
        is_public: row.get::<i64, _>("is_public") != 0,
        created_at: row
            .get::<&str, _>("created_at")
            .parse()
            .unwrap_or_else(|_| Utc::now()),
        updated_at: row
            .get::<&str, _>("updated_at")
            .parse()
            .unwrap_or_else(|_| Utc::now()),
    }
}

fn row_to_play_history_with_track(row: &sqlx::sqlite::SqliteRow) -> (PlayHistory, Track) {
    let play_history = PlayHistory {
        id: Uuid::parse_str(row.get::<&str, _>("id")).unwrap_or(Uuid::nil()),
        track_id: Uuid::parse_str(row.get::<&str, _>("track_id")).unwrap_or(Uuid::nil()),
        played_at: row
            .get::<&str, _>("played_at")
            .parse()
            .unwrap_or_else(|_| Utc::now()),
        duration_ms: row.get::<Option<i64>, _>("duration_ms").map(|v| v as u64),
        scrobbled: row.get::<i64, _>("scrobbled") != 0,
    };
    let track = Track {
        id: play_history.track_id,
        title: row.get("title"),
        artist: row.get("artist"),
        album: row.get("album"),
        album_artist: row.get("album_artist"),
        duration_ms: row.get::<Option<i64>, _>("t_duration_ms").map(|v| v as u64),
        file_path: row.get("file_path"),
        format: parse_format(row.get::<&str, _>("format")),
        sample_rate: row.get::<Option<i64>, _>("sample_rate").map(|v| v as u32),
        bit_depth: row.get::<Option<i64>, _>("bit_depth").map(|v| v as u8),
        channels: row.get::<Option<i64>, _>("channels").map(|v| v as u8),
        artwork_id: row
            .get::<Option<&str>, _>("artwork_id")
            .and_then(|s| Uuid::parse_str(s).ok()),
        created_at: row
            .get::<&str, _>("created_at")
            .parse()
            .unwrap_or_else(|_| Utc::now()),
        updated_at: row
            .get::<&str, _>("updated_at")
            .parse()
            .unwrap_or_else(|_| Utc::now()),
    };
    (play_history, track)
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
