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
│   ├── michi-homeassistant/ # HA integration (future)
│   ├── michi-sync/          # Sync (future)
│   └── michi-multiroom/     # Multiroom (future)
├── docs/                    # Documentation
├── Dockerfile
├── docker-compose.yml
└── casaos/                  # CasaOS support
```

## Web UI

Open http://localhost:8096 in your browser for the built-in web interface:

- Server status and library statistics
- Scan your music library
- Browse tracks (title, artist, album, format, duration)
- Play tracks directly in the browser via `<audio>` element

The Web UI is served directly by the server, no build step or frontend framework needed.

## Quick Start

### Local Development

```bash
# Prerequisites: Rust 1.77+, FFmpeg, SQLite dev libraries

# Clone and run
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
# Run all tests (73 tests across all crates)
cargo test
```

### Testing the server

```bash
# Health check
curl http://localhost:8096/api/status

# Scan music library
curl -X POST http://localhost:8096/api/library/scan

# List tracks
curl http://localhost:8096/api/tracks

# Get a single track
curl http://localhost:8096/api/tracks/<UUID>

# Update a track
curl -X PUT http://localhost:8096/api/tracks/<UUID> \
  -H "Content-Type: application/json" \
  -d '{"title": "New Title", "artist": "New Artist"}'

# Delete a track
curl -X DELETE http://localhost:8096/api/tracks/<UUID>

# Library statistics
curl http://localhost:8096/api/library/stats

# Purge all tracks
curl -X DELETE http://localhost:8096/api/library/tracks
```

### Docker Compose (Recommended)

```bash
# Create directories
mkdir -p data/config data/cache music

# Build and start
docker compose up -d

# View logs
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

## Endpoints

| Method | Path | Description |
|--------|------|-------------|
| GET | `/` | Server status page |
| GET | `/api/status` | JSON health check |
| POST | `/api/library/scan` | Scan music library and save tracks |
| DELETE | `/api/library/tracks` | Delete all tracks from the library |
| GET | `/api/tracks` | List all tracks |
| GET | `/api/tracks/:id` | Get a single track by UUID |
| PUT | `/api/tracks/:id` | Update track metadata (partial) |
| DELETE | `/api/tracks/:id` | Delete a track by UUID |
| GET | `/api/library/stats` | Library statistics |
| GET | `/api/stream/:id` | Stream audio file with Range Request support |

### Health Check

```bash
curl http://localhost:8096/api/status
```

Response:
```json
{
  "status": "ok",
  "service": "michi-micro-server",
  "version": "0.1.0",
  "port": 8096
}
```

### Scan Library

```bash
curl -X POST http://localhost:8096/api/library/scan
```

Response:
```json
{
  "status": "ok",
  "scanned": 120,
  "saved": 120
}
```

### List Tracks

```bash
curl http://localhost:8096/api/tracks
```

### Library Stats

```bash
curl http://localhost:8096/api/library/stats
```

Response:
```json
{
  "tracks": 120,
  "albums": 15,
  "artists": 42
}
```

### Streaming

Stream audio files with HTTP Range Request support for seeking:

```bash
# Get the full file
curl -v http://localhost:8096/api/stream/<UUID>

# Request a specific byte range (used by clients for seeking)
curl -v -H "Range: bytes=0-1023" http://localhost:8096/api/stream/<UUID>
```

The streaming endpoint:
- Returns `200 OK` with the full file streamed asynchronously (no full load in RAM)
- Returns `206 Partial Content` with the requested byte range
- Returns `416 Range Not Satisfiable` for invalid ranges
- Returns `403 Forbidden` for files outside the configured music path
- Detects MIME types based on file extension
- Sets `Accept-Ranges: bytes` header

## Configuration

All configuration is done via environment variables:

| Variable | Default | Description |
|----------|---------|-------------|
| `MICHI_PORT` | `8096` | HTTP server port |
| `MICHI_MUSIC_PATH` | `/music` | Music library path |
| `MICHI_CONFIG_PATH` | `/config` | Configuration path |
| `MICHI_CACHE_PATH` | `/cache` | Cache path |
| `MICHI_DATABASE` | `sqlite:///config/michi.db` | SQLite database URL (created automatically if missing, no `?mode=rwc` needed) |

## Compatibility

### CasaOS / ZimaOS

Michi Micro Server is designed for easy deployment on CasaOS and ZimaOS via Docker. A CasaOS-compatible `docker-compose.yml` is provided in the `casaos/` directory.

### Home Assistant (Future)

Integration via MQTT Discovery — see [docs/HOME_ASSISTANT.md](docs/HOME_ASSISTANT.md).

### Michi Music Player / Michi Mobile (Future)

Shared data models ensure seamless synchronization — see [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## License

GNU General Public License v3.0 — see [LICENSE](LICENSE).

---

Built with ❤️ using Rust.
