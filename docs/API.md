# API Documentation

## Interactive Docs

Open `http://<server>:8096/api/docs` for the full Swagger UI with all endpoints.

## Versioned API (v1)

A stable API at `/api/v1` for native clients.
**Michi Music Player must use `/api/v1`, not the legacy `/api` endpoints.**
See [MICHI_LINK.md](MICHI_LINK.md) for the contract and
[CLIENT_INTEGRATION_SPEC.md](CLIENT_INTEGRATION_SPEC.md) for client developers.

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| GET | `/api/v1/server/info` | No | Server identity + features |
| GET | `/api/v1/status` | Yes* | Health check |
| GET | `/api/v1/library/stats` | Yes* | Library statistics |
| GET | `/api/v1/tracks` | Yes* | List tracks |
| GET | `/api/v1/tracks/:id` | Yes* | Get track |
| GET | `/api/v1/search?q=` | Yes* | Search |
| GET | `/api/v1/stream/:id` | Yes* | Stream audio |

V1 error format:
```json
{
  "error": {
    "code": "TRACK_NOT_FOUND",
    "message": "Track not found"
  }
}
```

## Base URL

```
http://<server>:8096
```

## Endpoints Summary

### Status
| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/status` | Server health check |

### Library
| Method | Path | Description |
|--------|------|-------------|
| POST | `/api/library/scan` | Scan music directories |
| GET | `/api/library/stats` | Library statistics |
| DELETE | `/api/library/tracks` | Delete all tracks |

### Tracks
| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/tracks` | List all tracks |
| GET | `/api/search?q=` | Search tracks |
| GET | `/api/tracks/:id` | Get track |
| PUT | `/api/tracks/:id` | Update metadata |
| DELETE | `/api/tracks/:id` | Delete track |

### Albums / Artists
| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/albums` | List albums |
| GET | `/api/albums/:album` | Album tracks |
| GET | `/api/artists` | List artists |
| GET | `/api/artists/:artist` | Artist tracks |

### Streaming
| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/stream/:id` | Stream audio (`?format=mp3\|ogg` for transcoding) |
| GET | `/api/artwork/:id` | Cover art image |

### Playlists
| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/playlists` | List playlists |
| POST | `/api/playlists` | Create playlist |
| GET/DELETE | `/api/playlists/:id` | Get/delete playlist |
| GET | `/api/playlists/:id/tracks` | Playlist tracks |
| POST/DELETE | `/api/playlists/:pid/tracks/:tid` | Add/remove track |
| PUT | `/api/playlists/:id/reorder` | Reorder tracks |
| GET | `/api/playlists/:id/export` | Export M3U |
| POST | `/api/playlists/import` | Import M3U |
| GET/POST/DELETE | `/api/playlists/:id/share` | Share/unshare |
| GET | `/api/shared/:code` | View shared playlist |

### Playback
| Method | Path | Description |
|--------|------|-------------|
| GET/POST | `/api/playback/state` | Get/set playback state |
| POST | `/api/playback/record` | Record play + scrobble |
| GET | `/api/history` | Play history |

### Auth
| Method | Path | Description |
|--------|------|-------------|
| POST | `/api/auth/login` | Login |
| POST | `/api/auth/register` | Register |
| POST | `/api/auth/logout` | Logout |
| GET | `/api/auth/check` | Check auth status |

### WebSocket
| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/ws` | Real-time events (scan, library, sync) |
| GET | `/api/sync` | Multi-room sync bridge |

## Response Format

All API responses use JSON. Errors follow:

```json
{
  "status": "error",
  "message": "description of the error"
}
```

## Config Reference

| Variable | Default | Description |
|----------|---------|-------------|
| `MICHI_PORT` | `8096` | HTTP port |
| `MICHI_MUSIC_PATH` | `/music` | Music path(s), comma-separated |
| `MICHI_CONFIG_PATH` | `/config` | Config dir |
| `MICHI_CACHE_PATH` | `/cache` | Cache dir |
| `MICHI_DATABASE` | `sqlite:///config/michi.db` | DB URL |
| `MICHI_SYNC_PEERS` | (none) | Multi-room peers |
| `MICHI_SYNC_NAME` | `default` | Room name |
| `MICHI_LISTENBRAINZ_TOKEN` | (none) | ListenBrainz API token |
| `MICHI_SCROBBLE_ENABLED` | `false` | Enable scrobbling |
| `MICHI_AUTH_USERNAME` | (none) | Admin username (enables auth if set) |
| `MICHI_AUTH_PASSWORD` | (none) | Admin password |
| `MICHI_ALLOW_REGISTRATION` | `false` | Allow user registration |
| `MICHI_MQTT_HOST` | (none) | MQTT broker (enables HA) |
