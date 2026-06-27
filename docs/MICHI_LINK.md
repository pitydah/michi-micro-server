# Michi Link

The stable API contract between Michi Micro Server and native clients
(Michi Music Player, Michi Mobile, third-party apps).

**Michi Music Player must use `/api/v1` endpoints exclusively for native integration.
Legacy `/api/...` endpoints exist but are not part of the v1 contract.**

## Stable (v1 contract)

These endpoints are the official Michi Link contract. They will not break
without a major version bump.

| Method | Path | Auth | Purpose |
|--------|------|------|---------|
| GET | `/api/v1/server/info` | No | Server discovery and capabilities |
| GET | `/api/v1/status` | Yes* | Health check |
| GET | `/api/v1/library/stats` | Yes* | Library statistics |
| GET | `/api/v1/tracks` | Yes* | List all tracks |
| GET | `/api/v1/tracks/{id}` | Yes* | Get track metadata |
| GET | `/api/v1/search?q=` | Yes* | Search library |
| GET | `/api/v1/stream/{id}` | Yes* | Stream audio |

\* Auth required only if `MICHI_AUTH_USERNAME` is configured.

## Server Info

```
GET /api/v1/server/info
```

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
    "websocket": false
  }
}
```

### Feature Flags (v1)

| Flag | Stable | Notes |
|------|--------|-------|
| `library` | true | Scanner, SQLite, CRUD — tested, stable |
| `search` | true | SQL LIKE case-insensitive — tested, stable |
| `streaming` | true | Range requests (206), MIME detection — tested, stable |
| `web_ui` | true | Vanilla HTML/CSS/JS, no build step — stable |
| `playlists` | true | CRUD, reorder, share, M3U — stable in v1.1 |
| `artwork` | true | Cover art serving from disk cache — stable in v1.1 |
| `sync` | false | Experimental multi-room sync. Not part of v1 contract. |
| `transcoding` | false | Optional, requires external ffmpeg. Not guaranteed. |
| `websocket` | false | Functional but experimental. Will become true when v1 WS spec exists. |

## server_id

UUID v4 persisted in `{MICHI_CONFIG_PATH}/server_id`.
Generated once on first startup, stable across restarts.
Clients must store it to detect server identity changes.

## Error Format

All `/api/v1` errors follow this structure:

```json
{
  "error": {
    "code": "TRACK_NOT_FOUND",
    "message": "Track not found"
  }
}
```

| Code | HTTP | Meaning |
|------|------|---------|
| `TRACK_NOT_FOUND` | 404 | Track ID does not exist |
| `INVALID_ID` | 400 | Malformed UUID |
| `NOT_FOUND` | 404 | Resource not found |
| `FORBIDDEN` | 403 | Path outside library |
| `BAD_REQUEST` | 400 | Invalid parameters |
| `STREAM_ERROR` | varies | Streaming error |
| `INTERNAL_ERROR` | 500 | Server error |

## Streaming

```
GET /api/v1/stream/{id}
GET /api/v1/stream/{id}?format=mp3
GET /api/v1/stream/{id}?format=ogg
```

Range requests (206 Partial Content) supported.
`?format=mp3|ogg` triggers FFmpeg transcoding (experimental — see feature flag).

## Client Data Model

```rust
struct ServerConnection {
    server_url: String,       // "http://192.168.1.50:8096"
    server_id: Uuid,          // from /api/v1/server/info
    server_name: String,      // "Michi Micro Server"
    api_version: String,      // "v1"
    features: ServerFeatures, // feature flags
    token: Option<String>,    // future: auth token
    last_connected: DateTime, // ISO 8601
}

struct ServerFeatures {
    library: bool,
    search: bool,
    streaming: bool,
    web_ui: bool,
    playlists: bool,
    artwork: bool,
    sync: bool,
    transcoding: bool,
    websocket: bool,
}
```

## Experimental (not part of v1 contract)

Functional in the server but not yet in the stable Michi Link contract.
May change without a major version bump.

| Feature | Endpoint(s) | Notes |
|---------|-------------|-------|
| Auth | `/api/auth/*` | Login/register/logout. Will be integrated into v1 later. |
| Playlists | `/api/playlists/*` | CRUD, reorder, M3U, share. Working but needs v1 spec. |
| Artwork | `/api/artwork/:id` | Cover art from disk cache. Working but needs v1 spec. |
| WebSocket | `/api/ws` | Real-time events. Working but needs v1 spec. |
| Scrobbling | `/api/playback/record` | ListenBrainz integration. Working but needs v1 spec. |
| History | `/api/history` | Play history tracking. Working but needs v1 spec. |
| OpenAPI | `/api/docs` | Swagger UI. Available but not part of v1 contract. |
| PWA | `/manifest.json`, `/sw.js` | Offline support. Available but not part of v1 contract. |

## Future (planned, not implemented or not stable)

| Feature | Status |
|---------|--------|
| Pairing/token | Native device pairing. Not implemented. |
| Playback control | Remote play/pause/seek via v1. Not specified. |
| Sync offline | Partial library download. Not implemented. |
| Playlist sync | Bidirectional playlist sync. Not implemented. |
| Home Assistant | MQTT integration exists but experimental. |
| Multi-room | WebSocket sync exists but experimental. |
| Transcoding mobile | ffmpeg-based. Exists but depends on external binary. |
