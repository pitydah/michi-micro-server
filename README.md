# Michi Micro Server

[![CI](https://github.com/pitydah/michi-micro-server/actions/workflows/ci.yml/badge.svg)](https://github.com/pitydah/michi-micro-server/actions/workflows/ci.yml)
[![v0.2.0](https://img.shields.io/badge/version-0.2.0-blue)](https://github.com/pitydah/michi-micro-server/releases)

> Lightweight, robust home music server written in Rust.

Michi Micro Server centralizes your local music library, reads advanced metadata,
manages playlists, serves music over your local network, and integrates with
Michi Music Player, Michi Mobile, Home Assistant, and CasaOS/ZimaOS.

## Features

- **Library Management** — Scan, index, search, and organize music files
- **Streaming** — HTTP Range requests, transcoding (MP3/Ogg/HLS), gapless
- **Playlists** — CRUD, smart playlists (8 rules), M3U export/import, sync
- **Playback Chains** — Route audio to multiple receivers with per-device volume
- **Play History** — Paginated, stats (today/week/month/total), export
- **Search** — Full-text with field filters (`artist:`, `album:`, `year:>`, `format:`, `rating:>=`)
- **Artist/Album Insights** — Lossless count, format breakdown, health score
- **Michi Link** — Native pairing protocol, token auth, feature negotiation
- **Receivers** — mDNS discovery, pairing, session management, multi-room groups
- **Sync** — WebSocket state sync, handoff (takeover), cross-device queue
- **Upload** — Resumable chunked upload with SHA-256 dedup
- **Webhook** — Post-sync notifications, configurable URL
- **Backup/Snapshot** — Full library export, integrity verification
- **OpenSubsonic** — Compatible API layer
- **Security** — Rate limiting, security headers, bearer token auth
- **Web UI** — Premium dark theme, responsive, cache busting

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
| Streaming | HTTP Range + FFmpeg transcoding |
| Container | Docker multi-stage + Compose |

## Project Structure

```
michi-micro-server/
├── apps/michi-server/       # Main binary
├── crates/
│   ├── michi-core/          # Shared models (Track, Playlist, Chain, etc.)
│   ├── michi-api/           # HTTP routes, WebSocket, auth, Web UI static
│   ├── michi-config/        # Configuration from env vars
│   ├── michi-db/            # Database layer + 26 migrations
│   ├── michi-metadata/      # Audio tag reading (Lofty)
│   ├── michi-scanner/       # Library scanner
│   ├── michi-streaming/     # Audio streaming + transcoding
│   ├── michi-sync/          # Sync protocol, handoff, upload engine
│   ├── michi-link/          # Michi Link protocol (pairing, permissions)
│   ├── michi-receivers/     # Receiver client + session manager
│   ├── michi-rooms/         # Snapcast multi-room abstraction
│   ├── michi-opensubsonic/  # OpenSubsonic API compatibility
│   ├── michi-security/      # Rate limiting, security middleware
│   ├── michi-m3u/           # M3U playlist parsing
│   ├── michi-homeassistant/ # Home Assistant MQTT integration
│   ├── michi-client/        # HTTP client for external consumers
│   └── michi-tui/           # Terminal UI
├── docs/                    # Architecture, API, deployment docs
├── deploy/                  # systemd service, Debian package
├── scripts/                 # Receiver simulator helpers
├── tests/                   # Integration tests
├── Dockerfile
├── docker-compose.yml
└── casaos/                  # CasaOS app metadata
```

## Quick Start

```bash
# Clone and build
git clone https://github.com/pitydah/michi-micro-server.git
cd michi-micro-server
cargo build --release --package michi-server

# Run with defaults
MICHI_PORT=8096 MICHI_MUSIC_PATH=/path/to/music ./target/release/michi-server
```

### Docker

```bash
docker build -t michi-server .
docker run -d \
  --name michi \
  -p 8096:8096 \
  -v /path/to/music:/music:ro \
  -v ./config:/config \
  -v ./cache:/cache \
  michi-server
```

### Docker Compose

```bash
docker compose up -d
# Server available at http://localhost:8096
```

## Configuration

All configuration via environment variables:

| Variable | Default | Description |
|----------|---------|-------------|
| `MICHI_PORT` | `8096` | HTTP port |
| `MICHI_MUSIC_PATH` | `/music` | Comma-separated library paths |
| `MICHI_CONFIG_PATH` | `/config` | Config directory |
| `MICHI_CACHE_PATH` | `/cache` | Cache directory |
| `MICHI_DATABASE` | `sqlite:///config/michi.db` | SQLite database URL |
| `MICHI_SYNC_NAME` | hostname | Sync peer identifier |
| `MICHI_CORS_ORIGIN` | — | CORS origin (restrictive by default) |
| `MICHI_MQTT_HOST` | — | Home Assistant MQTT broker |
| `MICHI_LASTFM_TOKEN` | — | Last.fm API token |
| `MICHI_LISTENBRAINZ_TOKEN` | — | ListenBrainz API token |

## API Endpoints

See [docs/API.md](docs/API.md) for the complete API reference.

### Web UI

Open `http://localhost:8096` in your browser to access the premium Web UI:
- **Dashboard** — Library stats, playback status, health, recent tracks
- **Library** — Browse tracks with search, sort, format badges
- **Scan** — Start library scan, view progress and results
- **Playlists** — Browse, create smart playlists, export M3U
- **History** — Track play history with stats and export
- **Chains** — Create multi-receiver playback chains with per-device volume
- **Settings** — Upload files, handoff, receiver discovery, webhooks, backup

### Key API Routes

```
GET  /api/status                    Server health
GET  /api/v1/server/info            Server info + features
GET  /api/v1/library/stats          Library statistics
GET  /api/v1/home/dashboard         Dashboard snapshot
GET  /api/v1/tracks                 List tracks (paginated)
GET  /api/v1/search/advanced?q=...  Advanced search
GET  /api/v1/playlists              List playlists
GET  /api/v1/chains                 List playback chains
GET  /api/v1/history                Play history (paginated)
POST /api/v1/player/handoff         Transfer playback to server
POST /api/v1/sync/upload/file       Upload file (base64)
POST /api/v1/devices/discover       mDNS receiver discovery
POST /api/v1/webhook/test           Test webhook
GET  /api/v1/library/health         Library health report
```

## Architecture

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for detailed architecture.

## License

GPL-3.0-only
