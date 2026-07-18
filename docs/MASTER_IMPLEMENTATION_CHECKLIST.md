# Master Implementation Checklist

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
- [ ] cargo tree --workspace registrado (parcial)
- [ ] apps/michi-server/src/main.rs analizado
- [ ] crates/michi-webui eliminado o integrado

## Fase 1 — Consolidación del Workspace Rust
- [ ] michi-webui agregado a workspace.members o eliminado
- [ ] Versiones unificadas
- [ ] Licencia unificada (GPL-3.0-only)
- [ ] Dependencias directas completas
- [ ] cargo check --workspace pasa

## Fase 2 — Limpieza y Formato Real del Repositorio
- [ ] cargo fmt --check pasa
- [ ] Archivos YAML/TOML/MD formateados

## Fase 3 — Una Sola Web UI
- [ ] michi-api/static/ es la canónica
- [ ] michi-webui eliminado del workspace
- [ ] cache busting agregado
- [ ] Versión dinámica desde servidor

## Fase 4 — Corrección Funcional Completa Web UI
- [ ] Cliente API centralizado (MichiAPI)
- [ ] Estado global (State)
- [ ] Estados universales (loading/empty/error)
- [ ] Dashboard funcional
- [ ] Library funcional
- [ ] Scan funcional
- [ ] Playlists funcional
- [ ] Michi Link funcional
- [ ] Status funcional
- [ ] Settings funcional
- [ ] History funcional
- [ ] Chains funcional
- [ ] Reproducción funcional

## Fase 5 — Rediseño Premium Completo
- [ ] Paleta implementada
- [ ] Layout desktop correcto
- [ ] Sidebar actualizada
- [ ] Topbar actualizada
- [ ] Hero strips en cada página
- [ ] Dashboard premium
- [ ] Library premium
- [ ] Scan premium
- [ ] Playlists premium
- [ ] Michi Link premium
- [ ] Status premium
- [ ] Right rail Now Playing
- [ ] Mini-player
- [ ] Componentes reutilizables
- [ ] Animaciones
- [ ] Responsive

## Fase 6 — Accesibilidad y Rendimiento
- [ ] aria-labels en icon buttons
- [ ] focus visible
- [ ] contraste suficiente
- [ ] polling moderado
- [ ] sin librerías innecesarias

## Fase 7 — Seguridad
- [ ] michi-security integrado
- [ ] Bearer tokens en rutas protegidas
- [ ] Rate limiting
- [ ] CORS restrictivo
- [ ] Tests de autorización

## Fase 8 — Sync, Receivers, Chains
- [ ] Reconexión con backoff en sync peers
- [ ] Offline detection en receivers
- [ ] Chains validado

## Fase 9 — CI, Docker y Release
- [ ] CI dividido en jobs
- [ ] Docker build pasa
- [ ] docker compose config pasa

## Fase 10 — Tests Completos
- [ ] Tests existentes pasan (192+)
- [ ] Nuevos tests agregados

## Fase 11 — Documentación
- [ ] README actualizado
- [ ] CHANGELOG actualizado
- [ ] Docs actualizados

## Fase 12 — Validación Final
- [ ] cargo fmt --check
- [ ] cargo check --workspace
- [ ] cargo test --workspace
- [ ] cargo clippy --workspace --all-targets -- -D warnings
- [ ] docker build .
- [ ] docker compose config
- [ ] Smoke tests HTTP
