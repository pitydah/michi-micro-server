# Stabilization Audit Specification

## Purpose

Define the evidence-backed baseline/audit artifact that proves the Phase 0 stabilization
state at exact HEAD `e6f6dd6`. The artifact is a deliverable, not a claim: every statement
MUST cite reproducible file:line or command evidence.

## Requirements

### Requirement: Evidence-backed audit artifact

The system SHALL produce `docs/V1_STABILIZATION_AUDIT.md` derived from the exploration
baseline (§4/§6/§9). The artifact MUST state the exact audited HEAD (`e6f6dd6`), the
commands executed, their exit codes and results, and MUST cite file:line evidence for each
finding.

#### Scenario: Audit doc cites exact HEAD and baseline results

- GIVEN a stabilization change derived from exact HEAD `e6f6dd6`
- WHEN `docs/V1_STABILIZATION_AUDIT.md` is authored
- THEN it MUST record the exact commit hash, the four green gates (`fmt`/`check`/`test`/`clippy`),
  the test totals (217 passed / 0 failed / 14 ignored), and the Docker smoke result (200 OK)
- AND every finding MUST include a file:line citation

#### Scenario: Audit doc MUST NOT fabricate evidence

- GIVEN a claim not reproducible at exact HEAD `e6f6dd6`
- WHEN the audit doc is authored
- THEN that claim MUST NOT appear as an audited result
- AND any working-tree-only observation MUST be labeled as uncommitted/contaminated context

### Requirement: Severity model with P0=0 and remaining P1 inventory

The artifact MUST apply the user-defined severity model and MUST state that P0 = 0
(no demonstrated data corruption/loss, auth bypass, path/RCE, boot failure, destructive
migration, or unrecoverable queue/DB failure). It MUST list every remaining P1 item
explicitly as unresolved.

#### Scenario: Severity verdict is evidence-backed

- GIVEN the user-defined P0 CRITICAL criteria
- WHEN the severity inventory is written
- THEN P0 MUST be reported as 0 with no exception claimed
- AND each remaining P1 (contract chain, arm64 gap, Docker cache latent, receiver gap)
  MUST be listed with its classification and evidence

### Requirement: Ignored-test inventory

The artifact MUST contain the complete 14-test `#[ignore]` inventory
(`crates/michi-receivers/tests/receiver_simulator_integration.rs`) with the behavior each
test verifies, and MUST NOT claim those tests are passing.

#### Scenario: Ignored tests are inventoried, not claimed green

- GIVEN 14 `#[ignore]` receiver tests
- WHEN the audit doc is written
- THEN each test MUST be enumerated with its verified behavior
- AND the doc MUST state the tests are ignored/unverified, not passing
