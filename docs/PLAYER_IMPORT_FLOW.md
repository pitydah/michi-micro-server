# Player Import Flow — Micro Server as Library Target

This document describes how **Michi Music Player** sends its library to **Michi Micro Server** for backup and autonomous playback.

## Overview

1. Player initiates an import session.
2. Player uploads each track as base64 with SHA-256 hash.
3. Player commits the session.
4. Micro Server scans the imported files and adds them to its library.
5. Micro Server can now play imported tracks even when Player is offline.

## Preconditions

- Player must be paired with Micro Server via `POST /api/v1/pair/start` + `pair/confirm`.
- Player must have permissions: `library.write`, `sync.read_manifest`, `stream.read`.

## Session

`POST /api/v1/import/session`

```json
{
  "total_tracks": 100,
  "total_playlists": 5
}
```

Response:

```json
{
  "session_id": "uuid",
  "expires_at": "RFC3339",
  "max_chunk_size": 10485760,
  "allowed_extensions": ["mp3", "flac", "ogg", "opus", "aac", "m4a", "wav", "aiff", "dsf", "dff"],
  "max_file_size": 104857600
}
```

## Upload

`POST /api/v1/import/upload/{session_id}`

```json
{
  "filename": "song.flac",
  "hash": "sha256-hex",
  "data": "base64-encoded-file-content"
}
```

Validation:
- Extension must be in `allowed_extensions` list.
- File size must not exceed `max_file_size`.
- Session total must not exceed 1 GB.
- SHA-256 hash is verified server-side.
- Duplicate hash → `accepted: false, is_duplicate: true`.

Response:

```json
{
  "accepted": true,
  "is_duplicate": false,
  "track_id": "uuid"
}
```

## Commit

`POST /api/v1/import/commit/{session_id}`

Response:

```json
{
  "tracks_imported": 100,
  "playlists_imported": 0,
  "total_size_bytes": 123456789
}
```

After commit, Micro Server scans the import directory and adds all tracks to its library.

## File Persistence

Imported files are stored under the first music path in `.import/{session_id}/`.
This means:
- Files survive container restarts if music path is a volume.
- Files are eligible for regular library scanning.
- Files can be served via `/api/v1/stream/{id}` and `/api/v1/download/{id}`.
