# Toolchain Reproducibility Specification

## Purpose

Define a consistent Rust channel/pin policy so builds are reproducible without claiming
support for a minimum version that has not been tested.

## Requirements

### Requirement: Consistent Rust channel/pin policy

The repository MUST pin a single Rust channel via `rust-toolchain.toml` (stable channel)
and MUST align `RUST_VERSION` in the `Dockerfile` and the CI toolchain action to that same
channel, eliminating drift between local, Docker, and CI builds.

#### Scenario: Single source of toolchain truth

- GIVEN `Dockerfile` pins `RUST_VERSION=1.88` while CI/local use stable 1.96
- WHEN the reproducibility fix is applied
- THEN a `rust-toolchain.toml` MUST be added pinning the stable channel
- AND `RUST_VERSION`/CI toolchain MUST reference the same channel

#### Scenario: Reproducible build across environments

- GIVEN the pinned toolchain is present
- WHEN `cargo build`/`docker build`/CI run
- THEN all three MUST use the same Rust channel and produce consistent results

### Requirement: No unverified MSRV claim

The documentation and metadata MUST NOT claim a Minimum Supported Rust Version (MSRV)
unless that exact version has been built and tested. A channel pin is NOT an MSRV claim.

#### Scenario: MSRV is not asserted without testing

- GIVEN no older Rust version has been tested in this increment
- WHEN the toolchain policy is documented
- THEN the docs MUST NOT state an MSRV (e.g. "supports Rust 1.88+")
- AND the pin MUST be described as "reproducible channel" rather than a compatibility floor
