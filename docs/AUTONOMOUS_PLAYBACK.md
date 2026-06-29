# Autonomous Playback — Micro Server as Self-Hosted Player

This document describes how **Michi Micro Server** can maintain playback independently, without any connected Michi Music Player or Michi Music Mobile client.

## Design

Micro Server has its own in-memory `PlaybackState`:

| Field | Description |
|-------|-------------|
| `track_id` | Current track UUID (optional, None when stopped) |
| `position_ms` | Current playback position in milliseconds |
| `playing` | Whether playback is active |
| `volume` | Volume as float 0.0–1.0 |
| `updated_at` | Timestamp of last change |

This state is:
- Persistent across HTTP requests (in `Arc<RwLock<PlaybackState>>`).
- Queryable via `GET /api/v1/playback/state`.
- Controllable via `POST /api/v1/playback/control`.
- Synchronizable with peers via WebSocket (`/api/v1/sync/state`).

## State Contract

`GET /api/v1/playback/state` returns:

```json
{
  "state": "playing",
  "track_id": "uuid-or-null",
  "current_track": {
    "id": "uuid",
    "title": "Song Title",
    "artist": "Artist Name",
    "album": "Album Name",
    "duration_ms": 240000
  },
  "position_ms": 12345,
  "duration_ms": 240000,
  "volume": 70,
  "shuffle": false,
  "repeat": "none",
  "playing": true
}
```

## Control Commands

`POST /api/v1/playback/control`

| Command | Behavior |
|---------|----------|
| `play` | Resume playback, optionally set `track_id` + `position_ms` |
| `pause` | Pause at current position |
| `toggle` | Toggle play/pause |
| `next` | Skip to next track (resets position) |
| `previous` | Go to previous track (resets position to 0) |
| `stop` | Stop playback, reset position |
| `seek` | Set `position_ms` |
| `set_volume` | Set volume 0–100 |
| `mute` | Set volume to 0 |
| `unmute` | Restore volume to 80% |

Body format:

```json
{
  "command": "seek",
  "position_ms": 50000
}
```

Legacy fallback:

```json
{
  "command": "set_volume",
  "value": { "volume": 50 }
}
```

## Session-based Playback (Continue on Server)

`POST /api/v1/playback/session`

Allows Player to transfer its current queue and position to Micro Server:

```json
{
  "queue": ["track-uuid-1", "track-uuid-2"],
  "current_track_id": "track-uuid-1",
  "position_ms": 12345,
  "playing": true
}
```

Server creates a `PlaybackSession` in the database and applies the state to its in-memory `PlaybackState`. Player can now shut down; Micro Server continues playing.

## Autonomy Guarantees

- Micro Server's playback state does **not** depend on any client connection.
- State persists as long as the process is running.
- Any paired client can query or control playback at any time.
- Multiple clients can observe the same state simultaneously.
