# Michi Micro Server — Beta Readiness

## Status: BETA-READY

Michi Micro Server is ready for beta testing as:

- **Library server** — stores and indexes music
- **Stream server** — serves audio via HTTP with Range support
- **Sync source** — provides library manifest for clients
- **Home server** — runs autonomously on a Raspberry Pi, NAS, or mini PC
- **Playback host** — maintains playback state independently
- **Multiroom host** — prepared for future Michi Music Stream integration

## Confirmed Capabilities

| Feature | Status | Notes |
|---------|--------|-------|
| Pairing (code-based) | ✅ Stable | SERVER_CODE strategy |
| Token auth + refresh | ✅ Stable | SHA-256 hashed, Device/Refresh types |
| Library stats | ✅ Stable | |
| Track listing | ✅ Stable | No file_path exposed |
| Search | ✅ Stable | |
| Streaming | ✅ Stable | 200 + 206 Range |
| Download | ✅ Stable | 200 + 206 Range, safe filename |
| Artwork | ✅ Stable | |
| Playlists CRUD | ✅ Stable | |
| Sync manifest | ✅ Stable | With cursor |
| Sync manifest delta | ✅ Stable | GET with cursor query |
| Sync state upload | ✅ Stable | POST |
| Playback state | ✅ Stable | state/track_id/position/volume/shuffle/repeat |
| Playback control | ✅ Stable | play/pause/toggle/next/prev/stop/seek/volume |
| Playback session | ✅ Stable | Continue-on-server |
| Queue | ⚠️ Partial | In-memory + DB, no full persistence across restarts |
| Import | ✅ Stable | SHA-256 verify, dedup, size limits, rollback |
| Events WebSocket | ⚠️ Partial | Working but no auth on WS; Mobile should poll as fallback |
| Receivers | 🔲 Planned | See RECEIVER_INTEGRATION_PLAN.md |
| Rooms | 🔲 Planned | See RECEIVER_INTEGRATION_PLAN.md |

## Testing

All 168+ workspace tests pass, including:

- E2E Mobile flow (pair → sync → tracks → playback → revoke)
- Player import flow (session → upload → hash verify → dedup → commit)
- Auth flow (start → confirm → wrong code → token format)
- Import validation (extensions, size, validation)
- Stream/download Range (200, 206, 416)
- Error format with `details: {}`
- Autonomous playback (state independence, control commands)

## Known Limitations

- Queue state is not yet restored from DB on restart.
- Events WebSocket has no token auth; clients should fall back to polling `/api/v1/playback/state`.
- No TLS/HTTPS; run behind reverse proxy for production.
- Receivers/rooms integration pending Michi Music Stream hardware.
