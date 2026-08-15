# CI Contract Gate Specification

## Purpose

Define the semantics of the CI job that boots a real server and runs the Player↔Micro
contract test. The gate MUST exercise a genuine process, wait for health, propagate
failure, and tear down deterministically without port leakage or hidden skips.

## Requirements

### Requirement: Real server process boot on isolated writable path

The CI job MUST build the `michi-server` binary and boot a real server process on an
isolated writable path (dedicated data/config directory), using a deterministic,
collision-free port (ephemeral allocation or explicit `--port` override). It SHALL NOT
depend on a pre-existing server or a shared database.

#### Scenario: Server boots on a deterministic port

- GIVEN the `ci-python-contract` job (needs `ci-rust`) builds the binary
- WHEN the job starts the server with an explicit or ephemeral port
- THEN a real server process MUST be listening on that port before the contract runs

### Requirement: Health wait before contract execution

The job MUST wait for the server to report healthy (e.g. `/health/live` or `/api/v1/status`
200) before invoking the contract test, with a bounded timeout.

#### Scenario: Server never becomes healthy

- GIVEN the server fails to start or report healthy
- WHEN the health wait exceeds its bounded timeout
- THEN the job MUST fail with a clear health-wait error
- AND the contract test MUST NOT run against a dead server
- AND failure MUST propagate to the CI result (non-zero exit)

### Requirement: Failure propagation

Any non-zero exit from the server boot, health wait, or contract test MUST fail the job.
The job MUST NOT treat contract failure as a warning or neutral outcome.

#### Scenario: Contract failure fails the gate

- GIVEN the contract test reports `FAIL > 0`
- WHEN the job evaluates the result
- THEN the job MUST exit non-zero and mark the CI check red

### Requirement: Deterministic teardown on success and failure

The job MUST terminate the server process and release its port in all cases, including
when the contract test fails or the health wait times out (trap/cleanup on failure).

#### Scenario: Cleanup runs on test failure

- GIVEN the contract test fails after the server has booted
- WHEN the job exits
- THEN the cleanup handler MUST stop the server and free the port
- AND no orphaned server process or leaked port MUST remain

#### Scenario: No port leakage across runs

- GIVEN repeated job runs
- WHEN each run allocates and releases its port
- THEN consecutive runs MUST NOT collide on a lingering port

### Requirement: No hidden skip

The job MUST NOT silently skip the contract test. Any skipped check (e.g. missing Python
interpreter, missing server binary) MUST fail the job loudly rather than pass.

#### Scenario: Missing prerequisite fails loudly

- GIVEN the Python interpreter or server binary is absent in the CI environment
- WHEN the job runs
- THEN the job MUST fail with an explicit prerequisite error, not pass as "skipped"
