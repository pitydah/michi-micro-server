# Changelog

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
