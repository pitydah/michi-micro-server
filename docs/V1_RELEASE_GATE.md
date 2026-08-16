# Michi Micro Server — Release Gate v1.0.0

Este documento constituye la **fuente de verdad** y el criterio no negociable para autorizar la publicación de la versión **v1.0.0 Stable** de Michi Micro Server.

---

## 🚦 Matriz de Release Gates

| Criterio / Gate | Requisito de Validación | Estado Actual |
| :--- | :--- | :---: |
| **BUILD** | Compilación sin errores en todo el workspace (`cargo check --workspace`) | 🟢 **GREEN** |
| **FMT** | Formato estricto según guía de estilo (`cargo fmt --check`) | 🟢 **GREEN** |
| **CLIPPY** | Cero advertencias bajo `-D warnings` en todos los targets | 🟢 **GREEN** |
| **UNIT TESTS** | 100% de tests unitarios pasando sin regresiones | 🟢 **GREEN** |
| **API TESTS** | Cobertura completa de endpoints en `michi-api` | 🟢 **GREEN** |
| **PLAYER CONTRACT** | `test_player_micro_contract_compatibility.py` exitoso con auth | 🟢 **GREEN** |
| **MOBILE CONTRACT** | Contrato Michi Link validado para clientes móviles | 🟡 **IN PROGRESS** |
| **STREAM/RECEIVER** | Pruebas de integración de receptor con simulador stream | 🟡 **IN PROGRESS** |
| **DATABASE MIGRATION**| Pruebas de migración SQLite limpias (de DB vacía a versión 35) | 🟢 **GREEN** |
| **BACKUP / RESTORE** | Verificación de roundtrip completo con integridad SHA-256 | 🟢 **GREEN** |
| **SCANNER** | Tolerancia a metadatos corruptos, omisión de symlinks y IDs estables | 🟢 **GREEN** |
| **STREAMING** | HTTP Range Requests exhaustivamente validados (206, 416, multi-range) | 🟢 **GREEN** |
| **QUEUE** | Persistencia y recuperación del estado de cola ante reinicios | 🟢 **GREEN** |
| **HANDOFF** | Transferencia fluida de reproducción vía WebSockets | 🟢 **GREEN** |
| **RECEIVER E2E** | Suite de 14 tests de `michi-receivers` ejecutada contra simulador | 🟡 **IN PROGRESS** |
| **SNAPCAST E2E** | Multi-room audio con degradación limpia ante desconexión | 🟢 **GREEN** |
| **HOME ASSISTANT** | Integración MQTT opcional sin dependencia bloqueante del servidor | 🟢 **GREEN** |
| **SECURITY** | Rate limiting activo, validación de entradas y protección SSRF | 🟢 **GREEN** |
| **DOCKER AMD64** | Imagen Docker `linux/amd64` construida y probada con smoke test | 🟢 **GREEN** |
| **DOCKER ARM64** | Imagen Docker `linux/arm64` construida en pipeline multi-arch | 🟢 **GREEN** |
| **RASPBERRY PI** | Validación cualificada para ejecución continua en Raspberry Pi 4/5 | 🟡 **QUALIFYING** |
| **CASAOS / ZIMAOS** | Metadata y `docker-compose.casaos.yml` alineados a versión de release | 🟢 **GREEN** |
| **DOCUMENTATION** | Documentación técnica completa (API, Arquitectura, Configuración) | 🟢 **GREEN** |
| **P0 BUGS** | Cero defectos críticos pendientes | 🟢 **ZERO (0)** |
| **P1 BUGS** | Cero defectos de alta severidad pendientes | 🟢 **ZERO (0)** |
| **SOAK TEST** | Prueba de estabilidad continua (24h/48h) sin fugas de recursos | 🟡 **PENDING** |

---

## 📋 Reglas de Aprobación de Release
1. **Ningún gate en rojo**: La versión v1.0.0 no puede ser etiquetada si cualquiera de los gates anteriores se encuentra en estado `FAIL` o `RED`.
2. **Cero regresiones en CI**: El pipeline de CI en GitHub Actions debe completar en verde tanto en `ci-rust` como en `ci-docker` (con smoke test y contract test integrados).
3. **Cero warnings permitidos**: No se aceptan atributos `#![allow(...)]` globales ni relajaciones de flags de compilación.
