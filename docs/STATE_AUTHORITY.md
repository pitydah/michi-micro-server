# Michi Ecosystem — State Authority & Lifecycle Model

This document establishes the state authority hierarchy across the Michi ecosystem components to prevent split-brain conditions, stale caching, and playback desynchronization.

---

## 1. Authority Hierarchy

```
┌─────────────────────────────────────────────────────────────┐
│ 1. USER INTENT AUTHORITY (Mobile App)                       │
│    - What track to play next                                │
│    - User interaction (skip, seek, pause)                   │
│    - Target audio route selection (Phone vs Michi Stream)   │
└──────────────────────────────┬──────────────────────────────┘
                               │
                               ▼
┌─────────────────────────────────────────────────────────────┐
│ 2. PLAYBACK STATE & QUEUE AUTHORITY (Micro Server)          │
│    - Canonical track queue and shuffle state                │
│    - Active playback position & clock                       │
│    - Transcoding pipeline state                             │
│    - Room & Receiver session orchestration                  │
│    - Heartbeat lease management                             │
└──────────────────────────────┬──────────────────────────────┘
                               │
                               ▼
┌─────────────────────────────────────────────────────────────┐
│ 3. HARDWARE & RUNTIME EXECUTION AUTHORITY (Music Stream)     │
│    - Physical DAC state and health                          │
│    - Hardware volume and temporal overlay UI                │
│    - Jitter buffer health & underrun detection              │
│    - Physical display rendering (320x240 landscape)         │
│    - Hardware button input & pairing window                 │
└─────────────────────────────────────────────────────────────┘
```

---

## 2. Handoff & Failure Recovery Invariants

1. **Atomic Handoff**:
   - When switching target from Mobile to Stream, Mobile maintains local playback buffer until Micro Server confirms the Stream receiver session has bound the UDP port and acknowledged the stream creation.
2. **Local Continuation Guard**:
   - If receiver session creation fails, network times out, or format negotiation fails, Mobile immediately falls back to local Media3 playback with 0ms interruption to user listening.
3. **Server Independence**:
   - Once Micro Server is streaming to Michi Stream, Mobile disconnecting or exiting Wi-Fi range does NOT disrupt Stream playback. Micro Server maintains the session lease and stream until user explicitly stops or queue finishes.
4. **Watchdog Expiration**:
   - If Micro Server crashes or network drops between Micro and Stream for > 30 seconds without heartbeat, Michi Stream cleanly expires the session, mutes DAC to prevent static, and returns to the Ready screen.
