# V1 Stabilization — Phase 0 Baseline Audit

**Audited object**: exact `HEAD` `e6f6dd6f1b043ba9483614027bee2284586cf4ef` (branch `main`, merge of PR #4)
**Date**: 2026-08-14
**Mode**: characterization/regression audit (strict_tdd: false)
**Isolation**: disposable detached worktree; main working tree untouched

## 1. Executive summary

Michi Micro Server v0.2.0 at exact `HEAD e6f6dd6` is a **green baseline**: all four Rust
gates pass with exit 0 (`fmt`, `check`, `test`, `clippy` with `RUSTFLAGS=-D warnings`),
**217 tests pass / 0 fail / 14 ignored**, and a Docker build + smoke test on HEAD serves a
real, healthy server (`/api/status` 200, `version 0.2.0`, **37 migrations applied**).

**Severity verdict: P0 = 0.** No user-defined P0 condition is demonstrated on HEAD. However,
the v1 stabilization targets are NOT met: three P1 release-gate/deployment blockers plus one
P1 verification gap remain, and several P2 drift items are present (see §5).

## 2. Severity model (user-defined, authoritative)

- **P0 CRITICAL** — demonstrated data corruption/loss, auth bypass, arbitrary path/RCE,
  boot failure, destructive migration, unrecoverable queue/DB failure.
- **P1 HIGH** — receiver reconnect failure, incorrect handoff, inconsistent
  scanner/sync/token/Snapcast/HA recovery, significant API contract break.
- **P2** — non-critical reliability, uncommon failure, performance regression, admin/UX.
- **P3** — hygiene (safe, cheap).

Release-gate blockers may be red without being P0 runtime incidents.

## 3. Baseline commands and results (exact HEAD)

Run in a detached worktree at `e6f6dd6` with `RUSTFLAGS="-D warnings"` and an isolated
`CARGO_TARGET_DIR`.

| Command | Exit | Result |
|---|---|---|
| `cargo fmt --check` | 0 | PASS |
| `cargo check --workspace` | 0 | PASS, zero warnings |
| `cargo test --workspace` | 0 | PASS — **217 passed, 0 failed, 14 ignored** |
| `cargo clippy --workspace --all-targets -- -D warnings` | 0 | PASS, zero warnings |
| Docker build (`michi-audit-head:test`) | 0 | image built from HEAD `Dockerfile` |
| Docker smoke (port 9092): `GET /api/status`, `GET /` | 0 | **200 / 200**; real server `{"status":"ok","version":"0.2.0","database":"ok"}`; container `healthy`; 37 migrations applied on boot |

Test breakdown: `crates/michi-api/tests/api.rs` = 102 tests (all pass); the 14 `#[ignore]`
tests belong to the receiver-simulator integration target
(`crates/michi-receivers/tests/receiver_simulator_integration.rs`, §4); 21 crate doc-tests
(0 tests each); remaining unit tests across 21 crates.

## 4. Ignored-test inventory (14 `#[ignore]` — unverified, NOT passing)

Source: `crates/michi-receivers/tests/receiver_simulator_integration.rs` (only `#[ignore]`
tests in the repo). These tests are **ignored and unverified** — they are never run in CI and
depend on an external simulator. They are reported here as **unverified, not passing**.

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

Dependency: external simulator `receiver_sim.py` from `pitydah/michi-music-stream`;
env `MICHI_RECEIVER_SIM_URL` (default :8080) / `MICHI_RECEIVER_SIM_HIFI_URL` (:8081).

## 5. Risk inventory (file:line evidence)

### P0 CRITICAL — count: 0

No finding qualifies. The strongest candidates are release-gate/verification/deployment
blockers or latent risks, none of which is a demonstrated runtime incident.

### P1 HIGH — release-gate, deployment, and verification blockers

- **P1.1 Player↔Micro API contract chain broken** (significant API contract break; release-gate).
  - `tests/e2e/test_player_micro_contract_compatibility.py:69` (`default=BASE_URL`) vs `:71`
    (`global BASE_URL`) → `SyntaxError`; the test cannot parse.
  - `:84` asserts `info.get("michi_link_version") == "1.0.0-alpha"` — a field the server never
    emits (`crates/michi-link/src/version.rs:3-4`: "There is no `michi_link_version` — the API
    contract version is solely `api_version`"; `crates/michi-api/src/routes/v1/server.rs:7-17`
    `V1ServerInfo` has `api_version: "v1"` at `:51` and no `michi_link_version`).
  - The real client checks only `api_version == "v1"`
    (`clients/python-michi-client/michi_client.py:106-110`).
  - No Python/E2E test runs in CI (`.github/workflows/ci.yml`).
  - Three faults in one chain: syntax + contract field + CI wiring.
- **P1.2 GHCR ARM64 release/deployment blocker**: `.github/workflows/ci.yml:95`
  `platforms: linux/amd64` only, while CasaOS/ZimaOS declares arm64 and v1 requires
  amd64+arm64.
- **P1.3 Dockerfile dummy-source cache bug (latent)**: `Dockerfile:41` writes placeholder
  `crates/*/src/lib.rs`, `Dockerfile:43` writes placeholder `apps/michi-server/src/main.rs`,
  with no mtime invalidation before the final `cargo build`; some cache states can ship a no-op
  placeholder binary. The exact clean-HEAD baseline built and smoked correctly (200 OK); the
  prior reproduction occurred in a stale-mtime working-tree context (2026-08-07) and is NOT
  reproduced on this baseline.
- **P1.4 Receiver verification gap**: the 14 `#[ignore]` tests (§4) never run;
  `scripts/test_receiver_e2e.sh:43` runs `cargo test --test receiver_simulator_integration`
  from the virtual-workspace root (needs `-p michi-receivers`); sim paths hardcode a
  machine-specific default (`scripts/run_receiver_sim_standard.sh:5`). Receiver
  reconnect/handoff/session correctness is unverified.

### P2 — non-critical drift (no demonstrated failure)

- **P2.1** CasaOS metadata drift: `casaos/data.yml:2` `version: 0.1.0` vs workspace 0.2.0.
- **P2.2** Toolchain pin drift: `Dockerfile:1` `RUST_VERSION=1.88` vs local/CI 1.96; no
  `rust-toolchain.toml`.
- **P2.3** Dead auth helpers: `#[allow(dead_code)]` on `extract_bearer_token` /
  `resolve_device_id` (`crates/michi-api/src/auth.rs:26,33`).
- **P2.4** Dead config defaults: `default_port/music_paths/lang/backup_keep/max_jobs/reconnect`
  (`crates/michi-config/src/lib.rs:57-77`).
- **P2.5** Docs drift: `CHANGELOG.md:37` "35 migraciones" vs 37
  (`crates/michi-db/src/lib.rs` has 37 `fn migration_0*`).
- **P2.6** No dependency vulnerability scanning / coverage in CI.
- **P2.7** Prod-path `unwrap`s: `michi-api/src/lib.rs:76,82,131`; `michi-opensubsonic/src/routes.rs:294,330`;
  `michi-api/src/library.rs:598,937,942`.
- **P2.8** Migration code organization: definitions out of order (`migration_034-037` at
  `michi-db/src/lib.rs:1115-1164` before `001-015` at `:1216+`).

### P3 — hygiene

- **P3.1** `crates/michi-tui/src/app.rs:69` uses deprecated `music_path()`.
- **P3.2** `apps/michi-server/src/main.rs:182,184` signal-handler `expect`s (acceptable).
- **P3.3** Dual API surfaces (`/api` legacy + `/api/v1`) both live; `docs/API.md` is manual.
- **P3.4** Dead duplicate test files `tests/receiver_simulator_integration.rs` and
  `tests/e2e/test_receiver_simulator_integration.rs` (never compiled).
- **P3.5** `apps/michi-server/src/main.rs:109` `unwrap_or_default()` on `module_tokens`.

## 6. Contamination context (NOT audited — working-tree only)

The main working tree carries **uncommitted/contaminated** WebUI/PWA/design work that is NOT
part of this audit and NOT part of the v1-stabilization change: `.gitignore`, `Dockerfile`,
`crates/michi-api/src/{lib,pwa,static_files}.rs`, WebUI statics (`app.js`, `hero-cat.css`,
`i18n/en.json`, `index.html`, `styles.css`), `tests/api.rs`, and untracked `.impeccable/`,
`DESIGN.md`, `PRODUCT.md`, `static/assets/*`, `openspec/`. These files were NOT modified,
absorbed, or depended on by this audit.

## 7. Conclusion

HEAD is a green baseline (P0 = 0) with four green Rust gates, a passing Docker smoke, and a
complete, honest ignored-test inventory. The v1 release gate is blocked by P1.1 (contract
chain), P1.2 (arm64 release), P1.3 (latent Docker cache bug), and P1.4 (receiver verification
gap) — all release-gate/verification/deployment blockers, none a demonstrated runtime
incident. P2/P3 items are drift and hygiene, deferred to later stabilization slices.
