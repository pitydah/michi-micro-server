# MICHI MICRO SERVER — PROJECT STATUS & CERTIFICATION READINESS

**CURRENT VERSION:** v0.2.0  
**CURRENT PHASE:** R3.1 — CERTIFICATION CLOSURE (SEALED)  
**CURRENT HEAD:** e059d57ecb1ec43ed00c458e46ac28a938f1dcb0 (R3.1-SEALED)  
**STATUS DATE:** 2026-08-23  

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
| **Discovery Announcer (mDNS & Multicast UDP)** | `IMPLEMENTED` | `UNIT_PASS`, `NETWORK_E2E_PASS` | Verified |
| **Receiver 2-Step Pairing Lifecycle** | `IMPLEMENTED` | `UNIT_PASS`, `CONTRACT_PASS`, `NETWORK_E2E_PASS` | Verified |
| **PIN Retry & Expiration Management** | `IMPLEMENTED` | `UNIT_PASS`, `NETWORK_E2E_PASS` | Verified |
| **Negotiated Session DTO & Strict Schema** | `IMPLEMENTED` | `UNIT_PASS`, `CONTRACT_PASS`, `NETWORK_E2E_PASS` | Verified |
| **Single SSRC Authority & Negotiated Port** | `IMPLEMENTED` | `UNIT_PASS`, `NETWORK_E2E_PASS` | Verified |
| **Transactional Teardown (`Closing`/`Failed`)** | `IMPLEMENTED` | `UNIT_PASS`, `NETWORK_E2E_PASS` | Verified |
| **RTP Packetization (1920B, +480 frames, PT 97)** | `IMPLEMENTED` | `UNIT_PASS`, `NETWORK_E2E_PASS` | Verified |
| **AudioTransport Buffer (`pending_pcm`)** | `IMPLEMENTED` | `UNIT_PASS`, `NETWORK_E2E_PASS` | Verified |
| **Managed Monotonic Heartbeat** | `IMPLEMENTED` | `UNIT_PASS`, `NETWORK_E2E_PASS` | Verified |
| **Three-Way Ecosystem Flow (Mobile->Micro->Stream)** | `IMPLEMENTED` | `NETWORK_E2E_PASS` | Verified |
| **Physical ESP32-S3 / PCM5122 Hardware** | `PLANNED` | `DEVICE_E2E_PENDING` | Pending R4 |

---

## 3. KNOWN LIMITATIONS & EXPLICIT BOUNDARIES

1. **Physical Hardware Certification:**
   - Physical device verification on ESP32-S3 hardware + PCM5122 DAC via RCA is explicitly pending phase **R4 — PHYSICAL CERTIFICATION**.
   - Current certification level is **NETWORK_E2E_PASS** verified against canonical pinned reference (`pitydah/michi-music-stream@05265da` / `pitydah/michi-link@1b0684a`).
2. **Audio Format Scope:**
   - Standard profile is strictly frozen at PCM S16LE / 48 kHz / 16-bit / 2 channels / 10 ms (1920 bytes payload).
   - DSD, DoP, 24/96, Sendspin, and Audio Lab are out of scope for receiver-v1-lite.
3. **Multiroom Synchronization:**
   - Synchronized hardware multiroom remains disabled pending physical clock synchronization qualification.

---

## 4. NEXT GATE

- **R4 — PHYSICAL CERTIFICATION**: End-to-end hardware testing with physical Michi Micro Server appliance, dedicated LAN, physical ESP32-S3 receiver, and audio analyzer / oscilloscope qualification.
