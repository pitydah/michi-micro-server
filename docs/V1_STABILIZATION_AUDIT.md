# V1 Stabilization — Phase 0 Baseline Audit

**Audited object**: exact `HEAD e6f6dd6f1b043ba9483614027bee2284586cf4ef` (branch `main`, merge of PR #4)
**Date**: 2026-08-14 · **Mode**: characterization/regression (strict_tdd: false) · **Isolation**: disposable detached worktree; main tree untouched

## 1. Executive summary

Michi Micro Server v0.2.0 at exact `HEAD e6f6dd6` is a **green baseline**: all four Rust gates pass (`fmt`, `check`, `test`, `clippy` with `RUSTFLAGS=-D warnings`), **217 tests pass / 0 fail / 14 ignored**, and a Docker build + smoke on HEAD serves a real healthy server (`/api/status` 200, `version 0.2.0`, **37 migrations**).

**Verdict: P0 = 0** (no user-defined P0 demonstrated on HEAD). v1 targets NOT met: three P1 release-gate/deployment blockers + one P1 verification gap (§5); P2/P3 drift listed.

## 2. Severity model (user-defined, authoritative)

- **P0 CRITICAL** — demonstrated data corruption/loss, auth bypass, arbitrary path/RCE, boot failure, destructive migration, unrecoverable queue/DB failure.
- **P1 HIGH** — receiver reconnect failure, incorrect handoff, inconsistent scanner/sync/token/Snapcast/HA recovery, significant API contract break.
- **P2** — non-critical reliability, uncommon failure, perf regression, admin/UX. **P3** — hygiene.
- Release-gate blockers may be red without being P0 runtime incidents.

## 3. Baseline commands and results (exact HEAD)

| Command | Exit | Result |
|---|---|---|
| `cargo fmt --check` | 0 | PASS |
| `cargo check --workspace` | 0 | PASS, zero warnings |
| `cargo test --workspace` | 0 | **217 passed, 0 failed, 14 ignored** |
| `cargo clippy --workspace --all-targets -- -D warnings` | 0 | PASS, zero warnings |
| Docker build (`michi-audit-head:test`) | 0 | image built from HEAD `Dockerfile` |
| Docker smoke (port 9092): `GET /api/status`, `GET /` | 0 | 200/200; `{"status":"ok","version":"0.2.0","database":"ok"}`; container `healthy`; 37 migrations |

Breakdown: `crates/michi-api/tests/api.rs` = 102 tests (all pass); the 14 `#[ignore]` tests belong to the receiver-simulator target (`crates/michi-receivers/tests/receiver_simulator_integration.rs`, §4); 21 doc-tests; remaining unit tests across 21 crates.

## 4. Ignored-test inventory (14 `#[ignore]` — unverified, NOT passing)

Source: `crates/michi-receivers/tests/receiver_simulator_integration.rs` (only `#[ignore]` tests in repo). Never run in CI; depend on external simulator. Reported **unverified, not passing**.

| # | Test | Behavior verified |
|---|---|---|
| 1 | `test_receiver_info_standard` (:34) | GET /api/v1/receiver/info standard identity |
| 2 | `test_receiver_info_hifi` (:44) | Hi-Fi receiver identity |
| 3 | `test_receiver_info_standard_output` (:53) | jack_3_5, 48 kHz, 16-bit, pcm_s16le output |
| 4 | `test_receiver_info_hifi_output` (:78) | rca_stereo, 96 kHz, 24-bit, pcm_s24le output |
| 5 | `test_receiver_pairing_flow` (:103) | pairing start → confirm roundtrip |
| 6 | `test_receiver_pairing_window_closed_rejected` (:127) | re-pair after window close rejected |
| 7 | `test_receiver_standard_full_lifecycle` (:158) | pair → session → heartbeat → volume → stop |
| 8 | `test_receiver_hifi_full_lifecycle` (:206) | same full lifecycle for Hi-Fi |
| 9 | `test_receiver_errors_unsupported_codec` (:249) | aac rejected on Standard |
| 10 | `test_receiver_errors_sample_rate_exceeds` (:265) | 96 kHz rejected on Standard |
| 11 | `test_receiver_errors_duplicate_session` (:291) | 409 on second session |
| 12 | `test_receiver_errors_volume_out_of_range` (:332) | volume 101 clamps to 100 |
| 13 | `test_receiver_errors_unauthenticated` (:357) | heartbeat without token fails |
| 14 | `test_receiver_registry_tracks_state` (:372) | ReceiverRegistry stores paired state |

Dependency: external simulator `receiver_sim.py` (`pitydah/michi-music-stream`); env `MICHI_RECEIVER_SIM_URL` (:8080) / `MICHI_RECEIVER_SIM_HIFI_URL` (:8081).

## 5. Risk inventory (file:line evidence)

### P0 CRITICAL — count: 0
No finding qualifies; strongest candidates are release-gate/verification/deployment blockers or latent risks, none a demonstrated runtime incident.

### P1 HIGH — release-gate / deployment / verification blockers
- **P1.1 Player↔Micro API contract chain broken** (significant API contract break; release-gate).
  - `tests/e2e/test_player_micro_contract_compatibility.py:69` (`default=BASE_URL`) vs `:71` (`global BASE_URL`) → `SyntaxError`; test cannot parse.
  - `:84` asserts `michi_link_version == "1.0.0-alpha"` — never emitted (`crates/michi-link/src/version.rs:3-4`: contract version is solely `api_version`; `crates/michi-api/src/routes/v1/server.rs:7-17` `V1ServerInfo` has `api_version: "v1"` at `:51`, no `michi_link_version`).
  - Real client checks only `api_version == "v1"` (`clients/python-michi-client/michi_client.py:106-110`).
  - No Python/E2E test in CI (`.github/workflows/ci.yml`). Three faults in one chain: syntax + contract field + CI wiring.
- **P1.2 GHCR ARM64 release/deployment blocker**: `.github/workflows/ci.yml:95` `platforms: linux/amd64` only; CasaOS/ZimaOS declares arm64 and v1 requires amd64+arm64.
- **P1.3 Dockerfile dummy-source cache bug (latent)**: `Dockerfile:41` placeholder `crates/*/src/lib.rs`, `Dockerfile:43` placeholder `apps/michi-server/src/main.rs`, no mtime invalidation before final `cargo build`; some cache states ship a no-op placeholder binary. Clean-HEAD baseline built+smoked 200 OK; prior repro (2026-08-07) was stale-mtime working-tree context, NOT reproduced on this baseline.
- **P1.4 Receiver verification gap**: the 14 `#[ignore]` tests (§4) never run; `scripts/test_receiver_e2e.sh:43` runs `cargo test --test receiver_simulator_integration` from the virtual-workspace root (needs `-p michi-receivers`); sim paths hardcode a machine default (`scripts/run_receiver_sim_standard.sh:5`). Receiver reconnect/handoff/session correctness unverified.

### P2 — non-critical drift (no demonstrated failure)
- **P2.1** CasaOS metadata: `casaos/data.yml:2` `0.1.0` vs workspace 0.2.0.
- **P2.2** Toolchain pin: `Dockerfile:1` `RUST_VERSION=1.88` vs local/CI 1.96; no `rust-toolchain.toml`.
- **P2.3** Dead auth helpers: `#[allow(dead_code)]` `extract_bearer_token`/`resolve_device_id` (`crates/michi-api/src/auth.rs:26,33`).
- **P2.4** Dead config defaults: `default_port/music_paths/lang/backup_keep/max_jobs/reconnect` (`crates/michi-config/src/lib.rs:57-77`).
- **P2.5** Docs drift: `CHANGELOG.md:37` "35 migraciones" vs 37 (`crates/michi-db/src/lib.rs` 37 `fn migration_0*`).
- **P2.6** No dependency vulnerability scanning / coverage in CI.
- **P2.7** Prod-path `unwrap`s: `michi-api/src/lib.rs:76,82,131`; `michi-opensubsonic/src/routes.rs:294,330`; `michi-api/src/library.rs:598,937,942`.
- **P2.8** Migration defs out of order (`migration_034-037` at `michi-db/src/lib.rs:1115-1164` before `001-015` at `:1216+`).

### P3 — hygiene
- **P3.1** `crates/michi-tui/src/app.rs:69` deprecated `music_path()`.
- **P3.2** `apps/michi-server/src/main.rs:182,184` signal-handler `expect`s (acceptable).
- **P3.3** Dual API surfaces (`/api` legacy + `/api/v1`); `docs/API.md` manual.
- **P3.4** Dead duplicate tests `tests/receiver_simulator_integration.rs` + `tests/e2e/test_receiver_simulator_integration.rs` (never compiled).
- **P3.5** `apps/michi-server/src/main.rs:109` `unwrap_or_default()` on `module_tokens`.

## 6. Contamination context (NOT audited — working-tree only)

Main working tree carries uncommitted/contaminated WebUI/PWA/design work NOT part of this audit or the v1 change: `.gitignore`, `Dockerfile`, `crates/michi-api/src/{lib,pwa,static_files}.rs`, WebUI statics (`app.js`, `hero-cat.css`, `i18n/en.json`, `index.html`, `styles.css`), `tests/api.rs`, untracked `.impeccable/`, `DESIGN.md`, `PRODUCT.md`, `static/assets/*`, `openspec/`. Not modified/absorbed/depended on by this audit.

## 7. Conclusion

HEAD is a green baseline (P0 = 0) with four green Rust gates, a passing Docker smoke, and a complete honest ignored-test inventory. The v1 release gate is blocked by P1.1 (contract chain), P1.2 (arm64 release), P1.3 (latent Docker cache), and P1.4 (receiver verification gap) — all release-gate/verification/deployment blockers, none a demonstrated runtime incident. P2/P3 items are drift and hygiene, deferred to later slices.
