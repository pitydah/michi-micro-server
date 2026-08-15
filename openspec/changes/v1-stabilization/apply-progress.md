# Apply Progress: v1-stabilization — Work Unit 0 + Work Unit 1

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

## Files Changed
| File | Action | What was done |
|---|---|---|
| `docs/V1_STABILIZATION_AUDIT.md` | Added | Phase 0 baseline audit (4 gates, 217/0/14, Docker 200 + 37 migrations, P0=0, 14 ignored-test inventory, P1.1–P1.4 file:line) |
| `tests/e2e/test_player_micro_contract_compatibility.py` | Modified | remove `global BASE_URL`; thread `base_url`; `api_version == "v1"`; admin auth + Bearer; loud failures; `supports_*` shape asserts |
| `docs/MICHI_LINK_MICRO_E2E.md` | Modified | `:10` `michi_link_version` → `api_version` |
| `openspec/changes/v1-stabilization/tasks.md` | Modified | marked 1.1–1.7 `[x]` |
| `openspec/changes/v1-stabilization/apply-progress.md` | Modified | this artifact (WU0+WU1 merged, corrective retry) |

## Deviations from Design
1. Tasks 1.2/1.3/1.4 landed as one atomic rewrite (same file); each acceptance individually verified.
2. `supports_*` corrected from `== True` to "field present" — spec/design never required `True`; asserting it would fabricate capability (latent 4th fault).

## Issues Found
- Pre-existing flaky Rust tests (parallel race on shared `/tmp/michi-test/music`), not caused by this slice.
- `player_compatibility.supports_*` are misleadingly-named runtime-state booleans.

## Remaining Tasks
Work Units 2–6 (Slices 02–05) not begun — out of scope for this slice.

## Workload / PR Boundary
- Mode: chained PR slice (force-chained / feature-branch-chain)
- Current work unit: 1 (Slice 01 — Audit + authenticated Player contract)
- Boundary: `d00438b` (tracker) → `01-audit-contract` (audit doc + contract test/doc + task/progress marks)
- Review budget (exact `git diff --numstat d00438b..HEAD`): 339 additions + 152 deletions = 491 changed lines (≤ 500)

## Status
9/9 tasks complete in Work Units 0–1 (2 + 7). Work Unit 1 (Slice 01) done. Nothing pushed; no PR created; Work Unit 2 NOT begun.
