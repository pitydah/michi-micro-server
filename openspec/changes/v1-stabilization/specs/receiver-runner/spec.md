# Receiver Runner Specification

## Purpose

Repair the receiver verification runner so it invokes the correct crate target, accepts
configurable simulator paths/URLs, fails gracefully when the external simulator is
unavailable, and removes dead duplicate test files — without claiming CI enablement.

## Requirements

### Requirement: Correct package and ignored invocation

`scripts/test_receiver_e2e.sh` MUST invoke the receiver integration tests with the correct
package (`-p michi-receivers`), for both simulator targets, passing `-- --ignored` so the
14 ignored tests actually run.

#### Scenario: Runner targets the correct crate

- GIVEN the script runs `cargo test --test receiver_simulator_integration` from the
  virtual-workspace root
- WHEN the repair is applied
- THEN the invocation MUST include `-p michi-receivers`
- AND MUST run both standard and Hi-Fi simulators with `-- --ignored`

### Requirement: Configurable simulator paths/URLs

Simulator locations MUST be overridable via environment variables (e.g. the sim path and
`MICHI_RECEIVER_SIM_URL`/`MICHI_RECEIVER_SIM_HIFI_URL`), with the machine-specific default
retained only as a fallback. The runner MUST NOT hard-require a single machine path.

#### Scenario: Simulator path overridable

- GIVEN a CI or alternate machine without `/home/cristian/michi-music-stream/...`
- WHEN the environment variables are set to valid simulator locations
- THEN the runner MUST use those locations and NOT the hardcoded default

### Requirement: Clear unavailable-dependency failure

When the external `receiver_sim.py` simulator is not running, the runner MUST fail with a
clear "simulator not running/unavailable" message. It MUST NOT hang, silently pass, or
obscure the dependency.

#### Scenario: Simulator missing

- GIVEN `receiver_sim.py` is not running at the configured URL/path
- WHEN the runner executes
- THEN it MUST exit non-zero with an explicit simulator-unavailable message
- AND the 14 tests MUST NOT be reported as passing

### Requirement: Dead duplicates removed

The two dead duplicate test files (`tests/receiver_simulator_integration.rs` and
`tests/e2e/test_receiver_simulator_integration.rs`), which are never compiled, MUST be
removed. The authoritative copy MUST remain `crates/michi-receivers/tests/...`.

#### Scenario: Duplicate files are gone

- GIVEN two root-level duplicate test files drifted from the compiled crate copy
- WHEN the dedup is applied
- THEN both dead files MUST be deleted
- AND the compiled crate copy MUST remain untouched

### Requirement: CI enablement remains future

The receiver suite MUST NOT be wired into CI in this increment. CI enablement SHALL remain
an explicit future gate pending a reproducible simulator (e.g. containerized
`pitydah/michi-music-stream`).

#### Scenario: Receiver CI is not faked green

- GIVEN the simulator is external and not reproducible in this increment
- WHEN CI configuration is reviewed
- THEN no `ci-receivers` job MUST be added
- AND the runner-repair status MUST be reported truthfully as "repaired, CI enablement deferred"
