# Apply Progress: v1-stabilization — Work Unit 0 + 1 + 2 + 3 + 4a + 4b + 5

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

### Work Unit 5 — Slice 04b: Receiver runner repair
- [x] 5.1 Fix runner target + both sims + ignored (`-p michi-receivers`)
- [x] 5.2 Env-overridable sim paths (already satisfied at HEAD — verified, no code change)
- [x] 5.3 Loud unavailable-dependency failure proof (sims-down non-zero + contract-drift evidence)
- [x] 5.4 Document simulator boundary truthfully

### Work Unit 6 — Slice 05: Metadata truthfulness
- [x] 5.0 WU3 native 4R review follow-ups (deferred): stop-grace note + arm64 status note + apply-progress wording fixes
- [x] 6.1 CasaOS version `0.1.0` → `0.2.0` (only the version line)
- [x] 6.2 CHANGELOG "35 migraciones" → "37 migraciones" (derived: `grep -c 'fn migration_0'` = 37)
- [x] 6.3 No build/test impact (metadata + bookkeeping only; cargo check unchanged green)

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

### Work Unit 5 (Slice 04b)
| Evidence | Actual |
|---|---|
| Focused test | `bash -n scripts/test_receiver_e2e.sh scripts/run_receiver_sim_standard.sh scripts/run_receiver_sim_hifi.sh` → exit 0 (all three). Repaired target compiles: `CARGO_TARGET_DIR=…/target cargo test -p michi-receivers --test receiver_simulator_integration --no-run` → exit 0. `--list` = 14 tests, all `#[ignore]` (source grep: 14 `#[ignore]` + 14 `#[tokio::test]`). Authoritative copy sha256 `2118d06f…` unchanged (dedup guard held) |
| Runtime harness | **sims-down (5.3)**: `bash scripts/test_receiver_e2e.sh` with no sims → exit 1, `ERROR: Standard simulator not running on port 8080`, elapsed 1s (no hang), 14 tests never run/reported passing. **5.2 missing path**: `MICHI_STREAM_SIM_PATH=/tmp/opencode/michi-v1s/does-not-exist-…` → both sim scripts print `ERROR: Simulator not found at …` + exit 1. **5.2 valid override**: `MICHI_STREAM_SIM_PATH=/tmp/opencode/michi-v1s/stub_receiver_sim.py` → stub executed (`STUB-SIM USED: args=['--type','standard','--port','8080']`), exit 0 — override used, machine default NOT used. **Contract-drift (honest N/A for sims-up green)**: the available `/home/cristian/michi-music-stream/simulator/receiver_sim.py` is the NEW canonical v1-lite sim (`/api/v1/server/info` 200, `version 0.3.0`); `GET /api/v1/receiver/info` → 404. The `michi-receivers` crate (`client.rs`) targets the legacy `/api/v1/receiver/*` contract. Running the repaired runner with the v1-lite sims UP on 8080/8081 still exits 1 with "Standard simulator not running on port 8080" (health check endpoint not served). A green sims-up run is therefore **not reproducible in this environment** — reported honestly, not fabricated |
| Rollback boundary | Revert `scripts/test_receiver_e2e.sh` and `docs/STREAM_SIMULATOR_INTEGRATION.md`; revert tasks.md/apply-progress.md marks. No Rust, no CI, no schema/state change; `run_receiver_sim_*.sh` untouched |

### Work Unit 6 (Slice 05)
| Evidence | Actual |
|---|---|
| Focused test | `grep -n '^version:' casaos/data.yml` → `0.2.0`; `grep -c 'fn migration_0' crates/michi-db/src/lib.rs` → 37 (and `grep -c 'fn migration_'` → 37, so 37 is the true total); `grep -n migraciones CHANGELOG.md` → `37: - 37 migraciones de base de datos`. All exit 0 |
| Runtime harness | N/A — two metadata lines + two truthful notes; no runtime boundary exists for storefront metadata/changelog text |
| Rollback boundary | Revert `casaos/data.yml` version line; revert the CHANGELOG count + `### Notas` block; revert tasks.md/apply-progress.md marks. No Rust, no WebUI/PWA, no CI, no schema/state change |

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

### Work Unit 5 (Slice 04b)
- Child worktree `git worktree add -b 04b-receiver-runner /home/cristian/.cache/michi-v1s/wt-05 ea330e5a667eff17284fccd7ed6a132d13fd8496` → HEAD `ea330e5`, clean. Parent ancestry exact: `ea330e5` is the `04a-receiver-dedup` tip.
- Main worktree before/after manifests (byte/path/mode identity, identical sha256 `f4cb84530f504332bce8b3004043bd0194f85a8f35d825e5a67d091530caa706`, 234 entries): `/tmp/opencode/michi-v1s/main-before-wu5.manifest`, `main-after-wu5.manifest` (captured at the end of this slice). Main dirty-state `status_porcelain_z` hash unchanged `d44a16570145160f64a9c99a05fe757543a259fa8457950729006e999f5e31b0`.
- **Baseline drift note**: WU4a recorded 231 entries / sha256 `66b631f4…`; the WU5 baseline is 234 entries / `f4cb8453…` because the WU4a native 4R review added exactly 3 gitignored `.atl/wu4a-review-context/{diff.txt,manifest.json,meta.json}` scratch files AFTER the WU4a after-manifest was captured. All 231 WU4a paths remain byte-identical; no existing file changed and this slice touched nothing in main.
- Task 5.1: `scripts/test_receiver_e2e.sh:43` `cargo test --test receiver_simulator_integration -- --ignored` → `cargo test -p michi-receivers --test receiver_simulator_integration -- --ignored` (1 line). Both `MICHI_RECEIVER_SIM_URL` and `MICHI_RECEIVER_SIM_HIFI_URL` exports (lines 39–40) were already present at HEAD and remain. `bash -n` exit 0; repaired target compiles (`--no-run` exit 0); `--list` = 14 tests all `#[ignore]`.
- Task 5.2: **no code change needed** — `scripts/run_receiver_sim_standard.sh` / `run_receiver_sim_hifi.sh` already implement `SIM_PATH="${MICHI_STREAM_SIM_PATH:-/home/cristian/michi-music-stream/simulator/receiver_sim.py}"` (override primary, machine default fallback) plus `[ ! -f "$SIM_PATH" ]` → `ERROR: Simulator not found at …` + `exit 1`. Verified: missing path → exit 1 (both scripts); valid `MICHI_STREAM_SIM_PATH` stub → executed (override used). The planned `fix(receivers): make simulator paths env-overridable…` commit is therefore dropped (empty diff).
- Task 5.3 (sims-down): `bash scripts/test_receiver_e2e.sh` (no sims) → exit 1, `ERROR: Standard simulator not running on port 8080`, elapsed 1s (no hang), the 14 tests never compiled/run/reported passing. This is the authoritative loud-failure proof.
- Task 5.3 (contract drift — honest N/A for sims-up green): the available simulator at `/home/cristian/michi-music-stream/simulator/receiver_sim.py` is the NEW canonical v1-lite implementation (`feat(simulator): implement canonical receiver v1-lite API`, serves `/api/v1/server/info` → 200 `version 0.3.0`, `/api/v1/receiver-lite/*`, `GET /api/v1/receiver/info` → 404). The `michi-receivers` crate (`client.rs:19-154`) targets the LEGACY `/api/v1/receiver/*` contract. Probe evidence: sims UP on 8080/8081 (`/api/v1/server/info` 200 both) → repaired runner still exits 1 with "Standard simulator not running on port 8080" (its health check hits `/api/v1/receiver/info` which the v1-lite sim no longer serves). **A green sims-up run cannot be reproduced in this environment**; reported honestly, NOT fabricated, per the delivery contract.
- Task 5.4: `docs/STREAM_SIMULATOR_INTEGRATION.md` — added a truthful "Simulator Boundary" section (external `pitydah/michi-music-stream` dependency, acquisition via `git clone`, required env vars `MICHI_RECEIVER_SIM_URL`/`MICHI_RECEIVER_SIM_HIFI_URL`/`MICHI_STREAM_SIM_PATH`, "runner repaired, CI enablement deferred", contract-drift warning); corrected the running-commands block to `-p michi-receivers` + the runner script. `grep -n ci-receivers .github/workflows/ci.yml` → empty (no receiver CI job; no receiver CI claim).
- Authoritative-copy guard held: `crates/michi-receivers/tests/receiver_simulator_integration.rs` sha256 `2118d06ff3f04652719464382a24435f4cefc92172482036a2cd96937e92ea73` unchanged (this slice did not touch it).

### Work Unit 6 (Slice 05)
- Child worktree `git worktree add -b 05-metadata /home/cristian/.cache/michi-v1s/wt-06 2a1ad1770837340e112b56d18e68eff6c12fe869` → HEAD `2a1ad17`, clean. Parent ancestry exact: `2a1ad17` is the `04b-receiver-runner` tip.
- Main worktree before/after manifests (byte/path/mode identity, identical sha256 `9dde5d1f97b0ed8d32bd9bc79796b956c870e7122518f6c9f893eadfd8ec760c`, 240 entries): `/tmp/opencode/michi-v1s/main-before-wu6.manifest`, `main-after-wu6.manifest`. Main dirty-state `status_porcelain_z` hash unchanged `d44a16570145160f64a9c99a05fe757543a259fa8457950729006e999f5e31b0`.
- **Baseline drift note**: WU5 recorded 234 entries / sha256 `f4cb8453…`; the WU6 baseline is 240 entries / `9dde5d1f…` because 6 gitignored files were added AFTER the WU5 after-manifest: 3 `.atl/wu4b-review-context/{diff.txt,manifest.json,meta.json}` (WU4b review lifecycle) + 3 `.impeccable/{design.json,assets/michi-hero-cat.md,surfaces/crates-michi-api-static-index-html.md}` (user design work). All 234 WU5 paths remain byte-identical; no existing file changed and this slice touched nothing in main.
- Task 5.0 (stop-grace): `casaos/data.yml` is storefront metadata only (`name/version/slug/description/developer/license/port_map/categories/tags/maintainer/more_info`) — it does NOT support `stop_grace_period`/stop-timeout. Verified against the CasaOS AppStore schema (v1 `data.yml` = storefront metadata; container runtime fields live in the separate compose definition — this repo's `casaos/docker-compose.casaos.yml`, out of slice scope). No field invented; truthful note added to CHANGELOG instead.
- Task 5.0 (arm64): honest one-line note added to CHANGELOG — arm64 configured in CI release, runtime qualification pending first successful arm64 build (QEMU unavailable). No qualification claim.
- Task 5.0 (wording): deviation #3 corrected "Gate script is 127 lines" → "145 lines" (127 at WU2 creation + 18 from WU3 3.0 hardening); WU5 budget line restated as exact numstat (70 additions + 17 deletions = 87 total).
- Task 6.1: `casaos/data.yml:2` `version: 0.1.0` → `0.2.0` (single line; no other metadata touched). Commit `f3ec368`.
- Task 6.2: `CHANGELOG.md` "35 migraciones" → "37 migraciones"; count derived via `grep -c 'fn migration_0' crates/michi-db/src/lib.rs` = 37 (also `grep -c 'fn migration_'` = 37, so 37 is the true total). Commit `62c0855`.
- Task 6.3: `git diff --stat 2a1ad17..HEAD` shows only `casaos/data.yml`, `CHANGELOG.md`, `openspec/.../tasks.md`, `openspec/.../apply-progress.md` (no WebUI/PWA/Rust/CI/scripts). `cargo check --workspace` unchanged green (no Rust change in this slice; prior evidence `217 passed / 0 failed / 14 ignored` still holds).

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
| `scripts/test_receiver_e2e.sh` | Modified | **WU5 (04b)**: `cargo test …` → `cargo test -p michi-receivers …` (5.1) |
| `docs/STREAM_SIMULATOR_INTEGRATION.md` | Modified | **WU5 (04b)**: add truthful "Simulator Boundary" section (external dependency, env vars, acquisition, "runner repaired, CI enablement deferred", contract-drift warning); fix running commands to `-p michi-receivers` (5.4) |
| `casaos/data.yml` | Modified | **WU6 (05)**: `version: 0.1.0` → `0.2.0` (6.1, single line) |
| `CHANGELOG.md` | Modified | **WU6 (05)**: "35 migraciones" → "37 migraciones" (6.2); `### Notas` block — stop-grace ≥25s + arm64 qualification status (5.0) |
| `openspec/changes/v1-stabilization/tasks.md` | Modified | marked 1.1–1.7 `[x]`; added + marked 2.0–2.3 `[x]`; added + marked 3.0–3.4 `[x]`, 3.5 `[ ]` (NOT EXECUTED), 3.6 `[x]`; **WU4a**: marked 4.1–4.3 `[x]`, corrected ~305 → 233 lines; **WU5**: marked 5.1–5.4 `[x]`; **WU6**: added + marked 5.0 `[x]`, marked 6.1–6.3 `[x]` |
| `openspec/changes/v1-stabilization/apply-progress.md` | Modified | this artifact (WU0+WU1+WU2+WU3+WU4a+WU4b+WU6 merged); **WU6**: deviation #3 gate-script line-count corrected (127 → 145), WU5 budget line restated as exact numstat |

## Deviations from Design
1. Tasks 1.2/1.3/1.4 landed as one atomic rewrite (same file); each acceptance individually verified.
2. `supports_*` corrected from `== True` to "field present" — spec/design never required `True`; asserting it would fabricate capability (latent 4th fault).
3. Gate script is 145 lines (127 at WU2 creation, +18 from WU3 3.0 hardening; vs ~55 forecast) — fuller comments/inline docs for the CI gate; within the 500-line slice budget.
4. Task 2.0 added by orchestrator as a WU1 4R-review follow-up (same file); recorded before 2.1 per delivery contract.
5. Task 3.0 added by orchestrator as a WU2 4R-review follow-up on the gate script; recorded before 3.1 per delivery contract.
6. Task 3.1: no `rm` of placeholders after COPY — COPY overwrites them (design D6); an `rm` after COPY would delete the real sources. Implemented as `find -exec touch` mtime refresh only.
7. Task 3.2: CI pin uses the dtolnay action's `toolchain:` input (verified in action.yml) rather than the literal `channel:` wording in the task; the action has no `channel` input, and `channel:` would be silently ignored.
8. Task 3.2: builder pinned to `rust:1.96.0-bookworm` (not bare `rust:1.96.0`) — bare 1.96.0 is Debian trixie/glibc 2.41, incompatible with the bookworm-slim runtime.
9. Task 3.5: NOT EXECUTED — QEMU/binfmt arm64 unavailable (exec format error). Recorded as environment limitation; no ARM64 qualification claimed.
10. Task 3.6 added by orchestrator as a P1 pre-review corrective: HEALTHCHECK moved from protected `/api/status` to public `/health/live`. One Dockerfile line; interval/timeout/start-period/retries unchanged.
11. Slice 04a (WU4a): actual deletion is 233 lines per file (466 total), not the ~305-line estimate in proposal/tasks (the estimate overshot; the live file is 233 unformatted lines). Budget recorded as exact numstat `0 233` + `0 233`.
12. Slice 04a: the two dead duplicates differ from the authoritative crate copy, but ONLY by rustfmt formatting plus one extra `#[allow(dead_code)]` attribute in the authoritative copy — the duplicates are a strict semantic subset (same 14 tests, same assertions, zero unique content). Verified before deleting (guard satisfied: not the only copy of live tests).
13. Slice 04b (WU5), task 5.2: the two `scripts/run_receiver_sim_*.sh` scripts already implemented the `MICHI_STREAM_SIM_PATH` override + machine-default fallback + `[ ! -f ]` missing-file check at HEAD (`5d21f92`). No code change was needed; the planned `fix(receivers): make simulator paths env-overridable…` commit was dropped (would have been empty). Behavior verified with real evidence instead (missing path → exit 1; valid override stub → executed).
14. Slice 04b (WU5), task 5.3: a sims-up green run is NOT reproducible in this environment. The available `receiver_sim.py` is the NEW canonical v1-lite implementation (serves `/api/v1/server/info`, `/api/v1/receiver-lite/*`; `GET /api/v1/receiver/info` → 404), while the `michi-receivers` crate targets the LEGACY `/api/v1/receiver/*` contract. Evidence recorded via the sims-down path plus the contract-drift probe; no green run fabricated. Migrating the crate to v1-lite is out of scope (future gate).
15. Slice 04b (WU5), task 5.1: `-p michi-receivers` is required by spec even though cargo auto-resolves the unique `receiver_simulator_integration` target in this workspace (verified `--no-run` exit 0 both with and without `-p`); the explicit `-p` removes ambiguity and guards against a future second same-named target.
16. Slice 05 (WU6), task 5.0 (stop-grace): `casaos/data.yml` is CasaOS storefront metadata only — it does NOT support `stop_grace_period`/stop-timeout (verified against the CasaOS AppStore schema; the repo's container runtime config lives in `casaos/docker-compose.casaos.yml`, outside this slice's `data.yml`+`CHANGELOG` scope). Per the task's own fallback, no field was invented; a truthful "≥25s grace" note was added to CHANGELOG instead.
17. Slice 05 (WU6), task 5.0 (numbering): the new follow-up task is numbered `5.0` per the orchestrator ("WU3 native 4R review follow-ups", deferred to the final slice), placed before 6.1 in the Work Unit 6 section — a deliberate continuation of the 2.0/3.0 follow-up numbering pattern, not a Work Unit 5 task.
18. Slice 05 (WU6), task 6.2 commit: the delivery contract's commit message is `fix(metadata): correct changelog migration count` (the "to 37" suffix from the task text was dropped to keep the subject concise); the derived count (37) is captured in the task evidence and diff.

## Issues Found
- Pre-existing flaky Rust tests (parallel race on shared `/tmp/michi-test/music`), not caused by this slice.
- `player_compatibility.supports_*` are misleadingly-named runtime-state booleans.
- `rust:1.96.0` default image moved to Debian trixie (glibc 2.41) — bare-tag Docker builds now require the `-bookworm` variant to match a bookworm-slim runtime.
- Server graceful shutdown takes ~15s (`shutdown_and_wait` timeout); default 10s `docker stop` SIGKILLs before the WAL checkpoint (use `-t 25`).
- Main-worktree manifest baseline drifted from WU3's `8ab29e83…` (228 entries) to `66b631f4…` (231 entries) purely from the 3 gitignored `.atl/wu3-review-context/` files written by the WU3 review lifecycle — no source file changed; pre-existing WebUI dirty state (`d44a1657…`) unchanged.
- **WU5 discovery (contract drift)**: the external `pitydah/michi-music-stream` simulator has evolved to a canonical v1-lite API (`/api/v1/server/info`, `/api/v1/pair/*`, `/api/v1/receiver-lite/*`; `version 0.3.0`, `api_version v1-lite`) and no longer serves the legacy `/api/v1/receiver/*` endpoints that `crates/michi-receivers/src/client.rs` targets. The 14 ignored tests cannot be exercised against the current simulator; CI enablement must wait for a crate→v1-lite migration (explicit future gate, documented).

## Remaining Tasks
Task 3.5 (ARM64 emulated boot) remains NOT EXECUTED pending a QEMU/binfmt-enabled environment (recorded honestly; arm64 qualification status documented in CHANGELOG). Receiver CI enablement remains deferred pending a crate→v1-lite contract migration (future gate). Work Unit 6 (Slice 05, metadata truthfulness) is complete — final implementation slice.

## Workload / PR Boundary
- Mode: chained PR slice (force-chained / feature-branch-chain)
- Current work unit: 6 (Slice 05 — Metadata truthfulness)
- Boundary: `2a1ad17` (04b-receiver-runner) → `05-metadata` (`data.yml` 0.2.0 + CHANGELOG 37 + truthful shutdown-grace/arm64 notes + task/progress marks)
- Review budget (exact `git diff --numstat 2a1ad17..HEAD`): see Status below — metadata + bookkeeping only, no WebUI/PWA/Rust/CI/scripts.

## Status
29/30 tasks complete across Work Units 0–6 (2 + 7 + 4 + 6 + 3 + 4 + 4 done; task 3.5 NOT EXECUTED). Work Unit 6 (Slice 05) done — final implementation slice. Nothing pushed; no PR created; no review lifecycle command run.
