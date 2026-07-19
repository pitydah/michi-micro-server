# Changelog

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
- 35 migraciones de base de datos
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
