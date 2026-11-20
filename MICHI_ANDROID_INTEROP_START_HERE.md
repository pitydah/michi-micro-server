# MICHI MICRO SERVER ↔ MICHI MUSIC MOBILE — START HERE (R1.4)

> **HONESTY BOUNDARY — READ FIRST**
> - `MICHI_MUSIC_MOBILE_INTEROP_IMPLEMENTATION = NOT_IMPLEMENTED / NOT_AUDITED`
> - `ANDROID_REAL_DEVICE_E2E = NOT_RUN`
> - `ZIMAOS_ANDROID_PHYSICAL_E2E = NOT_RUN`
>
> **Michi Micro Server** and **Michi Link** are frozen and certified at **R1.4 (`Server-First Interoperability Readiness`)**. Every server-side protocol, identity, TLS, Home Membership, Home Roster, Michi Control, Queue Coordinator, and AirCat Strict contract is implemented, schema-validated, and proven over real TCP HTTP + real `tokio-rustls` HTTPS by an independent 19-phase Pre-Mobile Reference Client (`crates/michi-api/tests/pre_mobile_reference_client_e2e.rs`).
>
> **OpenCode / Android engineers**: Your mission starts here. Implement the consumer in `pitydah/michi-music-mobile` strictly against these frozen v1 contracts — do **not** invent field aliases, weaken TLS/X.509/SPKI verification, or expect structural changes on Micro Server.

---

## 1. Authoritative Reading Order for OpenCode

1. **[`interop/mobile-readiness/README.md`](interop/mobile-readiness/README.md)** — Package map, non-negotiable security invariants, and golden verification checklist.
2. **[`interop/mobile-readiness/CONTRACT_INDEX.md`](interop/mobile-readiness/CONTRACT_INDEX.md)** — Exact JSON Schemas and golden vectors pinned in `vendor/michi-link` (`8dad0fb51128df98422bd74968e75f0ae8f85ccf`).
3. **[`interop/mobile-readiness/ENDPOINT_MATRIX.md`](interop/mobile-readiness/ENDPOINT_MATRIX.md)** — Every HTTP/HTTPS endpoint, transport requirement, auth mode, headers, and status codes.
4. **[`interop/mobile-readiness/AUTH_STATE_MACHINE.md`](interop/mobile-readiness/AUTH_STATE_MACHINE.md)** — Enrollment → Approval → Membership Verification → Mutual Challenge-Response → Active Session → Revocation → Reconnect.
5. **[`interop/mobile-readiness/TLS_TRUST.md`](interop/mobile-readiness/TLS_TRUST.md)** — Mandatory HTTPS (`pinnedBaseUrl`), OkHttp SPKI pinning (`sha256/<44-char-base64>`), Android Network Security Config, and dual-CA truth semantics.
6. **[`interop/mobile-readiness/DISCOVERY.md`](interop/mobile-readiness/DISCOVERY.md)** — Whisker UDP multicast (`224.0.0.167:53318`), mDNS (`_michi-link._tcp.local.`), Android `MulticastLock` / NSD, and provisional-until-verified rules.
7. **[`interop/mobile-readiness/AIRCAT.md`](interop/mobile-readiness/AIRCAT.md)** — AirCat Strict v1 (`michi.aircat.original.v1`): 5-flag capability gate, manifest, signed grant, replay defense, original download, and `Range`/`If-Range` resume.
8. **[`interop/mobile-readiness/MICHI_CONTROL.md`](interop/mobile-readiness/MICHI_CONTROL.md)** — Playback control (`command: set_volume` strict grammar without leading zeros), unified `ActiveQueueCoordinator` (`queue_version`, `503 QUEUE_DEGRADED`, `POST /api/v1/queue/reconcile`), and Home Roster v1.
9. **[`interop/mobile-readiness/ERRORS.md`](interop/mobile-readiness/ERRORS.md)** — Complete error envelope and client handling rules (`403 TLS_REQUIRED`, `403 MEMBER_REVOKED`, `503 REVOCATION_STORE_UNAVAILABLE`, `503 QUEUE_DEGRADED`, etc.).
10. **[`interop/mobile-readiness/ANDROID_IMPLEMENTATION_HANDOFF.md`](interop/mobile-readiness/ANDROID_IMPLEMENTATION_HANDOFF.md)** — Exact Kotlin target file checklist in `pitydah/michi-music-mobile`, step-by-step sprint plan, and Android test plan.
11. **[`docs/ANDROID_INTEROP_READINESS_R1_4.md`](docs/ANDROID_INTEROP_READINESS_R1_4.md)** — Full architecture, ZimaOS/Docker deployment runbook, physical LAN test recipe, and 9-scenario compatibility matrix.
12. **[`docs/MICHI_MICRO_MOBILE_HANDOFF.md`](docs/MICHI_MICRO_MOBILE_HANDOFF.md)** — Detailed DTO & cryptographic canonical byte reference (R1.4).

---

## 2. Frozen Baseline Commits & Artifacts

| Component | Repository / Path | Status |
| --- | --- | --- |
| Normative Contracts & Schemas | `pitydah/michi-link` (`main` @ `8dad0fb51128df98422bd74968e75f0ae8f85ccf`, vendored at `vendor/michi-link`) | Frozen R1.4 (CI 6/6 PASS) |
| Server Implementation | `pitydah/michi-micro-server` (`refactor/killcritic-interop-r1`, PR #26) | Frozen R1.4 Server-Ready |
| Independent 19-Phase Network Reference Client | `crates/michi-api/tests/pre_mobile_reference_client_e2e.rs` | Emits `target/release-evidence/pre-mobile-reference-client-receipt.json` |
| R1.4 Adversarial, Schema & Concurrency Suite | `crates/michi-api/tests/interop_r1_4_tests.rs` | 5/5 PASS |
| Consumer Android App | `pitydah/michi-music-mobile` | **NOT_IMPLEMENTED / NOT_AUDITED** (read-only during R1.4) |

---

## 3. Non-Negotiable Client Rules (Zero Exceptions)

1. **Identity First**: Always verify `identity_scheme == "ed25519-blake3-v1"` and `base64urlNoPad(BLAKE3(base64urlDecode(public_key))) == michi_id` before trusting any server. Never confuse `server_id` (UUID) with `michi_id` (43-char Base64URL).
2. **HTTPS Only for Home & AirCat**: Never send enrollment, Home auth, `michi_home_*` bearer tokens, or AirCat traffic over cleartext HTTP. Never use trust-all `X509TrustManager` or permissive `HostnameVerifier`. Always combine standard X.509 validation with SPKI pinning (`sha256/<44-char-base64>`).
3. **Canonical Headers**: Send the client's 43-char `client_michi_id` in `X-Michi-Device-Id` (never a random UUID).
4. **Protocol Separation**: Legacy `SERVER_CODE` pairing (`/api/v1/pair/*`) and Home Membership (`/api/v1/home/enrollment/*`, `/api/v1/auth/*`) are distinct domains. Never send a `SERVER_CODE` token to AirCat endpoints; never accept legacy QR (`legacy_server_code_v1`) as a cryptographic Home trust anchor.
5. **Fail-Closed Handling**: On `403 MEMBER_REVOKED`, immediately clear cached Home session + membership and transition to revoked UX. On `503 REVOCATION_STORE_UNAVAILABLE` or `503 QUEUE_DEGRADED`, surface degraded status and never fabricate success.
