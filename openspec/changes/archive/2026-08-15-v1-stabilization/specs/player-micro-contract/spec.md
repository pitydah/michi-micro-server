# Player↔Micro Contract Specification

## Purpose

Make the Player↔Micro contract executable end-to-end and align the contract version
assertion to the authoritative server truth. This domain fixes the broken E2E test and
documents the version-authority decision without introducing any new server field.

## Requirements

### Requirement: Python contract file parses

`tests/e2e/test_player_micro_contract_compatibility.py` MUST parse under
`python3 -m py_compile` with exit code 0. The `global BASE_URL` declaration (line 71,
after `default=BASE_URL` at line 69) SHALL be corrected so the module is importable and
executable.

#### Scenario: Python SyntaxError is eliminated

- GIVEN the current file declares `global BASE_URL` after `BASE_URL` is read as a default
- WHEN `python3 -m py_compile tests/e2e/test_player_micro_contract_compatibility.py` runs
- THEN the command MUST exit 0 (no `SyntaxError`)

#### Scenario: Regression guard for parseability

- GIVEN any future edit to the contract test
- WHEN `python3 -m py_compile` is executed
- THEN a `SyntaxError` MUST fail the gate and MUST NOT be silently skipped

### Requirement: Contract version authority is `api_version` only (Option A)

The contract MUST assert `api_version == "v1"` and MUST NOT assert, read, or add a
`michi_link_version` field. Authority: `crates/michi-link/src/version.rs:3-4` states the
API contract version is solely `api_version`; `crates/michi-api/src/routes/v1/server.rs`
`V1ServerInfo` emits `api_version: "v1"` with no `michi_link_version`; the real client
(`clients/python-michi-client/michi_client.py`) checks only `api_version == "v1"`.

#### Scenario: Phantom contract field removed

- GIVEN the server emits no `michi_link_version` in `/api/v1/server/info`
- WHEN the contract test asserts `info.get("michi_link_version") == "1.0.0-alpha"`
- THEN the assertion MUST be removed and replaced with `api_version == "v1"`

#### Scenario: No server field added

- GIVEN the Option A decision
- WHEN the contract fix is implemented
- THEN `V1ServerInfo` MUST NOT be extended with `michi_link_version`
- AND no server-side code change is permitted for this requirement

#### Scenario: Stale documentation corrected

- GIVEN `docs/MICHI_LINK_MICRO_E2E.md:10` documents `michi_link_version: "1.0.0-alpha"`
- WHEN the contract decision is applied
- THEN that documentation MUST be corrected to reference `api_version: "v1"`

### Requirement: Contract checks real server surface

The executable contract MUST verify: GET `/api/v1/server/info` (public); import preflight in
both new and legacy fixture formats; queue transfer error behavior (400 on empty body); the
`/api/v1/diagnostics` `player_compatibility` block; and playback state shape. All endpoints
except `/api/v1/server/info` are protected by `v1_auth_middleware`
(`crates/michi-api/src/auth.rs:155-197`), which never fails open.

#### Scenario: Server info is public

- GIVEN a running server
- WHEN the test hits GET `/api/v1/server/info` without an Authorization header
- THEN the endpoint MUST return 200
- AND `service == "michi-micro-server"`, `api_version == "v1"`,
  `auth.strategy == "SERVER_CODE"`, `auth.token_refresh == true`, and
  `features.{import,playback,queue} == true` MUST all hold

#### Scenario: Harness authenticates as admin before protected requests

- GIVEN the server is booted with configured admin credentials (`MICHI_AUTH_USERNAME` /
  `MICHI_AUTH_PASSWORD`)
- WHEN the contract harness runs
- THEN it MUST call POST `/api/auth/login` with those credentials
- AND it MUST attach the returned token as `Authorization: Bearer <token>` on every
  protected request (import preflight, queue transfer, diagnostics, playback state)

#### Scenario: Missing or invalid auth fails loudly

- GIVEN no admin credentials are configured, login fails, or a protected request is made
  without a valid token
- WHEN the contract harness runs
- THEN it MUST record a FAIL (never a SKIP) and exit non-zero
- AND a 401 response MUST NOT be treated as a passing contract result

#### Scenario: Import preflight new + legacy formats

- GIVEN an authenticated admin session and fixtures `preflight_new.json` and
  `preflight_legacy.json`
- WHEN POST `/api/v1/import/preflight` is exercised with each
- THEN result fields MUST match the documented shape for each format

#### Scenario: Queue transfer error behavior

- GIVEN an authenticated admin session and an empty `track_ids` body posted to
  POST `/api/v1/queue/transfer`
- WHEN the endpoint responds
- THEN the test MUST accept the documented error status (400) as the expected behavior

#### Scenario: No server field added

- GIVEN the Option A decision
- WHEN the contract is fixed
- THEN `V1ServerInfo` MUST NOT be extended with `michi_link_version`
- AND authentication is test-only; no server-side code change is permitted

### Requirement: Compatibility rationale captured; Option B gated

The design MUST record the compatibility rationale for Option A: zero API break, zero
speculative expansion, real client unaffected. Option B (additive `michi_link_version`)
SHALL remain an explicit unresolved decision, adoptable ONLY if the design phase confirms
a verified external authority (Michi Link v1 spec or a real client) mandating the field.

#### Scenario: Option B is not silently adopted

- GIVEN no verified external authority mandates `michi_link_version`
- WHEN the contract decision is made
- THEN Option B MUST NOT be implemented
- AND the unresolved external-authority gate MUST be recorded for the design phase
