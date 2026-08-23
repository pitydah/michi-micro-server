# MICHI MICRO SERVER — PROJECT STATUS & CERTIFICATION READINESS

**CURRENT VERSION:** v0.2.0  
**CURRENT PHASE:** R3.1 — CERTIFICATION BASELINE (PINNED & HARNESS VERIFIED)  
**STATUS DATE:** 2026-08-23  
**PINNED NORMATIVE REFERENCES:**
- `pitydah/michi-link` @ `1b0684a9457beb0f8d78b491af16a06541f8508d` (submodule `vendor/michi-link`)
- `pitydah/michi-music-stream` @ `05265da00d9186dbae810a4cbad448ed6438f943` (submodule `vendor/michi-music-stream`)

---

## 1. ARCHITECTURAL OVERVIEW

```text
Controller (Mobile Client / Web UI)
    │
    │ Michi Link (v1 WebSocket & HTTP JSON)
    ▼
Michi Micro Server (Rust / Tokio / Axum)
    │
    ├── ReceiverSessionManager (ActiveSession authority, persistent Identity)
    ├── Heartbeat Task (Managed background monotonic keepalive)
    └── AudioTransport (RtpReceiverTransport, 10ms / 1920B buffered PCM)
             │
             │ RTP / UDP (Payload Type 97, SSRC authoritative)
             ▼
      Michi Music Stream (ESP32-S3 / Linux Simulator)
```

**Canonical Operational Directive:**
> MOBILE CONTROLS. MICHI SERVER ORCHESTRATES. MICHI SERVER STREAMS. MICHI STREAM PLAYS.

---

## 2. EVIDENCE MATRIX

| COMPONENT / FEATURE | IMPLEMENTATION STATUS | EVIDENCE LEVEL | STATUS |
|---|---|---|---|
| **Identity & Persistent Ed25519 Signing** | `IMPLEMENTED` | `UNIT_PASS`, `CONTRACT_PASS` | Verified |
| **Discovery Announcer (mDNS & Multicast UDP)** | `IMPLEMENTED` | `UNIT_PASS`, `NETWORK_E2E_PASS` | Verified (derived from ServerCapabilities) |
| **Receiver 2-Step Pairing Lifecycle** | `IMPLEMENTED` | `UNIT_PASS`, `CONTRACT_PASS`, `NETWORK_E2E_PASS` | Verified |
| **PIN Retry & Expiration Management** | `IMPLEMENTED` | `UNIT_PASS`, `NETWORK_E2E_PASS` | Verified |
| **Negotiated Session DTO & Strict Schema** | `IMPLEMENTED` | `UNIT_PASS`, `CONTRACT_PASS`, `NETWORK_E2E_PASS` | Verified |
| **Single SSRC Authority & Negotiated Port** | `IMPLEMENTED` | `UNIT_PASS`, `NETWORK_E2E_PASS` | Verified |
| **RTP Source Port Equality (`local_port` == Stream `source_port`)** | `IMPLEMENTED` | `UNIT_PASS`, `NETWORK_E2E_PASS` | Verified |
| **Transactional Teardown (`Closing`/`Failed`)** | `IMPLEMENTED` | `UNIT_PASS`, `NETWORK_E2E_PASS` | Verified |
| **RTP Packetization (1920B, +480 frames, PT 97)** | `IMPLEMENTED` | `UNIT_PASS`, `NETWORK_E2E_PASS` | Verified |
| **AudioTransport Buffer (`pending_pcm`)** | `IMPLEMENTED` | `UNIT_PASS`, `NETWORK_E2E_PASS` | Verified |
| **Managed Monotonic Heartbeat (Autonomous Background Task)** | `IMPLEMENTED` | `UNIT_PASS`, `NETWORK_E2E_PASS` | Verified |
| **Three-Way Ecosystem Flow (Mobile->Micro->Stream)** | `IMPLEMENTED` | `LOCAL_HARNESS_PASS` | Verified (8/8 phases via test harness) |
| **Canonical Stream Simulator Direct Execution** | `PENDING_HARNESS` | `SUBMODULE_PINNED` | Pending R4 / Stream test-control module |
| **Physical ESP32-S3 / PCM5122 Hardware** | `PLANNED` | `DEVICE_E2E_PENDING` | Pending R4 |

---

## 3. KNOWN LIMITATIONS & EXPLICIT BOUNDARIES

1. **Simulator Execution & Test Hooks:**
   - Canonical reference `pitydah/michi-music-stream@05265da` is vendored and pinned as a git submodule.
   - The canonical simulator currently implements only standard product routes and lacks test-observer endpoints (`/api/v1/test/metrics`, `/api/v1/test/active_pin`). Automated E2E tests currently execute against the local test harness (`scripts/receiver_sim.py`) until a formal test-control module is exported by Michi Music Stream in R4.
2. **Physical Hardware Certification:**
   - Physical device verification on ESP32-S3 hardware + PCM5122 DAC via RCA is explicitly pending phase **R4 — PHYSICAL CERTIFICATION**.
3. **Audio Format Scope:**
   - Standard profile is strictly frozen at PCM S16LE / 48 kHz / 16-bit / 2 channels / 10 ms (1920 bytes payload).
   - DSD, DoP, 24/96, Sendspin, and Audio Lab are out of scope for receiver-v1-lite.
4. **Multiroom Synchronization:**
   - Synchronized hardware multiroom remains disabled pending physical clock synchronization qualification.

---

## 4. NEXT GATE

- **R4 — PHYSICAL CERTIFICATION**: End-to-end hardware testing with physical Michi Micro Server appliance, dedicated LAN, physical ESP32-S3 receiver, and audio analyzer / oscilloscope qualification.
