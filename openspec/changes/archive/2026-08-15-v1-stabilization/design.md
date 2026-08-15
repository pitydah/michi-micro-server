# Design: v1-stabilization - Phase 0 + Phase 1

Baseline `e6f6dd6`: Rust gates green, 217 pass / 0 fail / 14 ignored, Docker 200, P0=0. Scope: Phase 0 + Phase 1 only; no implementation, UI, or v1.0 tag. Delivery: five authored feature-branch slices plus one pure-deletion slice.

## Decisions

| # | Decision | Choice | Rejected | Spec |
|---|---|---|---|---|
| 1 | Contract authority | Option A: `api_version` only; zero server change, API break, or speculative expansion; real client unaffected | Add `michi_link_version` without verified external authority | player-micro-contract |
| 2 | Isolation | Feature-branch chain and detached worktrees from `e6f6dd6` | Stacked PRs | stabilization-isolation |
| 3 | Slices | 01 audit+contract; 02 CI; 03 Docker/toolchain/ARM64; 04a dedup; 04b runner; 05 metadata | Single PR | stabilization-isolation |
| 4 | Harness | `main(base_url, token)`, configured throwaway admin login, no global mutation | Unauthenticated protected calls | player-micro-contract, ci-contract-gate |
| 5 | CI | `ci-python-contract` needs `ci-rust`; cache-warm build; `scripts/ci_contract_gate.sh` | Docker gate | ci-contract-gate |
| 6 | Docker | COPY overwrites placeholders, then `touch` sources/assets; smoke asserts version and lifecycle | Global clean | docker-build-integrity |
| 7 | Toolchain | Pin `1.96.0` in rust-toolchain.toml, Docker, and CI; proven on four Rust gates, not an MSRV claim | Moving `stable`; `1.88` | toolchain-reproducibility |
| 8 | ARM64 | Publish `amd64,arm64`; prereleases never publish or update `latest`; emulated boot is local evidence only | Flaky CI QEMU gate | multiarch-release |
| 9 | Receiver | Runner uses `-p michi-receivers -- --ignored`, env paths, and loud graceful failure; no CI job | Fake green | receiver-runner |
| 10 | Audit/metadata | Record command exit codes, file:line evidence, and every ignored test; `data.yml` 0.2.0; CHANGELOG 37 | Invented evidence | stabilization-audit, metadata-truthfulness |

`version.rs:3-4`, `server.rs:8-17`, and `michi_client.py:106-110` establish `api_version == "v1"`; Option B requires verified external authority.

## Topology and isolation

```
main e6f6dd6 -> feat/v1-stabilization (draft tracker -> main)
  -> 01-audit-contract -> 02-ci-contract -> 03-build-release
  -> 04a-receiver-dedup -> 04b-receiver-runner -> 05-metadata
```

Use detached worktrees at `/home/cristian/.cache/michi-v1s/wt-NN`, shared `CARGO_TARGET_DIR`, and read-only main. OpenSpec lands on the clean tracker before children by exact-path staging of `openspec/config.yaml` and `openspec/changes/v1-stabilization/**`; never use `git add openspec/` or `git add -A`, preventing dirty WebUI contamination. Re-derive Dockerfile changes from HEAD. Restack with `git rebase <parent>`; rollback with `git revert`; shared 02/03 `ci.yml` requires the chain.

Auth-gated routes are under `v1_auth_middleware`, which never fails open. The harness MUST execute this ordered sequence:

1. GET public `/api/v1/server/info` with no `Authorization` header; require 200 and `api_version == "v1"` plus service, auth, and feature fields.
2. POST `/api/auth/login` with configured throwaway admin credentials.
3. Extract the returned token and require it to be present.
4. Send `Authorization: Bearer <token>` on every protected import, queue, diagnostics, and playback request.

Missing credentials, login failure, absent token, invalid token, or any protected `401` MUST record FAIL and exit non-zero; NEVER SKIP or pass. Exercise new and legacy import preflight fixtures, queue transfer empty-body 400, diagnostics `player_compatibility`, and playback state shape.

## CI flow

```text
cache-warm cargo build -> start server with isolated temp paths
-> poll public /health/live (60s) -> ordered auth harness
-> capture log -> terminate server -> remove temp directory
```

Use public `/health/live`, never protected `/api/status`. Require an executable server, `set -euo pipefail`, 5-second request timeouts, per-run password, failure log, and EXIT cleanup.

## Files

Create `docs/V1_STABILIZATION_AUDIT.md`, `scripts/ci_contract_gate.sh`, and `rust-toolchain.toml`; modify the contract test, E2E docs, CI workflow, Dockerfile, receiver scripts, and metadata; delete duplicate Rust/Python receiver integration tests.

## Verification and rollback

| Slice | Verification | Rollback |
|---|---|---|
| 01 | Python compile 0; authenticated contract OK; Rust green | Revert test/docs; delete audit |
| 02 | CI contract job green; local exit 0; auth failures non-zero | Remove job/script |
| 03 | Docker version 0.2.0; exactly 37 migrations; root and health 200; container healthy; observable WAL checkpoint and clean termination; ARM64 emulated boot 200; container and image cleanup | Revert Docker/CI; delete toolchain file |
| 04a | Duplicate files gone; Rust 217/0/14 | Restore files |
| 04b | Green with simulators; clear non-zero without them | Revert scripts |
| 05 | `data.yml` 0.2.0; CHANGELOG 37 | Revert metadata |

Authored units are <=500 lines. Slice 04a is the permitted ~610-line pure-deletion exception.

## Threats and recovery

Classification/execution applies: runner and harness verify files/binaries before execution; missing inputs fail non-zero, never skip. Git/PR automation is N/A: operator-run. Revert slices in reverse order; no data migration is required.

## Open questions

- Option B external authority remains gated and deferred.
- Prerelease tagging is deferred; this increment creates no tag.
