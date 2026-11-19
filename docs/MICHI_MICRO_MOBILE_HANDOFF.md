# MICHI MICRO SERVER ↔ MICHI MUSIC MOBILE — INTEROP HANDOFF SPEC (R1.1)

Target consumer repository (read-only audit baseline): `pitydah/michi-music-mobile` (`master` @ `07a646e`, inspected working branch `feat/aircat-a71-experience` @ `41b0e2c`).
Normative contract source: `pitydah/michi-link` (`docs/MICHI_INTEROP_CONTRACT.md` & `tests/vectors/interop/micro-mobile-interop-vectors.json`).

> **Scope note**: This document is the authoritative implementation specification for OpenCode / Android engineers updating `michi-music-mobile`. Do **not** invent alternate field names, bypass TLS/X.509/SPKI verification, install trust-all `X509TrustManager` or permissive `HostnameVerifier`, or send Home auth / enrollment / `michi_home_*` tokens over cleartext HTTP.

---

## 0. Verified Target File Inventory in `michi-music-mobile`

Exact files verified in `pitydah/michi-music-mobile` (NOT modified by Micro Server sprint):

| File Path | Required Update |
| --- | --- |
| `michi-link-client/src/main/java/org/michimusic/link/dto/ServerInfoDto.kt` | Add `@SerialName("michi_home_id") val michiHomeId: String? = null`; verify `identity_scheme == "ed25519-blake3-v1"` and `BLAKE3(public_key) == michi_id`. |
| `core/src/main/java/org/michimusic/core/devices/AirCatCapabilityPolicy.kt` | Require ALL FIVE canonical flags (`aircat_original_v1`, `home_tls_spki_pinned`, `aircat_manifest_v1`, `aircat_authorize_v1`, `aircat_original_download_v1`). |
| `michi-link-client/src/main/java/org/michimusic/link/capability/CapabilityResolver.kt` | Consume canonical flags (`sync`, `sync_manifest`, `playback`, `queue`, `multiroom`, AirCat v1 flags) without drifting aliases. |
| `app/src/main/java/org/michimusic/mobile/sync/DeviceActionResolver.kt` | Gate AirCat actions on canonical 5-flag eligibility + active `HOME_MEMBERSHIP` session (capability ≠ authorization). |
| `michi-link-client/src/main/java/org/michimusic/link/LinkClient.kt` | Route ALL Home enrollment, Home mutual auth, AirCat, and `michi_home_*` calls strictly over `pinnedBaseUrl` (HTTPS + OkHttp `CertificatePinner` + default `X509TrustManager`); send canonical `client_michi_id` in `X-Michi-Device-Id` (never `pairedClientDeviceId` UUID); verify `server_signature` over `session_token`; handle `503 REVOCATION_STORE_UNAVAILABLE`, `403 TLS_REQUIRED`, `403 HOME_SESSION_REQUIRED`, `403 MEMBER_REVOKED`. |
| `michi-link-client/src/main/java/org/michimusic/link/LinkClientConfig.kt` | Store separate `HOME_MEMBERSHIP` state vs legacy `SERVER_CODE` pairing state; never pass `SERVER_CODE` token to AirCat endpoints. |
| `app/src/main/java/org/michimusic/mobile/devices/aircat/AirCatHomeEnrollmentStore.kt` | Persist approved `MichiMembership` + `authorityPublicKeyBase64Url` + `pinnedBaseUrl` + `tlsSpkiSha256` after admin-approved enrollment; clear on `403 MEMBER_REVOKED`. |
| `app/src/main/java/org/michimusic/mobile/devices/aircat/AirCatHomeTrustAdmin.kt` | Implement QR scan (`michi.aircat.home.enrollment.v1` trust anchor ONLY) → `/home/enrollment/challenge` → Ed25519 PoP `/home/enrollment/request` → poll `/home/enrollment/{id}/status` with `X-Michi-Enrollment-Claim`. |

---

## 1. Canonical Identity Verification (`GET /api/v1/server/info`)

Michi Micro Server publishes its persistent Ed25519 identity and Home anchor on `GET /api/v1/server/info`:

```json
{
  "service": "michi-micro-server",
  "name": "Michi Micro Server",
  "server_id": "c2b3d4e5-f6a7-8901-bcde-f12345678901",
  "version": "1.0.0-rc.4",
  "deployment_platform": "zimaos",
  "commit": "9f19850",
  "api_version": "v1",
  "identity_scheme": "ed25519-blake3-v1",
  "michi_id": "NKN0pxzkWmTO9G3q4UWDq0oXVPjjU5tE80bIE4RIUcM",
  "public_key": "oJql9HpnWYAv-VX43C0qFKXJnSO-l_hkEn_5ODRVpPA",
  "michi_home_id": "SniHfEoJJvxdXLKCu0XBHISJsgL-GJD6DGWT5hkrn3U",
  "roles": ["music_server", "library_host", "playback_host"],
  "features": {
    "library": true,
    "search": true,
    "streaming": true,
    "download": true,
    "artwork": true,
    "playlists": true,
    "sync": true,
    "sync_manifest": true,
    "import": true,
    "playback": true,
    "queue": true,
    "receivers": true,
    "rooms": true,
    "multiroom": true,
    "events": true,
    "transcoding": true,
    "token_refresh": true,
    "aircat_original_v1": true,
    "home_tls_spki_pinned": true,
    "aircat_manifest_v1": true,
    "aircat_authorize_v1": true,
    "aircat_original_download_v1": true
  },
  "auth": {
    "required": true,
    "strategy": "SERVER_CODE",
    "token_refresh": true
  }
}
```

### Mandatory Identity Checks in Mobile
1. Verify `identity_scheme == "ed25519-blake3-v1"`.
2. Verify `michi_id.length == 43` and `public_key.length == 43` (strict unpadded Base64URL `[A-Za-z0-9_-]`).
3. Verify `base64urlNoPad(BLAKE3(base64urlDecode(public_key))) == michi_id`.
4. Never substitute `server_id` (administrative UUID) for `michi_id`.
5. Discovery (Whisker UDP multicast `224.0.0.167:53318` / mDNS `_michi-link._tcp.local.`) is **strictly provisional**: NEVER automatically send bearer tokens to a newly discovered IP/URL until identity and TLS + SPKI pin are verified.

---

## 2. Capability Normalization (Single Canonical Source)

Micro Server derives `/api/v1/server/info`, `/api/v1/capabilities`, and Whisker `discovery_features` from a **single** `michi_identity::CanonicalServerCapabilities` projected from real runtime module states (`library`, `stream`, `playback`, `queue`, `sync`, `sync_manifest`, `rooms`, `receivers`, `transcoding`, and `aircat_tls_listener_ready`).

Required AirCat v1 eligibility flags in Mobile (`AirCatCapabilityPolicy.kt`):
- `aircat_original_v1 == true`
- `home_tls_spki_pinned == true`
- `aircat_manifest_v1 == true`
- `aircat_authorize_v1 == true`
- `aircat_original_download_v1 == true`

Guarantees:
- If the HTTPS/TLS listener is not active, ALL FIVE flags are `false`.
- If the `library` module is disabled while TLS is active, `home_tls_spki_pinned` may remain `true`, while all four `aircat_*` producer flags flip to `false`.
- Capability flags indicate readiness ONLY — they NEVER imply authorization.

---

## 3. Mandatory HTTPS Transport Policy (`P0-A`)

**ALL Home authority, enrollment, admin approval, AirCat, and `michi_home_*` endpoints MUST be called exclusively over verified HTTPS (`pinnedBaseUrl`).**

Cleartext HTTP is rejected fail-closed (`403 TLS_REQUIRED` when TLS is active; `503 TLS_NOT_CONFIGURED` when TLS is not configured) on ALL 13 routes:
- `POST /api/v1/auth/challenge`
- `POST /api/v1/auth/session`
- `POST /api/v1/home/enrollment/challenge`
- `POST /api/v1/home/enrollment/request`
- `GET  /api/v1/home/enrollment/{request_id}/status`
- `GET  /api/v1/home/enrollment/pending`
- `POST /api/v1/home/enrollment/{request_id}/approve`
- `POST /api/v1/home/enrollment/{request_id}/reject`
- `POST /api/v1/home/members/enroll`
- `GET  /api/v1/aircat/enrollment-qr`
- `GET  /api/v1/aircat/manifest`
- `POST /api/v1/aircat/authorize`
- `GET  /api/v1/aircat/download/{remote_id}`

**AND** ANY `/api/v1/*` request (including `/api/v1/tracks`, `/api/v1/playback/*`, `/api/v1/queue/*`, `/api/v1/sync/*`) carrying `Authorization: Bearer michi_home_*` is ALSO rejected with `403 TLS_REQUIRED` over cleartext HTTP.

---

## 4. Android-Compatible TLS / X.509 / SPKI PKI Architecture (`P0-B`)

### Why Self-Signed Certificates Fail on Android
`michi-music-mobile/michi-link-client/src/main/java/org/michimusic/link/LinkClient.kt` uses OkHttp's `CertificatePinner` **on top of** Android's default platform `X509TrustManager`. OkHttp validates in TWO mandatory layers:
1. **Layer 1 — X.509 Trust Chain + Hostname/SAN + Validity**: The leaf certificate MUST chain to a trusted CA, carry a matching `SubjectAltName` (`DNS` or `IP`) for `pinnedBaseUrl`, and satisfy `notBefore <= now <= notAfter`.
2. **Layer 2 — RFC 7469 SPKI SHA-256 Pin**: `sha256/<BASE64_STANDARD>` matches the pinned leaf (or intermediate) SPKI hash.

Therefore, **a standalone self-signed certificate WILL FAIL with `SSLHandshakeException` on Android even when SAN and SPKI pin match**. Micro Server explicitly detects and surfaces this on `GET /api/v1/aircat/status`:
- `tls_trust_mode: "ca_chained"` → `android_default_trust_compatible: true`
- `tls_trust_mode: "self_signed"` → `android_default_trust_compatible: false` (logs startup warning; WebUI shows warning badge).

### Mode A — Public CA Certificate (PRIMARY, Android-Ready Immediately)
Compatible with Android's default `X509TrustManager` + OkHttp `CertificatePinner` with **zero custom `TrustManager` changes** in Mobile.

Recommended provisioning options for ZimaOS / CasaOS / Docker LAN deployments:
1. **Let's Encrypt via DNS-01 challenge** (e.g., `certbot` / `acme.sh` with a private LAN A/AAAA record pointing `music.home.example.com` to `192.168.1.50` — no inbound internet ports required!). Mount `fullchain.pem` as `/config/tls.crt` and `privkey.pem` as `/config/tls.key`:
   ```bash
   chmod 600 /DATA/AppData/michi/config/tls.key
   # docker-compose / ZimaOS environment:
   # MICHI_TLS_PORT=9443
   # MICHI_PUBLIC_HTTPS_URL=https://music.home.example.com:9443
   ```
2. **Tailscale HTTPS (`tailscale cert`)**: Provisions a real Let's Encrypt certificate for `<machine>.<tailnet>.ts.net` trusted natively by Android, with automatic SAN matching and SPKI extraction:
   ```bash
   tailscale cert \
     --cert-file /DATA/AppData/michi/config/tls.crt \
     --key-file  /DATA/AppData/michi/config/tls.key \
     michi-server.tailnet-name.ts.net
   chmod 600 /DATA/AppData/michi/config/tls.key
   # MICHI_TLS_PORT=9443
   # MICHI_PUBLIC_HTTPS_URL=https://michi-server.tailnet-name.ts.net:9443
   ```

### Mode B — Scoped Home Private CA (`NOT_IMPLEMENTED_IN_MOBILE`)
For strictly air-gapped LANs without DNS-01 or Tailscale:
- **Status**: `NOT_IMPLEMENTED_IN_MOBILE` (future extension; do **NOT** assume Mode B works on current Mobile builds).
- **Required future Mobile design**: Embed the Home CA public cert (or fingerprint) in the enrollment trust anchor and build a dedicated `TrustManagerFactory` scoped **exclusively** to the pinned Home origin (combined with strict hostname/SAN verification + SPKI pin).
- **STRICTLY FORBIDDEN**: Never install a global trust-all `X509TrustManager`, never disable `HostnameVerifier`, and never bypass certificate expiry checks.

### Certificate / Pin Rotation & IP/Hostname Change Policy
1. **Before rotating `/config/tls.crt` + `/config/tls.key`**:
   - Server validates on startup: cert+key cryptographic match (`rustls`), `notBefore <= now <= notAfter`, and X.509 v3 `SubjectAltName` covering `MICHI_PUBLIC_HTTPS_URL`. If any check fails, Micro Server fails closed (refuses TLS activation; all Home/AirCat endpoints return `503 TLS_NOT_CONFIGURED`).
   - If the SPKI changes (new keypair rather than key reuse), Mobile detects pin mismatch on handshake and MUST prompt the user to re-verify/re-scan the admin trust QR (never auto-accept an unverified pin change).
   - Tip: Reusing the same EC P-256 key (`--reuse-key` in ACME/certbot) during routine 90-day Let's Encrypt renewals preserves `tls_spki_sha256` cleanly without breaking Mobile pins.
2. **IP / Hostname change**:
   - Prefer a stable DNS hostname (Mode A) over a DHCP LAN IP.
   - If the LAN IP changes, update `MICHI_PUBLIC_HTTPS_URL`, issue a certificate whose SAN covers the new host/IP, restart Micro Server, and re-bind the trust anchor in Mobile.

---

## 5. Authorized `HOME_MEMBERSHIP` Bootstrap, Durable Queue & Mutual Auth

### 5.1 Trust Anchor QR (`GET /api/v1/aircat/enrollment-qr`, HTTPS + Admin Only)
- Scanned by Mobile from the HTTPS admin WebUI (`type == "michi.aircat.home.enrollment.v1"`).
- Carries `homeId`, `authorityPublicKeyBase64Url`, `sourceMichiId`, `pinnedBaseUrl`, and `tlsSpkiSha256`.
- **CRITICAL**: The QR is ONLY a trust anchor. Scanning it NEVER grants Home membership.

### 5.2 Enrollment Bootstrap Flow (Over Pinned HTTPS)
1. **Challenge**: `POST {pinnedBaseUrl}/api/v1/home/enrollment/challenge`
   - Body: `{ purpose: "home_enrollment", home_id, client_michi_id, client_public_key }`
   - Verify `server_michi_id == sourceMichiId` and `BLAKE3(server_public_key) == server_michi_id`.
2. **Submit PoP Request**: `POST {pinnedBaseUrl}/api/v1/home/enrollment/request`
   - Sign canonical UTF-8 bytes with client Ed25519 private key:
     ```text
     "michi-link-home-enrollment-pop-v1" || home_id || server_michi_id || client_michi_id || client_public_key || challenge_id || challenge_nonce
     ```
   - Allowed roles for `device_type == "mobile"`: **ONLY** `["mobile_player", "remote_controller", "sync_client"]` (requesting server roles returns `422 INVALID_ROLES`).
   - Response `202 Accepted`: `{ request_id, status: "pending", home_id, client_michi_id, expires_at, claim_token }`.
   - Persisted durably in SQLite (`home_enrollment_requests`, storing only `SHA-256(claim_token)` — survives server restarts). Submitting a duplicate request for the same device supersedes any prior pending request.
3. **Administrator Approval (HTTPS Admin WebUI or API)**:
   - Admin inspects `GET /api/v1/home/enrollment/pending` and clicks **Approve** (`POST /api/v1/home/enrollment/{request_id}/approve`) or **Reject** (`POST /api/v1/home/enrollment/{request_id}/reject`).
   - Executed atomically in SQLite `BEGIN IMMEDIATE` (monotonic serial counter in `home_serial_counters`; double approval returns `409 ALREADY_RESOLVED`; revoked devices return `403 MEMBER_REVOKED`).
4. **Poll & Claim Signed `MichiMembership`**:
   - `GET {pinnedBaseUrl}/api/v1/home/enrollment/{request_id}/status` with header `X-Michi-Enrollment-Claim: <claim_token>`.
   - When `status == "approved"`, verify `membership.signature` against pinned `authorityPublicKeyBase64Url` and persist in Android Keystore / EncryptedSharedPreferences.

### 5.3 Mutual Authentication Session, Renewal & Single Session Authority
1. `POST {pinnedBaseUrl}/api/v1/auth/challenge` → `{ challenge_id, challenge_nonce, server_michi_id, server_public_key, expires_in }`.
2. Sign canonical client challenge bytes:
   ```text
   "michi-link-device-auth-v1" || home_id || server_michi_id || client_michi_id || challenge_id || challenge_nonce
   ```
3. `POST {pinnedBaseUrl}/api/v1/auth/session` → `{ session_token, token_type: "Bearer", expires_in: 3600, server_michi_id, server_membership, server_signature }`.
4. **Verify Server Mutual Auth Confirmation** over `session_token` (NOT `challenge_nonce`):
   ```text
   "michi-link-server-auth-v1" || home_id || server_michi_id || client_michi_id || challenge_id || session_token
   ```
5. `michi_home_*` tokens are stored **exclusively** in Micro Server's dedicated Home session store (never duplicated into legacy `SERVER_CODE` `TokenStore`).

---

## 6. `X-Michi-Device-Id` Header Policy & Auth Separation

| Transport / API Class | Required Auth | `X-Michi-Device-Id` Header Rule |
| --- | --- | --- |
| `AIRCAT_STRICT` (`/api/v1/aircat/manifest`, `/authorize`, `/download/{remote_id}`) | **ONLY** `Authorization: Bearer michi_home_...` over pinned HTTPS | **MUST** be omitted OR equal to `<client_michi_id>` (43-char Base64URL). Sending a legacy `SERVER_CODE` UUID triggers `403 IDENTITY_MISMATCH`. |
| General Link APIs (`/api/v1/tracks`, `/stream`, `/playback/*`, `/queue/*`, `/sync/*`) | `HOME_MEMBERSHIP` (`michi_home_...` over HTTPS ONLY) **OR** legacy `SERVER_CODE` token | Send `<client_michi_id>` on Home sessions, or `pairedClientDeviceId` UUID on legacy `SERVER_CODE` sessions. |

---

## 7. AirCat Original-File Wire Contract & Fail-Closed Error Codes

Required response headers:
- `X-Michi-AirCat-Protocol: michi.aircat.original.v1`
- `Cache-Control: no-store` (`no-store, no-transform` on `/download/{remote_id}`)
- On `/download/{remote_id}`: `ETag: "sha256-<hex>-<bytes>"`, `Accept-Ranges: bytes`, `X-Michi-AirCat-Authorized-Grant-SHA256: <lowercase_hex(sha256(ascii(X-Michi-AirCat-Grant)))>`.
- Client MUST NOT send `Accept-Encoding: gzip` without `identity` (`406 TRANSFORM_ENCODING_REJECTED`).

Canonical error codes Mobile MUST handle explicitly:

| HTTP | `error.code` | Action in Mobile |
| --- | --- | --- |
| `503` | `TLS_NOT_CONFIGURED` | Show AirCat/Home TLS not configured on server; never fallback to HTTP |
| `503` | `REVOCATION_STORE_UNAVAILABLE` | Server failed closed due to revocation store error; retry with backoff, never assume authorized |
| `403` | `TLS_REQUIRED` | Refuse cleartext; switch strictly to `pinnedBaseUrl` (HTTPS) |
| `403` | `HOME_SESSION_REQUIRED` | Run Home mutual auth (`/auth/challenge` + `/auth/session`) or enroll first |
| `403` | `IDENTITY_MISMATCH` | Fix `X-Michi-Device-Id` to canonical `client_michi_id` |
| `403` | `WRONG_AUDIENCE` | Ensure `destinationMichiId == client_michi_id` |
| `403` | `MEMBER_REVOKED` | Clear Home session, abort transfer, display revoked state |
| `422` | `INVALID_ROLES` | Request only permitted mobile roles (`mobile_player`, `remote_controller`, `sync_client`) |
| `409` | `ALREADY_RESOLVED` | Enrollment was already approved or rejected |
| `409` | `NONCE_REPLAY` | Generate fresh 24-byte `clientNonceBase64Url` and re-authorize |
| `409` | `FILE_REVISION_CHANGED` | Re-fetch `/api/v1/aircat/manifest` and re-authorize |
| `406` | `TRANSFORM_ENCODING_REJECTED` | Send `Accept-Encoding: identity` |

---

## 8. Michi Control (Playback & Queue) Compatibility

Micro Server supports all Mobile `LinkClient.kt` playback & queue endpoints:
- `GET /api/v1/playback/state`
- `POST /api/v1/playback/control` (accepts Mobile `{"command": "...", "value": ""}`, `position_ms`, `volume`, `track_id`; rejects invalid repeat/shuffle values with `422`)
- `GET /api/v1/playback/output` & `PUT/POST /api/v1/playback/output`
- `GET /api/v1/queue`, `POST /api/v1/queue/items`, `DELETE /api/v1/queue/items/{item_id}`, `POST /api/v1/queue/jump`, `PUT /api/v1/queue/reorder`, `POST /api/v1/queue/repeat`, `POST /api/v1/queue/shuffle`, `POST /api/v1/queue/transfer`, `POST /api/v1/queue/save`, `GET /api/v1/queue/saved`
- `POST /api/v1/playback-transfer/{prepare,commit,abort}`
- `GET /api/v1/sessions/active`

---

## 9. Deterministic Golden Vectors (Copy into Mobile JUnit Tests)

Authoritative fixture file: `pitydah/michi-link/tests/vectors/interop/micro-mobile-interop-vectors.json` (verified by both `michi-link` and `michi-micro-server` automated test suites).

- `home_root`: seed `11...11` (32 bytes) → `home_id = "SniHfEoJJvxdXLKCu0XBHISJsgL-GJD6DGWT5hkrn3U"`, `public_key = "0EqyMnQrtKs6E2i9RhXk5tAiSrcaAWuvhSCjMsl3hzc"`
- `server`: seed `22...22` (32 bytes) → `michi_id = "NKN0pxzkWmTO9G3q4UWDq0oXVPjjU5tE80bIE4RIUcM"`, `public_key = "oJql9HpnWYAv-VX43C0qFKXJnSO-l_hkEn_5ODRVpPA"`
- `mobile_client`: seed `33...33` (32 bytes) → `michi_id = "rz2gnw8LRcZPMln7OHAknoEL5Dg8TFyJ7utfFKJyPlE"`, `public_key = "F8t5-ytBIPKx7GXkGY1uCLKOgT_rAeSkAIObheGAgM4"`
- `home_enrollment_pop.client_signature`: `"aPjc7KhYzS5WiexMph2QLGAtZkGEmZDw62i1Xip203XgSYlZpaFcsHxkZ_5iBp25yatLRlQ4T0ptaguS925FDQ"`
- `home_mutual_auth.client_signature`: `"BJtkvwhY-jRK_uM1wKGOSWZuG7ocJ9eukBdrkHYS5vSlTWCQEkG_1qNvv9Ni8dq03mWQmcrlukbmONrQ7oUjAg"`
- `home_mutual_auth.server_signature` (over `session_token = "michi_home_deterministic_golden_token_v1"`): `"9dc8fk0sVb3TdZdqElA72asiPDhAW8tDcz7HbnlnLtxoV4-K_SUvEp6GCca5yotTK7TewjYeP_0FBb9W2wLjAw"`
- `aircat_grant_v1.signed_grant.signatureBase64Url`: `"QzArcIu04GtNBi0-sesSCeQKpxfiUCsugWqC1w2BOIvvs9s5JCfV7K5N56p6xvAO65Ezfa8-YBe8S8LTnG9KCA"`

---

## 10. RC3 → RC4 / R1.1 ZimaOS / CasaOS / Docker Upgrade Guide

1. **Preserve `/config` volume** (`/DATA/AppData/michi/config`): keeps persistent Ed25519 `identity.json`, Home Root Authority key, and SQLite database (`migration_055` runs automatically on startup, creating `home_enrollment_requests` and `home_serial_counters` non-destructively).
2. **Provision Mode A TLS certificate** (Let's Encrypt DNS-01 or Tailscale HTTPS) into `/DATA/AppData/michi/config/tls.crt` (fullchain) and `/DATA/AppData/michi/config/tls.key` (`chmod 600`).
3. **Configure environment variables**:
   - `MICHI_TLS_PORT=9443`
   - `MICHI_PUBLIC_HTTPS_URL=https://<your-dns-hostname>:9443`
   - Expose TCP `9443` alongside HTTP `9090` (or `network_mode: host` on ZimaOS/CasaOS).
4. **Verify readiness**:
   - Open `https://<your-dns-hostname>:9443` in WebUI → **AirCat Original Transfer & TLS** → click **Inspect AirCat & TLS Pin**.
   - Confirm `TLS Ready: HTTPS ACTIVE` and `TLS Trust Mode: CA-CHAINED (ANDROID COMPATIBLE)`.
   - Confirm `GET /api/v1/server/info` publishes `michi_id`, `public_key`, `identity_scheme: "ed25519-blake3-v1"`, `michi_home_id`, and all 5 AirCat flags `true`.

---

## 11. Minimum Test Scenarios Required in `michi-music-mobile`

1. Golden vector parity test loading `micro-mobile-interop-vectors.json` (Ed25519 PoP, mutual auth client/server signatures over `session_token`, BLAKE3 `michi_id`, AirCat Grant v1 canonical bytes).
2. `ServerInfoDto` parsing + `BLAKE3(public_key) == michi_id` verification + rejection on tampered `michi_id`.
3. `AirCatCapabilityPolicy` requires all 5 canonical flags (`aircat_original_v1`, `home_tls_spki_pinned`, `aircat_manifest_v1`, `aircat_authorize_v1`, `aircat_original_download_v1`).
4. Home enrollment UI/client flow over pinned HTTPS: QR scan → `/home/enrollment/challenge` → Ed25519 PoP `/home/enrollment/request` (`202 pending`) → admin approval on HTTPS WebUI → `/home/enrollment/{id}/status` claim → persisted `MichiMembership`.
5. Mutual auth `/auth/challenge` + `/auth/session` over pinned HTTPS + mandatory `verify_server_auth_session` check + automatic renewal on expiry + immediate lockout on `403 MEMBER_REVOKED` + fail-closed handling of `503 REVOCATION_STORE_UNAVAILABLE`.
6. `LinkClient` sends `HOME_MEMBERSHIP` bearer + canonical `client_michi_id` in `X-Michi-Device-Id` (never UUID) strictly over pinned HTTPS, and refuses cleartext fallback on `403 TLS_REQUIRED` / `503 TLS_NOT_CONFIGURED`.
7. **Physical LAN E2E certification checklist (`NOT_RUN` until executed on real Android device + deployed Micro Server on LAN/ZimaOS)**: Mode A CA-chained HTTPS handshake + SPKI pin → discovery → enrollment → HTTPS WebUI approval → AirCat manifest/authorize/200+206 download + SHA-256 verification → mid-stream revocation → Michi Control playback/queue control.
