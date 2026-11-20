# Michi Mobile Readiness Package (`interop/mobile-readiness/`)

**Version:** KILLCRITIC R1.4 — Server-First Interoperability Readiness  
**Normative Authority:** `pitydah/michi-link` (`8dad0fb51128df98422bd74968e75f0ae8f85ccf`, pinned in `vendor/michi-link`)  
**Server Target:** `pitydah/michi-micro-server` (`1.0.0-rc.4` / PR #26)  
**Consumer Target:** `pitydah/michi-music-mobile` (`NOT_IMPLEMENTED / NOT_AUDITED`)

This directory is the **self-contained technical handoff pack** for OpenCode / Android engineers implementing real Michi Music Mobile interoperability against Michi Micro Server.

## Contents

| File | Purpose |
| --- | --- |
| [`CONTRACT_INDEX.md`](CONTRACT_INDEX.md) | Frozen JSON Schemas, Rust identity crate APIs, and golden vector paths (zero duplication). |
| [`ENDPOINT_MATRIX.md`](ENDPOINT_MATRIX.md) | Authoritative HTTP/HTTPS route table with auth modes, headers, status codes, and response shapes. |
| [`AUTH_STATE_MACHINE.md`](AUTH_STATE_MACHINE.md) | End-to-end Home enrollment, approval, membership verification, mutual auth, session usage, revocation, and reconnect state machine. |
| [`TLS_TRUST.md`](TLS_TRUST.md) | HTTPS enforcement, SPKI pinning (`sha256/<base64>`), Android trust verification boundary, and reverse-proxy rules. |
| [`DISCOVERY.md`](DISCOVERY.md) | Whisker UDP multicast, mDNS `_michi-link._tcp.local.`, Android Wi-Fi/NSD caveats, and provisional trust rules. |
| [`AIRCAT.md`](AIRCAT.md) | AirCat Strict v1 (`michi.aircat.original.v1`) 5-flag eligibility, manifest, signed grant, replay defense, full transfer, and `Range`/`If-Range` resume. |
| [`MICHI_CONTROL.md`](MICHI_CONTROL.md) | Canonical playback control (`command: set_volume` strict no-leading-zero grammar), unified `ActiveQueueCoordinator`, and `GET /api/v1/home/roster` v1. |
| [`ERRORS.md`](ERRORS.md) | Canonical error envelope, status codes, and mandatory client recovery behaviors. |
| [`ANDROID_IMPLEMENTATION_HANDOFF.md`](ANDROID_IMPLEMENTATION_HANDOFF.md) | Concrete Kotlin file map in `pitydah/michi-music-mobile`, task sequence, and verification checklist. |
| [`vectors/r1_4_reference_vectors.json`](vectors/r1_4_reference_vectors.json) | Index of canonical golden vectors in `vendor/michi-link` plus R1.4 schema conformance test payloads. |

## Verification Status Boundary

- **Verified by CI & Local Evidence:**
  - Michi Link contracts, schemas, cross-layer vectors, and Rust `michi-identity` crate (CI 6/6 PASS).
  - Michi Micro Server R1 / R1.1 / R1.2 / R1.3 / R1.4 integration suites + independent 19-phase Pre-Mobile Reference Client over real TCP HTTP and real `tokio-rustls` HTTPS (`crates/michi-api/tests/pre_mobile_reference_client_e2e.rs`).
- **Not Yet Verified (Requires Mobile Implementation + Physical Hardware):**
  - `MICHI_MUSIC_MOBILE_INTEROP_IMPLEMENTATION = NOT_IMPLEMENTED / NOT_AUDITED`
  - `ANDROID_REAL_DEVICE_E2E = NOT_RUN`
  - `ZIMAOS_ANDROID_PHYSICAL_E2E = NOT_RUN`
