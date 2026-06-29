# Receiver Integration Plan — Michi Music Stream

## Status: PROTOTYPE — Not stable

Receivers are **not yet fully integrated**. The v1 API declares `features.receivers=false` and `features.rooms=false`.

This document outlines the planned integration for **Michi Music Stream** as a receiver endpoint for multi-room audio distribution.

## Architecture

```
Michi Micro Server  ──session/start──►  Michi Music Stream (receiver)
                    ◄──heartbeat────
                    ──volume────────►
                    ◄──status───────
```

## Endpoints (planned v1)

### Discovery

Receiver announces itself via UDP broadcast or configurable endpoint.  
`GET /api/v1/receivers` — list discovered receivers.

### Pairing

Same code-based pairing flow as Mobile/Player:

1. `POST /api/v1/pair/start` with `device_type: "michi-stream"`
2. `POST /api/v1/pair/confirm` returns device + stream-specific permissions.

### Heartbeat

Receiver sends periodic heartbeat to confirm online status:

```
POST /api/v1/receivers/{id}/heartbeat
```

### Session Control

```
POST /api/v1/receivers/{id}/session/start
  { "track_id": "...", "position_ms": 0, "stream_url": "..." }

POST /api/v1/receivers/{id}/session/stop
  { "reason": "user_stop" }

POST /api/v1/receivers/{id}/volume
  { "volume": 70 }
```

### Room/Zones

```
POST /api/v1/rooms
  { "name": "Living Room", "receiver_ids": [...] }

POST /api/v1/rooms/{id}/play
  { "track_id": "...", "position_ms": 0 }

POST /api/v1/rooms/{id}/volume
  { "volume": 50 }

POST /api/v1/rooms/{id}/mute
  { "muted": true }
```

## Implementation Phases

| Phase | Scope | Status |
|-------|-------|--------|
| 1 | Device registry + pairing | ✅ Done |
| 2 | Receiver discovery + listing | ✅ Basic |
| 3 | Heartbeat + online status | 🔲 Planned |
| 4 | Session start/stop | 🔲 Planned |
| 5 | Volume + mute control | 🔲 Planned |
| 6 | Room management | 🔲 Planned |
| 7 | Multi-room sync | 🔲 Future |

## Permissions

Stream receivers get:

- `server.read`
- `stream.read`
- `playback.read`
- `receiver.read`
- `receiver.control`
- `receiver.session`
- `receiver.volume`
- `room.read`

## Testing

- Hardware test with Raspberry Pi + DAC + Michi Stream firmware.
- Latency measurement for multi-room sync.
- Failover when receiver disconnects.
