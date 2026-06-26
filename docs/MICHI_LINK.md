# Michi Link

## What is Michi Link?

Michi Link is the stable API contract between Michi Micro Server and native clients
(Michi Music Player, Michi Mobile, third-party apps). It defines a versioned REST API
(`/api/v1`) with predictable endpoints, error formats, and server identity.

## Connection Flow

1. Client connects to `http://<server>:8096/api/v1/server/info`
2. Client stores: `server_url`, `server_id`, `version`, `features`
3. Client uses stored `server_id` to detect server changes
4. Client browses library via `/api/v1/tracks` and `/api/v1/search`
5. Client streams via `/api/v1/stream/:id`

## Stable Endpoints (v1)

These endpoints are the official Michi Link contract and will not break
without a major version bump.

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| GET | `/api/v1/server/info` | No | Server identity + capabilities |
| GET | `/api/v1/status` | Yes* | Server health check |
| GET | `/api/v1/library/stats` | Yes* | Library statistics |
| GET | `/api/v1/tracks` | Yes* | List all tracks |
| GET | `/api/v1/tracks/:id` | Yes* | Get track metadata |
| GET | `/api/v1/search?q=` | Yes* | Search library |
| GET | `/api/v1/stream/:id` | Yes* | Stream audio |

\* Auth required only if `MICHI_AUTH_USERNAME` is configured.

## Server Info

```
GET /api/v1/server/info
```

Response:

```json
{
  "name": "Michi Micro Server",
  "server_id": "550e8400-e29b-41d4-a716-446655440000",
  "version": "0.1.0",
  "api_version": "v1",
  "features": {
    "library": true,
    "search": true,
    "streaming": true,
    "web_ui": true,
    "playlists": true,
    "artwork": true,
    "sync": false,
    "transcoding": false,
    "websocket": true
  }
}
```

### Feature Flags

| Feature | Stable | Notes |
|---------|--------|-------|
| `library` | ✅ | Scanner, SQLite, CRUD endpoints |
| `search` | ✅ | SQL LIKE case-insensitive |
| `streaming` | ✅ | Range requests (206), MIME detection |
| `web_ui` | ✅ | Vanilla HTML/CSS/JS, no build step |
| `playlists` | ✅ | CRUD, reorder, import/export, share |
| `artwork` | ✅ | Cover art serving from disk cache |
| `sync` | ❌ | Experimental multi-room sync |
| `transcoding` | ❌ | Optional, requires ffmpeg on server |
| `websocket` | ✅ | Real-time events (scan, library, sync) |

### server_id

The `server_id` is a UUID v4 persisted in `{MICHI_CONFIG_PATH}/server_id`.
Generated once on first startup and stable across restarts.
Clients must store it to detect server identity changes.

## Standard Error Format

All `/api/v1` endpoints return errors as:

```json
{
  "error": {
    "code": "TRACK_NOT_FOUND",
    "message": "Track not found"
  }
}
```

### Error Codes

| Code | Meaning |
|------|---------|
| `TRACK_NOT_FOUND` | Track ID does not exist in library |
| `INVALID_ID` | Malformed UUID |
| `NOT_FOUND` | Resource not found |
| `FORBIDDEN` | Path outside library |
| `BAD_REQUEST` | Invalid request parameters |
| `STREAM_ERROR` | Streaming error |
| `INTERNAL_ERROR` | Unexpected server error |

## Streaming

```
GET /api/v1/stream/:id
GET /api/v1/stream/:id?format=mp3
GET /api/v1/stream/:id?format=ogg
```

Supports Range requests (206 Partial Content) for seeking.
Optional `?format=mp3|ogg` triggers FFmpeg transcoding (experimental —
requires ffmpeg on server, feature flag is `false`).

## What Michi Music Player Should Store

| Key | Value | Source |
|-----|-------|--------|
| `server_url` | `http://<host>:8096` | User input or discovery |
| `server_id` | UUID | `/api/v1/server/info` |
| `server_name` | "Michi Micro Server" | `/api/v1/server/info` |
| `server_version` | "0.1.0" | `/api/v1/server/info` |
| `features` | `{...}` | `/api/v1/server/info` |
| `token` | (future) | `/api/auth/login` |
| `last_connected_at` | ISO 8601 | Local timestamp |

## Experimental Features (not in Michi Link contract)

These features exist in the server but are not part of the stable v1
contract. They may change or be removed.

| Feature | Status |
|---------|--------|
| Auth (login/register) | Functional, future v1 auth endpoint |
| Playlist sharing | Functional via `/api/shared/:code` |
| Scrobbling (ListenBrainz) | Functional, `features` flag pending |
| Multi-room sync | Functional via WebSocket, flag is `false` |
| Transcoding | Functional if ffmpeg installed, flag is `false` |
| PWA / offline | Functional, not exposed via v1 |
| M3U import/export | Functional, not exposed via v1 |

## Future (planned, not implemented)

- **Pairing/Token**: Automatic device pairing with secure token exchange
- **WebSocket Events**: `/api/v1/ws` for real-time changes
- **Playlist Sync**: Bidirectional playlist synchronization
- **Artwork Bulk**: Batch artwork endpoints for initial sync
- **Offline Sync**: Partial library download for offline playback
- **Playback Control**: Remote playback via `/api/v1/playback/*`
