# Apply Progress: v1-stabilization — Work Unit 0 + 1 + 2 + 3 + 4a

**Change**: v1-stabilization
**Mode**: Standard (strict_tdd: false — characterization/regression program)
**Date**: 2026-08-14
**Baseline**: e6f6dd6f1b043ba9483614027bee2284586cf4ef
**Delivery**: force-chained / feature-branch-chain

## Completed Tasks

### Work Unit 0 — Tracker + planning
- [x] 0.1 Create detached tracker worktree from exact HEAD
- [x] 0.2 Bring ONLY planning paths into tracker

### Work Unit 1 — Slice 01: Audit + authenticated Player contract
- [x] 1.1 Author `docs/V1_STABILIZATION_AUDIT.md`
- [x] 1.2 Fix Python parse fault without global BASE_URL mutation
- [x] 1.3 Swap phantom field assert (Option A)
- [x] 1.4 Implement ordered admin auth flow + loud failures
- [x] 1.5 Correct stale docs
- [x] 1.6 Local real-server harness proof
- [x] 1.7 Regression guard

### Work Unit 2 — Slice 02: CI contract gate
- [x] 2.0 WU1 native 4R review follow-ups on the contract test
- [x] 2.1 Create `scripts/ci_contract_gate.sh`
- [x] 2.2 Wire `ci-python-contract` job gated on `ci-rust`
- [x] 2.3 Local gate proof

### Work Unit 3 — Slice 03: Docker/toolchain/ARM64
- [x] 3.0 WU2 native 4R review follow-ups on `scripts/ci_contract_gate.sh`
- [x] 3.1 Re-derive Docker cache fix from HEAD
- [x] 3.2 Pin toolchain 1.96.0 consistently
- [x] 3.3 Multiarch release platforms + prerelease latest prohibition
- [x] 3.4 Docker smoke regression (local evidence)
- [ ] 3.5 buildx arm64 + emulated boot — NOT EXECUTED (environment limitation)
- [x] 3.6 Healthcheck probes public liveness endpoint (P1 corrective — auth-enabled smoke)

### Work Unit 4a — Slice 04a: Receiver dedup (pure deletion exception)
- [x] 4.1 Delete dead duplicate `tests/receiver_simulator_integration.rs` (233 lines)
- [x] 4.2 Delete dead duplicate `tests/e2e/test_receiver_simulator_integration.rs` (233 lines)
- [x] 4.3 Regression: no behavior change (cargo test --workspace 217/0/14)

## Work Unit Evidence

### Work Unit 0
| Evidence | Actual |
|---|---|
| Focused test | `git -C wt-tracker rev-parse HEAD` → `e6f6dd6f1b043ba9483614027bee2284586cf4ef`; `status --porcelain` empty before/after commit |
| Runtime harness | N/A — worktree + planning-artifact setup only |
| Rollback boundary | `git worktree remove wt-tracker --force`; nothing pushed; only `openspec/` paths staged |

### Work Unit 1
| Evidence | Actual |
|---|---|
| Focused test | `python3 -m py_compile tests/e2e/test_player_micro_contract_compatibility.py` → exit 0; `grep -n "global BASE_URL"` empty; `grep -n "michi_link_version"` empty |
| Runtime harness | Boot release binary (built 2026-08-14 20:49, `cargo build --release -p michi-server` exit 0) on port 18123, temp root `/tmp/opencode/michi-v1s/wt01-corrective-harness` (config/cache/music + sqlite), throwaway admin via `MICHI_AUTH_USERNAME/PASSWORD` (password redacted); contract `python3 … --url http://127.0.0.1:18123` → exit 0, `CONTRACT: OK`, 31 passed/0 failed — log `/tmp/opencode/michi-v1s/wt01-contract-happy.log`; server log `/tmp/opencode/michi-v1s/wt01-server.log`; SIGTERM → WAL checkpoint + shutdown complete, port freed |
| Rollback boundary | Revert `tests/e2e/test_player_micro_contract_compatibility.py` + `docs/MICHI_LINK_MICRO_E2E.md`; delete `docs/V1_STABILIZATION_AUDIT.md`; no server/schema/state change |

### Work Unit 2
| Evidence | Actual |
|---|---|
| Focused test | `python3 -m py_compile tests/e2e/test_player_micro_contract_compatibility.py` → exit 0; `bash -n scripts/ci_contract_gate.sh` → exit 0; `python3 -c "import yaml; yaml.safe_load(open('.github/workflows/ci.yml'))"` → `YAML OK` |
| Runtime harness | `CARGO_TARGET_DIR=/home/cristian/.cache/michi-v1s/target bash scripts/ci_contract_gate.sh` → exit 0, `CONTRACT: OK`, 33 passed/0 failed (31 WU1 + 2 new negative-auth checks); script booted real release binary on PID-derived port 18528, isolated temp paths, throwaway admin; trap cleanup stopped server, freed port, removed temp dir — no orphan/leak. Failure paths: dead server → `server did not become healthy on /health/live within 60s` exit 1; stub contract-FAIL server → `CONTRACT: FAILED` exit 1 (propagated). Persistent failure log verified via `MICHI_FAILURE_LOG` override; no credential leak (grep-verified) |
| Rollback boundary | Revert `tests/e2e/test_player_micro_contract_compatibility.py`; delete `scripts/ci_contract_gate.sh`; remove `ci-python-contract` job block from `.github/workflows/ci.yml`; revert task/progress marks |

### Work Unit 3
| Evidence | Actual |
|---|---|
| Focused test | `bash -n scripts/ci_contract_gate.sh` → exit 0; `python3 -c "import yaml; yaml.safe_load(open('.github/workflows/ci.yml'))"` → `YAML OK`; `rustc --version` → 1.96.0 (rust-toolchain.toml respected); `cargo test --workspace` → exit 0, 217 passed / 0 failed / 14 ignored (no regression) |
| Runtime harness | `CARGO_TARGET_DIR=/home/cristian/.cache/michi-v1s/target bash scripts/ci_contract_gate.sh` → exit 0, `CONTRACT: OK`, 33/0, stale-binary rebuild triggered then PASS; forced failure (occupied port) → exit 1 with no orphan + temp dir removed; TERM mid-run → exit 130, server killed, temp dir removed. Docker smoke: `docker build` OK → container `healthy` → `/api/status` 200 `version 0.2.0`, `/health/live` 200, root 200, 37 migrations → `docker stop -t 25` → `received SIGTERM` → `WAL checkpoint complete` → `shutdown complete` (exit 0) → container + image removed. Persisted runtime evidence: `/tmp/opencode/michi-v1s/wu3-docker-run.log` (full boot log, 37 migrations, `healthy`, graceful-stop tail, ExitCode 0) sha256 `6776c1518c2fdaaf44e3e7623d3e0a02d4f89fafdb7d8b81a5c7e8af78f46e6e`; `/tmp/opencode/michi-v1s/wu3-docker-health.json` (`Status: healthy`, FailingStreak 0) sha256 `4cd83277549f622348e35e1772eb397c4437dabbbecbc66ee744bf563d003ca1`. **3.6 corrective (auth ENABLED)**: `docker build -t michi-v1s-smoke:wu3-hc .` OK; boot with `MICHI_AUTH_USERNAME`/`MICHI_AUTH_PASSWORD` (throwaway admin) on isolated port 18097 → `docker inspect … .State.Health.Status` = `healthy`; `/health/live` 200, `/api/status` 401 without token (auth active AND healthcheck public), root 200; `docker stop -t 25` → SIGTERM → WAL checkpoint → shutdown complete (ExitCode 0); container + image removed, port freed. Persisted: `/tmp/opencode/michi-v1s/wu3-docker-hc-run.log` sha256 `9c5ffae9d75dd16803d925d79511f69e659a9e004418062545a2002dec8a3892`; `/tmp/opencode/michi-v1s/wu3-docker-hc-health.json` sha256 `e2607102813ddbc88a0c0e3c60798f1c2ffb0b443fc2c1d257256fe103b1fb88` (password redacted — no secrets in logs) |
| Rollback boundary | Revert `scripts/ci_contract_gate.sh`; revert `Dockerfile`; delete `rust-toolchain.toml`; revert `ci.yml` platform/toolchain lines; revert task/progress marks |

### Work Unit 4a
| Evidence | Actual |
|---|---|
| Focused test | `CARGO_TARGET_DIR=/home/cristian/.cache/michi-v1s/target cargo test --workspace` → exit 0, 217 passed / 0 failed / 14 ignored (full log `/tmp/opencode/michi-v1s/wu4a-cargo-test.log`). The `14 ignored` remain the authoritative crate target `crates/michi-receivers/tests/receiver_simulator_integration.rs` — proof the live copy still compiles and registers its ignored tests |
| Runtime harness | N/A — deletion-only slice: the two deleted files were never compiled (no `Cargo.toml` target or workspace member references them; `tests/` has no crate manifest; root workspace has 22 members, none is `tests/`). No runtime boundary exists for dead files |
| Rollback boundary | Restore `tests/receiver_simulator_integration.rs` and `tests/e2e/test_receiver_simulator_integration.rs` (exact bytes recoverable from parent `355cf0c`); revert the two commits; revert tasks.md/apply-progress.md marks |

## Evidence Log

### Work Unit 0 (tracker + planning)
- Main worktree manifests (byte/path/mode identity, all identical sha256 `8a8d53f0bf2dd5d4f778476c19e97482912ca8609df084492584d367447f9143`, 225 entries): `/tmp/opencode/michi-v1s/main-baseline-wu1.manifest`, `main-baseline-wu1-after.manifest`, `main-final.manifest`. Main dirty-state hashes unchanged (status_porcelain_z `d44a16570145160f64a9c99a05fe757543a259fa8457950729006e999f5e31b0`).
- `git worktree add -b feat/v1-stabilization /home/cristian/.cache/michi-v1s/wt-tracker e6f6dd6f1b043ba9483614027bee2284586cf4ef` → HEAD `e6f6dd6`; staged exact paths (15 under `openspec/`); commit `docs(openspec): track v1-stabilization planning artifacts` → `d00438b8659736df42630738ae076e8a24e51e59`.

### Work Unit 1 (Slice 01)
- Child worktree `git worktree add -b 01-audit-contract /home/cristian/.cache/michi-v1s/wt-01 d00438b8659736df42630738ae076e8a24e51e59` → HEAD `d00438b`, clean.
- Pre-state: py_compile → `SyntaxError: name 'BASE_URL' is used prior to global declaration` (exit 1). Post-fix: exit 0; `global BASE_URL` and `michi_link_version` gone; `api_version == "v1"` present; `git diff -- crates/michi-api crates/michi-link` empty (no server change).
- Discovery (latent 4th fault): `supports_*` are runtime-state booleans (`active_import_sessions > 0`, `total_queues > 0` — `diagnostics.rs:451-456`), legitimately false on fresh server → assert block shape + `contract_status`, never fabricate `True`.
- Happy path (persisted): command `MICHI_PORT=18123 MICHI_CONFIG_PATH/CACHE_PATH/MUSIC_PATH=$TMP MICHI_DATABASE=sqlite://$TMP/config/michi.db MICHI_AUTH_USERNAME=… MICHI_AUTH_PASSWORD=<redacted> …/michi-server` (temp root `/tmp/opencode/michi-v1s/wt01-corrective-harness`, removed after run); contract log `/tmp/opencode/michi-v1s/wt01-contract-happy.log` sha256 `ba707511f653072eb837898cac784f6ce4efbb1bc4e038f022423ac1092f7329`; server log `/tmp/opencode/michi-v1s/wt01-server.log` sha256 `dc8cb2d068e20747ac3ac44f8fd4d4b9011bbc07bb306f6f838b29fe0d6ffc5e` — 37 migrations, SIGTERM → "WAL checkpoint complete" + "shutdown complete", port 18123 freed. No secret/token in either log (grep-verified).
- Failure paths: missing creds → exit 1 (`0 passed/1 failed`); wrong password → login 401; protected no-token → 401. Never skips.
- Regression: `cargo fmt --check`, `cargo check --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` all exit 0 — 217 passed/0 failed/14 ignored (two runs). Flake: 2 order-dependent tests race on shared `/tmp/michi-test/music` (`crates/michi-api/tests/api.rs:31`); pass in isolation; pre-existing, unrelated.

### Work Unit 2 (Slice 02)
- Child worktree `git worktree add -b 02-ci-contract /home/cristian/.cache/michi-v1s/wt-02 52eaf0e73b574b0b45c997d5f04263fbf8524d50` → HEAD `52eaf0e`, clean.
- Main worktree before/after manifests (byte/path/mode identity, identical sha256 `8f5cd8665494d63e3c12622571c91bcfc235c43f5ccd8c9c73f1e9c7a382a703`, 228 entries, excludes `.git`/`target`/`node_modules`): `/tmp/opencode/michi-v1s/main-before-wu2.manifest`, `/tmp/opencode/michi-v1s/main-after-wu2.manifest`. Main dirty-state `status_porcelain_z` hash unchanged `d44a16570145160f64a9c99a05fe757543a259fa8457950729006e999f5e31b0`.
- Task 2.0: negative auth guard added — no-token and invalid-token requests to protected `/api/v1/diagnostics` asserted 401 (fail-open records FAIL + exits non-zero); `_safe_json_or_raw` helper guards HTTPError body decode (non-JSON → raw text); queue-transfer label/comment corrected to "empty track_ids → 400" (matches `queue.rs:117-123`); docstring lists only exercised failure modes. `py_compile` exit 0.
- Task 2.1/2.3: gate script `scripts/ci_contract_gate.sh` (127 lines, mode 755) — `set -euo pipefail`, prerequisite checks (python3/curl/cargo), build-if-absent, isolated temp paths + PID-derived port (band 18100–18999, `MICHI_CONTRACT_PORT` override), throwaway `ci-contract` admin + per-run password (never logged), `/health/live` poll ≤60s with process-death short-circuit, contract run (5s per-request timeout in-test), persistent failure log via `MICHI_FAILURE_LOG`, EXIT trap kill + `rm -rf`. Happy path exit 0 (33 passed/0 failed, port 18528); dead-server path exit 1; contract-FAIL path exit 1 (propagated); no orphan/leak after trap.
- Task 2.2: `.github/workflows/ci.yml` — `ci-python-contract` job (`needs: ci-rust`) with checkout, toolchain, rust-cache, system deps, gate run (`MICHI_FAILURE_LOG: contract-failure.log`), and `upload-artifact` on failure. YAML parses (`yaml.safe_load`); `ci-rust`/`ci-docker`/`release-ghcr` bodies untouched (diff shows only the inserted job block).
- No Rust change in this slice: `git diff 52eaf0e..HEAD -- crates/ apps/ Cargo.toml Cargo.lock` empty.

### Work Unit 3 (Slice 03)
- Child worktree `git worktree add -b 03-build-release /home/cristian/.cache/michi-v1s/wt-03 05223f43b58c959d8da101d8330a2f363391acb5` → HEAD `05223f4`, clean.
- Main worktree before/after manifests (byte/path/mode identity): before sha256 `8ab29e832185b6bb0b22e7ecbd486ed7968db6f50af4f026e831803ec314f9b1` (228 entries) — `/tmp/opencode/michi-v1s/main-before-wu3.manifest`; after sha256 `8ab29e832185b6bb0b22e7ecbd486ed7968db6f50af4f026e831803ec314f9b1` — `/tmp/opencode/michi-v1s/main-after-wu3.manifest`. Main dirty-state `status_porcelain_z` hash unchanged `d44a16570145160f64a9c99a05fe757543a259fa8457950729006e999f5e31b0` (before + after identical).
- Task 3.0 (gate 4R follow-ups): trap INT+TERM (`trap '…; exit 130' INT TERM`) added to the EXIT trap; cleanup ignores INT/TERM during teardown (`trap '' INT TERM`) so a second signal cannot abort cleanup mid-way; health-poll `curl --connect-timeout 3 --max-time 5`; failure-log comment corrected (default lives inside RUN_DIR unless `MICHI_FAILURE_LOG` overrides); rebuild check (`find apps crates Cargo.toml Cargo.lock -newer "$SERVER_BIN"`) rebuilds stale binaries. Proof: happy path exit 0 (33/0, rebuild triggered), forced failure (occupied port) exit 1 + no orphan + temp dir removed, TERM mid-health-wait exit 130 + server killed + temp dir removed.
- Task 3.1 (Dockerfile cache fix): placeholder pattern verified present in HEAD. Added `find apps crates -type f … -exec touch {} +` between `COPY crates ./crates` and the final `cargo build`; COPY overwrites placeholders (design D6) so no `rm` (an `rm` after COPY would delete the just-copied real sources).
- Task 3.2 (toolchain pin): `rust-toolchain.toml` (`channel = "1.96.0"`), `Dockerfile` `ARG RUST_VERSION=1.96.0` + `FROM rust:${RUST_VERSION}-bookworm`, CI `dtolnay/rust-toolchain@stable` with `toolchain: 1.96.0` (both jobs). **Discovery**: `rust:1.96.0` (no variant) resolves to Debian trixie / glibc 2.41, which produced a binary requiring `GLIBC_2.38`/`GLIBC_2.39` that the `debian:bookworm-slim` runtime (glibc 2.36) could not run — fixed with the `-bookworm` builder variant (glibc 2.36 == runtime). `rustc --version` = 1.96.0; `cargo test --workspace` 217/0/14 green. Note: the dtolnay action input is named `toolchain`, not `channel` (verified against the action's action.yml).
- Task 3.3 (multiarch): `platforms: linux/amd64,linux/arm64`; the `latest` meta guard (`startsWith(github.ref, 'refs/tags/v') && !contains(github.ref, '-')`) is unchanged.
- Task 3.4 (Docker smoke): `docker build` OK (image 178MB). Container reached `healthy`; `/api/status` 200 `version 0.2.0`, `/health/live` 200, root 200; boot log shows exactly 37 migrations (last `migration 37: server config`). Graceful stop: `docker stop -t 25` (15.7s real) → `received SIGTERM, starting graceful shutdown...` → `WAL checkpoint complete` → `shutdown complete`, ExitCode 0; container + image removed. Persisted evidence: `/tmp/opencode/michi-v1s/wu3-docker-run.log` sha256 `6776c1518c2fdaaf44e3e7623d3e0a02d4f89fafdb7d8b81a5c7e8af78f46e6e`; `/tmp/opencode/michi-v1s/wu3-docker-health.json` sha256 `4cd83277549f622348e35e1772eb397c4437dabbbecbc66ee744bf563d003ca1`. **Discovery**: graceful shutdown takes ~15s (`shutdown_and_wait(15s)`) because idle ingest/playback workers only stop at the 15s timeout; a default 10s `docker stop` SIGKILLs before `WAL checkpoint complete`.
- Task 3.5 (ARM64): **NOT EXECUTED — ENVIRONMENT LIMITATION**. Host binfmt_misc has no QEMU arm64 handler; `docker run --rm --platform linux/arm64 alpine uname -m` and `docker buildx build --platform linux/arm64` both fail `exec /bin/sh: exec format error` at the first RUN step. No arm64 qualification is claimed.
- Task 3.6 (healthcheck public liveness, P1 corrective): **Root cause** — `Dockerfile:91` HEALTHCHECK probed `/api/status`, which sits in the protected auth router (`crates/michi-api/src/lib.rs` `protected` router + `auth_middleware`); enabling `MICHI_AUTH_USERNAME`/`MICHI_AUTH_PASSWORD` made the probe 401 and the container permanently unhealthy. `/health/live` is in `v1_public_routes()` (public). **Fix**: one-line HEALTHCHECK change to `wget -qO- http://127.0.0.1:8096/health/live || exit 1` (interval/timeout/start-period/retries unchanged). **Proof**: `docker build -t michi-v1s-smoke:wu3-hc .` OK; boot WITH auth enabled (throwaway admin) on isolated port 18097 → `healthy`; `/health/live` 200, `/api/status` 401 without token (auth active AND healthcheck public), root 200; graceful stop (ExitCode 0); container + image removed, port freed. Persisted evidence: `/tmp/opencode/michi-v1s/wu3-docker-hc-run.log` sha256 `9c5ffae9d75dd16803d925d79511f69e659a9e004418062545a2002dec8a3892`; `/tmp/opencode/michi-v1s/wu3-docker-hc-health.json` sha256 `e2607102813ddbc88a0c0e3c60798f1c2ffb0b443fc2c1d257256fe103b1fb88` (password redacted).

### Work Unit 4a (Slice 04a)
- Child worktree `git worktree add -b 04a-receiver-dedup /home/cristian/.cache/michi-v1s/wt-04 355cf0ce5fc904670c835611818bfc6d9fe47649` → HEAD `355cf0c`, clean. Parent ancestry exact: `355cf0c` is the `03-build-release` tip.
- Main worktree before/after manifests (byte/path/mode identity, identical sha256 `66b631f43cd2bf22a11af2ca42c53dcf6d395a78b8868d9b0de520e251035492`, 231 entries): `/tmp/opencode/michi-v1s/main-before-wu4a.manifest`, `main-after-wu4a.manifest`. Main dirty-state `status_porcelain_z` hash unchanged `d44a16570145160f64a9c99a05fe757543a259fa8457950729006e999f5e31b0`.
- **Baseline drift note**: WU3 recorded 228 entries / sha256 `8ab29e83…`; the WU4a baseline is 231 entries / `66b631f4…` because the WU3 native 4R review added exactly 3 gitignored `.atl/wu3-review-context/{diff.txt,manifest.json,meta.json}` scratch files AFTER the WU3 after-manifest was captured. All 228 WU3 paths are still present with byte-identical hashes; no existing file changed and this slice touched nothing in main.
- Authoritative-copy guard: `crates/michi-receivers/tests/receiver_simulator_integration.rs` (388 lines) exists before and after, sha256 `2118d06ff3f04652719464382a24435f4cefc92172482036a2cd96937e92ea73` unchanged. The two dead duplicates are byte-identical to each other (233 lines, sha256 `2496930c…`), but differ from the authoritative copy: the difference is rustfmt formatting ONLY (authoritative is the formatted 388-line version) plus one extra `#[allow(dead_code)]` attribute in the authoritative copy. The duplicates are a strict SEMANTIC SUBSET — same 14 test functions, same 14 `#[tokio::test]` + 14 `#[ignore]` attributes, same assertions; zero unique content in the duplicates. This is NOT "deleting the only copy of live tests": the live compiled copy is the crate target and it is a superset. Proceeded after this verification (not blind deletion).
- Never-compiled proof: `grep -rn "tests/receiver_simulator_integration\|tests/e2e/test_receiver_simulator_integration" --include='Cargo.toml'` → no matches; `tests/` has no crate manifest; root workspace `members` list = 22 entries (apps + 21 crates), none is `tests/`. `scripts/test_receiver_e2e.sh:43` and docs reference `--test receiver_simulator_integration` (the crate target name), never the dead paths.
- Task 4.1/4.2: `git rm tests/receiver_simulator_integration.rs tests/e2e/test_receiver_simulator_integration.rs` → 2 files deleted, `git diff --cached --numstat` = `0 233` + `0 233` = 466 deletions, 0 additions. Commit `429deae`.
- Task 4.3: `CARGO_TARGET_DIR=/home/cristian/.cache/michi-v1s/target cargo test --workspace` → exit 0, 217 passed / 0 failed / 14 ignored (full log `/tmp/opencode/michi-v1s/wu4a-cargo-test.log`). The `14 ignored` is the authoritative crate target, confirming it still registers its tests.

## Files Changed
| File | Action | What was done |
|---|---|---|
| `docs/V1_STABILIZATION_AUDIT.md` | Added | Phase 0 baseline audit (4 gates, 217/0/14, Docker 200 + 37 migrations, P0=0, 14 ignored-test inventory, P1.1–P1.4 file:line) |
| `tests/e2e/test_player_micro_contract_compatibility.py` | Modified | remove `global BASE_URL`; thread `base_url`; `api_version == "v1"`; admin auth + Bearer; loud failures; `supports_*` shape asserts; **WU2**: negative auth guard, `_safe_json_or_raw`, queue-transfer wording, docstring |
| `docs/MICHI_LINK_MICRO_E2E.md` | Modified | `:10` `michi_link_version` → `api_version` |
| `scripts/ci_contract_gate.sh` | Added | CI contract gate (health wait, isolated boot, trap cleanup, failure propagation); **WU3**: INT/TERM trap, curl timeouts, stale-binary rebuild check, failure-log comment fix |
| `.github/workflows/ci.yml` | Modified | add `ci-python-contract` job gated on `ci-rust`; **WU3**: pin `toolchain: 1.96.0` (ci-rust + ci-python-contract), `platforms: linux/amd64,linux/arm64` |
| `Dockerfile` | Modified | **WU3**: `find -exec touch` cache invalidation; `ARG RUST_VERSION=1.96.0` + `-bookworm` builder variant; **3.6**: HEALTHCHECK CMD `/api/status` → `/health/live` |
| `rust-toolchain.toml` | Added | **WU3**: `channel = "1.96.0"` |
| `tests/receiver_simulator_integration.rs` | Deleted | **WU4a**: dead duplicate (233 lines, never compiled) — authoritative copy is `crates/michi-receivers/tests/receiver_simulator_integration.rs` |
| `tests/e2e/test_receiver_simulator_integration.rs` | Deleted | **WU4a**: dead duplicate (233 lines, never compiled) — byte-identical to the other dead duplicate |
| `openspec/changes/v1-stabilization/tasks.md` | Modified | marked 1.1–1.7 `[x]`; added + marked 2.0–2.3 `[x]`; added + marked 3.0–3.4 `[x]`, 3.5 `[ ]` (NOT EXECUTED), 3.6 `[x]`; **WU4a**: marked 4.1–4.3 `[x]`, corrected ~305 → 233 lines |
| `openspec/changes/v1-stabilization/apply-progress.md` | Modified | this artifact (WU0+WU1+WU2+WU3+WU4a merged) |

## Deviations from Design
1. Tasks 1.2/1.3/1.4 landed as one atomic rewrite (same file); each acceptance individually verified.
2. `supports_*` corrected from `== True` to "field present" — spec/design never required `True`; asserting it would fabricate capability (latent 4th fault).
3. Gate script is 127 lines (vs ~55 forecast) — fuller comments/inline docs for the CI gate; within the 500-line slice budget.
4. Task 2.0 added by orchestrator as a WU1 4R-review follow-up (same file); recorded before 2.1 per delivery contract.
5. Task 3.0 added by orchestrator as a WU2 4R-review follow-up on the gate script; recorded before 3.1 per delivery contract.
6. Task 3.1: no `rm` of placeholders after COPY — COPY overwrites them (design D6); an `rm` after COPY would delete the real sources. Implemented as `find -exec touch` mtime refresh only.
7. Task 3.2: CI pin uses the dtolnay action's `toolchain:` input (verified in action.yml) rather than the literal `channel:` wording in the task; the action has no `channel` input, and `channel:` would be silently ignored.
8. Task 3.2: builder pinned to `rust:1.96.0-bookworm` (not bare `rust:1.96.0`) — bare 1.96.0 is Debian trixie/glibc 2.41, incompatible with the bookworm-slim runtime.
9. Task 3.5: NOT EXECUTED — QEMU/binfmt arm64 unavailable (exec format error). Recorded as environment limitation; no ARM64 qualification claimed.
10. Task 3.6 added by orchestrator as a P1 pre-review corrective: HEALTHCHECK moved from protected `/api/status` to public `/health/live`. One Dockerfile line; interval/timeout/start-period/retries unchanged.
11. Slice 04a (WU4a): actual deletion is 233 lines per file (466 total), not the ~305-line estimate in proposal/tasks (the estimate overshot; the live file is 233 unformatted lines). Budget recorded as exact numstat `0 233` + `0 233`.
12. Slice 04a: the two dead duplicates differ from the authoritative crate copy, but ONLY by rustfmt formatting plus one extra `#[allow(dead_code)]` attribute in the authoritative copy — the duplicates are a strict semantic subset (same 14 tests, same assertions, zero unique content). Verified before deleting (guard satisfied: not the only copy of live tests).

## Issues Found
- Pre-existing flaky Rust tests (parallel race on shared `/tmp/michi-test/music`), not caused by this slice.
- `player_compatibility.supports_*` are misleadingly-named runtime-state booleans.
- `rust:1.96.0` default image moved to Debian trixie (glibc 2.41) — bare-tag Docker builds now require the `-bookworm` variant to match a bookworm-slim runtime.
- Server graceful shutdown takes ~15s (`shutdown_and_wait` timeout); default 10s `docker stop` SIGKILLs before the WAL checkpoint (use `-t 25`).
- Main-worktree manifest baseline drifted from WU3's `8ab29e83…` (228 entries) to `66b631f4…` (231 entries) purely from the 3 gitignored `.atl/wu3-review-context/` files written by the WU3 review lifecycle — no source file changed; pre-existing WebUI dirty state (`d44a1657…`) unchanged.

## Remaining Tasks
Work Units 4b–6 (Slices 04b/05) not begun — out of scope for this slice. Task 3.5 (ARM64 emulated boot) remains NOT EXECUTED pending a QEMU/binfmt-enabled environment. Slice 04a (this slice) is complete.

## Workload / PR Boundary
- Mode: chained PR slice (force-chained / feature-branch-chain)
- Current work unit: 4a (Slice 04a — Receiver dedup, pure deletion exception)
- Boundary: `355cf0c` (03-build-release) → `04a-receiver-dedup` (delete two dead duplicate receiver tests + task/progress marks)
- Review budget (exact `git diff --numstat 355cf0c..HEAD`): 0 additions + 466 deletions (deletion-only exception; 0 authored lines) + bookkeeping lines in `tasks.md`/`apply-progress.md`

## Status
22/23 tasks complete across Work Units 0–4a (2 + 7 + 4 + 6 + 3 done; task 3.5 NOT EXECUTED). Work Unit 4a (Slice 04a) done. Nothing pushed; no PR created; Work Unit 4b (Slice 04b) NOT begun.
