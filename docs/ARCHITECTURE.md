# Architecture

Michi Micro Server is organized as a Rust workspace with a modular crate structure.

## Workspace Layout

```
michi-micro-server/
├── apps/
│   └── michi-server/          # Main binary
├── crates/
│   ├── michi-core/            # Shared models and types
│   ├── michi-api/             # HTTP routes (Axum), auth, v1 API, WebSocket
│   ├── michi-config/          # Configuration from env vars, server_id
│   ├── michi-db/              # SQLite database layer + 35 migrations
│   ├── michi-metadata/        # Audio metadata reading (Lofty)
│   ├── michi-scanner/         # Music library scanner
│   ├── michi-streaming/       # Audio streaming (+ experimental transcoding)
│   ├── michi-m3u/             # M3U playlist import/export
│   ├── michi-sync/            # Multi-server sync (device pairing, jobs, playback state)
│   ├── michi-homeassistant/   # Home Assistant MQTT integration
│   ├── michi-tui/             # Terminal UI client (ratatui)
│   ├── michi-client/          # Client SDK for connecting to Michi servers
│   ├── michi-opensubsonic/    # OpenSubsonic (Subsonic API) compatibility
│   ├── michi-rooms/           # Snapcast multi-room integration
│   ├── michi-link/            # Device linking: tokens, pairing codes, permissions
│   ├── michi-receivers/       # Receiver adapter trait and client (Chromecast, etc.)
│   ├── michi-security/        # Rate limiting, input validation, idempotency
│   ├── michi-ingest/          # Universal stream ingest (radio, podcast, HLS) + SSRF protection
│   ├── michi-identity/        # Cryptographic identity (Ed25519 keypair, AEAD encryption)
│   ├── michi-connect/         # Multi-layer discovery: mDNS, QR codes, dynamic CORS
│   └── michi-onboard/         # First-time setup wizard
├── docs/                      # Documentation + MICHI_LINK.md
├── deploy/                    # Systemd + Debian packaging
├── Dockerfile
├── docker-compose.yml
├── Makefile
└── casaos/                    # CasaOS metadata
```

## Design Principles

- **Separation of concerns**: Each crate has a single responsibility.
- **Extensibility**: Future features are isolated in their own crates.
- **Error handling**: All fallible operations return typed errors via `thiserror`.
- **Observability**: All internal logging uses `tracing`.
- **Configuration**: Environment-driven configuration with sensible defaults.

## Crate Descriptions

### michi-core
Contains all shared data types: `Track`, `AudioFormat`, `AudioMetadata`,
`LibraryStats`, `TrackUpdate`. These models are designed to be compatible
with Michi Music Player for integration via Michi Link.

Key utility functions:
- `track_id_from_path()` — generates UUID v5 from a normalized full file path (legacy fallback)
- `track_id_from_library_path()` — generates UUID v5 from the **relative** path
  within the music library. This makes IDs stable across different mount points
  (e.g., `/music` vs `/mnt/music`) as long as the relative path is the same.
- `is_path_inside_library()` — canonicalizes both paths and validates that a file
  resides within the library root. Prevents path traversal attacks.
  Returns a `Result<bool, PathError>` with typed errors.
- `PathError` — typed error enum for path resolution failures (`CannotCanonicalizeRoot`, `CannotCanonicalizeFile`).
- `AudioFormat` — `#[non_exhaustive]` enum allowing new formats to be added without breaking changes. Implements `Display` and `FromStr`.

### michi-api
Axum-based HTTP router. Defines all endpoints and handlers. Receives shared state (Config).

Serves both JSON API endpoints (`/api/*`) and the Web UI at `GET /`. The Web UI is a self-contained HTML page with embedded CSS and vanilla JavaScript that consumes the API endpoints.

### michi-config
Reads configuration from environment variables with defaults suitable for containerized deployment.

### michi-db
SQLite database layer using SQLx. Handles connection pooling and schema migrations (35 migrations covering users, playlists, play history, sync devices, pairing tokens, players, queues, track metadata, link devices, receivers, playback sessions, content hashing, and more).

### michi-metadata
Audio metadata extraction using the Lofty crate. Parses tags and audio properties from music files.

### michi-scanner
Recursively scans directories for audio files, reads metadata, and builds a track database. 

Key behaviors:
- IDs are generated from the **relative path** within the library root via `track_id_from_library_path()`, ensuring stable IDs across mount points.
- Symlinks are **skipped** to prevent accidental traversal outside the library.
- Unreadable/corrupt files do not stop the scan; a warning is logged and the file is still registered with unknown metadata.
- Only supported audio extensions are processed (mp3, flac, ogg, opus, aac, m4a, wav, aiff, aif, dsf, dff).
- Blocking I/O runs inside `tokio::task::spawn_blocking` to avoid blocking the async runtime.

### michi-streaming
Audio streaming with HTTP Range Request support. Provides:
- `parse_range()` — parses `Range` headers into start/end byte offsets
- `validate_track_path()` — canonicalizes paths and prevents directory traversal
- `open_track_file_async()` — resolves a track's file path and opens it for async reading
- `mime_type_for_ext()` — maps file extensions to audio MIME types

The crate is consumed by `michi-api` handlers and calls `michi-db` to look up tracks. It intentionally contains no HTTP or database logic — only file I/O and range math.

### michi-m3u
M3U playlist import and export. Converts between M3U file format and the internal playlist database representation.

### michi-sync
Multi-server synchronization layer. Handles device pairing, sync job creation and execution, and playback state replication across linked servers.

### michi-homeassistant
Home Assistant integration via MQTT discovery and state reporting. Publishes sensor states (now playing, player status, track count) and accepts playback commands.

### michi-tui
Terminal UI client built with ratatui. Provides a text-based interface for browsing and controlling playback.

### michi-client
Client SDK for connecting to Michi servers. Provides `MichiClient` with connection state management, server feature detection, and a typed client error type.

### michi-opensubsonic
OpenSubsonic / Subsonic API compatibility layer. Implements the Subsonic REST API endpoints for interoperability with third-party clients (DSub, Ultrasonic, etc.).

### michi-rooms
Snapcast multi-room audio integration. Manages rooms, clients, and groups via Snapcast's JSON-RPC interface for synchronized multi-room playback.

### michi-link
Device linking and authorization. Provides token-based device authentication, 6-digit pairing codes, permission models, and version tracking for client-server connections.

### michi-receivers
Receiver abstraction layer. Defines the `ReceiverAdapter` trait with capabilities, play/pause/stop control, volume, and position tracking. Includes a `ReceiverClient` and session manager for receiver-based playback.

### michi-security
Security middleware layer. Provides IP-based rate limiting, idempotency keys for mutation safety, and input validation guards for API endpoints.

### michi-ingest
Universal stream ingest pipeline. Sniffs URLs to detect stream type (radio, podcast, direct file, HLS). Includes SSRF protection that blocks private/reserved IP ranges. Supports RSS/Atom podcast feed parsing with episode extraction.

### michi-identity
Cryptographic identity system. On first run, generates an Ed25519 keypair persisted to disk encrypted with ChaCha20-Poly1305 AEAD. The encryption key is derived from blake3(hostname + salt) so a copied identity file is not usable on another machine.

### michi-connect
Multi-layer server discovery and connection. Provides mDNS announcement of the server as `_michi._tcp`, QR code-based connection links (`michi://connect?id=...`), and dynamic CORS based on signed tokens.

## Web UI

The Web UI is a self-contained HTML page with embedded CSS and vanilla JavaScript served at `GET /`. It consumes the JSON API endpoints directly:
- `/api/status` — Server health and version
- `/api/library/stats` — Track/album/artist counts
- `/api/tracks` — Full track listing (supports `?limit=&offset=` for pagination)
- `/api/search?q=` — Case-insensitive search across title, artist, album, album_artist, format
- `/api/stream/:id` — Audio playback with `<audio>` element
- `/api/library/scan` — Trigger library rescan
- `/api/library/tracks` — Clear library (with DELETE + confirm dialog)

## Data Flow

```
Config (env) → main.rs → Router (michi-api)
                                  ↓
                           HTTP Request
                                  ↓
                           Handler → Response
```

When scanning:
```
Scanner → Metadata Reader → Database
```

When streaming:
```
Client → /api/stream/:id → michi-api handler → michi-db (lookup) → michi-streaming (file I/O) → Response
```
