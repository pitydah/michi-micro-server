# OpenSubsonic Compatibility

Michi Micro Server implements a compatibility layer for the
OpenSubsonic/Subsonic API at `/rest/...` endpoints.

## Implemented Endpoints

| Method | Path | Status | Notes |
|--------|------|--------|-------|
| GET | `/rest/ping` | ✅ | Authenticated ping |
| GET | `/rest/getLicense` | ✅ | Returns valid license |
| GET | `/rest/getMusicFolders` | ✅ | Returns configured music paths |
| GET | `/rest/getArtists` | ✅ | Indexed artist list |
| GET | `/rest/getArtist` | ❌ | Not yet implemented |
| GET | `/rest/getAlbum` | ❌ | Not yet implemented |
| GET | `/rest/getSong` | ❌ | Not yet implemented |
| GET | `/rest/search3` | ❌ | Not yet implemented |
| GET | `/rest/stream` | ❌ | Not yet implemented |
| GET | `/rest/download` | ❌ | Not yet implemented |
| GET | `/rest/getCoverArt` | ❌ | Not yet implemented |
| GET | `/rest/getLyrics` | ❌ | Not yet implemented |
| GET | `/rest/getPlaylists` | ✅ | Reuses existing playlist DB |
| GET | `/rest/getPlaylist` | ❌ | Not yet implemented |
| GET | `/rest/scrobble` | ❌ | Not yet implemented |
| GET | `/rest/star` | ❌ | Not yet implemented |
| GET | `/rest/unstar` | ❌ | Not yet implemented |
| GET | `/rest/startScan` | ❌ | Not yet implemented |
| GET | `/rest/getScanStatus` | ✅ | Returns track count |

## Response Format

All endpoints return JSON in the Subsonic envelope:

```json
{
  "subsonic-response": {
    "status": "ok",
    "version": "1.16.1",
    "type": "michi-micro-server",
    "serverVersion": "0.2.0",
    "openSubsonic": true
  }
}
```

## Authentication

OpenSubsonic endpoints accept a `u` (username) query parameter.
Full authentication (password, token) is not yet implemented — this is a
placeholder for client compatibility testing.

## Known Limitations

- No XML support (JSON only)
- No token-based auth (plain password only, placeholder)
- No Subsonic favorites/shared/radio/podcasts
- Response format is an approximation — may differ from Navidrome/Ampache

## Testing

```bash
# Ping
curl "http://localhost:8096/rest/ping?u=admin&f=json"

# Music folders
curl "http://localhost:8096/rest/getMusicFolders?u=admin&f=json"

# Artists
curl "http://localhost:8096/rest/getArtists?u=admin&f=json"
```
