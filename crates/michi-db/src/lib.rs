use std::path::Path;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use michi_core::{
    AlbumSummary, ArtistSummary, AudioFormat, ChainLink, ChainLinkCreate, ChainLinkUpdate,
    LibraryStats, PlayHistory, PlaybackChain, PlaybackChainCreate, PlaybackChainUpdate,
    Playlist, PlaylistCreate, PlaylistTrack, Track, TrackUpdate,
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

    if current < 9 {
        info!("applying migration 9: sync devices");
        migration_009(pool).await?;
        sqlx::query("INSERT INTO _migrations (version, applied_at) VALUES (9, ?)")
            .bind(Utc::now().to_rfc3339())
            .execute(pool)
            .await?;
        info!("migration 9 applied");
    }

    if current < 10 {
        info!("applying migration 10: sync pairing tokens");
        migration_010(pool).await?;
        sqlx::query("INSERT INTO _migrations (version, applied_at) VALUES (10, ?)")
            .bind(Utc::now().to_rfc3339())
            .execute(pool)
            .await?;
        info!("migration 10 applied");
    }

    if current < 11 {
        info!("applying migration 11: sync jobs");
        migration_011(pool).await?;
        sqlx::query("INSERT INTO _migrations (version, applied_at) VALUES (11, ?)")
            .bind(Utc::now().to_rfc3339())
            .execute(pool)
            .await?;
        info!("migration 11 applied");
    }

    if current < 12 {
        info!("applying migration 12: sync job items");
        migration_012(pool).await?;
        sqlx::query("INSERT INTO _migrations (version, applied_at) VALUES (12, ?)")
            .bind(Utc::now().to_rfc3339())
            .execute(pool)
            .await?;
        info!("migration 12 applied");
    }

    if current < 13 {
        info!("applying migration 13: players");
        migration_013(pool).await?;
        sqlx::query("INSERT INTO _migrations (version, applied_at) VALUES (13, ?)")
            .bind(Utc::now().to_rfc3339())
            .execute(pool)
            .await?;
        info!("migration 13 applied");
    }

    if current < 14 {
        info!("applying migration 14: queues");
        migration_014(pool).await?;
        sqlx::query("INSERT INTO _migrations (version, applied_at) VALUES (14, ?)")
            .bind(Utc::now().to_rfc3339())
            .execute(pool)
            .await?;
        info!("migration 14 applied");
    }

    if current < 15 {
        info!("applying migration 15: track metadata fields");
        migration_015(pool).await?;
        sqlx::query("INSERT INTO _migrations (version, applied_at) VALUES (15, ?)")
            .bind(Utc::now().to_rfc3339())
            .execute(pool)
            .await?;
        info!("migration 15 applied");
    }

    if current < 16 {
        info!("applying migration 16: link devices");
        migration_016(pool).await?;
        sqlx::query("INSERT INTO _migrations (version, applied_at) VALUES (16, ?)")
            .bind(Utc::now().to_rfc3339())
            .execute(pool)
            .await?;
        info!("migration 16 applied");
    }

    if current < 17 {
        info!("applying migration 17: pairing sessions");
        migration_017(pool).await?;
        sqlx::query("INSERT INTO _migrations (version, applied_at) VALUES (17, ?)")
            .bind(Utc::now().to_rfc3339())
            .execute(pool)
            .await?;
        info!("migration 17 applied");
    }

    if current < 18 {
        info!("applying migration 18: import sessions");
        migration_018(pool).await?;
        sqlx::query("INSERT INTO _migrations (version, applied_at) VALUES (18, ?)")
            .bind(Utc::now().to_rfc3339())
            .execute(pool)
            .await?;
        info!("migration 18 applied");
    }

    if current < 19 {
        info!("applying migration 19: receivers");
        migration_019(pool).await?;
        sqlx::query("INSERT INTO _migrations (version, applied_at) VALUES (19, ?)")
            .bind(Utc::now().to_rfc3339())
            .execute(pool)
            .await?;
        info!("migration 19 applied");
    }

    if current < 20 {
        info!("applying migration 20: playback sessions");
        migration_020(pool).await?;
        sqlx::query("INSERT INTO _migrations (version, applied_at) VALUES (20, ?)")
            .bind(Utc::now().to_rfc3339())
            .execute(pool)
            .await?;
        info!("migration 20 applied");
    }

    if current < 21 {
        info!("applying migration 21: import sessions extended");
        migration_021(pool).await?;
        sqlx::query("INSERT INTO _migrations (version, applied_at) VALUES (21, ?)")
            .bind(Utc::now().to_rfc3339())
            .execute(pool)
            .await?;
        info!("migration 21 applied");
    }

    if current < 22 {
        info!("applying migration 22: playback sessions extended");
        migration_022(pool).await?;
        sqlx::query("INSERT INTO _migrations (version, applied_at) VALUES (22, ?)")
            .bind(Utc::now().to_rfc3339())
            .execute(pool)
            .await?;
        info!("migration 22 applied");
    }

    if current < 23 {
        info!("applying migration 23: content_hash on tracks");
        migration_023(pool).await?;
        sqlx::query("INSERT INTO _migrations (version, applied_at) VALUES (23, ?)")
            .bind(Utc::now().to_rfc3339())
            .execute(pool)
            .await?;
        info!("migration 23 applied");
    }

    if current < 24 {
        info!("applying migration 24: star/rating on tracks");
        migration_024(pool).await?;
        sqlx::query("INSERT INTO _migrations (version, applied_at) VALUES (24, ?)")
            .bind(Utc::now().to_rfc3339())
            .execute(pool)
            .await?;
        info!("migration 24 applied");
    }

    if current < 25 {
        info!("applying migration 25: replaygain on tracks");
        migration_025(pool).await?;
        sqlx::query("INSERT INTO _migrations (version, applied_at) VALUES (25, ?)")
            .bind(Utc::now().to_rfc3339())
            .execute(pool)
            .await?;
        info!("migration 25 applied");
    }

    if current < 26 {
        info!("applying migration 26: playback chains");
        migration_026(pool).await?;
        sqlx::query("INSERT INTO _migrations (version, applied_at) VALUES (26, ?)")
            .bind(Utc::now().to_rfc3339())
            .execute(pool)
            .await?;
        info!("migration 26 applied");
    }

    info!("database schema at version 26");
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

async fn migration_009(pool: &SqlitePool) -> Result<(), DbError> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS sync_devices (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            device_type TEXT NOT NULL DEFAULT 'desktop',
            fingerprint TEXT,
            last_seen TEXT,
            paired_at TEXT NOT NULL,
            revoked INTEGER NOT NULL DEFAULT 0
        )",
    )
    .execute(pool)
    .await?;
    Ok(())
}

async fn migration_010(pool: &SqlitePool) -> Result<(), DbError> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS sync_pairing_tokens (
            id TEXT PRIMARY KEY,
            code TEXT NOT NULL UNIQUE,
            device_name TEXT NOT NULL,
            expires_at TEXT NOT NULL,
            used INTEGER NOT NULL DEFAULT 0
        )",
    )
    .execute(pool)
    .await?;
    Ok(())
}

async fn migration_011(pool: &SqlitePool) -> Result<(), DbError> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS sync_jobs (
            id TEXT PRIMARY KEY,
            device_id TEXT NOT NULL REFERENCES sync_devices(id),
            status TEXT NOT NULL DEFAULT 'pending',
            total_items INTEGER NOT NULL DEFAULT 0,
            completed_items INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )",
    )
    .execute(pool)
    .await?;
    Ok(())
}

async fn migration_012(pool: &SqlitePool) -> Result<(), DbError> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS sync_job_items (
            id TEXT PRIMARY KEY,
            job_id TEXT NOT NULL REFERENCES sync_jobs(id) ON DELETE CASCADE,
            track_id TEXT NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
            status TEXT NOT NULL DEFAULT 'pending',
            error TEXT
        )",
    )
    .execute(pool)
    .await?;
    Ok(())
}

async fn migration_013(pool: &SqlitePool) -> Result<(), DbError> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS players (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            kind TEXT NOT NULL DEFAULT 'webui',
            state TEXT NOT NULL DEFAULT 'idle',
            volume INTEGER NOT NULL DEFAULT 80,
            muted INTEGER NOT NULL DEFAULT 0,
            current_track_id TEXT REFERENCES tracks(id),
            position_ms INTEGER NOT NULL DEFAULT 0,
            last_seen TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )",
    )
    .execute(pool)
    .await?;
    Ok(())
}

async fn migration_014(pool: &SqlitePool) -> Result<(), DbError> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS queues (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            player_id TEXT REFERENCES players(id),
            current_index INTEGER NOT NULL DEFAULT 0,
            repeat_mode TEXT NOT NULL DEFAULT 'none',
            shuffle INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS queue_items (
            id TEXT PRIMARY KEY,
            queue_id TEXT NOT NULL REFERENCES queues(id) ON DELETE CASCADE,
            track_id TEXT NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
            position INTEGER NOT NULL,
            added_by TEXT,
            added_at TEXT NOT NULL
        )",
    )
    .execute(pool)
    .await?;

    Ok(())
}

async fn migration_016(pool: &SqlitePool) -> Result<(), DbError> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS link_devices (
            device_id TEXT PRIMARY KEY,
            alias TEXT NOT NULL,
            device_type TEXT NOT NULL DEFAULT 'unknown',
            device_model TEXT,
            token_hash TEXT NOT NULL,
            permissions TEXT NOT NULL DEFAULT '{}',
            created_at TEXT NOT NULL,
            last_seen TEXT,
            revoked INTEGER NOT NULL DEFAULT 0
        )",
    )
    .execute(pool)
    .await?;
    Ok(())
}

async fn migration_017(pool: &SqlitePool) -> Result<(), DbError> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS pairing_sessions (
            pairing_id TEXT PRIMARY KEY,
            code TEXT NOT NULL,
            device_name TEXT NOT NULL,
            device_type TEXT NOT NULL DEFAULT 'unknown',
            expires_at TEXT NOT NULL,
            confirmed INTEGER NOT NULL DEFAULT 0
        )",
    )
    .execute(pool)
    .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_pairing_code ON pairing_sessions(code)")
        .execute(pool)
        .await?;
    Ok(())
}

async fn migration_018(pool: &SqlitePool) -> Result<(), DbError> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS import_sessions (
            session_id TEXT PRIMARY KEY,
            device_id TEXT NOT NULL,
            total_tracks INTEGER NOT NULL DEFAULT 0,
            total_playlists INTEGER NOT NULL DEFAULT 0,
            imported_tracks INTEGER NOT NULL DEFAULT 0,
            imported_playlists INTEGER NOT NULL DEFAULT 0,
            total_size_bytes INTEGER NOT NULL DEFAULT 0,
            status TEXT NOT NULL DEFAULT 'active',
            expires_at TEXT NOT NULL,
            created_at TEXT NOT NULL
        )",
    )
    .execute(pool)
    .await?;
    Ok(())
}

async fn migration_019(pool: &SqlitePool) -> Result<(), DbError> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS receivers (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            device_type TEXT NOT NULL,
            host TEXT,
            port INTEGER,
            capabilities TEXT NOT NULL DEFAULT '[]',
            online INTEGER NOT NULL DEFAULT 0,
            last_seen TEXT,
            created_at TEXT NOT NULL
        )",
    )
    .execute(pool)
    .await?;
    Ok(())
}

async fn migration_020(pool: &SqlitePool) -> Result<(), DbError> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS playback_sessions (
            id TEXT PRIMARY KEY,
            device_id TEXT NOT NULL,
            queue_state TEXT NOT NULL DEFAULT '[]',
            current_index INTEGER NOT NULL DEFAULT 0,
            current_track_id TEXT,
            position_ms INTEGER NOT NULL DEFAULT 0,
            playing INTEGER NOT NULL DEFAULT 0,
            repeat_mode TEXT NOT NULL DEFAULT 'none',
            shuffle INTEGER NOT NULL DEFAULT 0,
            volume REAL NOT NULL DEFAULT 0.8,
            source TEXT NOT NULL DEFAULT 'player',
            resume_policy TEXT NOT NULL DEFAULT 'manual',
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )",
    )
    .execute(pool)
    .await?;
    Ok(())
}

async fn migration_021(pool: &SqlitePool) -> Result<(), DbError> {
    // Add status text field to import_sessions for state machine
    sqlx::query(
        "ALTER TABLE import_sessions ADD COLUMN status_text TEXT NOT NULL DEFAULT 'created'",
    )
    .execute(pool)
    .await
    .ok();
    sqlx::query("ALTER TABLE import_sessions ADD COLUMN error_message TEXT")
        .execute(pool)
        .await
        .ok();
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_import_sessions_status ON import_sessions(status)")
        .execute(pool)
        .await?;
    Ok(())
}

async fn migration_022(pool: &SqlitePool) -> Result<(), DbError> {
    sqlx::query("ALTER TABLE playback_sessions ADD COLUMN queue_id TEXT")
        .execute(pool)
        .await
        .ok();
    sqlx::query("ALTER TABLE playback_sessions ADD COLUMN source TEXT NOT NULL DEFAULT 'player'")
        .execute(pool)
        .await
        .ok();
    sqlx::query(
        "ALTER TABLE playback_sessions ADD COLUMN resume_policy TEXT NOT NULL DEFAULT 'manual'",
    )
    .execute(pool)
    .await
    .ok();
    sqlx::query("ALTER TABLE playback_sessions ADD COLUMN restored INTEGER NOT NULL DEFAULT 0")
        .execute(pool)
        .await
        .ok();
    sqlx::query("ALTER TABLE queues ADD COLUMN source_device_id TEXT")
        .execute(pool)
        .await
        .ok();
    Ok(())
}

async fn migration_023(pool: &SqlitePool) -> Result<(), DbError> {
    sqlx::query("ALTER TABLE tracks ADD COLUMN content_hash TEXT")
        .execute(pool)
        .await
        .ok();
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_tracks_content_hash ON tracks(content_hash)")
        .execute(pool)
        .await?;
    Ok(())
}

async fn migration_024(pool: &SqlitePool) -> Result<(), DbError> {
    sqlx::query("ALTER TABLE tracks ADD COLUMN starred INTEGER NOT NULL DEFAULT 0")
        .execute(pool)
        .await
        .ok();
    sqlx::query("ALTER TABLE tracks ADD COLUMN rating INTEGER NOT NULL DEFAULT 0")
        .execute(pool)
        .await
        .ok();
    sqlx::query("ALTER TABLE tracks ADD COLUMN starred_at TEXT")
        .execute(pool)
        .await
        .ok();
    Ok(())
}

async fn migration_025(pool: &SqlitePool) -> Result<(), DbError> {
    sqlx::query("ALTER TABLE tracks ADD COLUMN replaygain_track_gain REAL")
        .execute(pool)
        .await
        .ok();
    sqlx::query("ALTER TABLE tracks ADD COLUMN replaygain_track_peak REAL")
        .execute(pool)
        .await
        .ok();
    Ok(())
}

async fn migration_026(pool: &SqlitePool) -> Result<(), DbError> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS playback_chains (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            track_id TEXT REFERENCES tracks(id),
            position_ms INTEGER NOT NULL DEFAULT 0,
            playing INTEGER NOT NULL DEFAULT 0,
            shuffle INTEGER NOT NULL DEFAULT 0,
            repeat_mode TEXT NOT NULL DEFAULT 'none',
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )"
    )
        .execute(pool)
        .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS chain_links (
            id TEXT PRIMARY KEY,
            chain_id TEXT NOT NULL REFERENCES playback_chains(id) ON DELETE CASCADE,
            position INTEGER NOT NULL,
            receiver_id TEXT NOT NULL,
            volume INTEGER NOT NULL DEFAULT 80,
            muted INTEGER NOT NULL DEFAULT 0,
            delay_ms INTEGER NOT NULL DEFAULT 0
        )"
    )
        .execute(pool)
        .await?;

    Ok(())
}

async fn migration_015(pool: &SqlitePool) -> Result<(), DbError> {
    sqlx::query("ALTER TABLE tracks ADD COLUMN genre TEXT")
        .execute(pool)
        .await?;
    sqlx::query("ALTER TABLE tracks ADD COLUMN year INTEGER")
        .execute(pool)
        .await?;
    sqlx::query("ALTER TABLE tracks ADD COLUMN track_number INTEGER")
        .execute(pool)
        .await?;
    sqlx::query("ALTER TABLE tracks ADD COLUMN disc_number INTEGER")
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
                     t.genre, t.year, t.track_number, t.disc_number,
                     t.content_hash,
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
                     t.genre, t.year, t.track_number, t.disc_number,
                     t.content_hash,
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
                t.genre, t.year, t.track_number, t.disc_number,
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
         format, sample_rate, bit_depth, channels, artwork_id, genre, year, \
         track_number, disc_number, content_hash, starred, rating, starred_at, replaygain_track_gain, replaygain_track_peak, created_at, updated_at \
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
         format, sample_rate, bit_depth, channels, artwork_id, genre, year, \
         track_number, disc_number, content_hash, starred, rating, starred_at, replaygain_track_gain, replaygain_track_peak, created_at, updated_at \
          FROM tracks WHERE artist = ? ORDER BY album ASC, title ASC",
    )
    .bind(artist)
    .fetch_all(pool)
    .await?;

    Ok(rows.iter().map(row_to_track).collect())
}

// ── Playback Chains ──────────────────────────────────────────────

pub async fn create_chain(
    pool: &SqlitePool,
    input: &PlaybackChainCreate,
) -> Result<PlaybackChain, DbError> {
    let id = Uuid::new_v4();
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO playback_chains (id, name, position_ms, playing, shuffle, repeat_mode, created_at, updated_at)
         VALUES (?, ?, 0, 0, 0, 'none', ?, ?)"
    )
        .bind(id.to_string())
        .bind(&input.name)
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await?;

    Ok(PlaybackChain {
        id,
        name: input.name.clone(),
        track_id: None,
        position_ms: 0,
        playing: false,
        shuffle: false,
        repeat_mode: "none".into(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    })
}

pub async fn get_chain(pool: &SqlitePool, id: &Uuid) -> Result<Option<PlaybackChain>, DbError> {
    let id_str = id.to_string();
    let rows = sqlx::query(
        "SELECT id, name, track_id, position_ms, playing, shuffle, repeat_mode, created_at, updated_at
         FROM playback_chains WHERE id = ?"
    )
        .bind(&id_str)
        .fetch_all(pool)
        .await?;

    if rows.is_empty() {
        return Ok(None);
    }
    let r = &rows[0];
    let id_str: &str = r.try_get("id")?;
    let name: &str = r.try_get("name")?;
    let track_id_str: Option<&str> = r.try_get("track_id").ok();
    let pos: i64 = r.try_get("position_ms")?;
    let playing: i64 = r.try_get("playing")?;
    let shuffle: i64 = r.try_get("shuffle")?;
    let repeat_mode: &str = r.try_get("repeat_mode")?;
    let created_str: &str = r.try_get("created_at")?;
    let updated_str: &str = r.try_get("updated_at")?;

    Ok(Some(PlaybackChain {
        id: Uuid::parse_str(id_str).unwrap(),
        name: name.to_string(),
        track_id: track_id_str.and_then(|s| Uuid::parse_str(s).ok()),
        position_ms: pos as u64,
        playing: playing != 0,
        shuffle: shuffle != 0,
        repeat_mode: repeat_mode.to_string(),
        created_at: DateTime::parse_from_rfc3339(created_str).map(|d| d.with_timezone(&Utc)).unwrap_or_else(|_| Utc::now()),
        updated_at: DateTime::parse_from_rfc3339(updated_str).map(|d| d.with_timezone(&Utc)).unwrap_or_else(|_| Utc::now()),
    }))
}

pub async fn list_chains(pool: &SqlitePool) -> Result<Vec<PlaybackChain>, DbError> {
    let rows = sqlx::query(
        "SELECT id, name, track_id, position_ms, playing, shuffle, repeat_mode, created_at, updated_at
         FROM playback_chains ORDER BY created_at DESC"
    )
        .fetch_all(pool)
        .await?;

    let chains = rows.iter().map(|r| {
        let id_str: &str = r.try_get("id").unwrap();
        let name: &str = r.try_get("name").unwrap();
        let track_id_str: Option<&str> = r.try_get("track_id").ok();
        let pos: i64 = r.try_get("position_ms").unwrap();
        let playing: i64 = r.try_get("playing").unwrap();
        let shuffle_val: i64 = r.try_get("shuffle").unwrap();
        let repeat_mode: &str = r.try_get("repeat_mode").unwrap();
        let created_str: &str = r.try_get("created_at").unwrap();
        let updated_str: &str = r.try_get("updated_at").unwrap();
        PlaybackChain {
            id: Uuid::parse_str(id_str).unwrap(),
            name: name.to_string(),
            track_id: track_id_str.and_then(|s| Uuid::parse_str(s).ok()),
            position_ms: pos as u64,
            playing: playing != 0,
            shuffle: shuffle_val != 0,
            repeat_mode: repeat_mode.to_string(),
            created_at: DateTime::parse_from_rfc3339(created_str).map(|d| d.with_timezone(&Utc)).unwrap_or_else(|_| Utc::now()),
            updated_at: DateTime::parse_from_rfc3339(updated_str).map(|d| d.with_timezone(&Utc)).unwrap_or_else(|_| Utc::now()),
        }
    }).collect();
    Ok(chains)
}

pub async fn update_chain(
    pool: &SqlitePool,
    id: &Uuid,
    input: &PlaybackChainUpdate,
) -> Result<bool, DbError> {
    let now = Utc::now().to_rfc3339();
    let mut sets = Vec::new();
    let mut params: Vec<String> = Vec::new();

    if let Some(ref name) = input.name {
        params.push(name.clone());
        sets.push(format!("name = ?{}", params.len()));
    }
    if let Some(pos) = input.position_ms {
        params.push(pos.to_string());
        sets.push(format!("position_ms = ?{}", params.len()));
    }
    if let Some(playing) = input.playing {
        params.push((playing as i64).to_string());
        sets.push(format!("playing = ?{}", params.len()));
    }
    if let Some(shuffle) = input.shuffle {
        params.push((shuffle as i64).to_string());
        sets.push(format!("shuffle = ?{}", params.len()));
    }
    if let Some(ref mode) = input.repeat_mode {
        params.push(mode.clone());
        sets.push(format!("repeat_mode = ?{}", params.len()));
    }
    if let Some(track_id) = input.track_id {
        params.push(track_id.to_string());
        sets.push(format!("track_id = ?{}", params.len()));
    }

    if sets.is_empty() {
        return Ok(false);
    }

    params.push(now);
    sets.push(format!("updated_at = ?{}", params.len()));
    params.push(id.to_string());

    let sql = format!(
        "UPDATE playback_chains SET {} WHERE id = ?{}",
        sets.join(", "),
        params.len()
    );

    let mut q = sqlx::query(&sql);
    for p in &params {
        q = q.bind(p);
    }
    q.execute(pool).await?;
    Ok(true)
}

pub async fn delete_chain(pool: &SqlitePool, id: &Uuid) -> Result<bool, DbError> {
    let id_str = id.to_string();
    // Delete links first
    sqlx::query("DELETE FROM chain_links WHERE chain_id = ?")
        .bind(&id_str)
        .execute(pool).await?;
    let res = sqlx::query("DELETE FROM playback_chains WHERE id = ?")
        .bind(&id_str)
        .execute(pool).await?;
    Ok(res.rows_affected() > 0)
}

// ── Chain Links ──────────────────────────────────────────────────

pub async fn add_chain_link(
    pool: &SqlitePool,
    chain_id: &Uuid,
    input: &ChainLinkCreate,
) -> Result<ChainLink, DbError> {
    let id = Uuid::new_v4();
    let cid = chain_id.to_string();

    let max_pos: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(position), -1) FROM chain_links WHERE chain_id = ?")
        .bind(&cid)
        .fetch_one(pool)
        .await
        .unwrap_or(-1);

    let volume = input.volume.unwrap_or(80);
    let delay = input.delay_ms.unwrap_or(0);

    sqlx::query(
        "INSERT INTO chain_links (id, chain_id, position, receiver_id, volume, muted, delay_ms)
         VALUES (?, ?, ?, ?, ?, 0, ?)"
    )
        .bind(id.to_string())
        .bind(&cid)
        .bind(max_pos + 1)
        .bind(&input.receiver_id)
        .bind(volume)
        .bind(delay)
        .execute(pool)
        .await?;

    Ok(ChainLink {
        id,
        chain_id: *chain_id,
        position: max_pos + 1,
        receiver_id: input.receiver_id.clone(),
        receiver_name: None,
        volume,
        muted: false,
        delay_ms: delay,
    })
}

pub async fn get_chain_links(
    pool: &SqlitePool,
    chain_id: &Uuid,
) -> Result<Vec<ChainLink>, DbError> {
    let cid = chain_id.to_string();
    let rows = sqlx::query(
        "SELECT id, chain_id, position, receiver_id, volume, muted, delay_ms
         FROM chain_links WHERE chain_id = ? ORDER BY position ASC"
    )
        .bind(&cid)
        .fetch_all(pool)
        .await?;

    let links = rows.iter().map(|r| {
        let id_str: &str = r.try_get("id").unwrap();
        let cid_str: &str = r.try_get("chain_id").unwrap();
        let pos: i64 = r.try_get("position").unwrap();
        let recv_id: &str = r.try_get("receiver_id").unwrap();
        let vol: i64 = r.try_get("volume").unwrap();
        let muted: i64 = r.try_get("muted").unwrap();
        let delay: i64 = r.try_get("delay_ms").unwrap();
        ChainLink {
            id: Uuid::parse_str(id_str).unwrap(),
            chain_id: Uuid::parse_str(cid_str).unwrap(),
            position: pos,
            receiver_id: recv_id.to_string(),
            receiver_name: None,
            volume: vol,
            muted: muted != 0,
            delay_ms: delay,
        }
    }).collect();
    Ok(links)
}

pub async fn update_chain_link(
    pool: &SqlitePool,
    link_id: &Uuid,
    input: &ChainLinkUpdate,
) -> Result<bool, DbError> {
    let lid = link_id.to_string();
    let mut sets = Vec::new();
    let mut params: Vec<String> = Vec::new();

    if let Some(v) = input.volume {
        params.push(v.to_string());
        sets.push(format!("volume = ?{}", params.len()));
    }
    if let Some(m) = input.muted {
        params.push((m as i64).to_string());
        sets.push(format!("muted = ?{}", params.len()));
    }
    if let Some(d) = input.delay_ms {
        params.push(d.to_string());
        sets.push(format!("delay_ms = ?{}", params.len()));
    }
    if let Some(p) = input.position {
        params.push(p.to_string());
        sets.push(format!("position = ?{}", params.len()));
    }

    if sets.is_empty() {
        return Ok(false);
    }

    params.push(lid);
    let sql = format!(
        "UPDATE chain_links SET {} WHERE id = ?{}",
        sets.join(", "),
        params.len()
    );
    let mut q = sqlx::query(&sql);
    for p in &params {
        q = q.bind(p);
    }
    q.execute(pool).await?;
    Ok(true)
}

pub async fn delete_chain_link(pool: &SqlitePool, link_id: &Uuid) -> Result<bool, DbError> {
    let lid = link_id.to_string();
    let res = sqlx::query("DELETE FROM chain_links WHERE id = ?")
        .bind(&lid)
        .execute(pool).await?;
    Ok(res.rows_affected() > 0)
}

pub async fn reorder_chain_links(
    pool: &SqlitePool,
    chain_id: &Uuid,
    link_ids: &[Uuid],
) -> Result<(), DbError> {
    let cid = chain_id.to_string();
    for (i, link_id) in link_ids.iter().enumerate() {
        sqlx::query("UPDATE chain_links SET position = ? WHERE id = ? AND chain_id = ?")
            .bind(i as i64)
            .bind(link_id.to_string())
            .bind(&cid)
            .execute(pool)
            .await?;
    }
    Ok(())
}

pub async fn get_chain_with_links(
    pool: &SqlitePool,
    id: &Uuid,
) -> Result<Option<(PlaybackChain, Vec<ChainLink>)>, DbError> {
    let chain = get_chain(pool, id).await?;
    match chain {
        Some(c) => {
            let links = get_chain_links(pool, id).await?;
            Ok(Some((c, links)))
        }
        None => Ok(None),
    }
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
                 t.artwork_id, t.genre, t.year, t.track_number, t.disc_number, t.content_hash,
                t.created_at as t_created_at, t.updated_at as t_updated_at
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
                genre: r.get("genre"),
                year: r.get::<Option<i64>, _>("year").map(|v| v as i32),
                track_number: r.get::<Option<i64>, _>("track_number").map(|v| v as u32),
                disc_number: r.get::<Option<i64>, _>("disc_number").map(|v| v as u32),
                content_hash: r.get("content_hash"),
                starred: false,
                rating: 0,
                starred_at: None,
        replaygain_track_gain: None,
        replaygain_track_peak: None,
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
        "INSERT INTO tracks (id, title, artist, album, album_artist, duration_ms, file_path, format, sample_rate, bit_depth, channels, artwork_id, genre, year, track_number, disc_number, content_hash, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
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
            genre = excluded.genre,
            year = excluded.year,
            track_number = excluded.track_number,
            disc_number = excluded.disc_number,
            content_hash = excluded.content_hash,
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
    .bind(&track.genre)
    .bind(track.year.map(|v| v as i64))
    .bind(track.track_number.map(|v| v as i64))
    .bind(track.disc_number.map(|v| v as i64))
    .bind(&track.content_hash)
    .bind(&created)
    .bind(&now)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn find_tracks_by_content_hash(
    pool: &SqlitePool,
    hash: &str,
) -> Result<Vec<Track>, DbError> {
    let rows = sqlx::query(
        "SELECT id, title, artist, album, album_artist, duration_ms, file_path, format, sample_rate, bit_depth, channels, artwork_id, genre, year, track_number, disc_number, content_hash, starred, rating, starred_at, replaygain_track_gain, replaygain_track_peak, created_at, updated_at FROM tracks WHERE content_hash = ?",
    )
    .bind(hash)
    .fetch_all(pool)
    .await?;
    Ok(rows.iter().map(row_to_track).collect())
}

pub async fn star_track(
    pool: &SqlitePool,
    track_id: &Uuid,
    starred: bool,
) -> Result<bool, DbError> {
    let result = if starred {
        sqlx::query("UPDATE tracks SET starred = 1, starred_at = ? WHERE id = ?")
            .bind(Utc::now().to_rfc3339())
            .bind(track_id.to_string())
            .execute(pool)
            .await?
    } else {
        sqlx::query("UPDATE tracks SET starred = 0, starred_at = NULL WHERE id = ?")
            .bind(track_id.to_string())
            .execute(pool)
            .await?
    };
    Ok(result.rows_affected() > 0)
}

pub async fn rate_track(
    pool: &SqlitePool,
    track_id: &Uuid,
    rating: u8,
) -> Result<bool, DbError> {
    let rating = rating.min(5);
    let result = sqlx::query("UPDATE tracks SET rating = ? WHERE id = ?")
        .bind(rating as i64)
        .bind(track_id.to_string())
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn get_starred_tracks(
    pool: &SqlitePool,
) -> Result<Vec<Track>, DbError> {
    let rows = sqlx::query(
        "SELECT id, title, artist, album, album_artist, duration_ms, file_path, format, sample_rate, bit_depth, channels, artwork_id, genre, year, track_number, disc_number, content_hash, starred, rating, starred_at, replaygain_track_gain, replaygain_track_peak, created_at, updated_at FROM tracks WHERE starred = 1 ORDER BY starred_at DESC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.iter().map(row_to_track).collect())
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
        "SELECT id, title, artist, album, album_artist, duration_ms, file_path, format, sample_rate, bit_depth, channels, artwork_id, genre, year, track_number, disc_number, content_hash, starred, rating, starred_at, replaygain_track_gain, replaygain_track_peak, created_at, updated_at FROM tracks ORDER BY title ASC",
    )
    .fetch_all(pool)
    .await?;

    let tracks = rows.iter().map(row_to_track).collect();

    Ok(tracks)
}

pub async fn search_tracks(pool: &SqlitePool, q: &str) -> Result<Vec<Track>, DbError> {
    let pattern = format!("%{}%", q);
    let rows = sqlx::query(
        "SELECT id, title, artist, album, album_artist, duration_ms, file_path, format, sample_rate, bit_depth, channels, artwork_id, genre, year, track_number, disc_number, content_hash, starred, rating, starred_at, replaygain_track_gain, replaygain_track_peak, created_at, updated_at FROM tracks WHERE title LIKE ? OR artist LIKE ? OR album LIKE ? OR album_artist LIKE ? OR format LIKE ? ORDER BY title ASC",
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

pub async fn search_tracks_advanced(
    pool: &SqlitePool,
    query: &str,
) -> Result<Vec<Track>, DbError> {
    let mut sql = String::from(
        "SELECT id, title, artist, album, album_artist, duration_ms, file_path, format, sample_rate, bit_depth, channels, artwork_id, genre, year, track_number, disc_number, content_hash, starred, rating, starred_at, replaygain_track_gain, replaygain_track_peak, created_at, updated_at FROM tracks WHERE 1=1"
    );
    let mut params: Vec<String> = Vec::new();

    let mut fulltext_parts: Vec<String> = Vec::new();

    for token in query.split_whitespace() {
        if let Some((key, val)) = token.split_once(':') {
            match key.to_lowercase().as_str() {
                "artist" => {
                    params.push(format!("%{}%", val));
                    sql.push_str(&format!(" AND artist LIKE ?{}", params.len()));
                }
                "album" => {
                    params.push(format!("%{}%", val));
                    sql.push_str(&format!(" AND album LIKE ?{}", params.len()));
                }
                "genre" => {
                    params.push(format!("%{}%", val));
                    sql.push_str(&format!(" AND genre LIKE ?{}", params.len()));
                }
                "format" => {
                    params.push(val.to_uppercase());
                    sql.push_str(&format!(" AND format = ?{}", params.len()));
                }
                "year" => {
                    if let Some(rest) = val.strip_prefix('>') {
                        if let Ok(y) = rest.parse::<i32>() {
                            params.push(y.to_string());
                            sql.push_str(&format!(" AND year > ?{}", params.len()));
                        }
                    } else if let Some(rest) = val.strip_prefix('<') {
                        if let Ok(y) = rest.parse::<i32>() {
                            params.push(y.to_string());
                            sql.push_str(&format!(" AND year < ?{}", params.len()));
                        }
                    } else if let Some(rest) = val.strip_prefix(">=") {
                        if let Ok(y) = rest.parse::<i32>() {
                            params.push(y.to_string());
                            sql.push_str(&format!(" AND year >= ?{}", params.len()));
                        }
                    } else if let Some(rest) = val.strip_prefix("<=") {
                        if let Ok(y) = rest.parse::<i32>() {
                            params.push(y.to_string());
                            sql.push_str(&format!(" AND year <= ?{}", params.len()));
                        }
                    } else if let Ok(y) = val.parse::<i32>() {
                        params.push(y.to_string());
                        sql.push_str(&format!(" AND year = ?{}", params.len()));
                    }
                }
                "rating" => {
                    if let Some(rest) = val.strip_prefix('>') {
                        if let Ok(r) = rest.parse::<i32>() {
                            params.push(r.to_string());
                            sql.push_str(&format!(" AND rating >= ?{}", params.len()));
                        }
                    } else if let Ok(r) = val.parse::<i32>() {
                        params.push(r.to_string());
                        sql.push_str(&format!(" AND rating = ?{}", params.len()));
                    }
                }
                _ => {
                    fulltext_parts.push(format!("%{}%", token));
                }
            }
        } else {
            fulltext_parts.push(format!("%{}%", token));
        }
    }

    if !fulltext_parts.is_empty() {
        let like_clauses: Vec<String> = fulltext_parts.iter().map(|p| {
            params.push(p.clone());
            format!("(title LIKE ?{} OR artist LIKE ?{} OR album LIKE ?{})", params.len(), params.len(), params.len())
        }).collect();
        sql.push_str(" AND (");
        sql.push_str(&like_clauses.join(" AND "));
        sql.push(')');
    }

    sql.push_str(" ORDER BY title ASC LIMIT 100");

    let mut query_builder = sqlx::query(&sql);
    for p in &params {
        query_builder = query_builder.bind(p);
    }

    let rows = query_builder.fetch_all(pool).await?;
    let tracks = rows.iter().map(row_to_track).collect();
    Ok(tracks)
}

pub async fn list_tracks_paged(
    pool: &SqlitePool,
    limit: i64,
    offset: i64,
) -> Result<Vec<Track>, DbError> {
    let rows = sqlx::query(
        "SELECT id, title, artist, album, album_artist, duration_ms, file_path, format, sample_rate, bit_depth, channels, artwork_id, genre, year, track_number, disc_number, content_hash, starred, rating, starred_at, replaygain_track_gain, replaygain_track_peak, created_at, updated_at FROM tracks ORDER BY title ASC LIMIT ? OFFSET ?",
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
        "SELECT id, title, artist, album, album_artist, duration_ms, file_path, format, sample_rate, bit_depth, channels, artwork_id, genre, year, track_number, disc_number, content_hash, starred, rating, starred_at, replaygain_track_gain, replaygain_track_peak, created_at, updated_at FROM tracks WHERE id = ?",
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
        "SELECT id, title, artist, album, album_artist, duration_ms, file_path, format, sample_rate, bit_depth, channels, artwork_id, genre, year, track_number, disc_number, content_hash, starred, rating, starred_at, replaygain_track_gain, replaygain_track_peak, created_at, updated_at FROM tracks WHERE file_path = ?",
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
        "SELECT id, title, artist, album, album_artist, duration_ms, file_path, format, sample_rate, bit_depth, channels, artwork_id, genre, year, track_number, disc_number, content_hash, starred, rating, starred_at, replaygain_track_gain, replaygain_track_peak, created_at, updated_at FROM tracks WHERE file_path IN ({})",
        placeholders.join(",")
    );
    let mut query = sqlx::query(&sql);
    for path in paths {
        query = query.bind(path);
    }
    let rows = query.fetch_all(pool).await?;
    Ok(rows.iter().map(row_to_track).collect())
}

pub async fn create_sync_device(
    pool: &SqlitePool,
    id: &Uuid,
    name: &str,
    device_type: &str,
    fingerprint: Option<&str>,
) -> Result<(), DbError> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO sync_devices (id, name, device_type, fingerprint, paired_at) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(id.to_string())
    .bind(name)
    .bind(device_type)
    .bind(fingerprint)
    .bind(&now)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn list_sync_devices(
    pool: &SqlitePool,
) -> Result<Vec<(String, String, String, bool, Option<String>)>, DbError> {
    let rows = sqlx::query(
        "SELECT id, name, device_type, revoked, last_seen FROM sync_devices ORDER BY revoked ASC, name ASC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| {
            let revoked: bool = r.get::<i64, _>("revoked") != 0;
            (
                r.get("id"),
                r.get("name"),
                r.get("device_type"),
                revoked,
                r.get("last_seen"),
            )
        })
        .collect())
}

pub async fn revoke_sync_device(pool: &SqlitePool, id: &Uuid) -> Result<bool, DbError> {
    let result = sqlx::query("UPDATE sync_devices SET revoked = 1 WHERE id = ?")
        .bind(id.to_string())
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn create_pairing_token(
    pool: &SqlitePool,
    id: &Uuid,
    code: &str,
    device_name: &str,
    expires_at: &str,
) -> Result<(), DbError> {
    sqlx::query(
        "INSERT INTO sync_pairing_tokens (id, code, device_name, expires_at) VALUES (?, ?, ?, ?)",
    )
    .bind(id.to_string())
    .bind(code)
    .bind(device_name)
    .bind(expires_at)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn consume_pairing_token(
    pool: &SqlitePool,
    code: &str,
) -> Result<Option<(Uuid, String)>, DbError> {
    let rows = sqlx::query(
        "SELECT id, device_name, expires_at FROM sync_pairing_tokens WHERE code = ? AND used = 0",
    )
    .bind(code)
    .fetch_all(pool)
    .await?;
    if let Some(r) = rows.first() {
        let expires_at: &str = r.get("expires_at");
        if let Ok(exp) = chrono::DateTime::parse_from_rfc3339(expires_at) {
            if exp < Utc::now() {
                return Ok(None);
            }
        }
        let token_id: &str = r.get("id");
        sqlx::query("UPDATE sync_pairing_tokens SET used = 1 WHERE id = ?")
            .bind(token_id)
            .execute(pool)
            .await?;
        Ok(Some((
            Uuid::parse_str(r.get::<&str, _>("id")).unwrap_or(Uuid::nil()),
            r.get::<&str, _>("device_name").to_string(),
        )))
    } else {
        Ok(None)
    }
}

pub async fn create_sync_job(
    pool: &SqlitePool,
    id: &Uuid,
    device_id: &Uuid,
) -> Result<(), DbError> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO sync_jobs (id, device_id, status, created_at, updated_at) VALUES (?, ?, 'pending', ?, ?)",
    )
    .bind(id.to_string())
    .bind(device_id.to_string())
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn add_sync_job_item(
    pool: &SqlitePool,
    id: &Uuid,
    job_id: &Uuid,
    track_id: &Uuid,
) -> Result<(), DbError> {
    sqlx::query("INSERT INTO sync_job_items (id, job_id, track_id) VALUES (?, ?, ?)")
        .bind(id.to_string())
        .bind(job_id.to_string())
        .bind(track_id.to_string())
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn get_sync_job(
    pool: &SqlitePool,
    job_id: &Uuid,
) -> Result<Option<(String, i64, i64)>, DbError> {
    let rows =
        sqlx::query("SELECT status, total_items, completed_items FROM sync_jobs WHERE id = ?")
            .bind(job_id.to_string())
            .fetch_all(pool)
            .await?;
    Ok(rows.first().map(|r| {
        (
            r.get("status"),
            r.get("total_items"),
            r.get("completed_items"),
        )
    }))
}

pub async fn get_all_tracks_manifest(
    pool: &SqlitePool,
) -> Result<
    Vec<(
        Uuid,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<i64>,
        String,
    )>,
    DbError,
> {
    let rows = sqlx::query(
        "SELECT id, file_path, title, artist, album, duration_ms, artwork_id FROM tracks ORDER BY file_path ASC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| {
            let id = Uuid::parse_str(r.get::<&str, _>("id")).unwrap_or(Uuid::nil());
            (
                id,
                r.get::<&str, _>("file_path").to_string(),
                r.get("title"),
                r.get("artist"),
                r.get("album"),
                r.get::<Option<i64>, _>("duration_ms"),
                r.get::<Option<&str>, _>("artwork_id")
                    .map(|s| s.to_string())
                    .unwrap_or_default(),
            )
        })
        .collect())
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

// --- Link Device functions ---

pub async fn create_link_device(
    pool: &SqlitePool,
    device: &michi_core::LinkDevice,
) -> Result<(), DbError> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO link_devices (device_id, alias, device_type, device_model, token_hash, permissions, created_at, last_seen, revoked) VALUES (?, ?, ?, ?, ?, ?, ?, ?, 0)",
    )
    .bind(device.device_id.to_string())
    .bind(&device.alias)
    .bind(&device.device_type)
    .bind(&device.device_model)
    .bind(&device.token_hash)
    .bind(&device.permissions_json)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_link_device_by_token_hash(
    pool: &SqlitePool,
    token_hash: &str,
) -> Result<Option<michi_core::LinkDevice>, DbError> {
    let rows = sqlx::query(
        "SELECT device_id, alias, device_type, device_model, token_hash, permissions, created_at, last_seen, revoked FROM link_devices WHERE token_hash = ?",
    )
    .bind(token_hash)
    .fetch_all(pool)
    .await?;
    Ok(rows.first().map(row_to_link_device))
}

pub async fn get_link_device(
    pool: &SqlitePool,
    device_id: &Uuid,
) -> Result<Option<michi_core::LinkDevice>, DbError> {
    let rows = sqlx::query(
        "SELECT device_id, alias, device_type, device_model, token_hash, permissions, created_at, last_seen, revoked FROM link_devices WHERE device_id = ?",
    )
    .bind(device_id.to_string())
    .fetch_all(pool)
    .await?;
    Ok(rows.first().map(row_to_link_device))
}

pub async fn list_link_devices(pool: &SqlitePool) -> Result<Vec<michi_core::LinkDevice>, DbError> {
    let rows = sqlx::query(
        "SELECT device_id, alias, device_type, device_model, token_hash, permissions, created_at, last_seen, revoked FROM link_devices ORDER BY created_at DESC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.iter().map(row_to_link_device).collect())
}

pub async fn revoke_link_device(pool: &SqlitePool, device_id: &Uuid) -> Result<bool, DbError> {
    let result = sqlx::query("UPDATE link_devices SET revoked = 1 WHERE device_id = ?")
        .bind(device_id.to_string())
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn update_link_device_last_seen(
    pool: &SqlitePool,
    device_id: &Uuid,
) -> Result<(), DbError> {
    let now = Utc::now().to_rfc3339();
    sqlx::query("UPDATE link_devices SET last_seen = ? WHERE device_id = ?")
        .bind(&now)
        .bind(device_id.to_string())
        .execute(pool)
        .await?;
    Ok(())
}

// --- Pairing functions ---

pub async fn create_pairing_session(
    pool: &SqlitePool,
    session: &michi_core::PairingSessionDb,
) -> Result<(), DbError> {
    sqlx::query(
        "INSERT INTO pairing_sessions (pairing_id, code, device_name, device_type, expires_at, confirmed) VALUES (?, ?, ?, ?, ?, 0)",
    )
    .bind(session.pairing_id.to_string())
    .bind(&session.code)
    .bind(&session.device_name)
    .bind(&session.device_type)
    .bind(&session.expires_at)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_pairing_session_by_code(
    pool: &SqlitePool,
    code: &str,
) -> Result<Option<michi_core::PairingSessionDb>, DbError> {
    let rows = sqlx::query(
        "SELECT pairing_id, code, device_name, device_type, expires_at, confirmed FROM pairing_sessions WHERE code = ? AND confirmed = 0",
    )
    .bind(code)
    .fetch_all(pool)
    .await?;
    Ok(rows.first().map(row_to_pairing_session))
}

pub async fn confirm_pairing_session(
    pool: &SqlitePool,
    pairing_id: &Uuid,
) -> Result<bool, DbError> {
    let result = sqlx::query("UPDATE pairing_sessions SET confirmed = 1 WHERE pairing_id = ?")
        .bind(pairing_id.to_string())
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

// --- Import Session functions ---

pub async fn create_import_session(
    pool: &SqlitePool,
    session: &michi_core::ImportSessionDb,
) -> Result<(), DbError> {
    sqlx::query(
        "INSERT INTO import_sessions (session_id, device_id, total_tracks, total_playlists, imported_tracks, imported_playlists, total_size_bytes, status, expires_at, created_at) VALUES (?, ?, ?, ?, 0, 0, 0, 'active', ?, ?)",
    )
    .bind(session.session_id.to_string())
    .bind(session.device_id.to_string())
    .bind(session.total_tracks)
    .bind(session.total_playlists)
    .bind(&session.expires_at)
    .bind(&session.created_at)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_import_session(
    pool: &SqlitePool,
    session_id: &Uuid,
) -> Result<Option<michi_core::ImportSessionDb>, DbError> {
    let rows = sqlx::query(
        "SELECT session_id, device_id, total_tracks, total_playlists, imported_tracks, imported_playlists, total_size_bytes, status, expires_at, created_at FROM import_sessions WHERE session_id = ?",
    )
    .bind(session_id.to_string())
    .fetch_all(pool)
    .await?;
    Ok(rows.first().map(row_to_import_session))
}

pub async fn update_import_session_progress(
    pool: &SqlitePool,
    session_id: &Uuid,
    imported_tracks: u32,
    size_bytes: u64,
) -> Result<(), DbError> {
    sqlx::query(
        "UPDATE import_sessions SET imported_tracks = imported_tracks + ?, total_size_bytes = total_size_bytes + ? WHERE session_id = ?",
    )
    .bind(imported_tracks)
    .bind(size_bytes as i64)
    .bind(session_id.to_string())
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn close_import_session(pool: &SqlitePool, session_id: &Uuid) -> Result<(), DbError> {
    sqlx::query("UPDATE import_sessions SET status = 'completed', status_text = 'committed' WHERE session_id = ?")
        .bind(session_id.to_string())
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn set_import_session_status(
    pool: &SqlitePool,
    session_id: &Uuid,
    status: &michi_core::ImportState,
    error_message: Option<&str>,
) -> Result<(), DbError> {
    sqlx::query("UPDATE import_sessions SET status = ?, status_text = ?, error_message = ? WHERE session_id = ?")
        .bind(status.as_str())
        .bind(status.as_str())
        .bind(error_message)
        .bind(session_id.to_string())
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn get_import_session_full(
    pool: &SqlitePool,
    session_id: &Uuid,
) -> Result<Option<michi_core::ImportSessionDb>, DbError> {
    let rows = sqlx::query(
        "SELECT session_id, device_id, total_tracks, total_playlists, imported_tracks, imported_playlists, total_size_bytes, status, status_text, error_message, expires_at, created_at FROM import_sessions WHERE session_id = ?",
    )
    .bind(session_id.to_string())
    .fetch_all(pool)
    .await?;
    Ok(rows.first().map(row_to_import_session_full))
}

pub async fn list_expired_import_sessions(
    pool: &SqlitePool,
    cutoff: &str,
) -> Result<Vec<Uuid>, DbError> {
    let rows = sqlx::query(
        "SELECT session_id FROM import_sessions WHERE expires_at < ? AND status NOT IN ('committed', 'rolled_back')",
    )
    .bind(cutoff)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| Uuid::parse_str(r.get::<&str, _>("session_id")).unwrap_or(Uuid::nil()))
        .collect())
}

pub async fn expire_import_session(pool: &SqlitePool, session_id: &Uuid) -> Result<(), DbError> {
    sqlx::query("UPDATE import_sessions SET status = 'expired', status_text = 'expired' WHERE session_id = ?")
        .bind(session_id.to_string())
        .execute(pool)
        .await?;
    Ok(())
}

// --- Receiver functions ---

pub async fn upsert_receiver(
    pool: &SqlitePool,
    receiver: &michi_core::ReceiverDb,
) -> Result<(), DbError> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO receivers (id, name, device_type, host, port, capabilities, online, last_seen, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT(id) DO UPDATE SET name = excluded.name, host = excluded.host, port = excluded.port, capabilities = excluded.capabilities, online = excluded.online, last_seen = excluded.last_seen",
    )
    .bind(receiver.id.to_string())
    .bind(&receiver.name)
    .bind(&receiver.device_type)
    .bind(&receiver.host)
    .bind(receiver.port.map(|p| p as i64))
    .bind(&receiver.capabilities_json)
    .bind(receiver.online as i64)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn list_receivers(pool: &SqlitePool) -> Result<Vec<michi_core::ReceiverDb>, DbError> {
    let rows = sqlx::query(
        "SELECT id, name, device_type, host, port, capabilities, online, last_seen, created_at FROM receivers ORDER BY name ASC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.iter().map(row_to_receiver).collect())
}

pub async fn get_receiver(
    pool: &SqlitePool,
    id: &Uuid,
) -> Result<Option<michi_core::ReceiverDb>, DbError> {
    let rows = sqlx::query(
        "SELECT id, name, device_type, host, port, capabilities, online, last_seen, created_at FROM receivers WHERE id = ?",
    )
    .bind(id.to_string())
    .fetch_all(pool)
    .await?;
    Ok(rows.first().map(row_to_receiver))
}

// --- Playback Session functions ---

pub async fn create_playback_session(
    pool: &SqlitePool,
    session: &michi_core::PlaybackSessionDb,
) -> Result<(), DbError> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO playback_sessions (id, device_id, queue_id, queue_state, current_index, current_track_id, position_ms, playing, repeat_mode, shuffle, volume, source, resume_policy, restored, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(session.id.to_string())
    .bind(session.device_id.to_string())
    .bind(session.queue_id.map(|u| u.to_string()))
    .bind(&session.queue_state_json)
    .bind(session.current_index)
    .bind(session.current_track_id.map(|u| u.to_string()))
    .bind(session.position_ms as i64)
    .bind(session.playing as i64)
    .bind(&session.repeat_mode)
    .bind(session.shuffle as i64)
    .bind(session.volume)
    .bind(&session.source)
    .bind(&session.resume_policy)
    .bind(session.restored as i64)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn update_playback_session(
    pool: &SqlitePool,
    session: &michi_core::PlaybackSessionDb,
) -> Result<(), DbError> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "UPDATE playback_sessions SET queue_id = ?, queue_state = ?, current_index = ?, current_track_id = ?, position_ms = ?, playing = ?, repeat_mode = ?, shuffle = ?, volume = ?, source = ?, resume_policy = ?, restored = ?, updated_at = ? WHERE id = ?",
    )
    .bind(session.queue_id.map(|u| u.to_string()))
    .bind(&session.queue_state_json)
    .bind(session.current_index)
    .bind(session.current_track_id.map(|u| u.to_string()))
    .bind(session.position_ms as i64)
    .bind(session.playing as i64)
    .bind(&session.repeat_mode)
    .bind(session.shuffle as i64)
    .bind(session.volume)
    .bind(&session.source)
    .bind(&session.resume_policy)
    .bind(session.restored as i64)
    .bind(&now)
    .bind(session.id.to_string())
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_playback_session(
    pool: &SqlitePool,
    id: &Uuid,
) -> Result<Option<michi_core::PlaybackSessionDb>, DbError> {
    let rows = sqlx::query(
        "SELECT id, device_id, queue_id, queue_state, current_index, current_track_id, position_ms, playing, repeat_mode, shuffle, volume, source, resume_policy, restored, created_at, updated_at FROM playback_sessions WHERE id = ?",
    )
    .bind(id.to_string())
    .fetch_all(pool)
    .await?;
    Ok(rows.first().map(row_to_playback_session))
}

pub async fn get_latest_playback_session(
    pool: &SqlitePool,
) -> Result<Option<michi_core::PlaybackSessionDb>, DbError> {
    let rows = sqlx::query(
        "SELECT id, device_id, queue_id, queue_state, current_index, current_track_id, position_ms, playing, repeat_mode, shuffle, volume, source, resume_policy, restored, created_at, updated_at FROM playback_sessions ORDER BY updated_at DESC LIMIT 1",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.first().map(row_to_playback_session))
}

pub async fn get_queue_items(
    pool: &SqlitePool,
    queue_id: &Uuid,
) -> Result<Vec<(Uuid, i64)>, DbError> {
    let rows = sqlx::query(
        "SELECT track_id, position FROM queue_items WHERE queue_id = ? ORDER BY position ASC",
    )
    .bind(queue_id.to_string())
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| {
            let tid = Uuid::parse_str(r.get::<&str, _>("track_id")).unwrap_or(Uuid::nil());
            let pos: i64 = r.get("position");
            (tid, pos)
        })
        .collect())
}

pub async fn save_queue_state(
    pool: &SqlitePool,
    name: &str,
    track_ids: &[Uuid],
    current_index: i32,
    position_ms: u64,
) -> Result<Uuid, DbError> {
    let queue_id = Uuid::new_v4();
    let now = Utc::now().to_rfc3339();

    sqlx::query("INSERT INTO queues (id, name, created_at, updated_at) VALUES (?, ?, ?, ?)")
        .bind(queue_id.to_string()).bind(name).bind(&now).bind(&now)
        .execute(pool).await?;

    for (i, tid) in track_ids.iter().enumerate() {
        let item_id = Uuid::new_v4();
        let _ = sqlx::query(
            "INSERT INTO queue_items (id, queue_id, track_id, position, added_at) VALUES (?, ?, ?, ?, ?)",
        ).bind(item_id.to_string()).bind(queue_id.to_string())
         .bind(tid.to_string()).bind(i as i64).bind(&now)
         .execute(pool).await;
    }

    let session_id = Uuid::new_v4();
    let queue_json = serde_json::to_string(track_ids).unwrap_or_default();
    sqlx::query(
        "INSERT INTO playback_sessions (id, device_id, queue_id, queue_state, current_index, current_track_id, position_ms, playing, repeat_mode, shuffle, volume, source, resume_policy, restored, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(session_id.to_string())
    .bind(Uuid::nil().to_string())
    .bind(queue_id.to_string())
    .bind(&queue_json)
    .bind(current_index)
    .bind(track_ids.first().map(|u| u.to_string()))
    .bind(position_ms as i64)
    .bind(0i64)
    .bind("none")
    .bind(0i64)
    .bind(0.8f64)
    .bind("cross-device")
    .bind("manual")
    .bind(0i64)
    .bind(&now)
    .bind(&now)
    .execute(pool).await?;

    Ok(session_id)
}

fn row_to_link_device(row: &sqlx::sqlite::SqliteRow) -> michi_core::LinkDevice {
    michi_core::LinkDevice {
        device_id: Uuid::parse_str(row.get::<&str, _>("device_id")).unwrap_or(Uuid::nil()),
        alias: row.get("alias"),
        device_type: row.get("device_type"),
        device_model: row.get("device_model"),
        token_hash: row.get("token_hash"),
        permissions_json: row.get("permissions"),
        created_at: row
            .get::<&str, _>("created_at")
            .parse()
            .unwrap_or_else(|_| Utc::now()),
        last_seen: row.get("last_seen"),
        revoked: row.get::<i64, _>("revoked") != 0,
    }
}

fn row_to_pairing_session(row: &sqlx::sqlite::SqliteRow) -> michi_core::PairingSessionDb {
    michi_core::PairingSessionDb {
        pairing_id: Uuid::parse_str(row.get::<&str, _>("pairing_id")).unwrap_or(Uuid::nil()),
        code: row.get("code"),
        device_name: row.get("device_name"),
        device_type: row.get("device_type"),
        expires_at: row.get("expires_at"),
        confirmed: row.get::<i64, _>("confirmed") != 0,
    }
}

fn row_to_import_session(row: &sqlx::sqlite::SqliteRow) -> michi_core::ImportSessionDb {
    michi_core::ImportSessionDb {
        session_id: Uuid::parse_str(row.get::<&str, _>("session_id")).unwrap_or(Uuid::nil()),
        device_id: Uuid::parse_str(row.get::<&str, _>("device_id")).unwrap_or(Uuid::nil()),
        total_tracks: row.get::<i64, _>("total_tracks") as u32,
        total_playlists: row.get::<i64, _>("total_playlists") as u32,
        imported_tracks: row.get::<i64, _>("imported_tracks") as u32,
        imported_playlists: row.get::<i64, _>("imported_playlists") as u32,
        total_size_bytes: row.get::<i64, _>("total_size_bytes") as u64,
        status: row.get("status"),
        expires_at: row.get("expires_at"),
        created_at: row.get("created_at"),
    }
}

fn row_to_receiver(row: &sqlx::sqlite::SqliteRow) -> michi_core::ReceiverDb {
    michi_core::ReceiverDb {
        id: Uuid::parse_str(row.get::<&str, _>("id")).unwrap_or(Uuid::nil()),
        name: row.get("name"),
        device_type: row.get("device_type"),
        host: row.get("host"),
        port: row.get::<Option<i64>, _>("port").map(|v| v as u16),
        capabilities_json: row.get("capabilities"),
        online: row.get::<i64, _>("online") != 0,
        last_seen: row.get("last_seen"),
    }
}

fn row_to_playback_session(row: &sqlx::sqlite::SqliteRow) -> michi_core::PlaybackSessionDb {
    michi_core::PlaybackSessionDb {
        id: Uuid::parse_str(row.get::<&str, _>("id")).unwrap_or(Uuid::nil()),
        device_id: Uuid::parse_str(row.get::<&str, _>("device_id")).unwrap_or(Uuid::nil()),
        queue_id: row
            .get::<Option<&str>, _>("queue_id")
            .and_then(|s| Uuid::parse_str(s).ok()),
        queue_state_json: row.get("queue_state"),
        current_index: row.get::<i64, _>("current_index") as i32,
        current_track_id: row
            .get::<Option<&str>, _>("current_track_id")
            .and_then(|s| Uuid::parse_str(s).ok()),
        position_ms: row.get::<i64, _>("position_ms") as u64,
        playing: row.get::<i64, _>("playing") != 0,
        repeat_mode: row.get("repeat_mode"),
        shuffle: row.get::<i64, _>("shuffle") != 0,
        volume: row.get::<f64, _>("volume"),
        source: row.get("source"),
        resume_policy: row.get("resume_policy"),
        restored: row.get::<i64, _>("restored") != 0,
    }
}

fn row_to_import_session_full(row: &sqlx::sqlite::SqliteRow) -> michi_core::ImportSessionDb {
    michi_core::ImportSessionDb {
        session_id: Uuid::parse_str(row.get::<&str, _>("session_id")).unwrap_or(Uuid::nil()),
        device_id: Uuid::parse_str(row.get::<&str, _>("device_id")).unwrap_or(Uuid::nil()),
        total_tracks: row.get::<i64, _>("total_tracks") as u32,
        total_playlists: row.get::<i64, _>("total_playlists") as u32,
        imported_tracks: row.get::<i64, _>("imported_tracks") as u32,
        imported_playlists: row.get::<i64, _>("imported_playlists") as u32,
        total_size_bytes: row.get::<i64, _>("total_size_bytes") as u64,
        status: row.get::<&str, _>("status").to_string(),
        expires_at: row.get("expires_at"),
        created_at: row.get("created_at"),
    }
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
        genre: row.get("genre"),
        year: row.get::<Option<i64>, _>("year").map(|v| v as i32),
        track_number: row.get::<Option<i64>, _>("track_number").map(|v| v as u32),
        disc_number: row.get::<Option<i64>, _>("disc_number").map(|v| v as u32),
        created_at: DateTime::parse_from_rfc3339(created_str)
            .map(|d| d.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now()),
        updated_at: DateTime::parse_from_rfc3339(updated_str)
            .map(|d| d.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now()),
        content_hash: row.get("content_hash"),
        starred: row.get::<i64, _>("starred") != 0,
        rating: row.get::<i64, _>("rating") as u8,
        starred_at: row.get("starred_at"),
        replaygain_track_gain: row.get("replaygain_track_gain"),
        replaygain_track_peak: row.get("replaygain_track_peak"),
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
        genre: row.get("genre"),
        year: row.get::<Option<i64>, _>("year").map(|v| v as i32),
        track_number: row.get::<Option<i64>, _>("track_number").map(|v| v as u32),
        disc_number: row.get::<Option<i64>, _>("disc_number").map(|v| v as u32),
        content_hash: row.get("content_hash"),
        starred: false,
        rating: 0,
        starred_at: None,
        replaygain_track_gain: None,
        replaygain_track_peak: None,
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
            genre: None,
            year: None,
            track_number: None,
            disc_number: None,
            content_hash: None,
        starred: false,
        rating: 0,
        starred_at: None,
        replaygain_track_gain: None,
        replaygain_track_peak: None,
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
