# Multiarch Release Configuration Specification

## Purpose

Configure the GHCR release pipeline for `linux/amd64` + `linux/arm64` while keeping the
truthfulness boundary: QEMU proves build and emulated boot only, never Raspberry Pi
hardware qualification, and prerelease tags never publish `latest`.

## Requirements

### Requirement: amd64 + arm64 release platforms

The `release-ghcr` job MUST build and publish both `linux/amd64` and `linux/arm64` images,
matching the CasaOS/ZimaOS declared architectures.

#### Scenario: Multiarch matrix

- GIVEN the `release-ghcr` job currently sets `platforms: linux/amd64`
- WHEN the multiarch fix is applied
- THEN the job MUST set `platforms: linux/amd64,linux/arm64`

### Requirement: QEMU proves build and emulated boot only

An arm64 image built under `buildx` + QEMU MUST be reported as "build + emulated boot"
qualification ONLY. It MUST NOT be described as Raspberry Pi or arm64 hardware
qualification.

#### Scenario: arm64 configured but not hardware-qualified

- GIVEN an arm64 image builds and boots under QEMU emulation
- WHEN the result is reported
- THEN the wording MUST state emulated build/boot only
- AND the report MUST NOT claim Raspberry Pi or any physical arm64 hardware runtime proof

### Requirement: No `latest` for prerelease

The release tagging logic MUST NOT publish the `latest` tag for any prerelease tag
(any ref containing `-`). `latest` SHALL be reserved for non-prerelease tags only.

#### Scenario: Prerelease tag does not move latest

- GIVEN a prerelease tag (e.g. `v1.0.0-alpha.1`)
- WHEN the release job runs
- THEN the image MUST be published under its semver prerelease tag
- AND `latest` MUST NOT be created or updated
