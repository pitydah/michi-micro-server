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
| **STREAM/RECEIVER** | Pruebas de integración de receptor con simulador | 🟢 **GREEN** | 14/14 tests automatizados con `scripts/receiver_sim.py` |
| **DATABASE MIGRATION**| Pruebas de migración SQLite limpias (1 a 35) | 🟢 **GREEN** | Migraciones automáticas validadas en tests |
| **BACKUP / RESTORE** | Verificación de roundtrip completo con SHA-256 | 🟢 **GREEN** | Módulos y rutas de backup validados en tests |
| **SCANNER** | Tolerancia a metadatos corruptos y symlinks | 🟢 **GREEN** | 6 tests aprobados en `michi-scanner` |
| **STREAMING** | HTTP Range Requests (206, 416, multi-range) | 🟢 **GREEN** | 26 tests aprobados en `michi-streaming` |
| **QUEUE** | Persistencia y recuperación del estado de cola | 🟢 **GREEN** | Rutas de cola y migraciones operativas |
| **HANDOFF** | Transferencia fluida de reproducción vía WebSockets | 🟢 **GREEN** | 5 tests aprobados en `michi-sync` |
| **RECEIVER E2E** | Suite de 14 tests ejecutada contra simulador | 🟢 **GREEN** | `scripts/test_receiver_e2e.sh` integrado en job `ci-receivers` |
| **SNAPCAST E2E** | Multi-room audio con degradación limpia | 🟡 **YELLOW** | `michi-rooms` probado unitariamente; pendiente E2E con Snapserver |
| **HOME ASSISTANT** | Integración MQTT opcional sin bloqueo | 🟡 **YELLOW** | `michi-homeassistant` implementado; pendiente broker en CI |
| **SECURITY** | Rate limiting, validación y protección SSRF | 🟢 **GREEN** | 4 tests aprobados en `michi-security` |
| **DOCKER AMD64** | Imagen `linux/amd64` construida y probada | 🟢 **GREEN** | `docker build` + smoke test + liveness HTTP 200 PASS |
| **DOCKER ARM64** | Imagen `linux/arm64` construida en multi-arch | ⚪ **GRAY** | Declarada en CI; no ejecutada localmente por host AMD64 |
| **RASPBERRY PI** | Validación cualificada en Raspberry Pi 4/5 | ⚪ **GRAY** | Requiere prueba física en dispositivo real |
| **CASAOS / ZIMAOS** | Metadata y `docker-compose.casaos.yml` alineados | 🟡 **YELLOW** | Archivos sincronizados a v0.2.0; pendiente test en App Store |
| **DOCUMENTATION** | Documentación técnica y auditoría completas | 🟢 **GREEN** | `V1_STABILIZATION_AUDIT.md`, `API.md`, `ARCHITECTURE.md` |
| **P0 BUGS** | Cero defectos críticos pendientes | 🟢 **ZERO (0)** | Ningún defecto P0 activo |
| **P1 BUGS** | Cero defectos de alta severidad pendientes | 🟢 **ZERO (0)** | 14/14 tests de receptor integrados y automatizados en CI |
| **SOAK TEST** | Prueba de estabilidad continua (24h/48h) | ⚪ **GRAY** | Pendiente de ejecución en ambiente de laboratorio |

---

## 📋 Reglas de Aprobación de Release v1.0.0
1. **Cero Gates en RED, YELLOW o GRAY para release final**: Ningún componente crítico puede quedar en estado no cualificado al momento de etiquetar `v1.0.0`.
2. **Cero regresiones en CI**: El pipeline de CI en GitHub Actions debe completar en verde en todos sus jobs (`ci-rust`, `ci-receivers`, `ci-docker`, smoke test y contract test).
3. **Cero warnings permitidos**: Prohibido el uso de `#![allow(...)]` globales o relajación de flags `-D warnings`.
