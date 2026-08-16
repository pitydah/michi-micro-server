# Michi Ecosystem — Cross-Repository Integration Architecture

This document specifies the end-to-end integration architecture between the three core pillars of the Michi Audio Ecosystem:
1. **Michi Music Mobile** (Android / Kotlin / Jetpack Compose) — User UI, Controller, Local Media3 player.
2. **Michi Micro Server** (Rust / Axum / SQLite / Tokio) — Music Server, State Authority, Transcoding & Streaming Engine, Receiver Session Manager.
3. **Michi Music Stream** (ESP32-S3 / C / ESP-IDF / ST7789) — Physical Embedded Audio Receiver with hardware DAC & 320x240 landscape display.

---

## 1. System Topology & Core Axiom

> **"Mobile controls. Micro Server streams. Michi Stream plays."**

```
┌─────────────────────────────────────────────────────────────┐
│                     Michi Music Mobile                      │
│                  (Android / Jetpack Compose)                │
│                                                             │
│  - User Intent & Browsing     - Output Picker Dialog        │
│  - Remote Queue Management    - Local Audio Playback Fallback│
└──────────────┬───────────────────────────────▲──────────────┘
               │ REST / WebSockets             │ Live Events
               │ (/api/v1/*, /api/v1/events)   │ (track, queue, state)
               ▼                               │
┌──────────────────────────────────────────────┴──────────────┐
│                     Michi Micro Server                      │
│                       (Rust Backend)                        │
│                                                             │
│  - Playback State Authority   - Library & Audio DB          │
│  - Receiver Session Manager   - Real-time RTP Stream Engine │
│  - Room & Group Orchestration - Multi-Client Synchronization│
└──────────────────────────────┬──────────────────────────────┘
                               │ Canonical Michi Link v1-lite
                               │ (HTTP Control + RTP UDP Audio)
                               ▼
┌─────────────────────────────────────────────────────────────┐
│                     Michi Music Stream                      │
│                (ESP32-S3 Physical Hardware)                 │
│                                                             │
│  - 320×240 IPS Physical UI   - I2S / DMA Audio Engine       │
│  - Hardware Volume & Mute    - Jitter Buffer & RTP Guard    │
│  - Certified Audio: PCM S16LE, 48 kHz, 16-bit, 2 Channels    │
└─────────────────────────────────────────────────────────────┘
```

---

## 2. Audio Capability Truth & Certification

Physical hardware constraints and certification boundaries:
- **Certified Audio Profile**:
  - Codec: `pcm_s16le`
  - Sample Rate: `48000` Hz
  - Bit Depth: `16` bit
  - Channels: `2` (Stereo)
  - RTP Packet Duration: `10` ms (1920 bytes payload)
  - RTP Payload Type: `97`
  - SSRC: 32-bit unsigned identifier
- **No Drift Guarantee**: Neither Micro Server nor Mobile shall simulate, advertise, or assume uncertified capabilities (such as 24-bit / 96 kHz) on certified v1-lite stream endpoints until physical hardware certification is completed.

---

## 3. Protocol & Endpoint Governance

| Direction | Canonical Route | Method | Purpose |
|---|---|---|---|
| Server $\rightarrow$ Stream | `/api/v1/server/info` | `GET` | Query receiver identity, features, and audio capabilities |
| Server $\rightarrow$ Stream | `/api/v1/pair/start` | `POST` | Request pairing window (requires physical button trigger) |
| Server $\rightarrow$ Stream | `/api/v1/pair/confirm` | `POST` | Confirm pairing using 6-digit PIN and obtain session token |
| Server $\rightarrow$ Stream | `/api/v1/receiver-lite/session` | `POST` | Create RTP stream session (returns bound UDP port) |
| Server $\rightarrow$ Stream | `/api/v1/receiver-lite/session` | `GET` | Query live session status |
| Server $\rightarrow$ Stream | `/api/v1/receiver-lite/session` | `PATCH` | Update receiver volume (0..100) or pause state |
| Server $\rightarrow$ Stream | `/api/v1/receiver-lite/session` | `DELETE` | Terminate session and release hardware buffer/port |
| Server $\rightarrow$ Stream | `/api/v1/receiver-lite/heartbeat`| `POST` | Renew 30-second session lease |
| Mobile $\rightarrow$ Server | `/api/v1/receivers` | `GET` | List all discovered, paired, and online receivers |
| Mobile $\rightarrow$ Server | `/api/v1/playback/target` | `POST` | Switch active playback target (Phone vs Receiver/Room) |
| Mobile $\rightarrow$ Server | `/api/v1/playback/control` | `POST` | Send playback intent (play, pause, next, seek) |
| Mobile $\rightarrow$ Server | `/api/v1/events` | `WS` | Real-time bi-directional playback and state sync |
