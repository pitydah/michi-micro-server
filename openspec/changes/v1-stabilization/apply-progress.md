# Apply Progress: v1-stabilization — Work Unit 0 + Work Unit 1 + Work Unit 2

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

## Files Changed
| File | Action | What was done |
|---|---|---|
| `docs/V1_STABILIZATION_AUDIT.md` | Added | Phase 0 baseline audit (4 gates, 217/0/14, Docker 200 + 37 migrations, P0=0, 14 ignored-test inventory, P1.1–P1.4 file:line) |
| `tests/e2e/test_player_micro_contract_compatibility.py` | Modified | remove `global BASE_URL`; thread `base_url`; `api_version == "v1"`; admin auth + Bearer; loud failures; `supports_*` shape asserts; **WU2**: negative auth guard, `_safe_json_or_raw`, queue-transfer wording, docstring |
| `docs/MICHI_LINK_MICRO_E2E.md` | Modified | `:10` `michi_link_version` → `api_version` |
| `scripts/ci_contract_gate.sh` | Added | CI contract gate (health wait, isolated boot, trap cleanup, failure propagation) |
| `.github/workflows/ci.yml` | Modified | add `ci-python-contract` job gated on `ci-rust` |
| `openspec/changes/v1-stabilization/tasks.md` | Modified | marked 1.1–1.7 `[x]`; added + marked 2.0–2.3 `[x]` |
| `openspec/changes/v1-stabilization/apply-progress.md` | Modified | this artifact (WU0+WU1+WU2 merged) |

## Deviations from Design
1. Tasks 1.2/1.3/1.4 landed as one atomic rewrite (same file); each acceptance individually verified.
2. `supports_*` corrected from `== True` to "field present" — spec/design never required `True`; asserting it would fabricate capability (latent 4th fault).
3. Gate script is 127 lines (vs ~55 forecast) — fuller comments/inline docs for the CI gate; within the 500-line slice budget.
4. Task 2.0 added by orchestrator as a WU1 4R-review follow-up (same file); recorded before 2.1 per delivery contract.

## Issues Found
- Pre-existing flaky Rust tests (parallel race on shared `/tmp/michi-test/music`), not caused by this slice.
- `player_compatibility.supports_*` are misleadingly-named runtime-state booleans.

## Remaining Tasks
Work Units 3–6 (Slices 03–05) not begun — out of scope for this slice.

## Workload / PR Boundary
- Mode: chained PR slice (force-chained / feature-branch-chain)
- Current work unit: 2 (Slice 02 — CI contract gate)
- Boundary: `52eaf0e` (01-audit-contract) → `02-ci-contract` (contract test follow-ups + gate script + ci job + task/progress marks)
- Review budget (exact `git diff --numstat 52eaf0e..HEAD`): 46+127+21 additions + 9 deletions in code; total changed lines = additions + deletions across all four commits (≤ 500)

## Status
13/13 tasks complete in Work Units 0–2 (2 + 7 + 4). Work Unit 2 (Slice 02) done. Nothing pushed; no PR created; Work Unit 3 NOT begun.
