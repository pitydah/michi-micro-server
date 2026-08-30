# Reglas del Proyecto y Preferencias del Usuario

## Flujo de Trabajo y Ciclo de Vida de Ingeniería Obligatorio
Todo desarrollo debe seguir estrictamente este ciclo:
1. **UNDERSTAND**: Analizar requisitos, invariantes del sistema y contratos.
2. **REPRODUCE**: Reproducir y documentar estados de fallo o casos borde.
3. **DESIGN INVARIANT**: Modelar la verdad funcional (`STATE = REALITY`).
4. **CREATE FALSIFICATION TEST**: Escribir pruebas que intenten romper activamente la invariante.
5. **IMPLEMENT**: Implementar la solución limpia, sin stubs, sin clones innecesarios, sin supresión de errores.
6. **FORMAT**: `cargo fmt --all && cargo fmt --all -- --check`.
7. **CHECK**: `cargo check --workspace --all-targets`.
8. **TARGETED TEST**: Ejecutar tests específicos del crate modificado.
9. **WORKSPACE TEST**: `cargo test --workspace`.
10. **CLIPPY**: `cargo clippy --workspace --all-targets --all-features -- -D warnings`.
11. **DIFF REVIEW**: Auditar `git diff` verificando que no existan regresiones ni supresión de errores.
12. **COMMIT**: `git commit` descriptivo.
13. **PUSH**: `git push` a la rama de trabajo.
14. **VERIFY FULL CI**: Verificar que todos los jobs de CI terminen en verde.
15. **ONLY THEN CLAIM COMPLETION**.

## Reglas Críticas
- **NEVER push code that has not passed `cargo fmt --all -- --check`**.
- **GitHub Actions is a certification gate, not a replacement for local validation**.
- **Commit y Push obligatorio**: Siempre realizar `git commit` con un mensaje descriptivo y `git push` al terminar cualquier tarea o trabajo de modificación.
