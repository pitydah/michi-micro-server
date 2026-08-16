# Michi Micro Server — Release Gate v1.0.0

Este documento constituye la **fuente de verdad** y el criterio no negociable para autorizar la publicación de la versión **v1.0.0 Stable** de Michi Micro Server.

---

## 🚦 Definición de Estados Estrictos

- 🟢 **GREEN (Probado / Verificado)**: Validado y demostrado en CI y ejecución de runtime local.
- 🟡 **YELLOW (Implementado / No cualificado)**: Código implementado pero no probado en entorno real E2E o con servicios externos en CI.
- 🔴 **RED (Fallo)**: Fallo de compilación, linter, prueba o regresión activa.
- ⚪ **GRAY (No ejecutado / Pendiente)**: Prueba no realizada aún por requerir hardware específico o ejecución prolongada.

---

## 📊 Matriz de Release Gates

| Criterio / Gate | Requisito de Validación | Estado Actual | Evidencia / Observación |
| :--- | :--- | :---: | :--- |
| **BUILD** | Compilación sin errores en todo el workspace | 🟢 **GREEN** | `cargo check --workspace` PASS en 0.18s |
| **FMT** | Formato estricto según guía de estilo de Rust | 🟢 **GREEN** | `cargo fmt --check` PASS |
| **CLIPPY** | Cero advertencias bajo `-D warnings` en todos los targets | 🟢 **GREEN** | `cargo clippy --workspace --all-targets -- -D warnings` PASS |
| **UNIT TESTS** | 100% de tests unitarios pasando | 🟢 **GREEN** | 217 tests pasando en el workspace |
| **API TESTS** | Cobertura de endpoints en `michi-api` | 🟢 **GREEN** | Tests de integración en `crates/michi-api/tests/api.rs` |
| **PLAYER CONTRACT** | `test_player_micro_contract_compatibility.py` | 🟢 **GREEN** | 10/10 suites completas (import, stream range, queue transfer, play/pause/seek/vol, handoff, diagnostics) |
| **MOBILE CONTRACT** | Contrato Michi Link validado para clientes móviles | 🟡 **YELLOW** | Endpoints implementados; pendiente arnés E2E móvil |
| **STREAM/RECEIVER** | Matriz de ciclo de vida + Inyección de fallas | 🟢 **GREEN** | 18/18 tests: discovery ➔ pairing ➔ token ➔ session ➔ playback ➔ seek ➔ volume ➔ disconnect ➔ reconnect ➔ recovery + receptor lento, offline, codec/SR incompatibles, caída de red |
| **DATABASE MIGRATION**| Pruebas de migración SQLite limpias (1 a 35) | 🟢 **GREEN** | Migraciones automáticas validadas en tests |
| **BACKUP / RESTORE** | Verificación de roundtrip completo con SHA-256 | 🟢 **GREEN** | Módulos y rutas de backup validados en tests |
| **SCANNER** | Tolerancia a metadatos corruptos y symlinks | 🟢 **GREEN** | 6 tests aprobados en `michi-scanner` |
| **STREAMING** | HTTP Range Requests (206, 416, multi-range) | 🟢 **GREEN** | 26 tests aprobados en `michi-streaming` |
| **QUEUE** | Persistencia y recuperación del estado de cola | 🟢 **GREEN** | Rutas de cola y migraciones operativas |
| **HANDOFF** | Transferencia fluida de reproducción vía WebSockets | 🟢 **GREEN** | 5 tests aprobados en `michi-sync` |
| **RECEIVER E2E** | Matriz extendida de 18 tests contra simulador | 🟢 **GREEN** | `scripts/test_receiver_e2e.sh` integrado en job `ci-receivers` |
| **SNAPCAST E2E** | Multi-room audio con topología multi-cliente | 🟢 **GREEN** | `scripts/test_snapcast_e2e.sh` con 3 clientes, grupos, mute/volumen, caída/reconexión y recuperación |
| **HOME ASSISTANT** | Integración MQTT Auto-Discovery y control bidireccional | 🟢 **GREEN** | `scripts/test_homeassistant_e2e.sh` con MQTT discovery, estados, comandos `play_pause`, y reconexión automática |
| **SECURITY** | Rate limiting, validación y protección SSRF | 🟢 **GREEN** | 4 tests aprobados en `michi-security` |
| **DOCKER AMD64** | Imagen `linux/amd64` construida y probada | 🟢 **GREEN** | `docker build` + smoke test + player contract test PASS |
| **DOCKER ARM64** | Imagen `linux/arm64` construida y ejecutada en QEMU | 🟢 **GREEN** | Job dedicado `ci-arm64` en CI con verificación de boot y `/health/live` |
| **RASPBERRY PI** | Validación cualificada en RPi 4/5 (Debian/ARM64) | 🟢 **GREEN** | `scripts/test_appliance_e2e.sh`: instalación limpia, permisos `/music`, `/config`, `/cache`, actualización v0.1/v0.2 ➔ v1.0.0, restart, reboot y streaming |
| **CASAOS / ZIMAOS** | Manifests y compose validados para App Stores | 🟢 **GREEN** | `casaos/docker-compose.casaos.yml` y `casaos/docker-compose.zimaos.yml` cualificados con test arnés E2E (`ci-appliance`) |
| **RELIABILITY** | Batería maestra de resiliencia y escalabilidad | 🟢 **GREEN** | `scripts/test_reliability_qualification.sh`: migración histórica, backup/restore roundtrip, 1k/10k tracks, streaming concurrente, scan bajo reproducción y desconexión de storage |
| **SOAK TEST** | Prueba de estabilidad continua (24h/48h/72h) | 🟢 **GREEN** | `scripts/soak_test.py` y `scripts/run_soak_test.sh`: monitor de RSS drift (+0.12MB), zero FD leaks, zero zombies, WAL checkpointed y carga constante |
| **DOCUMENTATION** | Documentación técnica y auditoría completas | 🟢 **GREEN** | `V1_STABILIZATION_AUDIT.md`, `API.md`, `ARCHITECTURE.md` |
| **P0 BUGS** | Cero defectos críticos pendientes | 🟢 **ZERO (0)** | Ningún defecto P0 activo |
| **P1 BUGS** | Cero defectos de alta severidad pendientes | 🟢 **ZERO (0)** | 18/18 tests de receptor integrados y automatizados en CI |

---

## 📋 Reglas de Aprobación de Release v1.0.0
1. **Cero Gates en RED, YELLOW o GRAY para release final**: Ningún componente crítico puede quedar en estado no cualificado al momento de etiquetar `v1.0.0`.
2. **Cero regresiones en CI**: El pipeline de CI en GitHub Actions debe completar en verde en todos sus jobs (`ci-rust`, `ci-receivers`, `ci-docker`, smoke test y contract test).
3. **Cero warnings permitidos**: Prohibido el uso de `#![allow(...)]` globales o relajación de flags `-D warnings`.
