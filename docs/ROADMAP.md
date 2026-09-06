## Canonical v1 Capability State

<!-- BEGIN GENERATED V1 FEATURE MATRIX -->
| Feature | Release Scope | Maturity | Description |
| :--- | :---: | :---: | :--- |
| **adaptive_hls** | `post-v1` | ⚪ `unavailable` | Multi-bitrate adaptive ABR / DASH streaming |
| **autonomous_playback** | `v1-beta` | 🟡 `beta` | Server-side autonomous playback projection |
| **backup** | `core` | 🟢 `stable` | Database backup, export and restoration |
| **gapless** | `post-v1` | ⚪ `unavailable` | Sample-perfect gapless audio playback |
| **handoff** | `v1-beta` | 🟡 `beta` | Direct stream handoff between peers |
| **hls_vod** | `core` | 🟢 `stable` | Single-rendition HLS VOD audio streaming |
| **library** | `core` | 🟢 `stable` | Library scanning, indexing and browsing with watcher |
| **opensubsonic** | `subset` | 🟡 `beta` | OpenSubsonic compatible subset layer (JSON-only) |
| **playback_history** | `core` | 🟢 `stable` | Play history tracking, stats and scrobbling |
| **playlists** | `core` | 🟢 `stable` | CRUD playlists, smart playlists and M3U import/export |
| **receivers** | `v1-beta` | 🟡 `beta` | Michi Link v1-lite receiver playback plane |
| **rooms** | `v1-beta` | 🟡 `beta` | Multi-room group playback routing |
| **search** | `core` | 🟢 `stable` | Full-text library search with field filters |
| **security** | `core` | 🟢 `stable` | Argon2 auth, bearer tokens, rate limiting and security headers |
| **stream** | `core` | 🟢 `stable` | HTTP Range streaming (200/206/416) and direct play |
| **sync** | `core` | 🟢 `stable` | State sync with Lamport logical clocks and epoch precedence |
| **transcode** | `core` | 🟢 `stable` | On-demand transcoding to MP3/Ogg/Opus via FFmpeg |
<!-- END GENERATED V1 FEATURE MATRIX -->

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
- [x] FFmpeg transcoding (experimental, requires ffmpeg on server)
- [x] Cover art serving
- [x] HLS VOD single-rendition streaming
- [ ] Multi-bitrate adaptive ABR / DASH streaming (post-v1)

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
- [x] Michi Link v1 contract stabilized (+ docs/CLIENT_INTEGRATION_SPEC.md)
- [x] Persistent server_id
- [x] conservative feature flags for stable v1 integration
- [x] Michi Music Player Python client (CLI + desktop PySide6 skeleton)
- [x] GHCR publish CI (multi-arch amd64+arm64)
- [x] Docker image published to ghcr.io

## Phase 9: Future
- [ ] Mobile client (Michi Music Player)
- [x] Podcast support (RSS feeds via michi-ingest)
- [ ] HLS/DASH adaptive streaming
- [x] Docker image on ghcr.io
- [x] Multi-room dynamic room groups (Party/Relax/Custom)
- [x] QR Code pairing for devices
- [x] Broadcast & Cast proxy streaming
- [x] Bookmark system
- [x] Job queue with audit log
- [x] Stream sources / Radio stations
- [x] Mount guard monitoring
- [x] i18n (9 idiomas)
- [x] Identity: Ed25519 + AEAD encryption
- [x] Setup wizard (onboard)
- [x] mDNS device discovery (connect)

## Phase 10: Future
- [ ] Mobile client (Michi Music Player)
- [ ] HLS/DASH adaptive streaming
- [ ] AI-powered recommendations
- [ ] Lyrics integration
