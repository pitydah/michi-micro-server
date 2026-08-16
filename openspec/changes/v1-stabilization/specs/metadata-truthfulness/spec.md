# Metadata Truthfulness Specification

## Purpose

Keep the minimal delivery metadata honest: the CasaOS store version must match the
workspace, and the changelog migration count must match the implementation. No other
metadata/icon/screenshot work is in scope.

## Requirements

### Requirement: CasaOS version matches workspace

`casaos/data.yml` MUST declare the same version as the workspace crate version (0.2.0).
Only the version truthfulness item is in scope.

#### Scenario: Metadata drift corrected

- GIVEN `casaos/data.yml` declares `version: 0.1.0` while the workspace is `0.2.0`
- WHEN the metadata alignment is applied
- THEN `data.yml` MUST declare `0.2.0`

#### Scenario: Out-of-scope metadata not touched

- GIVEN icon/screenshot/description gaps exist in CasaOS metadata
- WHEN this increment is implemented
- THEN those items MUST remain unchanged (deferred), and only the version is corrected

### Requirement: Migration count matches implementation

The CHANGELOG migration count MUST match the actual number of migration functions
(37 in `crates/michi-db/src/lib.rs`). Any "35 migraciones" reference MUST be corrected.

#### Scenario: Count drift corrected

- GIVEN the CHANGELOG states "35 migraciones" while the implementation has 37 migrations
- WHEN the truthfulness fix is applied
- THEN the CHANGELOG MUST state the correct count (37)

#### Scenario: Count is derived, not invented

- GIVEN the migration count may change in the future
- WHEN the count is written
- THEN it MUST be derived from the actual `migration_*` functions present, not hardcoded
  independently of the implementation
