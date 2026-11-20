# Android Implementation Handoff for OpenCode (`pitydah/michi-music-mobile`)

> **READ-ONLY AUDIT BASELINE**: `pitydah/michi-music-mobile` (`feat/aircat-a71-experience` @ `6e529d6e2a86a400c213a0d2cc847aa591eb4523`).
> **STATUS**: `MICHI_MUSIC_MOBILE_INTEROP_IMPLEMENTATION = NOT_IMPLEMENTED / NOT_AUDITED`.

## 1. Exact Target Kotlin File Inventory

| Target File in `pitydah/michi-music-mobile` | Required Implementation Work |
| --- | --- |
| `michi-link-client/src/main/java/org/michimusic/link/dto/ServerInfoDto.kt` | Parse `michi_id`, `public_key`, `identity_scheme`, `michi_home_id`, `roles`, and all 5 AirCat feature flags (`aircat_original_v1`, `home_tls_spki_pinned`, `aircat_manifest_v1`, `aircat_authorize_v1`, `aircat_original_download_v1`). Enforce `BLAKE3(public_key) == michi_id` and `identity_scheme == "ed25519-blake3-v1"`. |
| `core/src/main/java/org/michimusic/core/devices/AirCatCapabilityPolicy.kt` | Gate AirCat readiness strictly on all 5 canonical flags. |
| `michi-link-client/src/main/java/org/michimusic/link/capability/CapabilityResolver.kt` | Map `/api/v1/server/info` + `/api/v1/capabilities` without legacy drift aliases. |
| `app/src/main/java/org/michimusic/mobile/sync/DeviceActionResolver.kt` | Require BOTH 5-flag eligibility AND active `HOME_MEMBERSHIP` session (`michi_home_*`) before enabling AirCat actions. |
| `michi-link-client/src/main/java/org/michimusic/link/LinkClient.kt` | Implement OkHttp `CertificatePinner` (`tlsSpkiSha256` format `sha256/<44-char-base64>`) + standard `X509TrustManager` + default `HostnameVerifier`; route ALL Home & AirCat calls exclusively to `pinnedBaseUrl`; send `X-Michi-Device-Id: <client_michi_id>`; verify `server_signature` on `/api/v1/auth/session`; implement `command: set_volume` with integer `volume` (never leading-zero strings); implement `queue_version` / `503 QUEUE_DEGRADED` / `POST /api/v1/queue/reconcile`; handle `403 TLS_REQUIRED`, `403 MEMBER_REVOKED`, `429`, `503 REVOCATION_STORE_UNAVAILABLE`. |
| `michi-link-client/src/main/java/org/michimusic/link/LinkClientConfig.kt` | Maintain strict separation between `HOME_MEMBERSHIP` state and legacy `SERVER_CODE` state. |
| `app/src/main/java/org/michimusic/mobile/devices/aircat/AirCatHomeEnrollmentStore.kt` | Persist encrypted Ed25519 client keypair (Android Keystore-backed / EncryptedSharedPreferences), signed `DeviceMembershipDto`, `authorityPublicKeyBase64Url`, `pinnedBaseUrl`, and `tlsSpkiSha256`. Wipe cleanly on `403 MEMBER_REVOKED`. |
| `app/src/main/java/org/michimusic/mobile/devices/aircat/AirCatHomeTrustAdmin.kt` | Parse `michi.aircat.home.enrollment.v1` QR → `POST /api/v1/home/enrollment/challenge` → Ed25519 PoP → `POST /api/v1/home/enrollment/request` → poll `GET /api/v1/home/enrollment/{id}/status` with `X-Michi-Enrollment-Claim` → cryptographically verify `DeviceMembershipDto` with `authorityPublicKeyBase64Url`. |

## 2. Recommended OpenCode Implementation Order

1. **Crypto & Golden Vectors First (JVM Unit Tests)**:
   - Implement Ed25519 + BLAKE3 `MichiId` derivation, `michi-home-enrollment-pop-v1`, `michi-home-membership-v1`, `michi-home-auth-v1`, `michi-home-auth-server-v1`, and `michi-aircat-grant-v1` canonical byte builders/verifiers.
   - Verify byte-for-byte against `vendor/michi-link/tests/vectors/interop/micro-mobile-interop-vectors.json` and `vendor/michi-link/tests/vectors/aircat/aircat-golden-v1.json`.
2. **TLS + OkHttp Client (`TLS_TRUST.md`)**:
   - Configure `CertificatePinner` + strict X.509 validation. Write MockWebServer / local TLS unit tests proving wrong-SPKI and wrong-CA both fail closed.
3. **Enrollment & Mutual Auth (`AUTH_STATE_MACHINE.md`)**:
   - Wire QR scan → challenge → request → claim → verify membership → mutual auth → verify `server_signature` → store `michi_home_*`.
4. **Michi Control, Queue & Home Roster (`MICHI_CONTROL.md`)**:
   - Consume `GET /api/v1/home/roster` (`version: 1`), `POST /api/v1/playback/control`, `GET /api/v1/playback/state`, and `/api/v1/queue*` (`queue_version`, `queue_degraded`, `/reconcile`).
5. **AirCat Strict v1 (`AIRCAT.md`)**:
   - Manifest fetch → canonical manifest SHA-256 → authorize grant → download with `X-Michi-AirCat-Grant` → `Range` + `If-Range` resume → SHA-256 verification.
6. **Physical LAN / ZimaOS E2E Validation**:
   - Run the physical test recipe in `docs/ANDROID_INTEROP_READINESS_R1_4.md` against a live ZimaOS / Docker Micro Server and real Android device to promote `ANDROID_REAL_DEVICE_E2E` and `ZIMAOS_ANDROID_PHYSICAL_E2E` from `NOT_RUN` to verified!
