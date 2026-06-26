# Roadmap

## Phase 1: Server Base
- [x] Rust workspace setup
- [x] HTTP server with Axum
- [x] Core models (Track, Album, Artist, etc.)
- [x] Configuration from environment
- [x] Health check endpoints
- [x] Unit tests for core models (15 tests)

## Phase 2: Scanner + SQLite
- [x] SQLite database layer
- [x] Metadata reading with Lofty
- [x] Directory scanner (with `spawn_blocking`)
- [x] CRUD endpoints for library (GET/PUT/DELETE /api/tracks/:id)
- [x] Database migrations (`_migrations` table + version tracking)
- [x] Library management API (DELETE /api/library/tracks)
- [x] Stable IDs via relative paths (`track_id_from_library_path`)
- [x] Path traversal protection (`is_path_inside_library`)
- [x] Scanner skips symlinks, resilient to corrupt files
- [x] SQLite URL simplified (no `?mode=rwc`)
- [x] Dead code removed: PlaybackState, Album, Artist, Playlist
- [x] Dead schema removed: albums, artists, playlists, playlist_tracks
- [x] Integration tests for DB layer (9 tests)
- [x] Integration tests for API handlers (19 tests)
- [x] Configuration tests (4 tests)
- [x] Scanner tests (5 tests)
- [x] Streaming module tests (15 tests)

## Phase 3: Streaming
- [x] Audio streaming endpoint (`GET /api/stream/:id`)
- [x] Range request support (206 Partial Content, 416 Range Not Satisfiable)
- [x] Path traversal protection (canonical path validation)
- [x] MIME type detection by file extension
- [x] Async file I/O (tokio::fs) for full file reads
- [x] Async range reads with tokio::fs::File
- [x] `ensure_db_parent_dir` robust to edge cases
- [ ] Transcoding via FFmpeg
- [ ] Cover art serving
- [ ] HLS or adaptive streaming

## Phase 4: Web UI
- [x] Built-in HTML interface served at `GET /`
- [x] Server status and library statistics display
- [x] One-click library scan
- [x] Track listing with metadata
- [x] In-browser audio playback via `<audio>` element
- [x] Search by title, artist, album, album_artist, format
- [x] Pagination support (`?limit=&offset=`)
- [x] Clear library with confirmation
- [x] Stop playback button
- [x] Now playing info with format and duration
- [x] Track counter
- [x] Responsive layout for mobile
- [ ] Playlist management

## Phase 5: CasaOS/ZimaOS
- [x] CasaOS metadata (x-casaos, data.yml)
- [x] Docker multi-architecture support (amd64, arm64)
- [ ] Docker image published to ghcr.io
- [ ] CasaOS app store submission

## Phase 6: Home Assistant / MQTT
- [ ] MQTT discovery
- [ ] Play/pause/next controls via HA
- [ ] Now playing sensor

## Phase 7: Sync
- [ ] Sync API for Michi Music Player / Michi Mobile
- [ ] Shared data models

## Phase 8: Multiroom
- [ ] Multiroom Snapcast

## Phase 9: Transcoding
- [ ] FFmpeg-based transcoding for mobile
- [ ] Adaptive bitrate streaming
