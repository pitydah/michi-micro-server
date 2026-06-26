# API Documentation

## Base URL

```
http://<server>:8096
```

## Current Endpoints

### GET /

Returns the **Web UI**: an HTML page with server status, library statistics, scan button, track listing, and built-in audio player. No frontend build step required.

### GET /api/status

Returns the server status as JSON.

**Response**:

```json
{
  "status": "ok",
  "service": "michi-micro-server",
  "version": "0.1.0",
  "port": 8096
}
```

**Status Codes**:
- `200 OK` — Server is running normally.

### POST /api/library/scan

Scans the configured music directory for audio files, reads metadata, and saves tracks to the database.

**Response**:

```json
{
  "status": "ok",
  "scanned": 120,
  "saved": 120
}
```

**Status Codes**:
- `200 OK` — Scan completed successfully.
- `404 Not Found` — Music path does not exist.
- `400 Bad Request` — Music path is not a directory.

### DELETE /api/library/tracks

Deletes all tracks from the library.

**Response**:

```json
{
  "deleted": 120
}
```

**Status Codes**:
- `200 OK` — Tracks deleted successfully.

### GET /api/tracks

Returns tracks from the library. Supports optional pagination.

**Query Parameters** (optional):
- `limit` — Maximum number of tracks to return (max 500).
- `offset` — Number of tracks to skip (default 0).

Without parameters, returns all tracks.

**Response**: Array of track objects.

```json
[
  {
    "id": "uuid-v5-from-path",
    "title": "Song Title",
    "artist": "Artist Name",
    "album": "Album Name",
    "album_artist": "Album Artist",
    "duration_ms": 240000,
    "file_path": "/music/artist/album/song.flac",
    "format": "Flac",
    "sample_rate": 44100,
    "bit_depth": 16,
    "channels": 2,
    "artwork_id": null,
    "created_at": "2026-01-01T00:00:00+00:00",
    "updated_at": "2026-01-01T00:00:00+00:00"
  }
]
```

**Status Codes**:
- `200 OK` — Tracks returned successfully.

### GET /api/tracks/:id

Returns a single track by UUID.

**Status Codes**:
- `200 OK` — Track returned successfully.
- `404 Not Found` — Track not found.

### DELETE /api/tracks/:id

Deletes a single track by UUID.

**Response**:

```json
{
  "deleted": true
}
```

**Status Codes**:
- `200 OK` — Track deleted successfully.
- `404 Not Found` — Track not found.

### PUT /api/tracks/:id

Updates metadata fields for a track. Only the fields sent in the request body are modified.

**Request Body** (all fields optional):

```json
{
  "title": "New Title",
  "artist": "New Artist",
  "album": "New Album",
  "album_artist": "New Album Artist",
  "duration_ms": 240000,
  "sample_rate": 44100,
  "bit_depth": 16,
  "channels": 2
}
```

**Response**: The updated track object.

**Status Codes**:
- `200 OK` — Track updated successfully.
- `404 Not Found` — Track not found.

### GET /api/stream/:id

Streams an audio file with optional HTTP Range Request support.

**Parameters**:
- `id` (path) — UUID of the track to stream.

**Response (no Range header)**: `200 OK`

Headers:
- `Content-Type`: Audio MIME type based on file extension
- `Content-Length`: Full file size
- `Accept-Ranges: bytes`

**Response (valid Range header)**: `206 Partial Content`

Headers:
- `Content-Type`: Audio MIME type
- `Content-Range`: Range in `bytes start-end/total` format
- `Content-Length`: Size of the range
- `Accept-Ranges: bytes`

**Response (invalid Range)**: `416 Range Not Satisfiable`

Body:
```json
{
  "status": "error",
  "message": "range not satisfiable: ..."
}
```

**Error Responses**:
- `400 Bad Request` — Invalid UUID or malformed `Range` header
- `403 Forbidden` — File is outside the configured music library path
- `404 Not Found` — Track not found or file missing from disk
- `416 Range Not Satisfiable` — Range start beyond file size

**Examples**:

```bash
# Full file
curl -v http://localhost:8096/api/stream/TRACK_ID

# Partial range
curl -v -H "Range: bytes=0-1023" http://localhost:8096/api/stream/TRACK_ID

# From offset to end
curl -v -H "Range: bytes=100-" http://localhost:8096/api/stream/TRACK_ID

# Suffix range (last N bytes)
curl -v -H "Range: bytes=-500" http://localhost:8096/api/stream/TRACK_ID
```

**Supported MIME Types**:

| Extension | MIME Type |
|-----------|-----------|
| mp3 | `audio/mpeg` |
| flac | `audio/flac` |
| ogg, opus | `audio/ogg` |
| m4a | `audio/mp4` |
| aac | `audio/aac` |
| wav | `audio/wav` |
| aiff, aif | `audio/aiff` |
| dsf | `audio/dsf` |
| dff | `audio/dff` |

### GET /api/search?q=

Searches tracks by title, artist, album, album_artist, or format. The search is case-insensitive and uses SQL LIKE.

**Query Parameters**:
- `q` (required) — Search query string. If empty, returns an empty array.

**Response**: Array of matching track objects (same format as GET /api/tracks).

```json
[
  {
    "id": "uuid-v5-from-path",
    "title": "Song Title",
    "artist": "Artist Name",
    "album": "Album Name",
    "album_artist": "Album Artist",
    "duration_ms": 240000,
    "file_path": "/music/artist/album/song.flac",
    "format": "Flac",
    "sample_rate": 44100,
    "bit_depth": 16,
    "channels": 2,
    "artwork_id": null,
    "created_at": "2026-01-01T00:00:00+00:00",
    "updated_at": "2026-01-01T00:00:00+00:00"
  }
]
```

**Status Codes**:
- `200 OK` — Search completed successfully (may return empty array if no matches or empty query).

**Example**:

```bash
curl "http://localhost:8096/api/search?q=pink+floyd"
```

### GET /api/library/stats

Returns library statistics.

**Response**:

```json
{
  "tracks": 120,
  "albums": 15,
  "artists": 42
}
```

**Status Codes**:
- `200 OK` — Stats returned successfully.

## Future Endpoints

### GET /api/albums
List all albums.

### GET /api/artists
List all artists.

### GET /api/playlists
List all playlists.

### POST /api/playlists
Create a new playlist.

### WebSocket /api/ws
Real-time updates for playback state and library changes.

## Response Format

All API responses use JSON. Errors follow:

```json
{
  "status": "error",
  "message": "description of the error"
}
```
