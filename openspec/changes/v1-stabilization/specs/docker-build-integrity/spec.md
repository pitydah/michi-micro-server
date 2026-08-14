# Docker Build Integrity Specification

## Purpose

Guarantee that a Docker image built from the repository runs the real server — not a
cached no-op placeholder — and that the build is deterministic with respect to source
changes.

## Requirements

### Requirement: Real source/assets invalidate the dummy-source cache

The Dockerfile MUST invalidate the dummy-source cache step so that a stale placeholder
binary can never be shipped. The placeholder `lib.rs`/`main.rs` MUST be removed and source
mtimes MUST be refreshed (e.g. `rm` + `touch`) before the final `cargo build`.

#### Scenario: Placeholder binary is never shipped

- GIVEN the Dockerfile's dependency-caching step creates placeholder sources
- WHEN the final `cargo build` runs
- THEN placeholder sources MUST have been removed or invalidated so cargo cannot reuse a
  no-op binary from cache

#### Scenario: Source change forces rebuild

- GIVEN a change to real server source after a prior cached build
- WHEN the image is rebuilt
- THEN the real, updated source MUST be compiled, and the image MUST NOT serve the stale
  placeholder binary

### Requirement: Built image runs the real server

A built image MUST run the real server: initialize the SQLite database and apply all
migrations on boot, serve the health endpoints and the WebUI, and stop gracefully where
testable.

#### Scenario: Smoke test proves a real server

- GIVEN a freshly built image
- WHEN a smoke test boots the container
- THEN `/api/status` (or `/health/live`) MUST return 200 with `version` matching the
  workspace version
- AND the server MUST have applied the full migration set on boot
- AND the WebUI root MUST be served (200)
- AND the container MUST reach `healthy` per its healthcheck

#### Scenario: Placeholder/no-op binary detected

- GIVEN a corrupted cache state that would produce a no-op binary
- WHEN the smoke test boots the resulting image
- THEN the smoke test MUST fail (no healthy server / wrong version) rather than pass

### Requirement: Graceful stop where testable

Where the runtime permits, the image MUST respond to a termination signal by shutting down
the server cleanly (WAL checkpoint on shutdown) without data corruption.

#### Scenario: Graceful shutdown

- GIVEN a running container with an initialized database
- WHEN the container receives a stop signal
- THEN the server MUST shut down cleanly with the SQLite WAL checkpointed
- AND the smoke teardown MUST remove the container and image
