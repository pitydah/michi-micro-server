# AirCat v1 Operations & Troubleshooting Runbook (`MICRO-17` / `MICRO-33`)

Operational guide for deploying, monitoring, troubleshooting, and rolling back **Michi Micro Server** as an authenticated `michi.aircat.original.v1` producer on Linux, Docker, and ZimaOS/CasaOS.

---

## 1. Network Ports & Persistent TLS Configuration (`MICRO-17`)

AirCat strict profile (`AIRCAT_STRICT` / I02) forbids cleartext HTTP fallback whenever HTTPS/TLS is active:

| Surface | Default Port | Env Variable | Notes |
|---|---|---|---|
| WebUI / Standard Link HTTP API | `9090` | `MICHI_PORT` | Serves WebUI, health checks, and legacy HTTP APIs; rejects `/api/v1/aircat/*` with `403 TLS_REQUIRED` when TLS listener is active |
| AirCat HTTPS/TLS Listener (`tokio-rustls`) | `9443` | `MICHI_TLS_PORT` | Defaults to `MICHI_PORT + 353`; binds `0.0.0.0:<MICHI_TLS_PORT>` with 10 s TLS handshake timeout |
| Enrolled Base URL (QR payload) | Computed | `MICHI_PUBLIC_HTTPS_URL` | Canonical origin placed in `pinnedBaseUrl` inside `GET /api/v1/aircat/enrollment-qr` |
| TLS Certificate & Key | `/config/tls.crt`, `/config/tls.key` | `MICHI_TLS_CERT_PATH`, `MICHI_TLS_KEY_PATH` | Persisted inside `/config` volume; SPKI pin (`sha256/<BASE64_STANDARD>`) is derived from certificate SubjectPublicKeyInfo at startup |

### Persistent `/config` & `/music` Volumes (ZimaOS / Docker)

- `/config` (`rw`): Must persist `michi.db`, Ed25519 identity files, Home Root Authority key (`home_root_authority.key`), and `tls.crt` / `tls.key` across container upgrades. Rotating `tls.crt` changes `tls_spki_sha256` and requires re-enrolling Mobile clients via `/api/v1/aircat/enrollment-qr`.
- `/music` (`ro`): Mount original music library read-only (`read_only: true`). Symlinks, special files, zero-byte files, and paths escaping configured roots are rejected (`404 ASSET_NOT_FOUND` / omitted from `/manifest`).

---

## 2. Live Readiness & Observability (`GET /api/v1/aircat/status`)

- Authenticated WebUI operators (`michi_web_session` cookie) or Bearer sessions can inspect AirCat readiness via `GET /api/v1/aircat/status` (surfaced in WebUI under Michi Link panel `#aircat-producer-panel`).
- `GET /api/v1/server/info` advertises `features.aircat_original_v1 = true` and `features.home_tls_spki_pinned = true` **only** when HTTPS/TLS + SPKI pinning is configured (`state.aircat_tls` active).
- `GET /api/v1/aircat/enrollment-qr` is strictly admin-only (`is_admin == true` session or Bearer admin) and returns `503 TLS_NOT_CONFIGURED` if TLS certificate/key files are missing or unreadable.

---

## 3. Canonical Status & Error Code Catalog (`MICRO-33` / `LINK-23`)

All JSON errors follow `{"error": {"code": "...", "message": "...", "details": {}}}` capped below `4 KiB` without filesystem paths, SQL details, bearer tokens, raw grants, or private keys.

| HTTP Status | `error.code` | Cause & Operator Action |
|---|---|---|
| `401` | `UNAUTHORIZED` / `INVALID_TOKEN` / `MISSING_GRANT` | Missing/expired Bearer session or missing `X-Michi-AirCat-Grant` header. Re-authenticate via `POST /api/v1/auth/challenge` + `POST /api/v1/auth/session`. |
| `403` | `TLS_REQUIRED` | Caller hit `/api/v1/aircat/*` over cleartext HTTP (`:9090`) while TLS (`:9443`) is active. Direct client to enrolled `https://...:<MICHI_TLS_PORT>`. |
| `403` | `ADMIN_REQUIRED` | Non-admin WebUI user or device session attempted `/api/v1/aircat/enrollment-qr` or third-party `destinationMichiId` authorization. |
| `403` | `MEMBER_REVOKED` / `DEVICE_REVOKED` / `UNTRUSTED_MEMBER` / `WRONG_AUDIENCE` / `WRONG_SOURCE` / `BAD_SIGNATURE` / `IDENTITY_MISMATCH` | Member revoked in `home_revocations` / in-memory cache (`POST /api/v1/home/revoke`), wrong destination/source Michi ID, or invalid Ed25519 signature. Streamed downloads abort mid-transfer on chunk boundaries. |
| `404` | `ASSET_NOT_FOUND` / `TRACK_NOT_FOUND` | Unknown track UUID, empty (`0`-byte) original file, symlink/path traversal outside `music_paths`, or missing authorization snapshot. |
| `406` | `TRANSFORM_ENCODING_REJECTED` | Download client requested compressing encodings (`gzip`, `br`, `deflate`, `zstd`) without `identity`/`*`. AirCat originals strictly use `Accept-Encoding: identity`. |
| `409` | `NONCE_REPLAY` / `MANIFEST_ALTERED` / `ORIGINAL_BYTES_MISMATCH` / `FILE_REVISION_CHANGED` | Reused client nonce for `(destinationMichiId, clientNonce)`, mismatched manifest SHA-256, or file size/inode/mtime changed on disk (`fstat`). Re-run `GET /manifest` -> `POST /authorize`. |
| `416` | `RANGE_NOT_SATISFIABLE` | Invalid byte range or exceeds file size; includes `Content-Range: bytes */<len>`, `ETag`, `Cache-Control: no-store, no-transform`, and `X-Michi-AirCat-Protocol`. |
| `422` | `PAYLOAD_TOO_LARGE` / `INVALID_FIELDS` / `GRANT_EXPIRED` / `INVALID_GRANT_*` / `INVALID_TRACK_ID` | `/authorize` body exceeds `64 KiB`, expired `expiresAt`, grant window `> 600 s`, non-canonical scopes, or malformed UUID/base64url header. |
| `429` | `RATE_LIMITED` | Rate/concurrency limiter triggered on `/api/v1/auth/{challenge,session}` or batch transfers. Client should back off on demand. |
| `503` | `TLS_NOT_CONFIGURED` / `CONCURRENCY_LIMIT` | Missing `tls.crt`/`tls.key` for `/enrollment-qr`, or hashing semaphore exhausted. Verify `/config/tls.crt` and `/config/tls.key` permissions and restart. |

---

## 4. Log Hygiene & Rate Limiting

- **Never log**: `Authorization: Bearer ...`, `michi_web_session` cookies, `X-Michi-AirCat-Grant`, enrollment QR nonces, private signing keys, or raw OS paths inside API responses.
- `/manifest` enforces bounded 64 KiB hashing via `AIRCAT_HASH_SEMAPHORE` (`4` permits), `10_000` max tracks, and `<= 8 MiB` serialized JSON budget.
- `/authorize` enforces `<= 64 KiB` request payload and `<= 600_000 ms` max grant TTL with a bounded replay nonce ledger (`4096`) and producer snapshot ledger (`1024`).
- `/download/:remote_id` streams 64 KiB chunks with `CompressionLayer` disabled for `application/octet-stream`, re-verifying revocation and `fstat` integrity on every chunk.

---

## 5. Safe Rollback Procedure

1. Drain active transfers / stop container: `docker stop michi-micro-server`.
2. Preserve `/config` (`michi.db`, `home_root_authority.key`, `tls.crt`, `tls.key`) — do **not** delete Home authority or identity keys during software rollback.
3. Revert to previous release image tag or commit SHA (for I02 PR #24 closure on `main`, roll back `83de048` -> `70fef23`; if rolling back Stream NVS revocation hardening, `73a3354` -> `da01191`), then restart service and confirm `GET /health/live` + `GET /api/v1/aircat/status`.
