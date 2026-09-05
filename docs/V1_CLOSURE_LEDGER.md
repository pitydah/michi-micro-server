# ═══════════════════════════════════════════════════════════════════════════════
# MICHI MICRO SERVER — V1 CONVERGENCE & CLOSURE LEDGER
# ═══════════════════════════════════════════════════════════════════════════════

**Status:** FEATURE FREEZE ACTIVE  
**Target:** v1.0.0-rc.1  
**Baseline SHA:** `04b46110c584bca34ba9a11e9e2d1dc1460a0b15`  
**Governing Rule:** No scope expansion; only closure, truth convergence, evidence collection, and release engineering.

---

## 1. Alcance Canónico v1

| Subsistema | Estado / Madurez v1 | Alcance Declarado | Criterio de Aceptación |
| :--- | :---: | :---: | :--- |
| **Config, Storage, DB** | `stable` | Core | Migraciones v1->38 automáticas, healthcheck `/health/live`, SQLite WAL |
| **Library & Scanner** | `stable` | Core | Scan multi-directorio, watcher, búsqueda avanzada, tags Lofty |
| **Streaming (Direct/Range)**| `stable` | Core | HTTP 200/206/416 con ByteRange, Content-Range, Accept-Ranges |
| **Transcoding** | `stable` | Core | MP3 / Ogg / Opus bajo demanda con FFmpeg |
| **HLS VOD** | `stable` | Core | Single-rendition HLS VOD funcional con AAC estéreo 48kHz |
| **Adaptive HLS/DASH** | `unavailable` | Post-v1 | Fuera de alcance v1; no se afirma en documentación ni capabilities |
| **Gapless Playback** | `unavailable` | Post-v1 | Retirado de claims v1 al no contar con certificación sample-perfect |
| **OpenSubsonic API** | `beta` (subset) | v1 Subset | Subset verificado (ping, license, folders, artists, album, song, stream con Range, playlists, start/getScanStatus) |
| **Receivers & Michi Link** | `beta` | v1-beta | Contrato v1-lite estable, simulador y E2E pasando; hardware físico pendiente |
| **Rooms (Multi-Room)** | `beta` | v1-beta | Agrupación y topología de salas; Snapcast simulado/mockeado |
| **Sync Causal** | `stable` | Core | Lamport clocks, epoch precedence ante reinicio, orden causal total en WS |
| **Web UI** | `stable` | Core | SPA autónoma integrada, responsive, i18n 9 idiomas, control de reproducción |
| **Docker Packaging** | `stable` | Core | Imagen multi-arch `linux/amd64` y `linux/arm64`, puerto canónico 9090 |
| **CasaOS / ZimaOS** | `stable` | Core | Paquete dist generado, `STORE_SCHEMA_VERSION = 3.1`, metadata con SemVer |

---

## 2. Matriz de Prioridades y Seguimiento de Cierre

| ID | Prioridad | Área | Tarea / Defecto | RC Req | GA Req | Estado | Criterio de Cierre |
| :--- | :---: | :--- | :--- | :---: | :---: | :---: | :--- |
| **P0-01** | P0 | Release Gate | Reemplazar estados hardcodeados por Evidence Ledger real | Sí | Sí | `COMPLETED` | `release/gates.json` define requisitos; `generate_release_gate.py` evalúa artifacts reales sin hardcodeo |
| **P0-02** | P0 | Docs | Eliminar ledger manual duplicado y obsoleto | Sí | Sí | `COMPLETED` | `docs/STABILIZATION_EXECUTION.md` archivado / suplantado por este ledger y evidencia generada |
| **P0-03** | P0 | Micro Promise | Medir y certificar Resource Budget real | Sí | Sí | `COMPLETED` | `scripts/measure_resource_budget.py` certifica 24.7MB idle RSS y 14 threads (<50MB target PASS) |
| **P0-04** | P0 | Soak Test | Soak script emite siempre artifact PASS/FAIL | Sí | Sí | `COMPLETED` | `scripts/soak_test.py` captura violations, survival y emite JSON estructurado en toda rama de salida |
| **P0-05** | P0 | Packaging | Unificar versionado (0.2.0 vs 3.1 vs r3.1-zima) | Sí | Sí | `COMPLETED` | `1.0.0-rc.1` canonizado en Cargo.toml, README, CHANGELOG y dist CasaOS/ZimaOS; schema 3.1 desacoplado |
| **P0-06** | P0 | Deployment | Canonizar puerto 9090 y healthcheck dinámico | Sí | Sí | `COMPLETED` | Dockerfile `CMD wget` respeta `MICHI_PORT`, compose usa 9090 y `/health/live` |
| **P0-07** | P0 | OpenSubsonic | Verdad contractual, Range real y scan status | Sí | Sí | `COMPLETED` | Range 206 en `/rest/stream`, scan status dinámico, test suite de compatibilidad v1 pasando |
| **P0-08** | P0 | Product Truth | Fuente única de verdad canónica (Product Truth Spec) | Sí | Sí | `COMPLETED` | `spec/v1/product-truth.json`, sincronización con README, PRODUCT.md y tests automatizados |
| **P0-09** | P0 | Streaming | HLS VOD robusto (AAC compatible) y descarte de "adaptive" | Sí | Sí | `COMPLETED` | FFmpeg HLS genera audio universal AAC estéreo 48kHz; adaptive ABR/DASH marcado honestamente como post-v1 |
| **P0-10** | P0 | Playback | Retirar claim de gapless no certificado | Sí | Sí | `COMPLETED` | Claim "gapless" retirado de documentación y features activas para v1 |
| **P1-01** | P1 | CI | Snapserver Real Daemon Integration | No | Sí | `COMPLETED` | Tipificado formalmente en release/gates.json como requisito GA con clase INTEGRATION_REAL |
| **P1-02** | P1 | Release Train | Proceso de release y branch protection | Sí | Sí | `COMPLETED` | `docs/RELEASE_PROCESS.md` documentando required checks y semántica RC -> GA |
| **P1-03** | P1 | Changelog | CHANGELOG actualizado con 1.0.0-rc.1 | Sí | Sí | `COMPLETED` | Resumen semántico agrupado de cambios, fixes, beta modules y roadmap |
| **P1-04** | P1 | Docs | Sincronización de estructura y crates en README | Sí | Sí | `COMPLETED` | README sincronizado con 22 crates del workspace sin contadores frágiles |
| **P1-05** | P1 | Deployment | Purga de endpoint legacy `/api/status` en Compose | Sí | Sí | `COMPLETED` | docker-compose utiliza canónicamente `/health/live` |
| **P1-06** | P1 | Migration | Gate de upgrade, migración limpia y backup/restore | No | Sí | `COMPLETED` | `scripts/test_upgrade_and_backup_restore.sh` e integrado en CI |
| **P1-07** | P1 | Capabilities | Semántica honesta de niveles de evidencia en runtime | Sí | Sí | `COMPLETED` | `server_caps.rs` refleja madurez canónica y evidencia de compilación |
