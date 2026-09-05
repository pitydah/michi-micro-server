# 🚪 Michi Micro Server — Release Gate v1.0.0 (Evidence Ledger)

- **Evaluated at:** `2026-09-05T19:12:41.552074+00:00`
- **Commit SHA:** `04b46110c584bca34ba9a11e9e2d1dc1460a0b15`
- **Evaluation Mode:** `RC`
- **Overall Decision:** 🔴 **RELEASE BLOCKED**

---

## 📊 Matriz Canónica de Requisitos y Evidencia Calculada

| Gate ID | Requerido en Modo | Clase de Evidencia | Estado Calculado | Observaciones / Provenance |
| :--- | :---: | :---: | :---: | :--- |
| **rust-quality** | `RC` (Mandatorio) | `STATIC_ANALYSIS` | 🟢 **PASS** | Command succeeded |
| **stream-contract** | `RC` (Mandatorio) | `STATIC_ANALYSIS` | 🟢 **PASS** | Command succeeded |
| **receiver-contract-simulator** | `RC` (Mandatorio) | `CONTRACT_SIMULATOR` | 🟢 **PASS** | Command succeeded |
| **snapcast-contract-mock** | `RC` (Mandatorio) | `CONTRACT_MOCK` | 🟢 **PASS** | Command succeeded |
| **snapserver-real** | Opcional / Futuro | `NONE` | ⚪ **NOT_RUN** | No evidence artifact submitted for this gate |
| **mqtt-contract-simulator** | `RC` (Mandatorio) | `CONTRACT_SIMULATOR` | 🟢 **PASS** | Command succeeded |
| **mosquitto-real** | `RC` (Mandatorio) | `NONE` | ⚪ **NOT_RUN** | No evidence artifact submitted for this gate |
| **generic-linux-appliance** | `RC` (Mandatorio) | `INTEGRATION_REAL` | 🟢 **PASS** | Command succeeded |
| **reliability-stress** | `RC` (Mandatorio) | `INTEGRATION_REAL` | 🟢 **PASS** | Command succeeded |
| **short-stability-smoke** | `RC` (Mandatorio) | `INTEGRATION_REAL` | 🟢 **PASS** | Telemetry stability monitor qualified under load: zero leaks (FD drift +0) |
| **resource-budget** | `RC` (Mandatorio) | `INTEGRATION_REAL` | 🟢 **PASS** | Qualified: Idle RSS median 24.73MB, p95 24.77MB (<50MB target PASS), threads 14 |
| **web-ui-functional-integrity** | `RC` (Mandatorio) | `UNIT` | 🟢 **PASS** | Command succeeded |
| **web-ui-browser-e2e** | `RC` (Mandatorio) | `INTEGRATION_REAL` | 🟢 **PASS** | Command succeeded |
| **three-way-ecosystem** | `RC` (Mandatorio) | `CONTRACT_SIMULATOR` | 🟢 **PASS** | Command succeeded |
| **docker-amd64** | `RC` (Mandatorio) | `NONE` | ⚪ **NOT_RUN** | No evidence artifact submitted for this gate |
| **arm64-qemu** | `RC` (Mandatorio) | `NONE` | ⚪ **NOT_RUN** | No evidence artifact submitted for this gate |
| **zimaos-package** | `RC` (Mandatorio) | `STATIC_ANALYSIS` | 🟢 **PASS** | Command succeeded |
| **raspberry-pi-physical** | Opcional / Futuro | `NONE` | ⚪ **NOT_RUN** | No evidence artifact submitted for this gate |
| **casaos-zimaos-real** | Opcional / Futuro | `NONE` | ⚪ **NOT_RUN** | No evidence artifact submitted for this gate |
| **soak-24h** | Opcional / Futuro | `NONE` | ⚪ **NOT_RUN** | No evidence artifact submitted for this gate |

---

## 📋 Principios de Certificación de Release
1. **Sin estados hardcodeados**: Toda fila refleja la evaluación de un artifact generado durante la ejecución sobre el commit actual.
2. **Cero tolerancia a STALE**: Evidencia generada para un commit diferente al HEAD actual es invalidada inmediatamente.
3. **Taxonomía rígida**: Mocks o simuladores jamás son aceptados para satisfacer requisitos de integración real o hardware físico.
