# Changelog

## [0.2.0] - 2026-07-17

### Added
- Workspace consolidado: 18 crates, linting limpio (clippy -D warnings)
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
- Seguridad: rate limiting, security headers, auth middleware en rutas sensibles
- CI: jobs separados (rust, docker, release GHCR)
- Docker: multi-stage build, healthcheck, Docker Compose

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
