# Exploration: v1-stabilization — Phase 0 Baseline Audit

**Change**: `v1-stabilization` · **Audited object**: exact `HEAD` `e6f6dd6f1b043ba9483614027bee2284586cf4ef` (branch `main`, merge of PR #4)
**Date**: 2026-08-14 · **Mode**: auto / hybrid (OpenSpec + Engram) · **Isolation**: disposable detached worktree
**Severity model**: user-defined (authoritative) — P0 CRITICAL = demonstrated data corruption/loss, auth bypass, arbitrary path/RCE, boot failure, destructive migration, unrecoverable queue/DB failure; P1 HIGH = receiver reconnect failure, incorrect handoff, inconsistent scanner/sync/token/Snapcast/HA recovery, significant API contract break; P2 = non-critical reliability, uncommon failure, performance regression, admin/UX. Release-gate blockers may be red without being P0 runtime incidents.

---

## 1. Executive Summary

Michi Micro Server v0.2.0 (`main` @ `e6f6dd6`) is a **green baseline**: all four Rust CI gates pass on exact HEAD
(`fmt`, `check`, `test`, `clippy` with `RUSTFLAGS=-D warnings`), **217 tests pass / 0 fail / 14 ignored**,
and a Docker build + smoke test on HEAD serves a real, healthy server (`/api/status` 200, `version 0.2.0`,
37 migrations applied). The architecture is a 22-member Cargo workspace (1 binary + 21 crates) with axum 0.7,
sqlx/SQLite, tokio, utoipa/Swagger, rustls, argon2, governor rate limiting, embedded WebUI/PWA, OpenSubsonic
compat, Python sidecars, and CI (rust → docker → GHCR release).

**Severity verdict: `P0 = 0`.** No user-defined P0 condition is demonstrated on HEAD: the server boots, the Docker
smoke passes, and no data corruption/loss, auth bypass, path traversal/RCE, boot failure, destructive migration,
or unrecoverable queue/DB failure was observed or proven by evidence.

**However, the v1 stabilization targets are NOT met by HEAD — three P1 release-gate/deployment blockers and
one P1 verification gap, plus P2 drift items**:

1. **P1 — Player↔Micro contract chain is broken (release-gate blocker).** The E2E Python test
   `tests/e2e/test_player_micro_contract_compatibility.py` does not even parse: `global BASE_URL` at line 71
   after `BASE_URL` is used at line 69 → `SyntaxError`. It also asserts `michi_link_version == "1.0.0-alpha"`
   (line 40) against a field the server **never emits** (`V1ServerInfo`, `crates/michi-api/src/routes/v1/server.rs:11-31`).
   And no Python/E2E test runs in CI. This is a significant API contract break (documented Michi Link v1 contract
   field absent + only verification dead). It does **not** prove product runtime failure; it blocks contract
   verification and therefore v1 release confidence.
2. **P1 — GHCR releases are amd64-only (release/deployment blocker).** `.github/workflows/ci.yml:61`
   (`platforms: linux/amd64`) while CasaOS/ZimaOS metadata declares `arm64`
   (`casaos/docker-compose.casaos.yml` `x-casaos.architectures`) and docs/ROADMAP claim "multi-arch amd64+arm64".
   The v1 constraint (amd64+arm64) is unmet at the release pipeline; arm64 deployers cannot pull the image.
3. **P1 — Receiver integration verification gap.** 14 `#[ignore]` tests covering critical receiver behaviors
   (pairing, session lifecycle, volume/codec enforcement — adjacent to reconnect/handoff correctness) depend on an
   external simulator from another repo; scripts hardcode machine-specific paths; the E2E script is broken from the
   workspace root; two dead duplicate copies of the test file drift from the compiled one. These behaviors are
   **unverified**; no runtime receiver failure was demonstrated, so this is a P1 verification gap, not a P0 incident.
4. **P1 (latent) — Docker build-integrity bug in the dummy-source cache step.** `Dockerfile:33-49` never removes
   placeholder sources nor invalidates mtimes; in some cache states it can ship a no-op placeholder binary.
   **Exact clean HEAD baseline passed** (fresh uniform-mtime checkout, smoke 200 OK). The prior reproduction
   happened in a different context (stale-mtime working tree, 2026-08-07 session) and is clearly distinguished
   from this exact-HEAD result; the working tree already carries an uncommitted fix (mtime invalidation) that HEAD lacks.
5. **P2 — Version/pinning and metadata drift (no demonstrated failure).** Dockerfile pins `RUST_VERSION=1.88` vs
   local/CI stable 1.96 with no `rust-toolchain.toml` (non-reproducible builds); CasaOS `data.yml` says `0.1.0`
   vs workspace `0.2.0`; CHANGELOG says "35 migraciones" vs 37 migration functions. None of these is demonstrated
   to cause runtime or deployment failure on HEAD.

No production `panic!`/`todo!`/`unimplemented!` exists. Prod-path `unwrap`s are limited and low-risk (see §9).

## 2. Scope, Method, Isolation

- **Contamination context (main worktree, NOT audited)**: HEAD `e6f6dd6`, branch `main`; uncommitted: `.gitignore`,
  `Dockerfile`, `crates/michi-api/src/{lib,pwa,static_files}.rs`, WebUI statics (`app.js`, `hero-cat.css`,
  `i18n/en.json`, `index.html`, `styles.css`), `tests/api.rs` (11 files, +761/−1967); untracked: `.impeccable/`,
  `DESIGN.md`, `PRODUCT.md`, `static/assets/*` (hero webp, PWA PNGs), `openspec/`. Main worktree was NOT modified.
- **Audit worktree**: `git worktree add --detach /tmp/opencode/michi-v1-audit-head e6f6dd6`; verified pristine
  after all commands.
- **Build isolation**: `CARGO_TARGET_DIR=/tmp/opencode/michi-v1-audit-target` (symlink → `/home/cristian/.cache/michi-v1-audit-target`,
  3.7 GB, because `/tmp` is tmpfs with 8.4 GB free and the main `target/` is 12 GB), `CARGO_INCREMENTAL=0`.
  No global `cargo clean`; only the disposable worktree + dedicated target are removed at the end.
- **Log**: `/tmp/opencode/michi-v1-audit-baseline.log`.
- **Tools detected**: cargo 1.96.0, rustc 1.96.0, docker 29.7.2 (buildx, linux/amd64 +3). NOT installed:
  `cargo-nextest`, `cargo-audit`, `cargo-deny`, `cargo-tarpaulin`, `cargo-machete`, `cargo-watch` (Makefile targets exist for audit/coverage/watch).

## 3. HEAD Architecture and Maturity

- **Workspace**: 22 members (`apps/michi-server` + 21 crates), `version 0.2.0`, edition 2021, GPL-3.0-only, resolver 2.
- **Dependency topology** (from `cargo metadata --no-deps`): `michi-server` (15 deps) → `michi-api` (61 deps, 173 `.route()` in lib.rs, 2 targets) is the hub; `michi-opensubsonic` (19), `michi-identity` (17), `michi-security` (15), `michi-link` (15), `michi-tui` (13, 2 targets incl. binary), `michi-connect` (12), `michi-homeassistant` (12), `michi-receivers` (11, 2 targets incl. integration test), `michi-sync` (10). `Cargo.lock`: 458 packages, no duplicate external versions (single axum 0.7.9, tokio 1.52.3, sqlx 0.8.6, chrono 0.4.45, serde 1.0.228); no git deps.
- **API surface**: public `/api/v1/server/info`, `/api/v1/status`, `/health/live|ready`, `/api/v1/capabilities|policy`, `/api/v1/pair/*`, `/api/v1/token/refresh`; everything else under `v1_auth_middleware` (admin list + user session/device token, `crates/michi-api/src/auth.rs:159-196`); legacy `/api` under `auth_middleware`; OpenSubsonic `/rest/*` uses per-route `check_auth` (22 routes). Swagger at `/api/docs` (utoipa).
- **WebUI/PWA**: embedded via `include_str!`/`include_bytes!` (9 i18n files, styles, hero-cat, app.js, logo/favicon PNG+SVG; `pwa.rs` manifest `display: standalone`, `purpose: any maskable`). At HEAD the WebUI predates the pending design work (contamination, separate change).
- **Workers/jobs**: sync peers, HA MQTT (gated on `MICHI_MQTT_HOST`), watchdog (10 s interval, 15 s hang threshold, `apps/michi-server/src/main.rs`), job queue with cancel tokens, module tokens (scan/sync/playback/backup/webhook/homeassistant).
- **DB**: hand-rolled migration runner with `_migrations` table, **37 migration functions** (`crates/michi-db/src/lib.rs`, `migration_001..037`; runner at :82; definitions out of order — 034–037 at :1115–1164 before 001–015 at :1216+); no down-migrations; WAL + `PRAGMA wal_checkpoint(TRUNCATE)` on shutdown; JSON backup/restore round-trip (PR #4) incl. tracks/playlists/starred/history; identity (`michi-identity`) Ed25519 + ChaCha20-Poly1305; persistent `server_id`.
- **Security**: `rate_limit_middleware` (governor, default 10 rps / 20 burst), `security_headers_middleware`, argon2 password hashing, rustls 0.23, SSRF guard in `michi-ingest` (`is_private_or_link_local`, `crates/michi-ingest/src/lib.rs:48-56`), auth sessions (24 h expiry), link token store with DB persistence + cleanup task. No `cargo audit`/`cargo deny` anywhere (local or CI).
- **Streaming/playback**: HTTP Range streaming, ffmpeg transcode (feature-flagged via `check_ffmpeg()`), WebSocket sync, rooms/Snapcast adapters, receivers (mDNS discovery, pairing, sessions), multi-room chains.
- **Deployment**: multi-stage Dockerfile (rust:1.88 → debian:bookworm-slim, non-root `michi`, healthcheck), compose + compose.dev, `deploy/` systemd unit + Debian control, `casaos/` store metadata, `scripts/` receiver sim runners, Makefile (fmt/check/test/clippy/docker/audit/coverage/watch).
- **Docs**: 24 markdown files (ARCHITECTURE, API, MICHI_LINK*, CLIENT_INTEGRATION_SPEC, RECEIVER_INTEGRATION_PLAN, CASAOS_ZIMAOS, STREAM_SIMULATOR_INTEGRATION, ROADMAP, CHANGELOG, IMPLEMENTATION_EVIDENCE, MASTER_IMPLEMENTATION_CHECKLIST, AUTONOMOUS_PLAYBACK, HOME_ASSISTANT, METADATA, RESOURCE_BUDGET, opensubsonic-compat, PLAYER_IMPORT_FLOW, licensing, inspirations).

## 4. Baseline Commands and Results (exact HEAD)

All commands ran in the detached worktree; `RUSTFLAGS="-D warnings"` for check/test/clippy; `CARGO_TARGET_DIR=/tmp/opencode/michi-v1-audit-target`.

| Command | Exit | Time | Result |
|---|---|---|---|
| `cargo fmt --check` | 0 | ~1 s | PASS |
| `cargo check --workspace` | 0 | 50 s | PASS, zero warnings |
| `cargo test --workspace` | 0 | ~82 s | PASS — **217 passed, 0 failed, 14 ignored** |
| `cargo clippy --workspace --all-targets -- -D warnings` | 0 | ~14 s | PASS, zero warnings |
| `cargo metadata --format-version 1 --no-deps` | 0 | instant | 22 members (see §3) |
| `cargo tree --workspace --duplicates` | 0 | — | no duplicate external versions |
| Docker build (`michi-audit-head:test`) | 0 | ~3 min | image built from HEAD Dockerfile |
| Docker smoke (port 9092): `/api/status` + `/` | 200/200 | — | **real server**: `{"status":"ok","version":"0.2.0","database":"ok"}`; container `healthy`; 37 migrations applied on boot. Cleaned up (container+image removed). |

Test breakdown: `crates/michi-api/tests/api.rs` = **102** tests (all pass); 14 `#[ignore]` = receiver-simulator
integration target (`crates/michi-receivers/tests/receiver_simulator_integration.rs`); 21 crate doc-tests (0 tests each);
unit tests across michi-client (7), connect (13), core (15), db (11), homeassistant (3), identity (2), ingest (2),
m3u (7), metadata (5), onboard (2), rooms (3), scanner (6), security (4), sync (26), tui (5), etc.

**Claim verification against the user brief**:
- fmt/check/test/clippy: all GREEN on HEAD; test count 217 + 14 ignored. Clippy root cause: none (green).
- "Docker blocked by clippy": **No** — clippy passes; `ci-docker` `needs: ci-rust` so clippy gates Docker, but it is green today.
- Player↔Micro Python contract: NOT executable (SyntaxError), asserts missing field, not in CI → **P1 release-gate blocker** (contract break; no runtime failure demonstrated).
- Receiver integration tests: ignored (14), external simulator required, not in CI, script broken → **P1 verification gap** (critical receiver behaviors unverified).
- CasaOS/ZimaOS declared arch: `amd64 + arm64` (`casaos/docker-compose.casaos.yml`) — **arm64 image never published** (P1 deployment blocker).
- GHCR actual platforms: release job builds `linux/amd64` only (`ci.yml:61`); remote manifest inspect unavailable (registry/network), CI config is authoritative. **ARM64 gap confirmed** (P1).
- Tags/version: `v0.1.1-alpha` (2026-06-26), `v0.2.0-beta` (2026-06-29), `v0.2.0` (2026-07-18) — all lightweight; HEAD is 28 commits past `v0.2.0`; no tag since; `latest` only for non-prerelease tags (`!contains(ref,'-')`).
- Migrations/persistence: 37 migrations (CHANGELOG says 35 — P2 drift), JSON backup/restore round-trip (PR #4), WAL checkpoint on shutdown.
- Dockerfile dummy-source cache bug on HEAD: pattern present, no `rm`/`touch` invalidation; empirically did NOT trigger on fresh uniform-mtime checkout (smoke passed) but previously reproduced in a stale-mtime context (2026-08-07; working tree fix uncommitted). HEAD truth: **P1 latent build-integrity risk, not failing on the exact clean baseline**.

## 5. CI / Release / Container State

**Dependency graph** (`ci.yml`): `ci-rust` (ubuntu-latest, `dtolnay/rust-toolchain@stable`, `RUSTFLAGS=-D warnings`, Swatinem rust-cache, `libsqlite3-dev`+`pkg-config`) runs fmt → check → test → clippy; `ci-docker` `needs: ci-rust` builds + smoke (`michi-test`, `-p 9090:9090`, curl `/api/status` and `/`); `release-ghcr` `needs: [ci-rust, ci-docker]`, only on `v*` tag pushes, QEMU+buildx present but **`platforms: linux/amd64`** (ARM64 gap — P1 release/deployment blocker), `type=semver` tags + `latest` (non-prerelease only), gha cache. Triggers: push main + PR main + `v*` tags + dispatch.
**Main/tag safety**: release is gated on both rust+docker jobs via `needs`; a red main blocks releases. Branch protection rules not verifiable from a local worktree (no API read performed).
**Container**: HEAD image builds and serves correctly on fresh checkout; healthcheck + compose + CasaOS compose all target `:8096`; CI smoke uses `:9090` (port override OK). Dockerfile `RUST_VERSION=1.88` vs local/CI 1.96 — non-reproducible toolchain pin (P2); no `rust-toolchain.toml`.
**Missing from CI**: Python contract/E2E tests (P1 chain), receiver-simulator tests (P1 verification gap), dependency audit (`cargo audit`/`deny` — P2), coverage (tarpaulin absent locally; Makefile targets exist — P2), arm64 publish (P1).

## 6. Ignored Test Inventory

Compiled target: `crates/michi-receivers/tests/receiver_simulator_integration.rs` — 14 `#[ignore]` tests (only ignored tests in the repo):

| # | Test | Verifies |
|---|---|---|
| 1 | `test_receiver_info_standard` | GET /api/v1/receiver/info standard identity |
| 2 | `test_receiver_info_hifi` | Hi-Fi identity |
| 3 | `test_receiver_info_standard_output` | jack_3_5, 48 kHz, 16-bit, pcm_s16le |
| 4 | `test_receiver_info_hifi_output` | rca_stereo, 96 kHz, 24-bit, pcm_s24le |
| 5 | `test_receiver_pairing_flow` | start → confirm roundtrip |
| 6 | `test_receiver_pairing_window_closed_rejected` | re-pair rejected |
| 7 | `test_receiver_standard_full_lifecycle` | pair → session → heartbeat → volume → stop |
| 8 | `test_receiver_hifi_full_lifecycle` | same for Hi-Fi |
| 9 | `test_receiver_errors_unsupported_codec` | aac rejected on Standard |
| 10 | `test_receiver_errors_sample_rate_exceeds` | 96 kHz rejected on Standard |
| 11 | `test_receiver_errors_duplicate_session` | 409 on second session |
| 12 | `test_receiver_errors_volume_out_of_range` | 101 clamps to 100 |
| 13 | `test_receiver_errors_unauthenticated` | heartbeat without token fails |
| 14 | `test_receiver_registry_tracks_state` | ReceiverRegistry stores paired state |

- **Dependency**: external simulator `receiver_sim.py` from `pitydah/michi-music-stream` (separate repo); env `MICHI_RECEIVER_SIM_URL` (default :8080) / `MICHI_RECEIVER_SIM_HIFI_URL` (:8081).
- **Scripts**: `scripts/run_receiver_sim_standard.sh` / `run_receiver_sim_hifi.sh` hardcode default `SIM_PATH=/home/cristian/michi-music-stream/simulator/receiver_sim.py` (machine-specific, breaks CI). `scripts/test_receiver_e2e.sh` runs `cargo test --test receiver_simulator_integration` from the workspace root — **broken**: the root manifest is a virtual workspace (no package), so the command errors without `-p michi-receivers`; also doesn't pass `--ignored` correctly for both sims (runs standard only).
- **Dead duplicates**: `tests/receiver_simulator_integration.rs` and `tests/e2e/test_receiver_simulator_integration.rs` are identical to each other but drifted from the compiled crate copy (rustfmt + an extra `#[allow(dead_code)]` in the crate copy); no `[[test]]` anywhere → the two root copies are **never compiled**.
- **CI feasibility**: yes — containerize the simulator or checkout `michi-music-stream` in CI and boot both sims before a `ci-receivers` job; currently never run in CI. Severity: **P1 verification gap** — these cover pairing, session lifecycle, and codec/volume enforcement (receiver reconnect/handoff correctness is unverified); no runtime failure is demonstrated, so not P0.

## 7. Player Contract Audit (Python ↔ Micro)

Files: `clients/python-michi-client/michi_client.py`, `clients/python-michi-player/{player.py,desktop_player.py}`, `tests/e2e/test_player_micro_contract_compatibility.py`, fixtures `tests/fixtures/micro_contract/*.json`.

- `michi_client.py` — syntax OK, aiohttp (lazy import), uses only `/api/v1`; `connect()` GETs `/api/v1/server/info`, raises `VERSION_MISMATCH` unless `api_version == "v1"`; `_handle_error` parses `{error:{code,message}}`. No unit tests.
- `player.py` — syntax OK; CLI requires `mpv` (graceful skip otherwise); passes `--token` to client; playlists/search/stream flows.
- `desktop_player.py` — syntax OK; PySide6 guarded by `HAS_PYSIDE`.
- **E2E contract test — BROKEN (P1 release-gate blocker; does not prove product runtime failure)**:
  1. `python3 -m py_compile` fails: `SyntaxError: name 'BASE_URL' is used prior to global declaration` — `global BASE_URL` at `:71` after `default=BASE_URL` at `:69`. **The test cannot run at all.**
  2. Even after the syntax fix, `:40` asserts `info.get("michi_link_version") == "1.0.0-alpha"` — the server never emits `michi_link_version` (`V1ServerInfo` at `crates/michi-api/src/routes/v1/server.rs:11-31` has `service,name,server_id,version,api_version,roles,features,auth`). Assert would fail. This is the **significant API contract break**: the documented Michi Link v1 field is absent server-side (or the spec/test must be aligned).
  3. Other asserts match HEAD: `service == "michi-micro-server"`, `auth.strategy == "SERVER_CODE"` (`server.rs:79`), `auth.token_refresh == true`, `features.import/playback/queue == true`.
  4. **Not wired into CI** (`ci.yml` has zero Python steps); requires a live server (`--url` default `http://127.0.0.1:8096`), no test server bootstrapping.
  5. `FIXTURES_DIR` resolution (`os.path.join(__file__, "..", "fixtures", ...)`) is correct for the current layout.
- Executability: client/player/desktop compile and run; the contract verification (E2E) is dead; coverage of the contract is therefore **zero in CI** — a release-gate gap for the Player ecosystem, not a demonstrated server runtime failure.

## 8. Receiver Contract Audit

- Crate: `crates/michi-receivers` (client.rs, session_manager.rs, models.rs) + 14 ignored integration tests (see §6).
- Wire contract exercised: `GET /api/v1/receiver/info`, pairing (`start`/`confirm`), session lifecycle, heartbeat, volume, codec/sample-rate rejection, registry state — all against the external `receiver_sim.py` only.
- Docs: `docs/RECEIVER_INTEGRATION_PLAN.md`, `docs/STREAM_SIMULATOR_INTEGRATION.md` (prereq: Flask + sim from another repo).
- Gaps: no in-repo simulator or fixtures; scripts machine-specific; E2E script broken at workspace root; dead duplicate test files; not in CI; Snapcast/rooms adapters exist (`michi-rooms`) but room tests are in the same ignored suite. Classification: **P1 verification gap** (receiver reconnect/session/handoff behaviors unverified); no runtime failure demonstrated → not P0.

## 9. Risk Inventory (file:line evidence)

Severity model applied: user-defined (see header). **P0 CRITICAL = 0 demonstrated.** HEAD boots, Docker smoke passes,
and no data corruption/loss, auth bypass, arbitrary path/RCE, boot failure, destructive migration, or unrecoverable
queue/DB failure was demonstrated by any evidence in this audit.

### P0 CRITICAL (user model) — count: 0
- No finding qualifies. The strongest candidates (contract verification dead, arm64 gap, latent Docker cache bug,
  orphaned receiver tests) are all release-gate/verification/deployment blockers or latent risks — none is a
  demonstrated runtime incident, and "release-gate blockers may be red without being P0 runtime incidents".

### P1 HIGH — release-gate, deployment, and verification blockers
- **P1.1 Player↔Micro API contract chain broken (significant API contract break; release-gate blocker)**:
  `tests/e2e/test_player_micro_contract_compatibility.py:69` (`default=BASE_URL`) vs `:71` (`global BASE_URL`) →
  `SyntaxError` (unexecutable); `:40` asserts `michi_link_version` not emitted by `crates/michi-api/src/routes/v1/server.rs:11-31`;
  not run in CI (`ci.yml`). Three independent faults in one chain: syntax, contract field, CI wiring. Does not prove
  server runtime failure — blocks contract verification and v1 release confidence.
- **P1.2 GHCR ARM64 release/deployment blocker**: `ci.yml:61` `platforms: linux/amd64` only; v1 constraint amd64+arm64;
  CasaOS declares arm64 (`casaos/docker-compose.casaos.yml` `x-casaos.architectures: [amd64, arm64]`); ROADMAP claims
  multi-arch (`docs/ROADMAP.md` Phase 8). arm64 deployers (Raspberry Pi, Rockchip, Apple Silicon) cannot pull a published image.
- **P1.3 Dockerfile dummy-source cache bug (latent build-integrity)**: `Dockerfile:33-49` placeholder `lib.rs`/`main.rs`
  never removed, no mtime invalidation before the final `cargo build`; in some cache states cargo can reuse the no-op
  placeholder binary. **Distinction**: exact clean HEAD baseline (fresh uniform-mtime checkout) built and smoked
  correctly (200 OK); the prior reproduction (2026-08-07) occurred in a stale-mtime working-tree context and is NOT
  reproduced on this baseline. Working tree carries the proven mtime-invalidation fix (uncommitted); align HEAD with it.
- **P1.4 Receiver verification gap**: 14 critical-behavior tests never run — pairing, session lifecycle, codec/sample-rate
  enforcement, registry state (`crates/michi-receivers/tests/receiver_simulator_integration.rs`); E2E runner broken
  (`scripts/test_receiver_e2e.sh` runs `cargo test --test ...` from the virtual-workspace root — needs `-p michi-receivers`);
  sim paths hardcoded (`scripts/run_receiver_sim_standard.sh:6` → `/home/cristian/...`); not in CI. Receiver reconnect/
  handoff/session correctness is **unverified** (P1 per model: "receiver reconnect failure, incorrect handoff"); no
  failure demonstrated → verification gap, not P0.

### P2 — non-critical reliability / drift / hygiene (no demonstrated failure)
- **P2.1 CasaOS metadata drift**: `casaos/data.yml:2` `version: 0.1.0` vs workspace 0.2.0; `icon: ""`
  (`docker-compose.casaos.yml`), no screenshots; `docs/CASAOS_ZIMAOS.md` pre-submission TODO list (icon/screenshots/
  publish/multi-arch CI/device test). Metadata drift only — no demonstrated deployment failure on HEAD.
- **P2.2 Toolchain pin drift**: `Dockerfile:1` `RUST_VERSION=1.88` vs local + CI `stable` (1.96.0 today); no
  `rust-toolchain.toml` → non-reproducible builds. No build failure demonstrated on HEAD (Docker build passed).
- **P2.3 Dead auth helpers**: `#[allow(dead_code)]` on `extract_bearer_token` / `resolve_device_id`
  (`crates/michi-api/src/auth.rs:26,33`) — unused; live path uses `extract_token` + middleware (`auth.rs:107-120`, `:159-196`).
  Dead security code should be removed or wired.
- **P2.4 Dead config defaults**: `default_port/music_paths/lang/backup_keep/max_jobs/reconnect`
  (`crates/michi-config/src/lib.rs:57-77`).
- **P2.5 Docs drift**: CHANGELOG "35 migraciones" vs 37 (`crates/michi-db/src/lib.rs`); ROADMAP multi-arch/publish claims
  (substance tracked under P1.2, claim text itself is docs drift); CASAOS doc TODOs.
- **P2.6 No dependency vulnerability scanning / coverage**: no `cargo audit`/`cargo deny` in CI (tools not installed
  locally); no coverage enforcement (tarpaulin absent; Makefile `coverage` target would fail). Admin/CI hardening.
- **P2.7 Prod-path `unwrap`s (uncommon failure paths)**: `michi-api/src/lib.rs:76,82,131` (std Mutex on `task_handles`
  — panic-poisoning risk); `michi-opensubsonic/src/routes.rs:294,330` (Response builder unwrap, effectively infallible);
  `michi-api/src/library.rs:598,937,942` (static mime parse). Low likelihood; recommend `expect("static")`/lock handling.
- **P2.8 Migration code organization**: definitions out of order (`migration_034-037` at `michi-db/src/lib.rs:1115-1164`,
  `migration_001-015` at `:1216+`); no down-migrations; runner is hand-rolled (works — 37 applied in container). No
  migration failure demonstrated.

### P3 — Hygiene (safe, cheap)
- **P3.1** `crates/michi-tui/src/app.rs:69` `#[allow(deprecated)]` uses deprecated `music_path()` (`crates/michi-config/src/lib.rs:317`) → migrate to `primary_music_path()`.
- **P3.2** `apps/michi-server/src/main.rs:182,184` signal-handler `expect`s (acceptable, process-level).
- **P3.3** Dual API surfaces (`/api` legacy + `/api/v1`) both live; `docs/API.md` is a manual table (drift risk) — could be generated from utoipa.
- **P3.4** `tests/e2e/test_receiver_simulator_integration.rs` + `tests/receiver_simulator_integration.rs` duplicate files (delete once deduped in P1.4).
- **P3.5** `apps/michi-server/src/main.rs:109` `unwrap_or_default()` on `module_tokens.try_read()` (silent fallback) — acceptable, worth a comment.

### Process note (not a HEAD defect, not severity-classified)
- **Pending uncommitted WebUI/PWA design work** in the main tree (contamination context, §2) must land as its own
  change before/alongside v1-stabilization; do not inherit HEAD's older WebUI as "final".

### POST-V1 (out of scope for stabilization)
- HLS/DASH adaptive streaming (ROADMAP unchecked), mobile client (Michi Music Player), AI recommendations, lyrics; coverage gate (tarpaulin) and `nextest` adoption; containerized receiver simulator in CI; GHCR provenance/SBOM; release automation for Debian `.deb`.

## 10. First Minimal Phase 1 Fix Set (ordered by dependency)

1. **Contract chain repair** (P1.1): fix the `global BASE_URL` SyntaxError; add `michi_link_version: "1.0.0-alpha"` to
   `V1ServerInfo` (single source of truth in `michi-config`) or align the assert; run the E2E test manually against a
   local instance and record PASS. *(Smallest slice, unblocks all contract work.)*
2. **CI Python job** (P1.1, CI wiring leg): add a `ci-python-contract` job (needs `ci-rust`) that builds the binary,
   boots it on a scratch port, and runs `test_player_micro_contract_compatibility.py`. (Do not touch the WebUI static files.)
3. **ARM64 release** (P1.2): add `linux/arm64` to `release-ghcr` platforms; validate locally with
   `docker buildx build --platform linux/arm64` (QEMU available on this host). Verify the image boots arm64
   (or note CI-only verification).
4. **Dockerfile hardening** (P1.3 + P2.2): adopt the proven placeholder-cleanup/mtime-invalidation from the working
   tree, and align `RUST_VERSION` with CI stable (or add `rust-toolchain.toml`).
5. **Receiver pipeline** (P1.4 + P3.4): delete the two dead duplicates; fix `test_receiver_e2e.sh`
   (`-p michi-receivers`, both sims, `-- --ignored`); make sim paths configurable (env-based) and document simulator
   acquisition; optionally vendor a minimal fixture server.
6. **CasaOS/version alignment** (P2.1): bump `data.yml` to 0.2.x, wire the pending icon asset (from the uncommitted
   WebUI work), align docs.
7. **Hygiene sweep** (P2.3–P2.8, P3.1): dead-code removal, docs drift fixes, `primary_music_path()` migration — sized
   to fit the 500-line review budget as a final slice.

Each item is a reversible, independently landable work unit (work-unit-commits); total Phase 1 exceeds 400 changed
lines → **chained PRs required** (force-chained delivery strategy; chain strategy pending user selection).

## 11. Recommended Next Planning Phase

**`sdd-propose`** for `v1-stabilization`, proposing 3–4 slices (contract-repair+CI, docker/arm64, receiver pipeline,
hygiene) with explicit rollback, feature-freeze impact (audio-only), and a `size:exception`-free chain plan; then
`sdd-spec`/`sdd-design` per slice, `sdd-tasks` with Review Workload Forecast, `sdd-verify` per slice. Deliverable
`docs/V1_STABILIZATION_AUDIT.md` is derived after specs/tasks (implementation phase), NOT now.

## Appendix A — Baseline log locations

- `/tmp/opencode/michi-v1-audit-baseline.log` (full command output, timings, per-target test results)
- Worktree `/tmp/opencode/michi-v1-audit-head` (removed after audit) · Target `/home/cristian/.cache/michi-v1-audit-target` (removed after audit)

## Appendix B — Tags / version strategy

Lightweight tags only: `v0.1.1-alpha` (06-26), `v0.2.0-beta` (06-29), `v0.2.0` (07-18). HEAD 28 commits past `v0.2.0`.
Recommend deciding the v1 pre-release scheme (`v1.0.0-alpha.1` vs `v0.3.0`) in proposal; `latest` tag auto-enabled
only for non-prerelease tags.
