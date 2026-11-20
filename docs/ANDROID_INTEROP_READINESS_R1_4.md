# ANDROID INTEROPERABILITY READINESS & ZIMAOS RUNBOOK (KILLCRITIC R1.4)

> **HONESTY BOUNDARY**
> - `MICHI_MUSIC_MOBILE_INTEROP_IMPLEMENTATION = NOT_IMPLEMENTED / NOT_AUDITED`
> - `ANDROID_REAL_DEVICE_E2E = NOT_RUN`
> - `ZIMAOS_ANDROID_PHYSICAL_E2E = NOT_RUN`
>
> This document describes the **frozen, verified R1.4 server-side readiness state** of `pitydah/michi-micro-server` + `pitydah/michi-link` and provides the exact ZimaOS/Docker deployment and physical Android E2E test recipe for when OpenCode completes the Android consumer implementation.

---

## 1. Server-First Architecture Summary

Michi Micro Server exposes a clean separation of concerns across four planes:

1. **Identity & Discovery Plane**:
   - Persistent Ed25519 identity (`ed25519-blake3-v1`) exposed identically via `GET /api/v1/server/info`, Whisker UDP multicast (`224.0.0.167:53318`), and mDNS (`_michi-link._tcp.local.`).
   - Home Root Authority (`michi_home_id`) persists across restarts in SQLite + config store.
2. **Transport & Trust Plane (Fail-Closed HTTPS)**:
   - Native `tokio-rustls` HTTPS listener (`MICHI_TLS_CERT_PATH` + `MICHI_TLS_KEY_PATH` + `MICHI_TLS_PORT`) OR verified trusted reverse proxy (`MICHI_TRUST_PROXY=true`, `MICHI_TRUSTED_PROXIES`, coherent `X-Forwarded-Proto` + `Forwarded`).
   - All Home enrollment, Home auth, AirCat, and `michi_home_*` endpoints reject cleartext HTTP (`403 TLS_REQUIRED` / `503 TLS_NOT_CONFIGURED`).
   - SPKI pin published in native OkHttp format `sha256/<44-char-base64>` (51 chars).
3. **Control & Unified Queue Plane**:
   - `POST /api/v1/playback/control` enforces canonical `command` + strict integer/decimal volume grammar without leading zeros (`^(0|[1-9][0-9]?|100)$`).
   - `ActiveQueueCoordinator` serializes ALL 10 queue mutation paths with `BEGIN IMMEDIATE` transactions (released before audio transitions), monotonic `queue_version`, shuffle permutation preservation, verified compensating rollback, and fail-closed `503 QUEUE_DEGRADED` until explicit `POST /api/v1/queue/reconcile`.
   - `GET /api/v1/home/roster` emits schema-verified `HomeDeviceRoster` v1 (`home-device-roster.schema.json`), separating `receiver_id` from `device_michi_id`, membership from authentication, and bounded live presence from historical SQLite records.
4. **AirCat Strict v1 Plane (`michi.aircat.original.v1`)**:
   - Gated by 5 canonical flags on `GET /api/v1/server/info` + active Home session (`michi_home_*`).
   - Single-use nonce authorization grants (`409` on replay), lossless byte-exact downloads (`application/octet-stream`, `no-store, no-transform`), `ETag` + `Range`/`If-Range` resumable transfers (`206 Partial Content`), and SHA-256 verification.

---

## 2. ZimaOS / CasaOS / Docker Deployment Runbook

### Option A (Recommended): Trusted Reverse Proxy (Caddy / Traefik / Nginx) + Real CA Certificate

```yaml
services:
  michi-micro-server:
    image: ghcr.io/pitydah/michi-micro-server:latest
    network_mode: host # Required for Whisker UDP multicast (53318) & mDNS (_michi-link._tcp)
    environment:
      MICHI_PORT: "9090"
      MICHI_ADVERTISE_HOST: "192.168.1.20" # Real ZimaOS LAN IP
      MICHI_TRUST_PROXY: "true"
      MICHI_TRUSTED_PROXIES: "127.0.0.1,::1" # Exact proxy IP only
      MICHI_PUBLIC_HTTPS_URL: "https://music.home.example.com"
      MICHI_ALLOWED_QR_HOSTS: "music.home.example.com,192.168.1.20"
    volumes:
      - /DATA/Media/Music:/music:ro
      - /DATA/AppData/michi-micro-server/config:/config
      - /DATA/AppData/michi-micro-server/cache:/cache
```

### Option B: Native Direct TLS on ZimaOS LAN (Private CA or Tailscale Cert)

```yaml
    environment:
      MICHI_PORT: "9090"
      MICHI_TLS_PORT: "9443"
      MICHI_TLS_CERT_PATH: "/config/tls/fullchain.pem"
      MICHI_TLS_KEY_PATH: "/config/tls/privkey.pem"
      MICHI_PUBLIC_HTTPS_URL: "https://192.168.1.20:9443" # Must match cert SAN!
      MICHI_ADVERTISE_HOST: "192.168.1.20"
```

> **Certificate SAN Requirement**: Leaf certificate MUST include `SAN` matching the exact host/IP in `MICHI_PUBLIC_HTTPS_URL` (e.g., `IP:192.168.1.20` or `DNS:michi.tailXXXX.ts.net`), `CA:FALSE`, `extendedKeyUsage=serverAuth`, and valid `notBefore`/`notAfter`. For private CAs on Android, install the Root CA on the Android test device (never disable X.509 validation in app code!).

---

## 3. Physical Android ↔ ZimaOS E2E Test Recipe (To Run After OpenCode Implements Mobile)

Once OpenCode finishes the Kotlin integration in `pitydah/michi-music-mobile`, execute these 10 physical verification steps on a real Android device over LAN Wi-Fi:

1. **Discovery**: Open Mobile discovery screen (`MulticastLock` active); confirm ZimaOS server appears via Whisker/mDNS with verified `michi_id` matching `GET /api/v1/server/info`.
2. **TLS & SPKI**: Scan `AirCatTrustedEnrollmentQr` from WebUI; confirm OkHttp connects to `pinnedBaseUrl` with valid X.509 + SPKI pin, and confirm a tampered SPKI pin fails closed.
3. **Enrollment & Approval**: Submit enrollment from Mobile (`202`), approve in Micro Server WebUI (`200`), confirm Mobile claims and cryptographically verifies `DeviceMembershipDto`.
4. **Mutual Auth**: Complete `/api/v1/auth/challenge` → `/api/v1/auth/session`; confirm Mobile verifies `server_signature` and receives `michi_home_*`.
5. **Home Roster Truth**: Verify `GET /api/v1/home/roster` shows the phone as `membership_status: "enrolled"`, `authenticated: true`, `online: true`, `presence_state: "online"`.
6. **Michi Control**: Change volume (`72`), play/pause/next from Mobile; confirm Micro Server reflects state on `/api/v1/playback/state`.
7. **Queue Stability**: Add 5 tracks, enable shuffle, reorder, delete currently playing item from Mobile; confirm `queue_version` increments and `queue_degraded: false`.
8. **AirCat Full + Resume Transfer**: Download a lossless FLAC track via AirCat Strict v1; force Wi-Fi drop mid-transfer, reconnect, confirm `206 Partial Content` resume with `If-Range: <ETag>` and matching final SHA-256.
9. **Revocation**: Revoke the Android device from WebUI; confirm immediate `401`/`403 MEMBER_REVOKED` on Mobile and clean wipe of local Home session + membership.
10. **Restart Resilience**: Restart ZimaOS container; confirm non-revoked enrolled phone reconnects via `/api/v1/auth/challenge` → `/auth/session` without re-enrolling, and revoked phone remains rejected.

---

## 4. Compatibility Matrix (9 Core Scenarios)

| # | Scenario | Server Status (R1.4) | Verification Evidence |
| --- | --- | --- | --- |
| 1 | Direct LAN HTTPS (Native `tokio-rustls` + CA-chained cert + SPKI pin) | **READY** | `pre_mobile_reference_client_e2e.rs` (19/19 PASS), `interop_r1_2_tests.rs` |
| 2 | Trusted Reverse Proxy HTTPS (Caddy/Traefik/Nginx with coherent headers) | **READY** | `interop_r1_2_tests.rs` (`test_r1_2_p0_a`), `interop_r1_3_tests.rs` |
| 3 | Contradictory or Spoofed Reverse Proxy Headers (`X-Forwarded-Proto` vs `Forwarded`) | **FAIL-CLOSED (403/400)** | `interop_r1_2_tests.rs`, `interop_r1_3_tests.rs` |
| 4 | Cleartext HTTP attempt on Home/Enrollment/AirCat/`michi_home_*` | **FAIL-CLOSED (403 `TLS_REQUIRED`)** | `pre_mobile_reference_client_e2e.rs`, `interop_r1_1_tests.rs` |
| 5 | Multi-Client Concurrent Control & Queue Mutations (A + B + Shuffle + Delete) | **READY (`ActiveQueueCoordinator`)** | `interop_r1_4_tests.rs` (`test_r14_01`, `test_r14_05`) |
| 6 | Queue Rollback Failure & Explicit Recovery | **FAIL-CLOSED (`503 QUEUE_DEGRADED` → `/queue/reconcile`)** | `interop_r1_4_tests.rs` (`test_r14_01`) |
| 7 | Home Roster v1 Schema + `receiver_id != device_michi_id` + Revocation | **READY (100% Schema-Validated)** | `interop_r1_4_tests.rs` (`test_r14_02`, `test_r14_03`) |
| 8 | Michi Music Player / Michi Stream Receiver Interoperability (v1-lite + RTP) | **CERTIFIED** | `true_cross_repo_certification_test.rs` + `ci-stream-contract` |
| 9 | Real Physical Android Device + Live ZimaOS Container E2E | **NOT_RUN (Awaiting Mobile Implementation)** | Pending OpenCode sprint on `pitydah/michi-music-mobile` |
