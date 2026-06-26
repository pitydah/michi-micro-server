# Michi Micro Server v0.1.0-alpha

First alpha release. Backend stable for integration with Michi Music Player.

## Requirements

- Rust 1.77+
- SQLite dev libraries (`libsqlite3-dev`)
- Docker (optional, for container deployment)

## Quick Install

### Local

```bash
git clone https://github.com/pitydah/michi-micro-server.git
cd michi-micro-server
cargo build --release -p michi-server
MICHI_MUSIC_PATH=./music cargo run -p michi-server
```

### Docker (build from source)

```bash
docker compose -f docker-compose.yml up -d
```

### Docker (dev mode)

```bash
docker compose -f docker-compose.dev.yml up -d
```

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `MICHI_PORT` | 8096 | HTTP port |
| `MICHI_MUSIC_PATH` | /music | Music path(s), comma-separated |
| `MICHI_CONFIG_PATH` | /config | Config directory |
| `MICHI_CACHE_PATH` | /cache | Cache directory |
| `MICHI_DATABASE` | sqlite:///config/michi.db | Database URL |
| `MICHI_DEV_MODE` | false | Enables permissive CORS for dev |
| `MICHI_CORS_ORIGIN` | (none) | Set CORS origin for production |

## Stable Features (v1)

| Feature | Endpoint |
|---------|----------|
| Server health | `GET /api/status` |
| Server info | `GET /api/v1/server/info` |
| Library scan | `POST /api/library/scan` |
| List tracks | `GET /api/tracks`, `GET /api/v1/tracks` |
| Search | `GET /api/search?q=`, `GET /api/v1/search?q=` |
| Stream | `GET /api/stream/:id`, `GET /api/v1/stream/:id` |
| Web UI | `GET /` |
| Swagger | `GET /api/docs` |

## Stable Michi Link v1 Endpoints

| Method | Path | Auth |
|--------|------|------|
| GET | `/api/v1/server/info` | No |
| GET | `/api/v1/status` | Yes* |
| GET | `/api/v1/library/stats` | Yes* |
| GET | `/api/v1/tracks` | Yes* |
| GET | `/api/v1/tracks/:id` | Yes* |
| GET | `/api/v1/search?q=` | Yes* |
| GET | `/api/v1/stream/:id` | Yes* |

\* Only if `MICHI_AUTH_USERNAME` is set.

## `/api/v1/server/info` Response

```json
{
  "name": "Michi Micro Server",
  "server_id": "<uuid>",
  "version": "0.1.0",
  "api_version": "v1",
  "features": {
    "library": true,
    "search": true,
    "streaming": true,
    "web_ui": true,
    "playlists": false,
    "artwork": false,
    "sync": false,
    "transcoding": false,
    "websocket": false
  }
}
```

## Experimental Features

| Feature | Status |
|---------|--------|
| Auth (login/register) | Functional, not in v1 contract |
| Playlists (CRUD, share, M3U) | Functional, not in v1 contract |
| Artwork (cover art) | Functional, not in v1 contract |
| WebSocket events | Functional, not in v1 contract |
| Scrobbling (ListenBrainz) | Functional, requires API token |
| Transcoding (ffmpeg) | Functional, requires external ffmpeg |
| PWA / offline mode | Functional, not in v1 contract |
| TUI client | Functional, external binary (`michi-tui`) |
| Home Assistant | Functional, requires MQTT broker |
| Multi-room sync | Functional, requires peer config |

## Known Limitations

- No HTTPS/TLS — run behind a reverse proxy for production
- CORS is restrictive by default in production — set `MICHI_CORS_ORIGIN` or use `MICHI_DEV_MODE=true`
- HLS/DASH adaptive streaming not implemented
- Docker image not published to ghcr.io yet (builds locally)
- Auth is experimental — not recommended for public exposure
- Max range size for streaming: 16MB
- Transcoding depends on external ffmpeg binary
- Michi Music Player must validate `api_version == "v1"` before connecting

## Test Commands

```bash
cargo fmt --check
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
docker build .
```

## Next Steps

- Publish Docker image to ghcr.io
- Start Michi Music Player integration via Michi Link v1
- Stabilize auth for production use
- Add playlist/artwork to v1 contract
