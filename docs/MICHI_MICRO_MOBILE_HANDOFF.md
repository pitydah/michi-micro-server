# MICHI MICRO SERVER ↔ MICHI MUSIC MOBILE — INTEROP HANDOFF SPEC (R1)

Target consumer repository: `pitydah/michi-music-mobile` (baseline audited read-only at `07a646e`).
Normative contract source: `pitydah/michi-link` (`docs/MICHI_INTEROP_CONTRACT.md` & `tests/vectors/interop/micro-mobile-interop-vectors.json`).

> **Scope note**: This document is the exact implementation specification for OpenCode / Android engineers updating `michi-music-mobile`. Do not invent alternate field names, bypass TLS/SPKI verification, or downgrade AirCat requests to cleartext HTTP.

---

## 1. Canonical Identity Verification (`GET /api/v1/server/info`)

Michi Micro Server now publishes its persistent Ed25519 identity and Home anchor on `GET /api/v1/server/info`:

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

### Required Mobile changes (`ServerInfoDto.kt` & identity verifier)
1. Add `@SerialName("michi_home_id") val michiHomeId: String? = null` to `ServerInfoDto`.
2. Verify cryptographic identity coherence before trusting `michi_id`:
   - `identity_scheme == "ed25519-blake3-v1"`
   - `michi_id.length == 43` and `public_key.length == 43` (strict Base64URL without padding, alphabet `[A-Za-z0-9_-]`)
   - `base64urlNoPad(BLAKE3(base64urlDecode(public_key))) == michi_id`
   - Never substitute `server_id` (UUID) for `michi_id`.
3. Cross-check Whisker signed UDP multicast announcements (`224.0.0.167:53318`) and mDNS TXT records (`_michi-link._tcp.local.`):
   - Whisker announces carry the exact same `michi_id`, `public_key`, and `michi_home_id`.
   - Discovery is strictly provisional; NEVER automatically send bearer tokens to a newly discovered IP/URL until identity and TLS SPKI pin are verified.

---

## 2. Capability Normalization (`AirCatCapabilityPolicy.kt` & `DeviceActionResolver.kt`)

### Defect found in Mobile `07a646e`
`core/src/main/java/org/michimusic/core/devices/AirCatCapabilityPolicy.kt` checked legacy aliases (`"aircat"`, `"aircat_transfer"`, `"devices_aircat"`, `"michi_devices_aircat"`, `"devices.aircat"`) and did NOT recognize the normative AirCat v1 capability keys.

### Required Mobile changes
Require ALL FIVE canonical keys in `serverInfo.features` (or `capabilities.features`) to consider a Micro Server AirCat-ready:
- `aircat_original_v1 == true`
- `home_tls_spki_pinned == true`
- `aircat_manifest_v1 == true`
- `aircat_authorize_v1 == true`
- `aircat_original_download_v1 == true`

Note:
- Micro Server sets ALL FIVE flags to `false` unless the `tokio-rustls` HTTPS listener with an Android-valid SAN certificate and SPKI pin is actively bound and serving.
- Micro Server now also advertises `"sync": true` alongside `"sync_manifest": true`, aligning cleanly with `CapabilityResolver.isSyncHost`.
- Capability flags indicate readiness ONLY — they NEVER imply authorization.

---

## 3. Authorized `HOME_MEMBERSHIP` Bootstrap & Mutual Auth

### 3.1 Trust Anchor QR (`GET /api/v1/aircat/enrollment-qr`)
Scanned by Mobile from the administrator's WebUI (`type == "michi.aircat.home.enrollment.v1"`):
- Stores `homeId`, `authorityPublicKeyBase64Url`, `sourceMichiId`, `pinnedBaseUrl` (`https://...`), and `tlsSpkiSha256` (`sha256/...`).
- **CRITICAL**: The QR is ONLY a trust anchor (Home Root public key + pinned HTTPS origin + SPKI pin). Scanning it does NOT grant Home membership.

### 3.2 Enrollment Bootstrap Flow (Over Pinned HTTPS)
1. **Request enrollment PoP challenge**:
   - `POST {pinnedBaseUrl}/api/v1/home/enrollment/challenge`
   - Body:
     ```json
     {
       "purpose": "home_enrollment",
       "home_id": "<43-char homeId>",
       "client_michi_id": "<43-char BLAKE3(client_public_key)>",
       "client_public_key": "<43-char Ed25519 pk>"
     }
     ```
   - Response `200 OK`: `{ challenge_id, challenge_nonce, home_id, server_michi_id, server_public_key, expires_in }`.
   - Verify `server_michi_id == sourceMichiId` from the QR trust anchor and `BLAKE3(server_public_key) == server_michi_id`.

2. **Sign Ed25519 proof-of-possession and submit request**:
   - Canonical UTF-8 bytes to sign with client Ed25519 private key:
     ```text
     "michi-link-home-enrollment-pop-v1" || home_id || server_michi_id || client_michi_id || client_public_key || challenge_id || challenge_nonce
     ```
   - `POST {pinnedBaseUrl}/api/v1/home/enrollment/request`
   - Body:
     ```json
     {
       "home_id": "<home_id>",
       "challenge_id": "<uuid>",
       "client_michi_id": "<client_michi_id>",
       "client_public_key": "<client_public_key>",
       "device_name": "Pixel 9 Michi Mobile",
       "device_type": "mobile",
       "requested_roles": ["mobile_player", "remote_controller", "sync_client"],
       "client_signature": "<86-char base64url Ed25519 sig>"
     }
     ```
   - Response `202 Accepted`: `{ request_id, status: "pending", home_id, client_michi_id, expires_at, claim_token }`.
   - Show UI prompt: *"Waiting for administrator approval on Michi Micro Server"*.
   - **Never** treat `claim_token` as an API bearer token.

3. **Administrator approves on Micro Server**:
   - Admin reviews `GET /api/v1/home/enrollment/pending` and approves via `POST /api/v1/home/enrollment/{request_id}/approve` (admin-only `is_admin == true`).

4. **Poll status & persist signed `MichiMembership`**:
   - `GET {pinnedBaseUrl}/api/v1/home/enrollment/{request_id}/status`
   - Header: `X-Michi-Enrollment-Claim: <claim_token>`
   - Response `200 OK`: `{ request_id, status: "pending"|"approved"|"rejected"|"expired", home_id, home_root_public_key?, membership? }`.
   - When `status == "approved"`:
     - Verify `membership.signature` against the QR-pinned `authorityPublicKeyBase64Url` using `canonical_membership_bytes`.
     - Persist `membership` securely in Android Keystore / EncryptedSharedPreferences.

### 3.3 Mutual Authentication Session, Renewal & Reconnection
1. `POST {pinnedBaseUrl}/api/v1/auth/challenge`
   - Body: `{ home_id, client_michi_id, client_public_key }`
   - Response `200 OK`: `{ challenge_id, challenge_nonce, server_michi_id, server_public_key, expires_in }`.
2. Sign canonical client challenge bytes with client Ed25519 private key:
   ```text
   "michi-link-device-auth-v1" || home_id || server_michi_id || client_michi_id || challenge_id || challenge_nonce
   ```
3. `POST {pinnedBaseUrl}/api/v1/auth/session`
   - Body: `{ challenge_id, client_michi_id, membership, client_signature }`
   - Response `200 OK`: `{ session_token, token_type: "Bearer", expires_in: 3600, server_michi_id, server_membership, server_signature }`.
4. **CRITICAL — Verify Server Mutual Authentication Confirmation**:
   - Verify `server_membership` against pinned `authorityPublicKeyBase64Url` and verify `server_membership.device_michi_id == server_michi_id`.
   - Verify `server_signature` with `server_membership.device_public_key` over:
     ```text
     "michi-link-server-auth-v1" || home_id || server_michi_id || client_michi_id || challenge_id || session_token
     ```
     (Note: signs `session_token`, NOT `challenge_nonce`!)
   - Only after verification succeeds, store `session_token` (`michi_home_*`) in memory/secure store.
5. **Renewal / Reconnection**:
   - On `401` or before `expires_in` elapses, re-run `POST /api/v1/auth/challenge` + `POST /api/v1/auth/session` using the persisted `membership`.
   - If the server returns `403 MEMBER_REVOKED`, clear the session, mark the Home trust binding revoked in UI, and do NOT retry silently.

---

## 4. `X-Michi-Device-Id` Header Policy & Auth Separation

| Transport / API Class | Required Auth | `X-Michi-Device-Id` Header Rule |
| --- | --- | --- |
| `AIRCAT_STRICT` (`/api/v1/aircat/manifest`, `/authorize`, `/download/{remote_id}`) | **ONLY** `Authorization: Bearer michi_home_...` over pinned HTTPS | **MUST** be omitted OR equal to `<client_michi_id>` (43-char Base64URL). Sending a legacy `SERVER_CODE` UUID triggers `403 IDENTITY_MISMATCH`. |
| General Link APIs (`/api/v1/tracks`, `/stream`, `/playback/*`, `/queue/*`, `/sync/*`) | `HOME_MEMBERSHIP` (`michi_home_...`) **OR** legacy `SERVER_CODE` token | Send `<client_michi_id>` on Home sessions, or `pairedClientDeviceId` UUID on legacy `SERVER_CODE` sessions. |

### Required Mobile fix in `LinkClient.kt`
- In `fetchAirCatManifest()`, `authorizeAirCatTransfer()`, and AirCat original downloads:
  - Use the active `HOME_MEMBERSHIP` bearer token (`michi_home_*`), NEVER a legacy `SERVER_CODE` pairing token (which Micro Server now rejects with `403 HOME_SESSION_REQUIRED`).
  - Set `X-Michi-Device-Id` to the client's canonical 43-char `client_michi_id` (NEVER `pairedClientDeviceId` UUID), and set `intent.destinationMichiId = client_michi_id`.

---

## 5. TLS / SPKI Pinning & Server Deployment Configuration

### Android TLS Rules
1. Every AirCat request MUST target `pinnedBaseUrl` (`https://...`). Cleartext HTTP returns `403 TLS_REQUIRED` (or `503 TLS_NOT_CONFIGURED` if TLS is off).
2. Enforce BOTH:
   - Standard Android X.509 certificate hostname/SAN verification (Micro Server refuses to activate AirCat TLS if `/config/tls.crt` lacks a SubjectAltName matching `MICHI_PUBLIC_HTTPS_URL`).
   - RFC 7469 SPKI SHA-256 pinning (`tlsSpkiSha256`, format `sha256/<BASE64_STANDARD>`).
3. Never install a trust-all `X509TrustManager` or permissive `HostnameVerifier`.

### Generating a Compatible Certificate for Docker / CasaOS / ZimaOS
```bash
# Replace 192.168.1.50 and michi.local with your real LAN IP / hostname
openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 \
  -keyout /DATA/AppData/michi/config/tls.key \
  -out /DATA/AppData/michi/config/tls.crt \
  -days 825 -nodes \
  -subj "/CN=michi.local" \
  -addext "subjectAltName=IP:192.168.1.50,DNS:michi.local"
chmod 600 /DATA/AppData/michi/config/tls.key
```
Set environment variables:
- `MICHI_TLS_PORT=9443`
- `MICHI_PUBLIC_HTTPS_URL=https://192.168.1.50:9443`

---

## 6. AirCat Original-File Wire Contract & Error Codes

Required headers on responses:
- `X-Michi-AirCat-Protocol: michi.aircat.original.v1`
- `Cache-Control: no-store` (`no-store, no-transform` on `/download/{remote_id}`)
- On `/download/{remote_id}`: `ETag: "sha256-<hex>-<bytes>"`, `Accept-Ranges: bytes`, `X-Michi-AirCat-Authorized-Grant-SHA256: <lowercase_hex(sha256(ascii(X-Michi-AirCat-Grant)))>`.
- Supports `Range: bytes=start-end`, `If-Range: <strong-etag>`, `If-None-Match: <strong-etag>` (304), 206 Partial Content, 416 Range Not Satisfiable.
- Client MUST NOT send `Accept-Encoding: gzip` without `identity` (server responds `406 TRANSFORM_ENCODING_REJECTED`).

Canonical error codes Mobile MUST handle explicitly:

| HTTP | `error.code` | Action in Mobile |
| --- | --- | --- |
| `503` | `TLS_NOT_CONFIGURED` | Show AirCat TLS not configured on server; never fallback to HTTP |
| `403` | `TLS_REQUIRED` | Refuse cleartext; switch strictly to `pinnedBaseUrl` |
| `403` | `HOME_SESSION_REQUIRED` | Run Home mutual auth (`/auth/challenge` + `/auth/session`) or enroll first |
| `403` | `IDENTITY_MISMATCH` | Fix `X-Michi-Device-Id` to canonical `client_michi_id` |
| `403` | `WRONG_AUDIENCE` | Ensure `destinationMichiId == client_michi_id` |
| `403` | `MEMBER_REVOKED` | Clear Home session, abort transfer, display revoked state |
| `409` | `NONCE_REPLAY` | Generate fresh 24-byte `clientNonceBase64Url` and re-authorize |
| `409` | `FILE_REVISION_CHANGED` | Re-fetch `/api/v1/aircat/manifest` and re-authorize |
| `406` | `TRANSFORM_ENCODING_REJECTED` | Send `Accept-Encoding: identity` |

---

## 7. Michi Control (Playback & Queue) Compatibility

Micro Server supports all Mobile `LinkClient.kt` playback & queue endpoints:
- `GET /api/v1/playback/state`
- `POST /api/v1/playback/control` (accepts Mobile `{"command": "...", "value": ""}`, `position_ms`, `volume`, `track_id`; rejects invalid repeat/shuffle values with `422`)
- `GET /api/v1/playback/output` & `PUT/POST /api/v1/playback/output`
- `GET /api/v1/queue`, `POST /api/v1/queue/items`, `DELETE /api/v1/queue/items/{item_id}`, `POST /api/v1/queue/jump`, `PUT /api/v1/queue/reorder`, `POST /api/v1/queue/repeat`, `POST /api/v1/queue/shuffle`, `POST /api/v1/queue/transfer`, `POST /api/v1/queue/save`, `GET /api/v1/queue/saved`
- `POST /api/v1/playback-transfer/{prepare,commit,abort}`
- `GET /api/v1/sessions/active`

---

## 8. Deterministic Golden Vectors (Copy into Mobile JUnit Tests)

Authoritative fixture file: `pitydah/michi-link/tests/vectors/interop/micro-mobile-interop-vectors.json` (verified by both `michi-link` and `michi-micro-server` automated test suites).

- `home_root`: seed `11...11` (32 bytes) → `home_id = "SniHfEoJJvxdXLKCu0XBHISJsgL-GJD6DGWT5hkrn3U"`, `public_key = "0EqyMnQrtKs6E2i9RhXk5tAiSrcaAWuvhSCjMsl3hzc"`
- `server`: seed `22...22` (32 bytes) → `michi_id = "NKN0pxzkWmTO9G3q4UWDq0oXVPjjU5tE80bIE4RIUcM"`, `public_key = "oJql9HpnWYAv-VX43C0qFKXJnSO-l_hkEn_5ODRVpPA"`
- `mobile_client`: seed `33...33` (32 bytes) → `michi_id = "rz2gnw8LRcZPMln7OHAknoEL5Dg8TFyJ7utfFKJyPlE"`, `public_key = "F8t5-ytBIPKx7GXkGY1uCLKOgT_rAeSkAIObheGAgM4"`
- `home_enrollment_pop.client_signature`: `"aPjc7KhYzS5WiexMph2QLGAtZkGEmZDw62i1Xip203XgSYlZpaFcsHxkZ_5iBp25yatLRlQ4T0ptaguS925FDQ"`
- `home_mutual_auth.client_signature`: `"BJtkvwhY-jRK_uM1wKGOSWZuG7ocJ9eukBdrkHYS5vSlTWCQEkG_1qNvv9Ni8dq03mWQmcrlukbmONrQ7oUjAg"`
- `home_mutual_auth.server_signature` (over `session_token = "michi_home_deterministic_golden_token_v1"`): `"9dc8fk0sVb3TdZdqElA72asiPDhAW8tDcz7HbnlnLtxoV4-K_SUvEp6GCca5yotTK7TewjYeP_0FBb9W2wLjAw"`
- `aircat_grant_v1.signed_grant.signatureBase64Url`: `"QzArcIu04GtNBi0-sesSCeQKpxfiUCsugWqC1w2BOIvvs9s5JCfV7K5N56p6xvAO65Ezfa8-YBe8S8LTnG9KCA"`

---

## 9. Minimum Test Scenarios Required in `michi-music-mobile`

1. Golden vector parity test loading `micro-mobile-interop-vectors.json` (Ed25519 PoP, mutual auth client/server signatures over `session_token`, BLAKE3 `michi_id`, AirCat Grant v1 canonical bytes).
2. `ServerInfoDto` parsing + `BLAKE3(public_key) == michi_id` verification + rejection on tampered `michi_id`.
3. `AirCatCapabilityPolicy` requires all 5 canonical flags (`aircat_original_v1`, `home_tls_spki_pinned`, `aircat_manifest_v1`, `aircat_authorize_v1`, `aircat_original_download_v1`).
4. Home enrollment UI/client flow: QR scan → `/home/enrollment/challenge` → Ed25519 PoP `/home/enrollment/request` (`202 pending`) → admin approval → `/home/enrollment/{id}/status` claim → persisted `MichiMembership`.
5. Mutual auth `/auth/challenge` + `/auth/session` + mandatory `verify_server_auth_session` check + automatic renewal on expiry + immediate lockout on `403 MEMBER_REVOKED`.
6. `LinkClient` sends `HOME_MEMBERSHIP` bearer + canonical `client_michi_id` in `X-Michi-Device-Id` (never UUID) on AirCat endpoints over pinned HTTPS, and surfaces `403 HOME_SESSION_REQUIRED` / `403 TLS_REQUIRED` / `503 TLS_NOT_CONFIGURED` cleanly.
7. Physical LAN E2E certification checklist (to be executed on real Android device + deployed Michi Micro Server on LAN/ZimaOS): discovery → enrollment → approval → pinned HTTPS AirCat manifest/authorize/200+206 download + SHA-256 verification → mid-stream revocation → Michi Control playback/queue control.
