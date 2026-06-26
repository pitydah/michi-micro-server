# Roadmap

## Phase 1: Server Base
- [x] Rust workspace setup
- [x] HTTP server with Axum
- [x] Core models (Track, Album, Artist, etc.)
- [x] Configuration from environment
- [x] Health check endpoints
- [x] Unit tests for core models

## Phase 2: Scanner + SQLite
- [x] SQLite database layer with migrations
- [x] Metadata reading with Lofty
- [x] Directory scanner (spawn_blocking)
- [x] CRUD endpoints for library
- [x] Library management (scan, stats, clear)
- [x] Path traversal protection
- [x] Multiple music paths support
- [x] Integration tests for DB and API

## Phase 3: Streaming
- [x] Audio streaming endpoint with Range Requests
- [x] MIME type detection by file extension
- [x] Async file I/O (tokio::fs)
- [x] FFmpeg transcoding (optional, ?format=mp3|ogg)
- [x] Cover art serving
- [ ] HLS or adaptive streaming

## Phase 4: Web UI
- [x] Built-in HTML interface (vanilla, no build step)
- [x] Server status and library statistics
- [x] Library scan with WebSocket progress
- [x] Tracks, Albums, Artists tabs
- [x] In-browser audio playback with volume control
- [x] Search, pagination, queue
- [x] Playlist management (CRUD, reorder, import/export, share)
- [x] Play History with ListenBrainz scrobbling
- [x] Offline tracks via IndexedDB
- [x] PWA support (manifest, service worker)
- [x] Dark/light theme
- [x] Keyboard shortcuts

## Phase 5: Authentication
- [x] Session-based auth (Bearer token)
- [x] Admin user from env vars
- [x] Optional user registration
- [x] Per-user playlists and history
- [x] Login/logout UI

## Phase 6: M3U + Multi-room
- [x] M3U import/export
- [x] Multi-room playback sync (WebSocket peer-to-peer)
- [x] Playback state push/pull

## Phase 7: Home Assistant
- [x] MQTT discovery (sensors + buttons)
- [x] Play/pause/next controls via HA
- [x] Now playing state publishing

## Phase 8: Extras
- [x] TUI client (michi-tui, ratatui)
- [x] Swagger UI at /api/docs
- [x] End-to-end tests (WebSocket, M3U, streaming)
- [x] Debian packaging + systemd unit + install script
- [x] Docker multi-arch build
- [ ] Docker image published to ghcr.io

## Phase 9: Future
- [ ] Mobile client (Michi Music Player)
- [ ] Podcast support (RSS feeds)
- [ ] HLS/DASH adaptive streaming
- [ ] Docker image on ghcr.io
