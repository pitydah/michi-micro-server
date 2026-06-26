# Michi Micro Server

[![CI](https://github.com/pitydah/michi-micro-server/actions/workflows/ci.yml/badge.svg)](https://github.com/pitydah/michi-micro-server/actions/workflows/ci.yml)

> Lightweight, robust, and efficient home music server written in Rust.

Michi Micro Server centralizes your local music library, reads advanced metadata,
manages playlists, and serves music over your local network or Tailscale.
It is designed to integrate with [Michi Music Player](https://github.com/pitydah/michi-music-player),
Michi Mobile, Home Assistant, and CasaOS/ZimaOS.

## Objectives

- **Lightweight** — Runs on Raspberry Pi, mini PCs, and NAS devices
- **Robust** — Resilient to corrupt files and network interruptions
- **Efficient** — Minimal CPU and memory footprint
- **Extensible** — Modular crate architecture for future features
- **Compatible** — Syncs with Michi Music Player and Michi Mobile
- **Containerized** — Docker-first deployment for CasaOS/ZimaOS

## Tech Stack

| Layer | Technology |
|-------|-----------|
| Language | Rust |
| Runtime | Tokio (async) |
| HTTP | Axum |
| Database | SQLite via SQLx |
| Metadata | Lofty |
| Serialization | Serde |
| Logging | Tracing |
| Audio | Native streaming (+ experimental FFmpeg transcoding) |
| Container | Docker + Compose |

## Project Structure

```
michi-micro-server/
├── apps/michi-server/       # Main binary
├── crates/
│   ├── michi-core/          # Shared models
│   ├── michi-api/           # HTTP routes, WebSocket, auth
│   ├── michi-config/        # Configuration from env
│   ├── michi-db/            # Database layer + migrations
│   ├── michi-metadata/      # Audio tag reading (Lofty)
│   ├── michi-scanner/       # Library scanner
│   ├── michi-streaming/     # Audio streaming (+ experimental transcoding)
│   ├── michi-homeassistant/ # Home Assistant MQTT integration
│   ├── michi-sync/          # Multi-room playback sync
│   ├── michi-m3u/           # M3U playlist import/export
│   └── michi-tui/           # Terminal UI client (ratatui)
├── docs/                    # Documentation
├── deploy/                  # Systemd + Debian packaging
├── Dockerfile
├── docker-compose.yml
├── Makefile
└── casaos/                  # CasaOS metadata
```

## Web UI

Open http://localhost:8096 in your browser for the built-in web interface.

**Features:**
- Server status, version, port, and library statistics
- One-click library scan with real-time WebSocket progress
- Tracks, Albums, Artists, Playlists, Queue, History, Offline tabs
- Search by title, artist, album, album_artist, or format
- In-browser audio playback with `<audio>` element
- Playlist create/delete/reorder/export/import (M3U) + sharing
- Drag-and-drop playlist reordering
- Play history with ListenBrainz scrobbling
- Dark/light theme toggle
- Keyboard shortcuts (space, arrows, N/P, +/-)
- Experimental FFmpeg transcoding toggle (requires ffmpeg)
- Offline mode: download tracks to IndexedDB (experimental)
- PWA support: install as app, offline caching (experimental)
- Authentication: session-based with admin + optional registration (experimental)
- Responsive layout — no build step or frontend framework required

## Quick Start

### Local Development

```bash
# Prerequisites: Rust 1.77+, SQLite dev libraries

git clone https://github.com/pitydah/michi-micro-server.git
cd michi-micro-server

# Run the server
MICHI_PORT=8096 \
MICHI_MUSIC_PATH=./music \
MICHI_CONFIG_PATH=./data/config \
MICHI_CACHE_PATH=./data/cache \
MICHI_DATABASE=sqlite://./data/config/michi.db \
cargo run -p michi-server

# Or with default paths (requires /music, /config, /cache):
cargo run -p michi-server
```

### Running Tests

```bash
# Run all tests (119 tests across all crates)
cargo test

# Code quality
cargo fmt
cargo clippy --all-targets
```

### Docker Compose (Recommended)

```bash
mkdir -p data/config data/cache music
docker compose up -d
docker compose logs -f
```

### Docker

```bash
docker build -t michi-micro-server .
docker run -d \
  --name michi-micro-server \
  -p 8096:8096 \
  -v ./data/config:/config \
  -v ./data/cache:/cache \
  -v ./music:/music \
  -e TZ=America/Santiago \
  michi-micro-server
```

## API Endpoints

| Method | Path | Description |
|--------|------|-------------|
| GET | `/` | Web UI (HTML) |
| GET | `/manifest.json` | PWA manifest |
| GET | `/sw.js` | Service worker |
| GET | `/api/status` | Server health check |
| POST | `/api/library/scan` | Scan music library |
| GET | `/api/library/stats` | Library statistics |
| DELETE | `/api/library/tracks` | Delete all tracks |
| GET | `/api/tracks` | List all tracks |
| GET | `/api/tracks/:id` | Get track metadata |
| PUT | `/api/tracks/:id` | Update track metadata |
| DELETE | `/api/tracks/:id` | Delete track |
| GET | `/api/search?q=` | Search library |
| GET | `/api/stream/:id` | Stream audio (`?format=mp3\|ogg` experimental) |
| GET | `/api/albums` | List albums |
| GET | `/api/albums/:album` | Album tracks |
| GET | `/api/artists` | List artists |
| GET | `/api/artists/:artist` | Artist tracks |
| GET | `/api/artwork/:id` | Cover art image |
| GET | `/api/playlists` | List playlists |
| POST | `/api/playlists` | Create playlist |
| GET | `/api/playlists/:id` | Get playlist |
| DELETE | `/api/playlists/:id` | Delete playlist |
| GET | `/api/playlists/:id/tracks` | Playlist tracks |
| POST | `/api/playlists/:id/tracks/:tid` | Add track to playlist |
| DELETE | `/api/playlists/:id/tracks/:tid` | Remove track from playlist |
| PUT | `/api/playlists/:id/reorder` | Reorder playlist |
| GET | `/api/playlists/:id/export` | Export M3U |
| POST | `/api/playlists/import` | Import M3U |
| GET/POST/DELETE | `/api/playlists/:id/share` | Share/unshare playlist |
| GET | `/api/shared/:code` | View shared playlist (no auth) |
| GET/POST | `/api/playback/state` | Get/set playback state |
| POST | `/api/playback/record` | Record play (scrobble) |
| GET | `/api/history` | Play history |
| GET | `/api/ws` | WebSocket (real-time events) |
| GET | `/api/sync` | WebSocket (multi-room sync) |
| POST | `/api/auth/login` | Authenticate |
| POST | `/api/auth/register` | Register (if enabled) |
| POST | `/api/auth/logout` | Logout |
| GET | `/api/auth/check` | Auth status |
| GET | `/api/docs` | Swagger UI

## Versioned API (v1)

A stable API contract (`/api/v1`) for native clients. See [docs/MICHI_LINK.md](docs/MICHI_LINK.md).

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/v1/server/info` | Server identity (no auth) |
| GET | `/api/v1/status` | Health check |
| GET | `/api/v1/library/stats` | Library statistics |
| GET | `/api/v1/tracks` | List tracks |
| GET | `/api/v1/tracks/:id` | Get track |
| GET | `/api/v1/search?q=` | Search |
| GET | `/api/v1/stream/:id` | Stream audio |

### Status

```bash
curl http://localhost:8096/api/status
```
```json
{ "status": "ok", "service": "michi-micro-server", "version": "0.1.0", "port": 8096 }
```

### Search

```bash
curl "http://localhost:8096/api/search?q=pink+floyd"
```

Returns matching tracks filtered by title, artist, album, album_artist, or format.

### Pagination

```bash
curl "http://localhost:8096/api/tracks?limit=50&offset=100"
```

### Streaming

```bash
# Full file
curl -v http://localhost:8096/api/stream/<UUID>

# Byte range
curl -v -H "Range: bytes=0-1023" http://localhost:8096/api/stream/<UUID>
```

| Code | Condition |
|------|-----------|
| 200 | Full file (no Range header) |
| 206 | Valid Range header |
| 400 | Invalid UUID or malformed Range |
| 403 | File outside library path |
| 404 | Track not found or file missing |
| 416 | Range not satisfiable |

## Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `MICHI_PORT` | `8096` | HTTP server port |
| `MICHI_MUSIC_PATH` | `/music` | Music library path(s), comma-separated |
| `MICHI_CONFIG_PATH` | `/config` | Configuration path |
| `MICHI_CACHE_PATH` | `/cache` | Cache path |
| `MICHI_DATABASE` | `sqlite:///config/michi.db` | SQLite database URL |
| `MICHI_SYNC_PEERS` | (none) | Comma-separated peer addresses for multi-room sync |
| `MICHI_SYNC_NAME` | `default` | Room name for multi-room sync |
| `MICHI_LISTENBRAINZ_TOKEN` | (none) | ListenBrainz API token for scrobbling |
| `MICHI_SCROBBLE_ENABLED` | `false` | Enable/disable ListenBrainz scrobbling |
| `MICHI_AUTH_USERNAME` | (none) | Admin username (auth enabled if set) |
| `MICHI_AUTH_PASSWORD` | (none) | Admin password (auth enabled if set) |
| `MICHI_ALLOW_REGISTRATION` | `false` | Allow new user registration |
| `MICHI_MQTT_HOST` | (none) | MQTT broker host (Home Assistant) |

## CasaOS / ZimaOS

Michi Micro Server is CasaOS/ZimaOS-ready with metadata in `casaos/`. See [docs/CASAOS_ZIMAOS.md](docs/CASAOS_ZIMAOS.md).

## Current Limitations

- No TLS/HTTPS (run behind a reverse proxy for production)
- HLS/DASH adaptive streaming not implemented
- Docker image not yet published to ghcr.io (build locally with `docker build .`)
- Streaming range requests limited to 16MB per chunk
- Mobile app clients not yet released (Michi Music Player planned)
- CI must be green before considering releases valid (see badge above)

## Security Notes for Alpha

- Recommended: run behind Tailscale or a reverse proxy with HTTPS
- Do not expose port 8096 directly to the internet
- Auth is experimental — not a final security layer
- Registration is disabled by default
- CORS is restrictive by default in production (set `MICHI_CORS_ORIGIN` or `MICHI_DEV_MODE=true` for dev)
- Passwords and tokens are never logged

## License

GPL-3.0-only — see [LICENSE](LICENSE).
