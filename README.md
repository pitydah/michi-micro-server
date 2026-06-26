# Michi Micro Server

> Lightweight, robust, and efficient home music server written in Rust.

Michi Micro Server centralizes your local music library, reads advanced metadata, manages playlists, and serves music over your local network or Tailscale. It is designed to integrate with [Michi Music Player](https://github.com/pitydah/michi-music-player), Michi Mobile, Home Assistant, and CasaOS/ZimaOS.

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
| Audio | Built-in streaming (no FFmpeg) |
| Container | Docker + Compose |

## Project Structure

```
michi-micro-server/
├── apps/michi-server/       # Main binary
├── crates/
│   ├── michi-core/          # Shared models
│   ├── michi-api/           # HTTP routes
│   ├── michi-config/        # Configuration
│   ├── michi-db/            # Database layer
│   ├── michi-metadata/      # Audio tag reading
│   ├── michi-scanner/       # Library scanner
│   ├── michi-streaming/     # Audio streaming with Range Requests
│   ├── michi-homeassistant/ # HA integration (inactive)
│   ├── michi-sync/          # Sync (inactive)
│   └── michi-multiroom/     # Multiroom (inactive)
├── docs/                    # Documentation
├── Dockerfile
├── docker-compose.yml
└── casaos/                  # CasaOS support
```

## Web UI

Open http://localhost:8096 in your browser for the built-in web interface.

**Features:**
- Server status, version, port, and library statistics
- One-click library scan
- Clear library with confirmation dialog
- Track listing with title, artist, album, format, duration
- Search by title, artist, album, album_artist, or format (case-insensitive)
- In-browser audio playback with `<audio>` element
- Now playing info (title, artist, album, format, duration)
- Auto-advance to next track on completion
- Stop playback
- Track counter
- Responsive layout for mobile
- No build step or frontend framework required

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
# Run all tests (79 tests across all crates)
cargo test
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
| GET | `/api/status` | Server health check |
| POST | `/api/library/scan` | Scan music library |
| GET | `/api/library/stats` | Library statistics |
| DELETE | `/api/library/tracks` | Delete all tracks |
| GET | `/api/tracks` | List tracks (supports `?limit=&offset=`) |
| GET | `/api/tracks/:id` | Get track by UUID |
| PUT | `/api/tracks/:id` | Update track metadata |
| DELETE | `/api/tracks/:id` | Delete track by UUID |
| GET | `/api/search?q=` | Search tracks |
| GET | `/api/stream/:id` | Stream audio file |

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
| `MICHI_MUSIC_PATH` | `/music` | Music library path |
| `MICHI_CONFIG_PATH` | `/config` | Configuration path |
| `MICHI_CACHE_PATH` | `/cache` | Cache path |
| `MICHI_DATABASE` | `sqlite:///config/michi.db` | SQLite database URL |

## CasaOS / ZimaOS

Michi Micro Server is CasaOS/ZimaOS-ready with metadata in `casaos/`. See [docs/CASAOS_ZIMAOS.md](docs/CASAOS_ZIMAOS.md).

## Current Limitations

- No authentication (run on trusted local networks or behind Tailscale)
- No FFmpeg transcoding yet (native format only; browser compatibility varies)
- No Home Assistant integration (planned for Phase 6)
- No multiroom/Snapcast support (planned for Phase 8)
- No mobile sync (planned for Phase 7)

## License

GPL-3.0-only — see [LICENSE](LICENSE).
