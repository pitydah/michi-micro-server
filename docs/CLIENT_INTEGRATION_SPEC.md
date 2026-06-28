# Michi Music Player — Native Client Specification

How Michi Music Player must consume Michi Link v1 to connect natively
to Michi Micro Server.

## Objective

Michi Music Player connects to a Michi Micro Server instance via the
stable `/api/v1` contract (Michi Link). This document defines the exact
behavior, data model, error handling, and constraints the client must follow.

## Persistent Data

The client must store per-server:

| Key | Type | Source |
|-----|------|--------|
| `server_url` | String | User input (e.g. `http://192.168.1.50:8096`) |
| `server_id` | UUID | `GET /api/v1/server/info` |
| `server_name` | String | `GET /api/v1/server/info` |
| `api_version` | String | `GET /api/v1/server/info` — must be `v1` |
| `features` | Object | `GET /api/v1/server/info` |
| `last_seen` | ISO 8601 | Local timestamp on successful response |
| `token` | String (future) | `POST /api/auth/login` |
| `connection_status` | Enum | `online`, `offline`, `timeout`, `error` |

## Suggested Class: MichiServerClient

```rust
struct MichiServerClient {
    server_url: String,
    server_id: Option<Uuid>,
    server_name: String,
    api_version: String,
    features: ServerFeatures,
    token: Option<String>,
    last_seen: Option<DateTime<Utc>>,
    status: ConnectionStatus,
    timeout: Duration,
}

enum ConnectionStatus {
    Online,
    Offline,
    Timeout,
    Error(String),
}
```

### Required Methods

| Method | Endpoint | Return |
|--------|----------|--------|
| `get_server_info()` | `GET /api/v1/server/info` | `ServerInfo` |
| `get_status()` | `GET /api/v1/status` | `Status` |
| `get_library_stats()` | `GET /api/v1/library/stats` | `LibraryStats` |
| `list_tracks()` | `GET /api/v1/tracks` | `Vec<Track>` |
| `search_tracks(query)` | `GET /api/v1/search?q=` | `Vec<Track>` |
| `get_track(id)` | `GET /api/v1/tracks/{id}` | `Track` |
| `stream_url(id)` | Internal | `String` (url to stream) |

### Required Behaviors

- `set_timeout(secs)` — default 10 seconds, configurable
- `handle_v1_error(response)` — parse `{ "error": { "code", "message" } }`
- Retry: max 3 attempts on timeout or 5xx, exponential backoff 1s/2s/4s
- `online/offline` state: if request fails, mark `Offline`; poll with retry
- Never block the UI thread
- `guardar server_id`: on first connect, persist `server_id`. If it changes
  on same URL, alert user: "This appears to be a different server"

## Connection Flow

```
1. User enters IP or URL
2. POST /api/auth/login (if server requires auth — check /api/auth/check)
3. GET /api/v1/server/info
4. Validate api_version == "v1"
5. Store server_id, name, features
6. GET /api/v1/status
7. If features.library && features.search && features.streaming:
   enable remote library browsing
8. User searches: GET /api/v1/search?q=...
9. User plays: stream_url(track_id) -> GET /api/v1/stream/{id}
10. On disconnect: mark server offline, do NOT freeze UI
11. On reconnect: re-validate server_id before trusting cached data
```

## Mandatory Rules

- Must use `/api/v1` endpoints only for native integration.
- Must NOT depend on legacy `/api/...` endpoints.
- Must NOT assume transcoding is available (feature flag is `false`).
- Must NOT assume sync is available (feature flag is `false`).
- Must NOT block UI during network calls — all async.
- Must use timeout on every request.
- Must handle v1 error format.
- Must allow reconnection.
- Must persist `server_id`, not just IP.
- Must warn user if `server_id` changes on same URL.

## /api/v1/server/info

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
    "websocket": true
  }
}
```

Only `library`, `search`, `streaming`, `web_ui` are guaranteed stable in v1.
All other flags are `false` — do not build features that depend on them.

## Error Handling

All `/api/v1` errors follow this format:

```json
{
  "error": {
    "code": "TRACK_NOT_FOUND",
    "message": "Track not found"
  }
}
```

### Mapping

| Code | HTTP | Client Action |
|------|------|---------------|
| `TRACK_NOT_FOUND` | 404 | Remove track from local cache, show toast |
| `INVALID_ID` | 400 | Log warning, check URL construction |
| `FILE_NOT_FOUND` | 404 | Show "file missing" to user |
| `FORBIDDEN` | 403 | Log security warning, do not retry |
| `BAD_REQUEST` | 400 | Log error, check request parameters |
| `INTERNAL_ERROR` | 500 | Retry with backoff (max 3) |
| `RANGE_NOT_SATISFIABLE` | 416 | Client seeking error, reset stream position |

### Client Error Handler (pseudo-code)

```rust
fn handle_v1_error(response: Response) -> Result<T, ClientError> {
    let status = response.status();
    let body: V1Error = response.json()?;
    match body.error.code.as_str() {
        "TRACK_NOT_FOUND" => Err(ClientError::TrackNotFound(body.error.message)),
        "FORBIDDEN" => Err(ClientError::Forbidden),
        "INTERNAL_ERROR" => Err(ClientError::ServerError(body.error.message)),
        _ => Err(ClientError::Unknown(body.error.message)),
    }
}
```

## Streaming

```
GET /api/v1/stream/{id}
```

Returns raw audio bytes. Client should:
- Support byte Range requests for seeking (206 Partial Content)
- Use Content-Type from the `/api/v1/tracks/{id}` metadata response
- `?format=mp3|ogg` is experimental — do NOT use in production client

## Future Phases (not in v1 contract)

| Feature | Status |
|---------|--------|
| Pairing/token | Not implemented |
| Remote playlists | Feature flag is `false` |
| Remote artwork | Feature flag is `false` |
| WebSocket events | Feature flag is `false` |
| Sync offline | Not implemented |
| Mobile transcoding | Experimental, depends on external ffmpeg |
