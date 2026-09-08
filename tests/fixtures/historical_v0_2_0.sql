PRAGMA foreign_keys=OFF;
BEGIN TRANSACTION;
CREATE TABLE _migrations (
            version INTEGER PRIMARY KEY,
            applied_at TEXT NOT NULL
        );
INSERT INTO _migrations VALUES(1,'2026-09-08T21:20:16.369892260+00:00');
INSERT INTO _migrations VALUES(2,'2026-09-08T21:20:16.370556395+00:00');
INSERT INTO _migrations VALUES(3,'2026-09-08T21:20:16.371537144+00:00');
INSERT INTO _migrations VALUES(4,'2026-09-08T21:20:16.372262934+00:00');
INSERT INTO _migrations VALUES(5,'2026-09-08T21:20:16.372661231+00:00');
INSERT INTO _migrations VALUES(6,'2026-09-08T21:20:16.373068585+00:00');
INSERT INTO _migrations VALUES(7,'2026-09-08T21:20:16.373444539+00:00');
INSERT INTO _migrations VALUES(8,'2026-09-08T21:20:16.374062448+00:00');
INSERT INTO _migrations VALUES(9,'2026-09-08T21:20:16.374384351+00:00');
INSERT INTO _migrations VALUES(10,'2026-09-08T21:20:16.374752041+00:00');
INSERT INTO _migrations VALUES(11,'2026-09-08T21:20:16.375084464+00:00');
INSERT INTO _migrations VALUES(12,'2026-09-08T21:20:16.375534908+00:00');
INSERT INTO _migrations VALUES(13,'2026-09-08T21:20:16.375965786+00:00');
INSERT INTO _migrations VALUES(14,'2026-09-08T21:20:16.376545403+00:00');
INSERT INTO _migrations VALUES(15,'2026-09-08T21:20:16.377983459+00:00');
INSERT INTO _migrations VALUES(16,'2026-09-08T21:20:16.378424716+00:00');
INSERT INTO _migrations VALUES(17,'2026-09-08T21:20:16.379236428+00:00');
INSERT INTO _migrations VALUES(18,'2026-09-08T21:20:16.379654011+00:00');
INSERT INTO _migrations VALUES(19,'2026-09-08T21:20:16.380058249+00:00');
INSERT INTO _migrations VALUES(20,'2026-09-08T21:20:16.380490439+00:00');
INSERT INTO _migrations VALUES(21,'2026-09-08T21:20:16.381607773+00:00');
INSERT INTO _migrations VALUES(22,'2026-09-08T21:20:16.382673181+00:00');
INSERT INTO _migrations VALUES(23,'2026-09-08T21:20:16.383467109+00:00');
INSERT INTO _migrations VALUES(24,'2026-09-08T21:20:16.384622034+00:00');
INSERT INTO _migrations VALUES(25,'2026-09-08T21:20:16.385645874+00:00');
INSERT INTO _migrations VALUES(26,'2026-09-08T21:20:16.386497180+00:00');
INSERT INTO _migrations VALUES(27,'2026-09-08T21:20:16.387443494+00:00');
INSERT INTO _migrations VALUES(28,'2026-09-08T21:20:16.388158334+00:00');
INSERT INTO _migrations VALUES(29,'2026-09-08T21:20:16.388915293+00:00');
CREATE TABLE tracks (
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
        , genre TEXT, year INTEGER, track_number INTEGER, disc_number INTEGER, content_hash TEXT, starred INTEGER NOT NULL DEFAULT 0, rating INTEGER NOT NULL DEFAULT 0, starred_at TEXT, replaygain_track_gain REAL, replaygain_track_peak REAL);
INSERT INTO tracks VALUES('hist-track-001','Historical Ballad','Vintage Artist','First Edition',NULL,180000,'/music/historical_track.flac','flac',NULL,NULL,NULL,NULL,'2026-09-08 21:20:16','2026-09-08 21:20:16',NULL,NULL,NULL,NULL,NULL,0,0,NULL,NULL,NULL);
CREATE TABLE playlists (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            description TEXT,
            track_count INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        , user_id BLOB REFERENCES users(id), share_code TEXT, is_public INTEGER NOT NULL DEFAULT 0);
INSERT INTO playlists VALUES('hist-playlist-001','Historical Favorites','Preserved across migrations',1,'2026-09-08 21:20:16','2026-09-08 21:20:16',NULL,NULL,0);
CREATE TABLE playlist_tracks (
            id TEXT PRIMARY KEY,
            playlist_id TEXT NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
            track_id TEXT NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
            position INTEGER NOT NULL,
            added_at TEXT NOT NULL
        );
INSERT INTO playlist_tracks VALUES('hist-pt-001','hist-playlist-001','hist-track-001',0,'2026-09-08 21:20:16');
CREATE TABLE play_history (
            id TEXT PRIMARY KEY,
            track_id TEXT NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
            played_at TEXT NOT NULL,
            duration_ms INTEGER,
            scrobbled INTEGER NOT NULL DEFAULT 0
        , user_id BLOB REFERENCES users(id));
INSERT INTO play_history VALUES('hist-play-001','hist-track-001','2026-09-08 21:20:16',180000,1,NULL);
CREATE TABLE users (
            id BLOB PRIMARY KEY,
            username TEXT NOT NULL UNIQUE,
            password_hash TEXT NOT NULL,
            is_admin INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
INSERT INTO users VALUES('c89f375a-b47b-43a3-8e2e-51a4bc93948a','admin','$argon2id$v=19$m=19456,t=2,p=1$LuTSBLc30htOxqtCMTtuYA$zNrDuZtCC8Ptesmrhi3DDyb+g7Zg+ynplx6szchaI0k',1,'2026-09-08 21:20:16');
CREATE TABLE sync_devices (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            device_type TEXT NOT NULL DEFAULT 'desktop',
            fingerprint TEXT,
            last_seen TEXT,
            paired_at TEXT NOT NULL,
            revoked INTEGER NOT NULL DEFAULT 0
        );
CREATE TABLE sync_pairing_tokens (
            id TEXT PRIMARY KEY,
            code TEXT NOT NULL UNIQUE,
            device_name TEXT NOT NULL,
            expires_at TEXT NOT NULL,
            used INTEGER NOT NULL DEFAULT 0
        );
CREATE TABLE sync_jobs (
            id TEXT PRIMARY KEY,
            device_id TEXT NOT NULL REFERENCES sync_devices(id),
            status TEXT NOT NULL DEFAULT 'pending',
            total_items INTEGER NOT NULL DEFAULT 0,
            completed_items INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
CREATE TABLE sync_job_items (
            id TEXT PRIMARY KEY,
            job_id TEXT NOT NULL REFERENCES sync_jobs(id) ON DELETE CASCADE,
            track_id TEXT NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
            status TEXT NOT NULL DEFAULT 'pending',
            error TEXT
        );
CREATE TABLE players (
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
        );
CREATE TABLE queues (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            player_id TEXT REFERENCES players(id),
            current_index INTEGER NOT NULL DEFAULT 0,
            repeat_mode TEXT NOT NULL DEFAULT 'none',
            shuffle INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        , source_device_id TEXT);
CREATE TABLE queue_items (
            id TEXT PRIMARY KEY,
            queue_id TEXT NOT NULL REFERENCES queues(id) ON DELETE CASCADE,
            track_id TEXT NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
            position INTEGER NOT NULL,
            added_by TEXT,
            added_at TEXT NOT NULL
        );
CREATE TABLE link_devices (
            device_id TEXT PRIMARY KEY,
            alias TEXT NOT NULL,
            device_type TEXT NOT NULL DEFAULT 'unknown',
            device_model TEXT,
            token_hash TEXT NOT NULL,
            permissions TEXT NOT NULL DEFAULT '{}',
            created_at TEXT NOT NULL,
            last_seen TEXT,
            revoked INTEGER NOT NULL DEFAULT 0
        );
CREATE TABLE pairing_sessions (
            pairing_id TEXT PRIMARY KEY,
            code TEXT NOT NULL,
            device_name TEXT NOT NULL,
            device_type TEXT NOT NULL DEFAULT 'unknown',
            expires_at TEXT NOT NULL,
            confirmed INTEGER NOT NULL DEFAULT 0
        );
CREATE TABLE import_sessions (
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
        , status_text TEXT NOT NULL DEFAULT 'created', error_message TEXT);
CREATE TABLE receivers (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            device_type TEXT NOT NULL,
            host TEXT,
            port INTEGER,
            capabilities TEXT NOT NULL DEFAULT '[]',
            online INTEGER NOT NULL DEFAULT 0,
            last_seen TEXT,
            created_at TEXT NOT NULL
        );
CREATE TABLE playback_sessions (
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
        , queue_id TEXT, restored INTEGER NOT NULL DEFAULT 0);
CREATE TABLE playback_chains (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            track_id TEXT REFERENCES tracks(id),
            position_ms INTEGER NOT NULL DEFAULT 0,
            playing INTEGER NOT NULL DEFAULT 0,
            shuffle INTEGER NOT NULL DEFAULT 0,
            repeat_mode TEXT NOT NULL DEFAULT 'none',
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
CREATE TABLE chain_links (
            id TEXT PRIMARY KEY,
            chain_id TEXT NOT NULL REFERENCES playback_chains(id) ON DELETE CASCADE,
            position INTEGER NOT NULL,
            receiver_id TEXT NOT NULL,
            volume INTEGER NOT NULL DEFAULT 80,
            muted INTEGER NOT NULL DEFAULT 0,
            delay_ms INTEGER NOT NULL DEFAULT 0
        );
CREATE TABLE bookmarks (
            id TEXT PRIMARY KEY,
            track_id TEXT NOT NULL REFERENCES tracks(id),
            user_id TEXT NOT NULL DEFAULT 'default',
            device_id TEXT,
            position_ms INTEGER NOT NULL DEFAULT 0,
            duration_ms INTEGER NOT NULL DEFAULT 0,
            finished INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
CREATE TABLE radio_stations (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            stream_url TEXT NOT NULL,
            homepage TEXT,
            icon TEXT,
            codec TEXT,
            bitrate INTEGER,
            last_checked TEXT,
            enabled INTEGER NOT NULL DEFAULT 1,
            favorite INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL
        );
CREATE TABLE shared_links (
            id TEXT PRIMARY KEY,
            token_hash TEXT NOT NULL,
            track_id TEXT NOT NULL REFERENCES tracks(id),
            password_hash TEXT,
            expires_at TEXT,
            max_plays INTEGER,
            max_downloads INTEGER,
            allow_stream INTEGER NOT NULL DEFAULT 1,
            allow_download INTEGER NOT NULL DEFAULT 0,
            play_count INTEGER NOT NULL DEFAULT 0,
            download_count INTEGER NOT NULL DEFAULT 0,
            last_accessed TEXT,
            created_at TEXT NOT NULL
        );
CREATE INDEX idx_playlist_tracks_playlist_id ON playlist_tracks(playlist_id);
CREATE UNIQUE INDEX idx_playlist_tracks_position ON playlist_tracks(playlist_id, position);
CREATE INDEX idx_tracks_title ON tracks(title);
CREATE INDEX idx_tracks_artist ON tracks(artist);
CREATE INDEX idx_tracks_album ON tracks(album);
CREATE INDEX idx_tracks_album_artist ON tracks(album_artist);
CREATE INDEX idx_play_history_played_at ON play_history(played_at);
CREATE INDEX idx_play_history_track_id ON play_history(track_id);
CREATE INDEX idx_pairing_code ON pairing_sessions(code);
CREATE INDEX idx_import_sessions_status ON import_sessions(status);
CREATE INDEX idx_tracks_content_hash ON tracks(content_hash);
CREATE INDEX idx_bookmarks_track_user ON bookmarks(track_id, user_id);
COMMIT;
