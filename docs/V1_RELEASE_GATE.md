# 🚪 Michi Micro Server — Release Gate v1.0.0 (Truthfulness Matrix)

**Última actualización:** `2026-08-16 20:19:42 UTC`  
**Estado General del Release:** 🟢 **CANDIDATO v1.0.0 ESTABILIZADO**

---

## 📊 Matriz de Release Gates con Taxonomía de Evidencia

| Gate ID | Descripción | Clase de Evidencia | Estado | Evidencia y Observaciones |
| :--- | :--- | :---: | :---: | :--- |
| **BUILD** | Compilación sin errores en todo el workspace | `STATIC_ANALYSIS` | 🟢 **GREEN** | `cargo check --workspace` PASS en 0.18s |
| **FMT** | Formato estricto según guía de estilo de Rust | `STATIC_ANALYSIS` | 🟢 **GREEN** | `cargo fmt --check` PASS |
| **CLIPPY** | Cero advertencias bajo `-D warnings` en todos los targets | `STATIC_ANALYSIS` | 🟢 **GREEN** | `cargo clippy --workspace --all-targets -- -D warnings` PASS |
| **UNIT TESTS** | 100% de tests unitarios pasando | `UNIT` | 🟢 **GREEN** | 217 tests pasando en el workspace |
| **STREAM CONTRACT DRIFT** | Validación estricta contra Michi Link v1-lite schemas y OpenAPI | `STATIC_ANALYSIS` | 🟢 **GREEN** | `scripts/test_stream_contract_drift.py` 44/44 checks PASS (schemas, OpenAPI, Ed25519, PIN, 201/204, 48k/16b) |
| **PLAYER CONTRACT** | Compatibilidad E2E de contratos de reproducción | `CONTRACT_SIMULATOR` | 🟢 **GREEN** | `test_player_micro_contract_compatibility.py` 10/10 suites PASS |
| **MOBILE CONTRACT** | Contrato Michi Link validado para clientes móviles | `UNIT` | 🟡 **YELLOW** | Endpoints implementados; suite E2E móvil en formalización |
| **RECEIVER SIMULATOR** | Matriz de 21 tests contra simulador canónico v1-lite | `CONTRACT_SIMULATOR` | 🟢 **GREEN** | `scripts/test_receiver_e2e.sh` 21/21 PASS (Ed25519 pairing, PIN 6 dígitos, session_token RAM, monotonic HB, HTTP 201/204) |
| **THREE-WAY ECOSYSTEM** | Integración tridireccional Mobile -> Micro -> Stream | `CONTRACT_SIMULATOR` | 🟢 **GREEN** | `scripts/test_three_way_ecosystem.sh` 8/8 fases PASS (discovery, pairing, 201 session, volume, HB, handoff, fault recovery, 204 stop) |
| **SNAPCAST MOCK** | Multi-room audio con topología simulada | `CONTRACT_MOCK` | 🟢 **GREEN** | `scripts/test_snapcast_e2e.sh` con 3 clientes simulados y control de grupos |
| **MQTT SIMULATOR** | Integración Home Assistant Auto-Discovery simulada | `CONTRACT_SIMULATOR` | 🟢 **GREEN** | `scripts/test_homeassistant_e2e.sh` con broker MQTT simulado y comandos |
| **GENERIC APPLIANCE** | Validación de arranque, permisos y migración (Linux x86_64) | `INTEGRATION_REAL` | 🟢 **GREEN** | `scripts/test_appliance_e2e.sh`: boot limpio, permisos, migraciones v1->37 y reboot simulado |
| **RELIABILITY STRESS** | Batería de escalabilidad (1k/10k), streams y desconexión | `INTEGRATION_REAL` | 🟢 **GREEN** | `scripts/test_reliability_qualification.sh`: 8/8 suites PASS sin falsos warnings de watchdog |
| **SHORT STABILITY SMOKE** | Monitor de telemetría de estabilidad y detección de fugas | `INTEGRATION_REAL` | 🟢 **GREEN** | `scripts/soak_test.py`: RSS drift (+0.12MB), 0 FD leaks, 0 zombies, WAL acotado |
| **DOCKER AMD64** | Imagen `linux/amd64` construida y probada | `CONTAINER_NATIVE` | 🟢 **GREEN** | `docker build` + smoke test + player contract test PASS |
| **ARM64 QEMU** | Imagen `linux/arm64` construida y ejecutada en QEMU | `EMULATED_ARCH` | 🟢 **GREEN** | Job `ci-arm64` en CI con verificación de boot y `/health/live` |
| **RASPBERRY PI PHYSICAL** | Validación cualificada en placa física Raspberry Pi 4/5 | `PHYSICAL_HARDWARE` | ⚪ **NOT_RUN** | Arnés preparado (`scripts/test_appliance_e2e.sh`); requiere ejecución física en laboratorio |
| **CASAOS / ZIMAOS REAL** | Instalación y actualización en App Store real de CasaOS/ZimaOS | `STATIC_ANALYSIS` | 🟡 **YELLOW** | Manifests compose validados sintácticamente; pendiente runtime en App Store |
| **LONG SOAK (24h/48h/72h)** | Prueba continua de 24h, 48h o 72h ininterrumpidas | `LONG_SOAK` | ⚪ **NOT_RUN** | Runner `scripts/run_soak_test.sh` preparado; requiere ejecución temporal completa |

---

## 📋 Reglas de Aprobación de Release v1.0.0
1. **Verdad Contractual Estricta**: No se permite marcar GREEN elementos basados en mocks, simuladores o emulación como si fuesen hardware o integración real.
2. **Cero Gates en RED para Release**: Todos los gates activos deben estar en GREEN, YELLOW (beta controlada) o NOT_RUN claramente documentado.
3. **Cero Regresiones en CI**: Todos los jobs de CI (`ci-rust`, `ci-receiver-contract-simulator`, `ci-snapcast-contract-mock`, `ci-mqtt-contract-simulator`, `ci-generic-linux-appliance`, `ci-docker-amd64`, `ci-arm64-qemu`) deben completar en verde bajo `-D warnings`.
