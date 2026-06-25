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

## Future Endpoints

### GET /api/tracks
List all tracks in the library.

### GET /api/tracks/:id
Get a single track by ID.

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

### POST /api/scan
Trigger a library rescan.

### WebSocket /api/ws
Real-time updates for playback state and library changes.

## Response Format

All API responses use JSON. Errors follow:

```json
{
  "error": "description of the error"
}
```
