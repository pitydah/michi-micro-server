# Changelog

## [v0.2.0-beta] — 2026-06-29

### Michi Link v1 API (stable)
- 46+ REST endpoints under `/api/v1/`
- Pairing: SERVER_CODE with 6-char codes, token refresh, device revoke
- Library: stats, scan, tracks (no file_path), search, albums, artists
- Streaming: full file (200) + Range Request (206/416), download endpoint
- Artwork, Playlists CRUD, Sync manifest/delta/state
- Import: session, preflight (4-tier matching), upload (with X-Track-Id), commit with mapping, rollback
- Playback: state, control (12 commands), session, restore
- Queue: items, jump, transfer (Player→Server), reorder, delete
- Diagnostics: DB, library, disk, staging, playback, queue, receiver, player_compatibility
- Events WebSocket with auth (send `{"token":"..."}` as first message)
- Receivers: discover, pair, session start/stop, volume, heartbeat
- Rooms: list, create, play

### Authentication & Security
- Link token authentication (SHA-256 hashed, Device/Refresh types)
- TokenStore keyed by hash, not plain token
- Revoke invalidates all device tokens
- WebSocket events require Bearer token
- Permission model (server.read, library.read, stream.read, download.read, etc.)
- No file_path exposed in any v1 API response

### Import (Player → Server)
- Preflight with exact_hash → quick_hash → sha256_prefix → metadata_duration matching
- Upload with X-Track-Id and X-Checksum headers
- Commit returns mapping (local_track_id → remote_track_id) with per-track status
- Rollback cleans staging directory, marks rolled_back in DB
- Size limits (100MB per file, 1GB per session), extension validation
- Background cleanup job for expired sessions
- SHA-256 content_hash stored on every track

### Persistence
- 23 DB migrations: tracks, playlists, users, sync, import, receivers, playback_sessions
- Automatic playback state restore on startup (track_id, position_ms, volume)
- Queue items persisted in DB, restorable after restart
- Playback sessions with source, resume_policy, restored flag

### Receivers & Multi-room
- michi-receivers crate: ReceiverClient, ReceiverSessionManager, ReceiverRegistry
- Automatic heartbeat monitoring (marks offline after 180s)
- Session negotiation: codec, sample_rate, bit_depth, channels, stream_port, buffer_ms
- Volume control
- Room creation with receiver validation
- Integration tests against receiver_sim.py (Standard + Hi-Fi)

### Diagnostics
- `GET /api/v1/diagnostics` returns 12 sections:
  - db, library, token_store, import_staging, playback, events, queues,
    disk, receiver, player_compatibility, config, warnings
- player_compatibility reports CONTRACT_OK for Player contract verification

### Testing
- 189 tests total (94 API integration + 95 unit)
- E2E Mobile flow test (pair → sync → tracks → playback → revoke)
- Player import flow test (session → upload → hash → dedup → commit)
- Auth real with Bearer tokens
- Queue/playback survive restart (simulated restart with new AppState)
- Stream/download Range (200, 206, 416)
- 15 receiver simulator integration tests (Standard + Hi-Fi)
- Player contract Python E2E script with 7 JSON fixtures
- No file_path exposure verified in tests

### Dev & Ops
- GitHub Actions CI: fmt, check, test, clippy, docker build + publish to ghcr.io
- Multi-arch Docker build (amd64 + arm64)
- Scripts: run_receiver_sim_standard.sh, run_receiver_sim_hifi.sh, test_receiver_e2e.sh
- 23 documentation files
- Public Rustdoc via docs.rs-style inline docs

## v0.1.0-alpha (unreleased)

### Core
- Async HTTP server with Axum + Tokio
- SQLite database with versioned migrations (8 migrations)
- Modular crate architecture (12 crates)
- Configuration via environment variables
- Path traversal protection
- Multiple music library paths support

### Library Scanner
- Recursive directory scanner with spawn_blocking
- Audio metadata reading via Lofty (FLAC, MP3, OGG, Opus, AAC, M4A, WAV, AIFF, DSF, DFF)
- UUID v5 stable track IDs from relative paths
- Resilient to corrupt files

### Audio Streaming
- Full file and Range Request streaming (206/416)
- MIME type detection by file extension
- Optional FFmpeg transcoding (`?format=mp3|ogg`)
- Cover art serving from disk cache

### Web UI
- Vanilla HTML/CSS/JS — no build step required
- Tabs: Tracks, Albums, Artists, Queue, Playlists, History, Offline
- Search, pagination, queue management
- Playlist CRUD with drag-and-drop reordering
- M3U import/export
- Playlist sharing (public links)
- Play history with ListenBrainz scrobbling
- Offline track downloads (IndexedDB)
- PWA support (manifest + service worker)
- Dark/light theme toggle
- Keyboard shortcuts

### Authentication
- Session-based auth (Bearer token)
- Admin user seeded from env vars
- Optional user registration
- Per-user playlists and history

### Multi-room Sync
- WebSocket peer-to-peer playback sync
- Configurable peer list via MICHI_SYNC_PEERS
- State push/pull between rooms
- Real-time position, play/pause, volume sync

### Home Assistant
- MQTT discovery (rumqttc)
- 4 sensors (track_title, artist, album, playback_status)
- 3 buttons (play_pause, next_track, previous_track)
- State publish every 5 seconds

### M3U
- Parse and serialize M3U playlists
- Import with path matching to library
- Export with full metadata (#EXTINF)

### TUI Client
- Terminal UI with ratatui + crossterm
- Browse library by tracks, albums, artists
- Search and play via mpv
- Connects via HTTP to any Michi server

### OpenAPI
- Swagger UI at /api/docs
- Full endpoint documentation with utoipa
- Request/response schema validation

### Dev & Ops
- Debian packaging + systemd unit + install script
- Docker multi-arch build
- End-to-end tests (WebSocket, M3U, streaming)
- Makefile with common targets
- CI-ready (GitHub Actions workflow for ghcr.io)
