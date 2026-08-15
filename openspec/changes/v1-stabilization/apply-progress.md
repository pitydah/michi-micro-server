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

| Evidence | Required value | Actual |
|---|---|---|
| Focused test command and exact result | Smallest command proving this unit | `git -C /home/cristian/.cache/michi-v1s/wt-tracker rev-parse HEAD` → `e6f6dd6f1b043ba9483614027bee2284586cf4ef`; `git -C wt-tracker status --porcelain` → empty before and after commit |
| Runtime harness command/scenario and exact result | N/A when no runtime boundary exists | N/A — worktree + planning-artifact setup only |
| Rollback boundary | Exact files/behavior revertible | `git worktree remove wt-tracker --force`; nothing pushed; only `openspec/` paths staged |

### Work Unit 1

| Evidence | Required value | Actual |
|---|---|---|
| Focused test command and exact result | Smallest command proving this unit | `python3 -m py_compile tests/e2e/test_player_micro_contract_compatibility.py` → exit 0; `grep -n "global BASE_URL"` → empty; `grep -n "michi_link_version"` → empty |
| Runtime harness command/scenario and exact result | Real integration/runtime path | `cargo build --release -p michi-server` → exit 0; boot on scratch port 18096 with throwaway admin (`MICHI_AUTH_USERNAME/PASSWORD`); contract run `--url http://127.0.0.1:18096` → exit 0, `CONTRACT: OK`, 31 passed/0 failed; failure paths: missing creds → exit 1, wrong password → exit 1 (login 401 recorded), protected endpoint without token → 401; server SIGTERM → WAL checkpoint + shutdown complete, port 18096 freed, temp state removed |
| Rollback boundary | Exact files/behavior revertible | Revert `tests/e2e/test_player_micro_contract_compatibility.py` + `docs/MICHI_LINK_MICRO_E2E.md`; delete `docs/V1_STABILIZATION_AUDIT.md`; no server/schema/state change |

## Evidence Log

### Work Unit 0 (tracker + planning)

- Main worktree baseline manifest: `/tmp/opencode/michi-v1s/main-baseline.manifest` — proves WebUI/PWA/openspec uncommitted work byte/path/mode identity; re-hashed identical after work.
- `git worktree add -b feat/v1-stabilization /home/cristian/.cache/michi-v1s/wt-tracker e6f6dd6f1b043ba9483614027bee2284586cf4ef` → HEAD at `e6f6dd6`.
- Staged via exact paths (`git add openspec/config.yaml openspec/changes/v1-stabilization`), 15 paths, no WebUI/static/tests/api.rs.
- Commit `docs(openspec): track v1-stabilization planning artifacts` → `d00438b8659736df42630738ae076e8a24e51e59`.

### Work Unit 1 (Slice 01)

- Child worktree `git worktree add -b 01-audit-contract /home/cristian/.cache/michi-v1s/wt-01 d00438b8659736df42630738ae076e8a24e51e59` → HEAD `d00438b…`, clean, branch `01-audit-contract`.
- Contract test pre-state reproduced: `python3 -m py_compile` → `SyntaxError: name 'BASE_URL' is used prior to global declaration` (exit 1).
- After repair: py_compile exit 0; `global BASE_URL` gone; `michi_link_version` gone; canonical `api_version == "v1"` assert present.
- No server change: `git diff -- crates/michi-api crates/michi-link` empty.
- Discovery (latent 4th fault): the original test also asserted `player_compatibility.supports_* == True`, but those fields are runtime state (`active_import_sessions > 0`, `total_queues > 0` — `diagnostics.rs:451-456`), legitimately `false` on a fresh server. Corrected to assert block shape (fields present) + `contract_status` valid — honest, no fabricated `True`.
- Release build: `CARGO_TARGET_DIR=/home/cristian/.cache/michi-v1s/target cargo build --release -p michi-server` → exit 0 (1m50s).
- Real-server harness: boot on port 18096 with `MICHI_CONFIG_PATH/CACHE_PATH/MUSIC_PATH` isolated under `/tmp/opencode/michi-v1s/wt01-harness`, `MICHI_DATABASE=sqlite:///tmp/.../michi.db`, throwaway admin. `/health/live` → 200 in 2s.
- Contract happy path: exit 0, `CONTRACT: OK`, 31 passed / 0 failed.
- Failure paths: missing creds → exit 1 (`0 passed/1 failed`); wrong password → login 401 recorded FAIL + exit 1; protected endpoints without token → 401 (never fails open).
- Server terminated via SIGTERM → "WAL checkpoint complete" + "shutdown complete"; port 18096 freed; temp dir removed.
- Regression: `cargo fmt --check` exit 0; `cargo check --workspace` exit 0; `cargo clippy --workspace --all-targets -- -D warnings` exit 0; `cargo test --workspace` exit 0 — **217 passed / 0 failed / 14 ignored** (two consecutive green runs).
- Flake note: first parallel `cargo test --workspace` run failed 2 order-dependent tests (`test_v1_commit_returns_mapping`, `test_v1_import_commit_returns_mapping_with_status`) due to a shared fixed path `/tmp/michi-test/music` (`crates/michi-api/tests/api.rs:31`) racing under parallel execution; both pass in isolation and on clean re-runs. Pre-existing, unrelated to this slice (no Rust change).
- Review budget: 343 additions + 111 deletions = **454 changed lines** (≤ 500). Files: audit doc (151), contract test (191+/110−), E2E doc (1+/1−), plus tasks.md/apply-progress.md marks.

## Files Changed

| File | Action | What was done |
|---|---|---|
| `docs/V1_STABILIZATION_AUDIT.md` | Added | evidence-backed Phase 0 baseline audit (HEAD, 4 gates, 217/0/14, Docker 200 + 37 migrations, P0=0, 14-test ignored inventory, P1.1–P1.4 file:line, contamination note) |
| `tests/e2e/test_player_micro_contract_compatibility.py` | Modified | removed `global BASE_URL`; threaded `base_url` explicitly; `api_version == "v1"` assert; admin login via `MICHI_AUTH_USERNAME/PASSWORD`; Bearer on protected requests; loud non-zero failures; corrected `supports_*` shape assertions |
| `docs/MICHI_LINK_MICRO_E2E.md` | Modified | `:10` `michi_link_version: "1.0.0-alpha"` → `api_version: "v1"` |
| `openspec/changes/v1-stabilization/tasks.md` | Modified | marked 1.1–1.7 `[x]` |
| `openspec/changes/v1-stabilization/apply-progress.md` | Modified | this artifact (WU0 + WU1 merged) |

## Deviations from Design

1. Tasks 1.2/1.3/1.4 (all in `tests/e2e/test_player_micro_contract_compatibility.py`) landed as one atomic file rewrite (syntax + version assert + auth flow) rather than three sequential commits — the file was changed as a single coherent "make the contract executable and correct" unit; each task's acceptance evidence is individually verified.
2. Diagnostics `supports_*` assertions corrected from `== True` to "field present" — the spec/design never required `True`; the fields are runtime state, so asserting `True` would fabricate a capability. Recorded as a latent 4th fault.

## Issues Found

- Pre-existing flaky Rust tests (parallel race on shared `/tmp/michi-test/music`). Not caused by this slice (no Rust change); documented honestly.
- The `player_compatibility.supports_*` fields are misleadingly named runtime-state booleans, not capability flags.

## Remaining Tasks

Work Units 2–6 (Slices 02–05) not begun — out of scope for this slice.

## Workload / PR Boundary

- Mode: chained PR slice (force-chained / feature-branch-chain)
- Current work unit: 1 (Slice 01 — Audit + authenticated Player contract)
- Boundary: `d00438b` (tracker) → `01-audit-contract` carrying audit doc + contract test/doc + task/progress marks
- Review budget impact: 454 changed lines (≤ 500)

## Status

9/9 tasks complete in Work Units 0–1 (2 + 7). Work Unit 1 (Slice 01) done. Nothing pushed; no PR created; Work Unit 2 NOT begun.
