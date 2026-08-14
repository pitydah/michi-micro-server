# Proposal: v1-stabilization — Phase 0 Baseline + Phase 1 CI/Build Integrity

## Intent

Stabilize `michi-micro-server` toward a first honest v1 pre-release **without publishing v1.0**. Baseline at exact `HEAD e6f6dd6` is green (`fmt`/`check`/`test`/`clippy`, 217 pass/14 ignored, Docker smoke 200) with **P0 = 0**. This increment closes the four P1 release-gate/verification blockers and a minimal P2 truthfulness set so the project can say "audited, contract-executable, CI-gated, arm64-config-validated" with evidence — not claimed runtime guarantees.

**First increment = Phase 0 (audit + Player↔Micro contract repair) + Phase 1 (CI contract gate, build/release integrity, receiver runner repair).** Receiver CI *enablement* (external simulator), hygiene sweep, coverage gate, and v1.0 tagging are **future gates**, not this scope.

## Authoritative constraints (binding)

- Feature freeze, audio-only, **no new features**, no mass rewrite, no cloud/SaaS dependency, no silent API break, reasonable legacy compatibility.
- Primary targets: Raspberry Pi/arm64, mini PC, CasaOS/ZimaOS, Linux, Docker, LAN/Tailscale, 24/7.
- **Isolation**: main working tree carries uncommitted WebUI/PWA/design work (`.gitignore`, `Dockerfile`, `crates/michi-api/src/{lib,pwa,static_files}.rs`, WebUI statics, `tests/api.rs`, `openspec/`, `static/assets/*`). All stabilization work happens in **detached worktrees from exact `e6f6dd6`**. The WebUI change is separate. **Do NOT absorb, overwrite, stage, or depend on those uncommitted files** — including the working-tree `Dockerfile` mtime fix (re-derive it cleanly from HEAD instead).
- Each slice independently testable and revertible; forecast ≤500 changed lines; no branch/PR/commit/push yet (chain strategy not selected).

## Severity baseline (corrected, evidence-backed)

| ID | Severity | Finding (root cause) |
|----|----------|----------------------|
| P1.1 | P1 release-gate | Player↔Micro contract chain broken: `tests/e2e/test_player_micro_contract_compatibility.py:69` `default=BASE_URL` vs `:71 global BASE_URL` → `SyntaxError`; `:84` asserts `michi_link_version` never emitted; no Python tests in CI. Three faults: syntax + contract field + CI wiring. |
| P1.2 | P1 deployment | GHCR release `ci.yml:95` `platforms: linux/amd64` only; CasaOS declares `[amd64, arm64]`. |
| P1.3 | P1 latent | `Dockerfile` dummy-source step has no mtime invalidation on HEAD; working-tree fix exists uncommitted (must re-derive from HEAD). Not reproduced on exact HEAD (fresh uniform-mtime checkout smoke 200). |
| P1.4 | P1 verification | 14 `#[ignore]` receiver tests need external `receiver_sim.py`; `scripts/test_receiver_e2e.sh:43` runs `cargo test --test ...` from virtual-workspace root (needs `-p michi-receivers`); sim paths machine-specific; two dead duplicate test files never compiled. |
| P2.x | P2 | CasaOS `data.yml` 0.1.0 vs 0.2.0; CHANGELOG "35 migraciones" vs 37; toolchain pin drift; dead auth/config code; no audit/coverage. |

## Contract field decision (P1.1 fork — surfaced, NOT silently resolved)

Evidence on both sides:

- **Code truth**: `crates/michi-link/src/version.rs:4` — *"There is no `michi_link_version` — the API contract version is solely `api_version`."* `V1ServerInfo` (`crates/michi-api/src/routes/v1/server.rs:7-17`) emits `api_version: "v1"` (`:51`), no `michi_link_version`. The real client `michi_client.py` checks `api_version == "v1"` only.
- **Doc/test claim**: `docs/MICHI_LINK_MICRO_E2E.md:10` documents `michi_link_version: "1.0.0-alpha"`; the E2E test asserts it. (WebUI `app.js:410` also reads it, but `app.js` is uncommitted/contaminated — not authoritative.)

**Recommendation (default): Option A — align test + docs to `api_version`.** Assert `api_version == "v1"`, remove the phantom `michi_link_version` assert, correct `MICHI_LINK_MICRO_E2E.md:10`. No server change → zero API break, zero speculative expansion. **Option B (fallback): add `michi_link_version` as an additive field** if design/spec finds an external Michi Link v1 authority or mobile client that mandates it (additive = non-breaking). **Gate: the design phase MUST confirm the external contract before the field is either dropped from docs or added to the server.** Not resolved by this proposal alone.

---

## Slice plan (first increment — force-chained; each independently testable/revertible)

### Slice 1 — Phase 0 audit deliverable + Player↔Micro contract repair

- **Problem/root cause**: P1.1 syntax fault (verified above); no evidence-backed audit artifact exists.
- **In scope**: author `docs/V1_STABILIZATION_AUDIT.md` derived from exploration §4/§6/§9 evidence (baseline table, 14 ignored-test inventory, P1/P2/P3 inventory, isolation note); fix Python `global BASE_URL` SyntaxError; apply contract-field decision (Option A default: assert `api_version == "v1"`); correct `MICHI_LINK_MICRO_E2E.md:10`; record manual E2E regression PASS against a local instance on a scratch port.
- **Out of scope**: any server-side field addition unless design flips to Option B; CI wiring (Slice 2); WebUI.
- **Acceptance**: `docs/V1_STABILIZATION_AUDIT.md` exists and cites file:line evidence; `python3 -m py_compile` passes; E2E runs green against local server (log captured); `cargo test --workspace` still green (no Rust change).
- **Rollback**: revert the two files (test + doc) and delete the audit doc; no server/schema state.
- **Dependencies**: none (reads HEAD).
- **Forecast**: ~210–250 lines (audit doc ~200, syntax ~4, assert/doc ~2–15).
- **Feature-freeze impact**: none.

### Slice 2 — CI contract gate (boot server + run Player contract)

- **Problem/root cause**: P1.1 CI leg missing — contract verification is dead in CI.
- **In scope**: add `ci-python-contract` job (needs `ci-rust`); build binary, boot on a scratch port with deterministic cleanup (trap + `--port` override), run the contract test. Deterministic ports and teardown; keep `ci-rust`/`ci-docker` green.
- **Out of scope**: receiver sim CI (Slice 4/future gate); arm64 publish (Slice 3).
- **Acceptance**: job present and green in CI; local equivalent (boot + script) documented with exit code; no port collisions (ephemeral port or explicit override + cleanup).
- **Rollback**: remove the job from `ci.yml`.
- **Dependencies**: Slice 1 (test must parse + assert correctly).
- **Forecast**: ~55–70 lines.
- **Feature-freeze impact**: none.

### Slice 3 — Build/release integrity (Docker cache bug + toolchain + arm64)

- **Problem/root cause**: P1.3 dummy-source step never invalidates mtimes (can ship no-op placeholder); P2.2 `RUST_VERSION=1.88` vs stable 1.96, no `rust-toolchain.toml`; P1.2 arm64 never published.
- **In scope**: re-derive (not absorb) the mtime-invalidation fix in `Dockerfile` (rm placeholder + `touch` before final `cargo build`); add `rust-toolchain.toml` (stable) and align `RUST_VERSION`; set `release-ghcr` `platforms: linux/amd64,linux/arm64`; add an arm64 build/emulation smoke qualification step.
- **Out of scope**: claiming runtime Pi qualification from QEMU build (explicitly disallowed — QEMU proves build+emulated boot only); runtime arm64 hardware testing (future gate).
- **Acceptance**: Docker build + smoke green locally; arm64 image builds under `buildx`/QEMU and reports emulated boot only (honest wording); `release-ghcr` matrix `linux/amd64,linux/arm64`.
- **Rollback**: revert `Dockerfile`, `ci.yml`, delete `rust-toolchain.toml`; no tag published.
- **Dependencies**: none hard; benefits from Slice 1/2 staying green.
- **Forecast**: ~40–60 lines.
- **Feature-freeze impact**: none.

### Slice 4 — Receiver verification pipeline: runner repair + dedup (CI enablement split)

- **Problem/root cause**: P1.4 — `test_receiver_e2e.sh:43` needs `-p michi-receivers`; `run_receiver_sim_*.sh:5` machine-specific default; two dead duplicate files (`tests/receiver_simulator_integration.rs`, `tests/e2e/test_receiver_simulator_integration.rs`) never compiled; 14 ignored tests unverifiable without external `receiver_sim.py`.
- **In scope**: fix runner (`-p michi-receivers`, both sims, `-- --ignored`); env-based sim paths (keep overridable default); delete the two dead duplicates; document the simulator boundary + acquisition truthfully.
- **Out of scope**: CI enablement of the receiver suite (requires external `pitydah/michi-music-stream` simulator — **explicit future gate, not faked green**); vendoring/containerizing the simulator now.
- **Split note (honest)**: "runner repair + dedup" (self-contained, in-repo) is separated from "CI enablement" (external dependency). The dead-file deletion is ~610 lines of pure removal — recommend landing dedup as its own minimal commit/slice so no authored slice exceeds ~40 lines.
- **Acceptance**: runner invokes the correct target and fails gracefully with a clear "simulator not running" message; dead duplicates removed; 14-test inventory truthful (already in exploration).
- **Rollback**: restore the two deleted files, revert script edits.
- **Dependencies**: none (no sim needed to land the repair).
- **Forecast**: ~40 authored lines + ~610 deletion-only lines (flag: split dedup).
- **Feature-freeze impact**: none.

### Slice 5 — Minimal P2 metadata/docs alignment (truthful first delivery only)

- **Problem/root cause**: P2.1 CasaOS `casaos/data.yml:2` 0.1.0 vs 0.2.0; P2.5 CHANGELOG "35 migraciones" vs 37.
- **In scope**: bump `data.yml` version; correct CHANGELOG migration count. **Only** these two truthfulness items.
- **Out of scope**: icon/screenshots, toolchain pin (Slice 3), dead auth/config code, audit/coverage, migration reordering (all later gates).
- **Acceptance**: metadata version matches workspace; CHANGELOG count correct; no build/test impact.
- **Rollback**: revert two files.
- **Dependencies**: none.
- **Forecast**: ~10–20 lines.
- **Feature-freeze impact**: none.

---

## Program success (exact, conservative — first delivery)

- [ ] `docs/V1_STABILIZATION_AUDIT.md` exists and is evidence-backed (file:line citations).
- [ ] `fmt` / `check` / `test` / `clippy` green at the integration commit (regression guard).
- [ ] Docker build + smoke green.
- [ ] Player↔Micro contract executable (parses) and gated in CI.
- [ ] 14 ignored receiver tests inventoried; runner-repair status reported truthfully (CI enablement NOT claimed).
- [ ] ARM64 build/release configuration validated (emulated build/boot) **without claiming hardware runtime**.
- [ ] P0 = 0; remaining P1 items listed explicitly (arm64 hardware runtime proof; receiver CI enablement).
- [ ] No v1.0 tag published.

## Versioning stance

Do **not** publish v1.0. Next pre-release tag is a design/spec decision (candidates `v1.0.0-alpha.1` vs `v0.3.0-beta`); `latest` auto-enables only for non-prerelease tags. **Not chosen here** — deferred to design/spec evidence.

## Capabilities (spec contract)

- **New Capabilities**: None (feature freeze — no new user-facing capability).
- **Modified Capabilities**: None at spec level for this increment. If the contract-field decision lands on **Option B** (additive `michi_link_version`), the design phase must emit a delta spec for the `server-info` response contract. (Note: `openspec/specs/` is currently empty.)

## Affected areas

| Area | Impact | Description |
|------|--------|-------------|
| `docs/V1_STABILIZATION_AUDIT.md` | New | evidence-backed audit deliverable |
| `tests/e2e/test_player_micro_contract_compatibility.py` | Modified | syntax + contract assert |
| `docs/MICHI_LINK_MICRO_E2E.md` | Modified | phantom field correction |
| `.github/workflows/ci.yml` | Modified | python contract job + arm64 platforms |
| `Dockerfile` | Modified | mtime invalidation + toolchain |
| `rust-toolchain.toml` | New | reproducible toolchain |
| `scripts/test_receiver_e2e.sh`, `scripts/run_receiver_sim_*.sh` | Modified | runner/path repair |
| `tests/receiver_simulator_integration.rs`, `tests/e2e/test_receiver_simulator_integration.rs` | Removed | dead duplicates |
| `casaos/data.yml`, `CHANGELOG.md` | Modified | version/count truthfulness |

## Risks

| Risk | Likelihood | Mitigation |
|------|------------|------------|
| Working-tree WebUI changes absorbed into slices | Med | detached worktrees from `e6f6dd6`; re-derive Dockerfile fix; never touch `static/`, `pwa.rs`, WebUI statics |
| Contract-field decision wrong (breaks external mobile client) | Low-Med | design-phase gate; Option A removes nothing server-side; Option B additive fallback |
| arm64 emulated boot misread as hardware qualification | Med | explicit wording: QEMU = build+emulated boot only, not runtime proof |
| Receiver CI "faked green" | Med | split runner-repair from CI-enablement; CI enablement = future gate |
| Chain-strategy ambiguity stalls delivery | Med | strategy selected before apply (pending); slices are independent work units |

## Rollback plan (global)

Per-slice rollback above. Integration rollback = revert the slice commit; slices are independently revertible and land in order (1→2→3→4→5) with no cross-slice schema/state coupling. No DB migration, no published tag, no remote state mutated in this increment.

## Dependencies

- Exact HEAD `e6f6dd6` (authoritative baseline) + detached worktrees.
- External receiver simulator (`pitydah/michi-music-stream`) — **only** for the deferred receiver-CI future gate.
- `buildx` + QEMU (already present in CI) for arm64 emulation build.

## Delivery strategy

`force-chained` per session contract; **chain strategy (stacked vs feature-branch) NOT yet selected** — no branches/PRs/commits/pushes in this phase. Slices map 1:1 to work units (work-unit-commits: tests/docs land with the behavior they verify).

### Slice changed-line forecasts

| Slice | Authored additions | Deletions (dead files) | Total raw | Budget (≤500) |
|-------|-------------------|------------------------|-----------|----------------|
| 1 Audit + contract | ~210–250 | 0 | ~230 | ✅ |
| 2 CI contract gate | ~55–70 | 0 | ~65 | ✅ |
| 3 Build/release | ~40–60 | 0 | ~50 | ✅ |
| 4 Receiver repair | ~40 | ~610 | ~650 | ⚠️ split dedup |
| 5 P2 metadata | ~10–20 | 0 | ~15 | ✅ |

### Review workload warning

Total authored additions ≈ 360–440 lines; total raw diff ≈ **~1000 lines** (dominated by Slice 4 dead-file deletion). This is **above the 400-line single-PR threshold → force-chained into ~5 review slices** (each ≤500 except Slice 4, which must split dedup). Reviewer load ≈ 5 focused reviews; recommend resolving the chain strategy and Slice 4 dedup split **before** `sdd-tasks`.
