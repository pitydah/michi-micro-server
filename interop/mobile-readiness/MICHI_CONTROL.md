# Michi Control, Queue Coordinator & Home Roster (R1.4)

## 1. Playback Control (`POST /api/v1/playback/control`)

- Official field is **`command`** (values: `play`, `pause`, `toggle`, `stop`, `next`, `previous`, `seek`, `set_volume`, `mute`, `unmute`, `shuffle`, `repeat`).
- **`set_volume` Canonical Grammar (R1.4 §3.5)**:
  - **Preferred**: `{ "command": "set_volume", "volume": 75 }` (strict JSON integer `0..=100`).
  - **Legacy fallback `value`**: accepts integer `0..=100` OR strict unsigned decimal string matching `^(0|[1-9][0-9]?|100)$`.
  - **REJECTED (`400 INVALID_REQUEST`, engine volume unchanged)**: leading zeros (`"00"`, `"01"`, `"075"`, `"0100"`), floats (`50.5`, `"50.5"`), signs (`"+50"`, `"-1"`), whitespace (`" 50 "`), booleans, out-of-range (`101`), or conflicting `volume` + `value`.
- **Playback State**: Read live state via `GET /api/v1/playback/state` (`volume`, `playing`, `track_id`, `position_ms`, `shuffle`, `repeat`).

## 2. Unified `ActiveQueueCoordinator` (`/api/v1/queue*`, R1.4 §3.1)

All active queue mutators (`POST /api/v1/queues`, `POST /api/v1/queue/items`, `PUT /api/v1/queue/reorder`, `DELETE /api/v1/queue/items/{id}`, `POST /api/v1/queue/jump`, `POST /api/v1/queue/transfer`, `DELETE /api/v1/queue/{id}`, `POST /api/v1/queue/save`, `POST /api/v1/playback/session`, `POST /api/v1/playback/restore`) are serialized through a single `ActiveQueueCoordinator`:
- **Monotonic `queue_version`**: Returned on `GET /api/v1/queue` and mutating responses (`queue_version: u64`). Clients should discard stale UI updates with lower `queue_version`.
- **Contiguous Positions**: Queue item positions are always strictly contiguous `0..N-1` after any deletion or reorder.
- **Canonical Item Deletion**: Always delete using `queue_item_id` (`items[i].id`), NOT `track_id`, so duplicate tracks in a queue are never ambiguously targeted.
- **Shuffle Preservation**: Deleting or appending items while `shuffle == true` preserves the active shuffle permutation for surviving tracks.
- **Fail-Closed Degraded State (`503 QUEUE_DEGRADED`)**: If `PlaybackEngine` sync fails AND compensating SQLite rollback also fails, the coordinator blocks ALL new mutations fail-closed with `503 QUEUE_DEGRADED` and `GET /api/v1/queue` reports `queue_degraded: true`. Recovery is explicit via `POST /api/v1/queue/reconcile` (with `Content-Type: application/json`, body `{}`).

## 3. Canonical Home Device Roster (`GET /api/v1/home/roster`, R1.4 §3.2 & §3.3)

Matches `vendor/michi-link/schemas/home-device-roster.schema.json` (`version: 1`) verbatim:
- Top-level fields: `version` (`1`), `home_id` (43 chars), `total`, `presence_ttl_seconds` (`180`), `devices[]`, `updated_at` (RFC3339).
- Per-device fields strictly separate four orthogonal concepts:
  - **Identity domains**: `device_michi_id` (43-char cryptographic Michi ID) vs optional `receiver_id` (receiver registry identifier — NEVER conflated!).
  - **Membership**: `membership_status` (`"enrolled"` | `"revoked"`) + signed `membership` DTO.
  - **Authentication**: `authenticated` (`true` ONLY with active non-expired session or live verified receiver auth).
  - **Bounded Live Presence**: `online` (`true` ONLY with recent verifiable activity within TTL or live active session — never inferred from SQLite membership alone!), `presence_state` (`"online"` | `"offline"` | `"revoked"` | `"unknown"`), `status`, `last_seen_at` (plus legacy alias `last_seen`).
- **Schema Invariants**:
  - `revoked == true` ⇒ `online == false`, `authenticated == false`, `membership_status == "revoked"`, `presence_state == "revoked"`, `status == "revoked"`.
  - `online == true` ⇒ `revoked == false`, `presence_state == "online"`, `status == "online"`.
- **Fail-Closed**: If SQLite `home_revocations` or `home_members` is unavailable, `GET /api/v1/home/roster` returns `503 REVOCATION_STORE_UNAVAILABLE`.
