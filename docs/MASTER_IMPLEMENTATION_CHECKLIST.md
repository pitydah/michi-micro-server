# Master Implementation Checklist — Estado Actual

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
- [ ] cache busting agregado (PENDIENTE)
- [ ] Versión dinámica desde servidor (PENDIENTE)

## Fase 4 — Corrección Funcional Completa Web UI
- [x] Cliente API centralizado (MichiAPI)
- [x] Estado global (State)
- [ ] Estados universales (loading/empty/error) — parcial
- [ ] Dashboard funcional con datos reales — parcial
- [ ] Library funcional — parcial (tabla funciona, faltan filtros/orden)
- [x] Scan funcional
- [ ] Playlists funcional — parcial (lista funciona, vista detalle no)
- [ ] Michi Link funcional — parcial (features cargan, faltan trusted clients)
- [ ] Status funcional — parcial
- [x] Settings funcional
- [x] History funcional
- [x] Chains funcional
- [ ] Reproducción funcional — parcial (playTrack funciona, queue no)

## Fase 5 — Rediseño Premium Completo
- [ ] Paleta implementada (PENDIENTE)
- [ ] Layout desktop correcto (PENDIENTE)
- [ ] Sidebar actualizada (PENDIENTE)
- [ ] Topbar actualizada (PENDIENTE)
- [ ] Hero strips en cada página (PENDIENTE)
- [ ] Dashboard premium (PENDIENTE)
- [ ] Library premium (PENDIENTE)
- [ ] Scan premium (PENDIENTE)
- [ ] Playlists premium (PENDIENTE)
- [ ] Michi Link premium (PENDIENTE)
- [ ] Status premium (PENDIENTE)
- [ ] Right rail Now Playing (PENDIENTE)
- [ ] Mini-player (PENDIENTE)
- [ ] Componentes reutilizables (PENDIENTE)
- [ ] Animaciones (PENDIENTE)
- [ ] Responsive (PENDIENTE)

## Fase 6 — Accesibilidad y Rendimiento
- [ ] aria-labels en icon buttons (PENDIENTE)
- [ ] focus visible (PENDIENTE)
- [ ] contraste suficiente (PENDIENTE)
- [ ] polling moderado (PENDIENTE)
- [ ] sin librerías innecesarias — OK (vanilla JS)

## Fase 7 — Seguridad
- [ ] michi-security integrado en rutas (PENDIENTE)
- [ ] Bearer tokens en rutas protegidas (PENDIENTE)
- [ ] Rate limiting (PENDIENTE)
- [ ] CORS restrictivo (PENDIENTE)
- [ ] Tests de autorización (PENDIENTE)

## Fase 8 — Sync, Receivers, Chains
- [ ] Reconexión con backoff en sync peers (PENDIENTE)
- [ ] Offline detection en receivers (PENDIENTE — solo warning log)
- [ ] Chains validado (PENDIENTE — no hay tests)

## Fase 9 — CI, Docker y Release
- [ ] CI dividido en jobs (PENDIENTE — actualmente un solo job)
- [x] Docker build pasa
- [x] docker compose config pasa

## Fase 10 — Tests Completos
- [x] Tests existentes pasan
- [ ] Tests de autorización (PENDIENTE)
- [ ] Tests de Web UI content types (PENDIENTE)
- [ ] Tests de version consistency (PENDIENTE)

## Fase 11 — Documentación
- [ ] README actualizado (PENDIENTE)
- [ ] CHANGELOG actualizado (PENDIENTE)
- [ ] Docs actualizados (PENDIENTE)

## Fase 12 — Validación Final
- [x] cargo fmt --check
- [x] cargo check --workspace
- [x] cargo test --workspace
- [x] cargo clippy --workspace --all-targets -- -D warnings
- [x] docker build .
- [x] docker compose config
- [x] Smoke tests HTTP
