# Client Integration Specification

For developers building a client for Michi Micro Server (e.g., Michi Music Player).

## Quick Start

```bash
# 1. Discover the server
curl http://<host>:8096/api/v1/server/info

# 2. Browse library
curl http://<host>:8096/api/v1/tracks
curl "http://<host>:8096/api/v1/search?q=artist+name"

# 3. Stream audio
curl http://<host>:8096/api/v1/stream/<track_id> --output song.mp3
```

## 1. Server Discovery

### GET /api/v1/server/info

No authentication required. This is the first endpoint every client must call.

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

### Client MUST store these values

| Field | Type | Purpose |
|-------|------|---------|
| `server_id` | UUID | Detect server identity changes |
| `server_url` | String | User-provided host:port |
| `api_version` | String | Must be `v1` |
| `features` | Object | Enable/disable UI features |

### Detecting Server Changes

```
IF stored_server_id != response.server_id:
    WARN user: "This appears to be a different server"
    ASK: "Continue? [yes/no]"
```

## 2. Authentication

If the server has auth enabled (`MICHI_AUTH_USERNAME` set), all endpoints
except `/api/v1/server/info` require an `Authorization` header.

### Login

```http
POST /api/auth/login
Content-Type: application/json

{"username": "user", "password": "pass"}
```

Response:
```json
{
  "token": "abcdef12-3456-...",
  "id": "user-uuid",
  "username": "user",
  "is_admin": false
}
```

### Using the token

```http
GET /api/v1/tracks
Authorization: Bearer abcdef12-3456-...
```

### Check auth status

```http
GET /api/auth/check
Authorization: Bearer abcdef12-3456-...
```

Response:
```json
{
  "enabled": true,
  "authenticated": true,
  "id": "user-uuid",
  "username": "user",
  "is_admin": false
}
```

## 3. Browse Library

### GET /api/v1/tracks

Returns all tracks. Response is an array of:

```json
{
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "title": "Bohemian Rhapsody",
  "artist": "Queen",
  "album": "A Night at the Opera",
  "album_artist": "Queen",
  "duration_ms": 355000,
  "file_path": "/music/queen/bohemian.flac",
  "format": "flac",
  "sample_rate": 44100,
  "bit_depth": 16,
  "channels": 2,
  "artwork_id": null,
  "created_at": "2025-01-01T00:00:00Z",
  "updated_at": "2025-01-01T00:00:00Z"
}
```

### GET /api/v1/tracks/{id}

Single track metadata. Uses the same Track schema.

### GET /api/v1/search?q=query

Case-insensitive search across title, artist, album. Returns array of Track.

### GET /api/v1/library/stats

```json
{
  "tracks": 1200,
  "albums": 85,
  "artists": 42
}
```

## 4. Streaming

### GET /api/v1/stream/{id}

Returns raw audio bytes with appropriate Content-Type header.

The client should:
1. Send a HEAD request first to get Content-Type and Content-Length
2. Support byte Range requests for seeking (206 Partial Content)
3. Handle stream errors in v1 format

### Example (simple)

```rust
// Rust pseudo-code for streaming
let url = format!("{}/api/v1/stream/{}", server_url, track_id);
let response = client.get(&url)
    .header("Authorization", format!("Bearer {}", token))
    .send().await?;
let content_type = response.headers().get("content-type");
```

## 5. Error Handling

All `/api/v1` errors follow this format:

```json
{
  "error": {
    "code": "TRACK_NOT_FOUND",
    "message": "Track not found"
  }
}
```

### How to handle

```rust
// Rust pseudo-code
match error.code.as_str() {
    "TRACK_NOT_FOUND" => // Show: "This track is no longer available"
    "INVALID_ID" => // Malformed request, check your URL
    "FORBIDDEN" => // Path security violation
    "INTERNAL_ERROR" => // Server error, retry later
    _ => // Show error.message to user
}
```

HTTP status codes:
- 200 = Success
- 400 = Bad request or invalid UUID
- 401 = Auth required (token missing or expired)
- 403 = Forbidden (path outside library)
- 404 = Not found
- 500 = Internal error

## 6. Client Data Model

Minimum fields to store per server connection:

```rust
struct ServerConnection {
    server_url: String,       // "http://192.168.1.50:8096"
    server_id: Uuid,          // From /api/v1/server/info
    server_name: String,      // "Michi Micro Server"
    api_version: String,      // "v1"
    features: ServerFeatures, // Feature flags
    token: Option<String>,    // Auth token if authenticated
    last_connected: DateTime, // ISO 8601 timestamp
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

## 7. What NOT to implement yet

These features exist in the server but are experimental or not part of
the stable v1 contract. Do not depend on them for production clients:

- `sync` (multi-room) — feature flag is `false`
- `transcoding` — requires ffmpeg, flag is `false`
- Playlist sharing via `/api/shared/:code`
- `/api/playback/state` (playback state push)
- `/api/ws` (WebSocket events)
- `/api/auth/register` (user registration)
- M3U import/export
- PWA / offline mode

## 8. Full API Reference

Interactive documentation: `http://<host>:8096/api/docs`
(Swagger UI with all endpoints, request/response schemas)

Stable contract reference: [MICHI_LINK.md](MICHI_LINK.md)
