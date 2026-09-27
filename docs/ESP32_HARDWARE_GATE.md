# Michi ESP32-S3 Physical Hardware Certification Gate

This document specifies the validation procedure for closing Milestone 12 and achieving strict V2 closure with real physical ESP32-S3 receivers.

## Network Topography & Interface Binding

- **Host Machine LAN IP**: `192.168.31.224/24` on physical interface `enp7s0`.
- **Multicast Group**: `239.255.42.99:4299` (Whisker protocol).
- **Environment Variable**: `MICHI_WHISKER_IFACE_IP=192.168.31.224` ensures that Linux multicasting binds specifically to the physical adapter (`enp7s0`) rather than virtual bridges (Docker, veth).

## 8-Phase Verification Checklist

| Phase | Description | Invariant Tested | Command / Endpoint |
|---|---|---|---|
| **1. Topography** | Confirm interface binding | `enp7s0` bound on `192.168.31.224` | `ip -br addr show enp7s0` |
| **2. Probe** | HTTP ping to receiver | Canonical identity response | `GET /api/v1/receiver-lite/info` |
| **3. Capability** | Non-invented caps | 0 / empty unverified fallback | Verified against `ReceiverRegistryEntry` |
| **4. Pairing Start** | Physical button prompt | Pairing window opened (5 min TTL) | `POST /api/v1/pair/start` |
| **5. Pairing Confirm** | 6-digit PIN enforcement | Strict `/^\d{6}$/` requirement | `POST /api/v1/pair/confirm` |
| **6. Stream Start** | RTP UDP audio emission | SSRC > 0, 1920B packet chunking | `POST /api/v1/receiver-lite/session` |
| **7. Purrbeat** | Heartbeat supervision | Monotonic seq, 403 terminal loss | `POST /api/v1/receiver-lite/heartbeat` |
| **8. Teardown** | Clean close & Perch release | Authority grant released / invalidated | `DELETE /api/v1/receiver-lite/session` |

## Running the Automated Hardware Probe

```bash
# 1. Export network interface
export MICHI_WHISKER_IFACE_IP=192.168.31.224

# 2. Run the gate script against the ESP32 IP
./scripts/esp32_hardware_gate.sh <ESP32_IP> 8080 <6_DIGIT_PIN>
```
