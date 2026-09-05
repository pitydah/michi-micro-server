# OpenSubsonic Compatibility Subset (v1)

Michi Micro Server implements a deliberate, JSON-only compatibility layer for the
OpenSubsonic / Subsonic API at `/rest/...` endpoints.

> [!NOTE]
> Michi implements a functional compatibility subset optimized for modern mobile and desktop Subsonic clients (e.g. DSub, Symfonium, Feishin, Substreamer). Full legacy Subsonic protocol parity or XML endpoints are explicitly not claimed for v1.

## Implemented Subset Matrix

| Method | Path | Status | Notes |
|:---|:---|:---:|:---|
| GET | `/rest/ping` | 🟢 `stable` | Authenticated ping check |
| GET | `/rest/getLicense` | 🟢 `stable` | Returns valid license object |
| GET | `/rest/getMusicFolders` | 🟢 `stable` | Configured primary and secondary music paths |
| GET | `/rest/getArtists` | 🟢 `stable` | Indexed artist catalog with alphabetical grouping |
| GET | `/rest/getArtist` | 🟢 `stable` | Artist details and associated albums |
| GET | `/rest/getAlbum` | 🟢 `stable` | Album details and song list |
| GET | `/rest/getSong` | 🟢 `stable` | Track metadata and duration |
| GET | `/rest/search3` | 🟡 `partial` | Searches song titles/metadata; artist/album arrays empty |
| GET | `/rest/stream` | 🟢 `stable` | Direct play with full HTTP Range (200, 206 Partial, 416) |
| GET | `/rest/download` | 🟢 `stable` | Direct binary download with attachment disposition |
| GET | `/rest/getCoverArt` | 🟢 `stable` | Serves cached or extracted cover artwork |
| GET | `/rest/getLyrics` | ⚪ `stub` | Returns valid envelope with empty lyrics placeholder |
| GET | `/rest/getPlaylists` | 🟢 `stable` | Lists user and system playlists |
| GET | `/rest/getPlaylist` | 🟢 `stable` | Detailed playlist tracks |
| GET | `/rest/scrobble` | 🟢 `stable` | Records play count and last played timestamp |
| GET | `/rest/star` | 🟢 `stable` | Marks track as starred/favorite |
| GET | `/rest/unstar` | 🟢 `stable` | Removes starred status |
| GET | `/rest/startScan` | 🟢 `stable` | Triggers background library scan |
| GET | `/rest/getScanStatus` | 🟢 `stable` | Returns real-time scanning boolean and track count |
| GET | `/rest/setRating` | 🟢 `stable` | Sets 1–5 star rating on track |
| GET | `/rest/getRandomSongs`| 🟢 `stable` | Random track sampling pool |
| GET | `/rest/getNowPlaying` | ⚪ `stub` | Returns valid envelope with empty entries placeholder |

## Response Format & Versioning

All responses conform to OpenSubsonic 1.16.1 specification in JSON format:

```json
{
  "subsonic-response": {
    "status": "ok",
    "version": "1.16.1",
    "type": "michi-micro-server",
    "serverVersion": "1.0.0-rc.1",
    "openSubsonic": true
  }
}
```

## Authentication

Authentication supports both standard Subsonic schemes:
1. **Token Auth (`t` + `s`)**: Standard MD5 hash `hex(md5(password + salt))` with salt.
2. **Plain / Hex Password (`p`)**: Cleartext password or `enc:<hex-encoded>` password against server admin or SQLite database users (verified via Argon2).

## Known Scope Exclusions for v1
- XML response format (clients must use `f=json`).
- Podcasts, Internet Radio, Bookmarks, and User Management via Subsonic legacy endpoints (administered via native `/api/v1` routes).
