# API Documentation

## Base URL

```
http://<server>:8096
```

## Current Endpoints

### GET /

Returns a simple HTML page confirming the server is running.

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

Returns all tracks stored in the library.

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
    "format": "flac",
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

### GET /api/stream/:id
Stream audio from a track.

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
