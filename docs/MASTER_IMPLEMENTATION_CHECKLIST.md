# Master Implementation Checklist — Estado Final

## Fase 0 — Inspección Real del Repositorio
- [x] pwd, git status, branch, log
- [x] tags listados
- [x] find . -maxdepth 3 -type f
- [x] Cargo.toml raíz analizado
- [x] apps/michi-server/Cargo.toml analizado
- [x] crates/*/Cargo.toml analizados
- [x] michi-webui sin src/ detectado
- [x] Frontend activo = michi-api/static/
- [x] CI workflow analizado
- [x] Dockerfile analizado
- [x] docker-compose.yml analizado
- [x] cargo metadata ejecutado
- [x] apps/michi-server/src/main.rs analizado

## Fase 1 — Consolidación del Workspace Rust
- [x] michi-webui eliminado del workspace y filesystem
- [x] Versiones unificadas (todos version.workspace = true)
- [x] Licencia unificada (GPL-3.0-only)
- [x] Dependencias directas completas
- [x] cargo check --workspace pasa

## Fase 2 — Limpieza y Formato Real del Repositorio
- [x] cargo fmt --check pasa
- [x] Archivos Rust formateados

## Fase 3 — Una Sola Web UI
- [x] michi-api/static/ es la canónica
- [x] michi-webui eliminado del workspace
- [x] cache busting agregado (styles.css?v=2, app.js?v=2)
- [x] Versión dinámica desde servidor (sidebar-ver desde serverInfo)

## Fase 4 — Corrección Funcional Completa Web UI
- [x] Cliente API centralizado (MichiAPI con fetch + timeout + errores)
- [x] Estado global (State con status, serverInfo, dashboard, tracks, etc.)
- [x] Estados universales (renderEmpty, renderError, loading skeletons)
- [x] Dashboard funcional (cards con métricas reales, N/D para datos faltantes)
- [x] Library funcional (tabla con covers, format badges, búsqueda local/remota)
- [x] Scan funcional (start scan, progress, music paths)
- [x] Playlists funcional (lista, creación, smart playlists)
- [x] Michi Link funcional (features grid, server URL, test connection)
- [x] Status funcional (grid de status-items con health checks)
- [x] Settings funcional (5 tabs: Sync, Handoff, Receivers, Webhook, Backup)
- [x] History funcional (paginated, stats, export, clear)
- [x] Chains funcional (CRUD, drag & drop reorder, per-device volume, play/stop)
- [x] Reproducción funcional (play/pause, progreso, now playing, mini-player)

## Fase 5 — Rediseño Premium Completo
- [x] Paleta premium (#070A12, #8B5CF6, #38BDF8, etc.)
- [x] Layout desktop correcto (sidebar 248px, main flexible, right rail 320px)
- [x] Sidebar actualizada (brand con gradient, grid footer con ID/Up/API/DB)
- [x] Topbar actualizada (search, status pill, scan/test buttons)
- [x] Hero strips en cada página (con gradiente radial y meta info)
- [x] Dashboard premium (7 cards con iconos SVG, hover glow)
- [x] Library premium (tabla con sticky header, format badges)
- [x] Scan premium (panel con progreso, music paths)
- [x] Playlists premium (grid responsive con badges)
- [x] Michi Link premium (feature cards, connection panel)
- [x] Status premium (dos columnas en desktop, health checks)
- [x] Right rail Now Playing (cover, title, progress, queue)
- [x] Mini-player (cover, info, progress bar, controls)
- [x] Componentes reutilizables (.panel, .tabs, .badge, .skeleton, .empty-state)
- [x] Animaciones (hover 120ms, fadeInUp 160ms, skeleton shimmer, pulse-dot)
- [x] Responsive (1280px, 1024px, 768px, prefers-reduced-motion)

## Fase 6 — Accesibilidad y Rendimiento
- [x] aria-live en toast (role="alert")
- [x] aria-label en search input
- [x] focus-visible styles (:focus-visible con outline primary)
- [x] polling moderado (60s con document.hidden check)
- [x] sin librerías innecesarias (vanilla JS, sin CDN obligatoria)

## Fase 7 — Seguridad
- [x] michi-security integrado en AppState + protected router
- [x] auth middleware en rutas sensibles (bearer token validation)
- [x] rate limiting middleware (10 rps, burst 20)
- [x] security headers middleware (X-Frame-Options, X-XSS-Protection, etc.)
- [x] CORS restrictivo (permissive solo en dev_mode)
- [x] Tests de autorización (pairing flow, permissions check)

## Fase 8 — Sync, Receivers, Chains
- [x] Reconexión con backoff exponencial (5s-300s + jitter) en sync peers
- [x] Cada peer en tarea independiente con loop infinito
- [x] Offline detection real en receivers (marca offline >180s, limpia session)
- [x] Chains funcional (CRUD, play/stop, reorder, per-receiver volume)

## Fase 9 — CI, Docker y Release
- [x] CI dividido en 3 jobs: ci-rust, ci-docker, release-ghcr
- [x] Docker build multi-stage (Rust 1.86, Debian bookworm-slim)
- [x] docker compose config válido (sin warnings)

## Fase 10 — Tests Completos
- [x] Tests existentes pasan (99+ en michi-api)
- [x] Tests de autorización (pairing + permissions)
- [x] Tests de content types (CSS, JS, SVG)
- [x] Tests de version consistency (status.version == server_info.version)

## Fase 11 — Documentación
- [x] README actualizado (features, structure, quick start, API, config)
- [x] CHANGELOG actualizado (0.2.0 con todos los cambios)
- [x] Docs de checklist y evidencia creados

## Fase 12 — Validación Final
- [x] cargo fmt --check
- [x] cargo check --workspace
- [x] cargo test --workspace
- [x] cargo clippy --workspace --all-targets -- -D warnings
- [x] docker build .
- [x] docker compose config
- [x] Smoke tests HTTP
