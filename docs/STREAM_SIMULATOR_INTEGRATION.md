# Stream Simulator Integration

This document describes how **Michi Micro Server** integrates with the **Michi Music Stream Simulator** (`receiver_sim.py`) for receiver testing without hardware.

## Prerequisites

- Python 3 with Flask: `pip install flask`
- The simulator script from `pitydah/michi-music-stream/simulator/receiver_sim.py`

## Starting the Simulator

```bash
# Standard receiver on port 8080
python3 receiver_sim.py --type standard --port 8080

# Hi-Fi receiver on port 8081 (separate terminal)
python3 receiver_sim.py --type hifi --port 8081
```

## Running Integration Tests

```bash
# With simulators on default ports
MICHI_RECEIVER_SIM_URL=http://127.0.0.1:8080 cargo test --test receiver_simulator_integration -- --ignored

# With custom ports
MICHI_RECEIVER_SIM_URL=http://127.0.0.1:9000 cargo test --test receiver_simulator_integration -- --ignored
```

Tests are `#[ignore]` by default because they require an external process.

## Test Coverage

| Test | Description |
|------|-------------|
| `test_receiver_info_standard` | GET /api/v1/receiver/info returns Standard config |
| `test_receiver_info_hifi` | GET /api/v1/receiver/info returns Hi-Fi config |
| `test_receiver_info_standard_output` | Connector=jack_3_5, max_sr=48000, max_bd=16 |
| `test_receiver_info_hifi_output` | Connector=rca_stereo, max_sr=96000, max_bd=24 |
| `test_receiver_pairing_flow` | Full start → confirm roundtrip |
| `test_receiver_pairing_window_closed_rejected` | Re-pair after confirm |
| `test_receiver_standard_full_lifecycle` | Pair → session → heartbeat → volume → stop |
| `test_receiver_hifi_full_lifecycle` | Same for Hi-Fi with pcm_s24le |
| `test_receiver_errors_unsupported_codec` | aac fails on Standard |
| `test_receiver_errors_sample_rate_exceeds` | 96000Hz fails on Standard (max 48000) |
| `test_receiver_errors_duplicate_session` | Second session returns 409 |
| `test_receiver_errors_volume_out_of_range` | Volume 101 clamps to 100 |
| `test_receiver_errors_unauthenticated` | Heartbeat without token fails |
| `test_receiver_registry_tracks_state` | ReceiverRegistry stores paired state |

## Receiver Types

### Standard (`michi_stream_standard`)
- `connector`: jack_3_5
- `max_sample_rate`: 48000
- `max_bit_depth`: 16
- `codecs`: pcm_s16le, opus

### Hi-Fi (`michi_stream_hifi`)
- `connector`: rca_stereo
- `max_sample_rate`: 96000
- `max_bit_depth`: 24
- `codecs`: pcm_s16le, pcm_s24le, opus

## API Contract (Receiver Side)

All receiver endpoints are under `/api/v1/receiver/`:

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| GET | /api/v1/receiver/info | No | Device capabilities |
| POST | /api/v1/receiver/pair/start | No | Open pairing window |
| POST | /api/v1/receiver/pair/confirm | No | Confirm pairing with nonce |
| POST | /api/v1/receiver/heartbeat | Yes | Keep-alive during session |
| POST | /api/v1/receiver/session/start | Yes | Start audio stream session |
| POST | /api/v1/receiver/session/stop | Yes | Stop active session |
| POST | /api/v1/receiver/volume | Yes | Set volume 0-100 |

## Error Codes

| Code | HTTP | Description |
|------|------|-------------|
| pairing_window_open | 409 | Another pairing is in progress |
| pairing_window_closed | 409 | No pairing window (press button) |
| invalid_token | 401 | Missing or bad Bearer token |
| unsupported_codec | 400 | Codec not in supported_codecs |
| unsupported_rate | 400 | Sample rate exceeds device max |
| bad_request | 400 | Bit depth or channels out of range |
| session_active | 409 | Already have an active session |

## Architecture

```
Micro Server ──ReceiverSessionManager──► Receiver Simulator (HTTP)
                     │
                     ▼
              ReceiverRegistry
              - device_id, type, base_url
              - paired, token, last_seen
              - active_session_id
              - capabilities
```

The `michi-receivers` crate provides:

- `ReceiverClient` — low-level HTTP client for all receiver endpoints
- `ReceiverSessionManager` — high-level: pair, start/stop session, volume, heartbeat
- `ReceiverRegistry` — in-memory state tracking for discovered receivers
