# Michi Micro Server — Release Process & Train (v1.0.0-rc.1 -> v1.0.0 GA)

This document establishes the release governance, qualification gates, and branch protection criteria for Michi Micro Server.

## 1. Release Train Milestones

### 1.1 Release Candidate (`v1.0.0-rc.1`)
A Release Candidate certifies that the software is feature-frozen, contractually truthful, reproducible, and passes all automatable quality gates:
- **Rust Quality**: `cargo fmt --check`, `cargo check --workspace`, `cargo test --workspace`, and `cargo clippy --workspace --all-targets -- -D warnings`.
- **Ecosystem & Simulators**: Stream contract drift, receiver contract simulator, multi-room mock, Home Assistant simulator, and three-way integration passing.
- **Real Daemons**: Native Mosquitto integration passing.
- **Packaging & Ports**: Canonical internal port 9090, env-aware healthcheck, docker compose passing, and ZimaOS/CasaOS distribution verified.
- **Resource Budget**: Real Linux measurement certifying idle RAM < 50MB and bounded thread allocation.
- **Release Gate Aggregator**: `scripts/generate_release_gate.py --mode rc --check` evaluates valid, current-commit artifacts with zero hardcoded statuses.

### 1.2 General Availability (`v1.0.0` GA)
GA requires all RC criteria plus real-world external validations:
- **Physical Hardware**: Execution and qualification on physical Raspberry Pi 4/5 board (`scripts/test_appliance_e2e.sh`).
- **Real Appliance OS**: Native installation and upgrade via CasaOS / ZimaOS App Store on real appliance hardware.
- **24-Hour Stability Soak**: Continuous uninterrupted 24h soak execution (`scripts/soak_test.py`) with zero zombie processes and bounded drift.
- **Native Snapserver**: Real Snapserver daemon integration test (`scripts/test_snapserver_real_e2e.sh`).

---

## 2. GitHub Branch Protection & Required Checks

The `main` branch must enforce required status checks before merging or tagging releases:

```text
ci-rust
ci-stream-contract
ci-receiver-contract-simulator
ci-snapcast-contract-mock
ci-mqtt-contract-simulator
ci-mosquitto-real
ci-generic-linux-appliance
ci-reliability-short
ci-web-ui-functional-integrity
ci-web-ui-browser-e2e
ci-three-way-ecosystem
ci-docker-amd64
ci-arm64-qemu
ci-zimaos-package
ci-release-gate
```

---

## 3. Versioning Invariants
1. **Workspace Version**: `Cargo.toml` is the single source of truth for the software SemVer (`1.0.0-rc.1`).
2. **Store Schema Version**: ZimaOS / CasaOS store catalog format version (`STORE_SCHEMA_VERSION = 3.1`) is strictly decoupled from the application software version.
3. **Container Tags**: Official multi-arch container images published to GHCR match the exact tag (e.g. `1.0.0-rc.1`). The `latest` tag is updated **only** upon final GA release (`v1.0.0`).
