# Endpoint Matrix (Michi Micro Server v1 — R1.4)

Transport legend:
- **HTTP or HTTPS**: Public discovery/readiness or admin/legacy routes.
- **HTTPS ONLY (Fail-Closed)**: Cleartext HTTP is unconditionally rejected with `403 TLS_REQUIRED` (or `503 TLS_NOT_CONFIGURED` if TLS is not configured), even if spoofed `X-Forwarded-Proto: https` headers are sent.

| Method | Path | Transport | Auth | Required Headers / Notes |
| --- | --- | --- | --- | --- |
| `GET` | `/health/live` | HTTP or HTTPS | Public | Liveness probe (`200 OK`) |
| `GET` | `/health/ready` | HTTP or HTTPS | Public | SQLite readiness probe (`200` / `503`) |
| `GET` | `/api/v1/server/info` | HTTP or HTTPS | Public | Canonical identity (`michi_id`, `public_key`, `identity_scheme`, `michi_home_id`, `roles`, `features`) |
| `GET` | `/api/v1/capabilities` | HTTP or HTTPS | Public | Runtime feature array (`library`, `playback`, `stream`, etc.) and protocol list |
| `POST` | `/api/auth/login` | HTTP or HTTPS (HTTPS strongly recommended) | Admin username/password | Returns admin bearer token (`Content-Type: application/json`) |
| `GET` | `/api/v1/aircat/status` | HTTP or HTTPS | Public / Admin | TLS profile, `tls_trust_mode`, `certificate_profile`, `chain_cryptographically_verified`, `android_trust_verification_status`, SPKI pin |
| `GET` | `/api/v1/aircat/enrollment-qr` | **HTTPS ONLY** | Admin Bearer | Returns `AirCatTrustedEnrollmentQr` (`homeId`, `authorityPublicKeyBase64Url`, `sourceMichiId`, `pinnedBaseUrl`, `tlsSpkiSha256`) |
| `POST` | `/api/v1/home/enrollment/challenge` | **HTTPS ONLY** | Public (rate-limited) | Issues single-use enrollment challenge nonce |
| `POST` | `/api/v1/home/enrollment/request` | **HTTPS ONLY** | Ed25519 PoP | Submits signed enrollment request → `202 Accepted` (`request_id`, `claim_token`) |
| `GET` | `/api/v1/home/enrollment/{id}/status` | **HTTPS ONLY** | `X-Michi-Enrollment-Claim` | Polls pending/approved/rejected status; returns signed `membership` + `home_root_public_key` when approved |
| `GET` | `/api/v1/home/enrollment/pending` | **HTTPS ONLY** | Admin Bearer | Lists pending enrollment requests |
| `POST` | `/api/v1/home/enrollment/{id}/approve` | **HTTPS ONLY** | Admin Bearer | Signs `DeviceMembershipDto` with Home Root Authority and persists member in SQLite |
| `POST` | `/api/v1/home/enrollment/{id}/reject` | **HTTPS ONLY** | Admin Bearer | Rejects pending enrollment request |
| `POST` | `/api/v1/auth/challenge` | **HTTPS ONLY** | Public (rate-limited) | Starts Home mutual challenge-response |
| `POST` | `/api/v1/auth/session` | **HTTPS ONLY** | Ed25519 + Membership | Verifies client sig + membership + revocation → returns `michi_home_*` token + `server_signature` |
| `GET` | `/api/v1/home/roster` | HTTP or HTTPS | Admin Bearer | Canonical `HomeDeviceRoster` v1 (`version: 1`, `home_id`, `devices`, `updated_at`, `total`, `presence_ttl_seconds`); fail-closed `503 REVOCATION_STORE_UNAVAILABLE` if DB unavailable |
| `GET` | `/api/v1/home/revocations` | HTTP or HTTPS | Admin Bearer | Lists signed Home device revocations |
| `POST` | `/api/v1/home/revoke` | HTTP or HTTPS | Admin Bearer | Revokes device immediately (invalidates active sessions + persists signed revocation) |
| `GET` | `/api/v1/tracks` | **HTTPS** (for `michi_home_*`) | Home / Admin / Device Bearer | Library track listing (`X-Michi-Device-Id: <client_michi_id>` for Home sessions) |
| `GET` | `/api/v1/playback/state` | **HTTPS** (for `michi_home_*`) | Home / Admin / Device Bearer | Live playback engine snapshot (`volume`, `playing`, `track_id`, `position_ms`, `shuffle`, `repeat`) |
| `POST` | `/api/v1/playback/control` | **HTTPS** (for `michi_home_*`) | Home / Admin / Device Bearer | Canonical `command` (`play`, `pause`, `toggle`, `stop`, `next`, `previous`, `seek`, `set_volume`, `mute`, `unmute`, `shuffle`, `repeat`) |
| `GET` | `/api/v1/queue` | **HTTPS** (for `michi_home_*`) | Home / Admin / Device Bearer | Active queue snapshot including `queue_version`, `queue_degraded`, `queue_degraded_reason`, `items` |
| `POST` | `/api/v1/queue/items` | **HTTPS** (for `michi_home_*`) | Home / Admin / Device Bearer | Append tracks (serialized via `ActiveQueueCoordinator`, `503 QUEUE_DEGRADED` if degraded) |
| `PUT` | `/api/v1/queue/reorder` | **HTTPS** (for `michi_home_*`) | Home / Admin / Device Bearer | Reorder active queue by `item_ids` (rejects duplicates / count mismatch) |
| `DELETE` | `/api/v1/queue/items/{item_id}` | **HTTPS** (for `michi_home_*`) | Home / Admin / Device Bearer | Delete single queue item by `queue_item_id` (syncs PlaybackEngine + verified rollback) |
| `POST` | `/api/v1/queue/jump` | **HTTPS** (for `michi_home_*`) | Home / Admin / Device Bearer | Jump to queue index |
| `POST` | `/api/v1/queue/transfer` | **HTTPS** (for `michi_home_*`) | Home / Admin / Device Bearer | Replace active queue + seek position |
| `POST` | `/api/v1/queue/shuffle` | **HTTPS** (for `michi_home_*`) | Home / Admin / Device Bearer | Toggle shuffle (permutation preserved across queue edits) |
| `POST` | `/api/v1/queue/repeat` | **HTTPS** (for `michi_home_*`) | Home / Admin / Device Bearer | Set repeat mode (`off`, `one`, `all`) |
| `POST` | `/api/v1/queue/reconcile` | **HTTPS** (for `michi_home_*`) | Home / Admin / Device Bearer | Explicitly reconciles SQLite active queue into PlaybackEngine and clears `QUEUE_DEGRADED` |
| `GET` | `/api/v1/aircat/manifest` | **HTTPS ONLY** | Home Bearer (`michi_home_*`) | Returns `AirCatCatalogManifest` (`X-Michi-AirCat-Protocol: michi.aircat.original.v1`, `Cache-Control: no-store`) |
| `POST` | `/api/v1/aircat/authorize` | **HTTPS ONLY** | Home Bearer (`michi_home_*`) | Issues signed `Grant` v1 (`409` on nonce replay) |
| `GET` | `/api/v1/aircat/download/{remote_id}` | **HTTPS ONLY** | Home Bearer + `X-Michi-AirCat-Grant` | Original lossless bytes (`200 OK` or `206 Partial Content` with `Range` + `If-Range: <ETag>`) |
| `POST` | `/api/v1/pair/qr` | HTTP or HTTPS | Admin Bearer | Legacy `SERVER_CODE` QR ONLY (host strictly bound to THIS server's NICs / mDNS / allowlist; never Home anchor) |
