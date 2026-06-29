# Michi Link E2E — Micro Server as Mobile Target

This document describes the end-to-end flow between **Michi Music Mobile** and **Michi Micro Server** over the Michi Link v1 API.

## Discovery

1. Mobile scans the local network for `/_michi` or configures a known IP:port.
2. Mobile calls `GET /api/v1/server/info` to discover:
   - `service`: must be `michi-micro-server`
   - `michi_link_version`: `"1.0.0-alpha"`
   - `auth.strategy`: `"SERVER_CODE"` or `"PLAYER_PASSWORD"`
   - `features`: boolean flags (library, streaming, playback, queue, etc.)
3. Mobile stores `server_id` for stable identification.

## Pairing

1. Mobile calls `POST /api/v1/pair/start` with:
   - `device_name` or `alias`
   - `device_type`: `"mobile"`
   - `device_model`: optional model string
2. Server returns a 6-character alphanumeric `code` visible on screen.
3. User reads the code from the server UI and enters it in Mobile.
4. Mobile calls `POST /api/v1/pair/confirm` with:
   - `code`: the displayed code
5. Server returns:
   - `device_token`: Bearer token for API calls
   - `refresh_token`: for token refresh
   - `device_id`: device UUID for server-side reference
   - `permissions`: canonical string array

## Token Refresh

- Mobile calls `POST /api/v1/token/refresh` with `refresh_token` before expiry (90 days).
- Server returns new `device_token` and `refresh_token`.
- If refresh fails, re-pairing is required.

## Library Sync

1. `GET /api/v1/library/stats` — track/album/artist counts.
2. `GET /api/v1/tracks` — paginated list of tracks (no `file_path`, has `stream_url`/`download_url`).
3. `GET /api/v1/tracks/{id}` — single track (no `file_path`).

## Streaming

- `GET /api/v1/stream/{track_id}` — full file (200) or Range (206).
- `GET /api/v1/download/{track_id}` — attachment download with Range support.

## Sync

1. `GET /api/v1/sync/manifest` — full library manifest with `cursor`.
2. `GET /api/v1/sync/manifest/delta?cursor=N` — incremental changes.
3. `POST /api/v1/sync/state` — upload playback state to server.

## Playback

1. `GET /api/v1/playback/state` — current server state: `state`, `track_id`, `position_ms`, `volume`, etc.
2. `POST /api/v1/playback/control` — commands: play, pause, next, seek, set_volume.
3. `GET /api/v1/queue` — current queue status.

## Error Format

All v1 endpoints return errors as:

```json
{
  "error": {
    "code": "ERROR_CODE",
    "message": "Human-readable description",
    "details": {}
  }
}
```
