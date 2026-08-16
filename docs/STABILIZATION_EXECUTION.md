# ═══════════════════════════════════════════════════════════════════════════════
# MICHI ECOSYSTEM — MASTER STABILIZATION & TRUTHFULNESS LEDGER
# ═══════════════════════════════════════════════════════════════════════════════

Este documento es el ledger central y vinculante de ejecución técnica entre:
- **Michi Micro Server** (`/home/cristian/michi-micro-server`)
- **Michi Music Stream** (`/home/cristian/michi-music-stream`)
- **Michi Music Mobile** (`/home/cristian/StudioProjects/michi-music-mobile`)

---

## 🏛️ Taxonomía de Clases de Evidencia

| Clase de Evidencia | Definición / Criterio |
| :--- | :--- |
| `STATIC_ANALYSIS` | `cargo fmt --check`, `cargo clippy -D warnings`, linting de schemas |
| `UNIT` | Tests unitarios puros aislados en memoria |
| `CONTRACT_MOCK` | Mock estático en memoria o script (e.g. `snapserver_mock.py`) |
| `CONTRACT_SIMULATOR`| Servidor HTTP/MQTT simulador con validación de contrato (e.g. `receiver_sim.py`, `mqtt_broker_sim.py`) |
| `INTEGRATION_REAL` | Proceso / demonio / runtime real levantado y conectado (e.g. Mosquitto real, Snapserver real, Micro binario) |
| `CONTAINER_NATIVE` | Contenedor Docker ejecutado en arquitectura nativa (`linux/amd64`) |
| `EMULATED_ARCH` | Contenedor o binario ejecutado bajo emulación QEMU (`linux/arm64`) |
| `PHYSICAL_HARDWARE` | Ejecución y certificación en placa física (Raspberry Pi 4/5, hardware Stream ESP32/DAC) |
| `LONG_SOAK` | Prueba continua de estabilidad y carga de 24 h, 48 h o 72 h ininterrumpidas |

---

## 📊 Matriz Maestra de Ejecución y Estado

| ID | TASK | REPO | STATUS | IMPLEMENTATION | TEST | EVIDENCE | BLOCKER | COMMIT |
| :--- | :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| **MS-A01** | Release Gate truthfulness & Evidence Classification | `michi-micro-server` | `VERIFIED` | yes | `scripts/generate_release_gate.py --check` | `STATIC_ANALYSIS` | none | pending |
| **MS-A02** | CI Jobs Renaming (Mock, Simulator, Emulated) | `michi-micro-server` | `VERIFIED` | yes | `.github/workflows/ci.yml` | `STATIC_ANALYSIS` | none | pending |
| **MS-A03** | Automated Contract Drift Gate (`ci-michi-link-stream-contract`) | `michi-micro-server` | `VERIFIED` | yes | `scripts/test_stream_contract_drift.py` (14/14 PASS) | `STATIC_ANALYSIS` | none | pending |
| **MS-B01** | Stream 48kHz/16-bit Truth (Purge 96k/24b active claims) | all 3 repos | `VERIFIED` | yes | contract tests | `UNIT` | none | pending |
| **MS-B02** | Strict Receiver Simulator (Reject invalid v1-lite requests) | `michi-micro-server` | `VERIFIED` | yes | `scripts/test_receiver_e2e.sh` (19/19 PASS) | `CONTRACT_SIMULATOR` | none | pending |
| **MS-C01** | Canonical v1-lite Pairing (PIN handshake / nonce validation) | `michi-micro-server` | `VERIFIED` | yes | pairing flow & rejection tests | `CONTRACT_SIMULATOR` | none | pending |
| **MS-C02** | Canonical Session Lifecycle (`session_token`, `X-Michi-Session`) | `michi-micro-server` | `VERIFIED` | yes | session create/patch/delete tests | `CONTRACT_SIMULATOR` | none | pending |
| **MS-C03** | Monotonic Heartbeat & Lease Renewal | `michi-micro-server` | `VERIFIED` | yes | monotonic seq & conflict tests | `CONTRACT_SIMULATOR` | none | pending |
| **MS-C04** | Dynamic Stream Port & Session SSRC | `michi-micro-server` | `VERIFIED` | yes | stream port & SSRC validation | `CONTRACT_SIMULATOR` | none | pending |
| **MS-C05** | Volume Validation & Rejection (0..100 strict) | `michi-micro-server` | `VERIFIED` | yes | out-of-range rejection tests | `CONTRACT_SIMULATOR` | none | pending |
| **MS-D01** | Watchdog Idle State Handling (Eliminate false hang warnings)| `michi-micro-server` | `VERIFIED` | yes | `apps/michi-server/src/main.rs` | `INTEGRATION_REAL` | none | pending |
| **MS-E01** | Mosquitto Real Integration Gate | `michi-micro-server` | `NOT_STARTED` | pending | integration runner | `INTEGRATION_REAL` | none | - |
| **MS-E02** | Snapserver Real Integration Gate | `michi-micro-server` | `NOT_STARTED` | pending | integration runner | `INTEGRATION_REAL` | none | - |
| **MS-F01** | Mobile ↔ Micro Contract Verification | `michi-music-mobile` | `NOT_STARTED` | pending | gradle tests | `UNIT` | none | - |
| **MS-G01** | Three-Way E2E (Mobile → Micro → Stream) | all 3 repos | `NOT_STARTED` | pending | 3-way runner | `CONTRACT_SIMULATOR` | none | - |
| **MS-H01** | Raspberry Pi Physical Qualification Artifacts | `michi-micro-server` | `BLOCKED_EXTERNAL` | runner prepared | manual lab run | `PHYSICAL_HARDWARE` | physical board needed | - |
| **MS-H02** | Long Soak Qualification (24h/48h/72h) | `michi-micro-server` | `BLOCKED_EXTERNAL` | runner prepared | 24h+ execution | `LONG_SOAK` | 24h+ run needed | - |

---

## 📌 Reglas de Cierre
1. Ninguna tarea se marca `VERIFIED` sin que su implementación productiva, suite de pruebas y artifact de evidencia coincidan.
2. Si una tarea depende de hardware o ejecución extendida no disponible en el ciclo actual, se documenta como `BLOCKED_EXTERNAL` o `NOT_RUN`, nunca como `VERIFIED`.
