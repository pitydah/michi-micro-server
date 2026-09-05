# Changelog

## [1.0.0-rc.1] - 2026-09-05

### Added
- **Release Evidence Ledger**: Marco riguroso de evaluación de evidencia en `release/gates.json`, con clasificación estricta de taxonomías (`STATIC_ANALYSIS`, `INTEGRATION_REAL`, `CONTRACT_SIMULATOR`, `PHYSICAL_HARDWARE`) sin estados hardcodeados.
- **Lamport Logical Clocks & Causal Ordering**: Reconciliación de estados con relojes lógicos Lamport, preservación de procedencia de línea de tiempo y precedencia estricta de encarnación (`epoch`) ante reinicios.
- **Product Truth Spec**: Especificación unificada de capacidades y madurez en `spec/v1/product-truth.json` y subset normativo OpenSubsonic en `spec/v1/opensubsonic.json`.
- **Ecosistema E2E**: Baterías de integración tridireccional Mobile -> Micro -> Stream y cualificación en hardware/emulación QEMU ARM64.
- **Packaging ZimaOS / CasaOS**: Distribución automatizada con assets normalizados y versionado SemVer coordinado con el workspace.

### Changed
- **Canonización de Puertos**: Puerto interno y healthcheck estándar unificado a `9090` en Dockerfile, docker-compose y appliance.
- **Transmisión de Estado WebSocket**: El despachador WebSocket utiliza orden total determinista causal (`has_precedence_over`) en lugar de comparaciones de reloj de pared.
- **OpenSubsonic Range Support**: `/rest/stream` procesa solicitudes parciales con cabeceras `Range` retornando `206 Partial Content` y `416 Range Not Satisfiable`.
- **HLS VOD**: Ruta de codificación universal compatible a AAC estéreo (48kHz) desacoplada de la copia directa de codecs incompatibles en streaming.

### Fixed
- Eliminada dependencia de estados hardcodeados en el release gate.
- Reparada la verificación de procedencia en `/api/v1/sync/state` requiriendo `device_id` explícito.
- Evitada fuga de zombies y descriptores de archivo en pruebas de estabilidad soak prolongadas.

### Known Beta
- Control de receptores y salas multi-room clasificados como `beta` hasta la certificación con hardware físico Michi Stream.
- Protocolo OpenSubsonic implementado como subset v1 deliberado sin afirmar paridad completa del protocolo heredado.

### Deferred
- Reproducción gapless sample-perfect postergada a post-v1 tras certificación en banco de audio.
- Streaming adaptativo ABR / DASH fuera de alcance de la versión Micro.


## [0.2.0] - 2026-07-17

### Added
- Workspace consolidado: 21 crates, linting limpio (clippy -D warnings)
- Web UI premium: paleta oscura, hero strips, sidebar con grid, cache busting, responsive
- Dashboard: cards con métricas reales, estado de reproducción, health, ecosystem
- Library: tabla con tracks, covers, format badges, búsqueda
- Playlists: CRUD, smart playlists con 8 reglas, export M3U
- History: paginada con stats, export JSON, clear
- Chains: cadena de reproducción multi-receptor con drag & drop, volumen por receptor
- Playback: WebSocket sync, handoff (takeover), control remoto REST
- Sync peers: reconexión exponencial con backoff y jitter
- Receivers: mDNS discovery, pairing, session management, offline detection
- Upload resumable: init/chunk/complete con verificación SHA-256
- Webhook: configuración URL, test, trigger post-sync
- Snapshot: estadísticas de biblioteca exportables
- Integrity check: verificación de archivos en disco
- Identidad criptográfica (michi-identity): Ed25519 + ChaCha20-Poly1305
- Descubrimiento (michi-connect): mDNS, QR pairing, verificación de firmas
- Asistente novato (michi-onboard): wizard de configuración inicial
- Ingesta de streams (michi-ingest): RSS/radio, protección SSRF
- Bookmars: guardar y restaurar posición de reproducción
- Job Queue: procesamiento asíncrono con prioridades y reintentos
- Job Queue: persistente con historial, reintentos, prioridades y auditoría
- Radio stations: emisoras con favoritos, búsqueda y stream URLs
- Dynamic Room Groups: modos Party/Relax/Custom multi-room
- Broadcast & Cast: proxy streaming + UI premium
- Mount Guard: monitorización de salud de directorios de música
- Auditoría: registro de cambios con journal de eventos
- Seguridad: rate limiting, security headers, auth middleware en rutas sensibles
- i18n: 9 idiomas (EN, ES, PT, DE, FR, IT, RU, ZH, JA)
- Configuración persistente: UI settings guardados en config.json
- CI: jobs separados (rust, docker, release GHCR)
- Docker: multi-stage build, healthcheck, Docker Compose
- 38 migraciones de base de datos
- 13 nuevos tests para michi-connect

### Changed
- Versión unificada a 0.2.0 en todos los crates
- Licencia consistente GPL-3.0-only
- `michi-webui` removido (código muerto, sin src/)
- CSS reescrito: -1183 líneas, diseño premium
- Cache busting: styles.css?v=2, app.js?v=2
- Polling: 60s con check de visibilidad de página

### Fixed
- Dashboard `missing_files` query invertida corregida
- `PlaybackChainUpdate` ahora soporta `track_id`
- Search avanzado conectado al frontend
- Clippy warnings: todos resueltos (150+)
- Docker build: Rust 1.86 para compatibilidad de crates
