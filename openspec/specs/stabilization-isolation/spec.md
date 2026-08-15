# Stabilization Isolation & Delivery Specification

## Purpose

Enforce the isolation and delivery constraints that keep stabilization work from
contaminating or being contaminated by the uncommitted WebUI change, and that keep review
slices within budget.

## Requirements

### Requirement: Work derives from exact HEAD in isolated worktrees

All stabilization work MUST derive from exact HEAD `e6f6dd6` in detached worktrees. The
main working tree SHALL NOT be modified by this change.

#### Scenario: Detached worktree from exact HEAD

- GIVEN the main tree carries uncommitted work
- WHEN a stabilization slice is produced
- THEN it MUST be created from a detached worktree at `e6f6dd6`
- AND the main tree MUST remain pristine after the work

### Requirement: Never absorb uncommitted WebUI work

The stabilization change MUST NOT absorb, overwrite, stage, or depend on the uncommitted
WebUI/PWA/design files (`.gitignore`, `Dockerfile`, `crates/michi-api/src/{lib,pwa,static_files}.rs`,
WebUI statics, `tests/api.rs`, `openspec/`, `static/assets/*`). The working-tree
`Dockerfile` mtime fix MUST be re-derived cleanly from HEAD, not copied.

#### Scenario: Uncommitted files are not staged

- GIVEN uncommitted WebUI/PWA/Dockerfile changes exist in the main tree
- WHEN a stabilization slice is staged
- THEN none of the contaminated files MUST appear in the slice diff
- AND the Dockerfile mtime fix MUST be re-authored from HEAD, not absorbed

#### Scenario: WebUI change remains separate

- GIVEN the WebUI change is a distinct piece of work
- WHEN stabilization slices land
- THEN the WebUI change MUST remain a separate change and MUST NOT be a dependency of
  any stabilization slice

### Requirement: Feature-branch-chain slices within budget

Slices MUST be delivered as a feature-branch chain, each slice ≤500 changed lines
(additions + deletions). Pure-deletion work (e.g. the ~610-line dead-file removal) MAY be
landed as its own separate review unit.

#### Scenario: Oversized slice is split

- GIVEN a slice whose authored + deletion lines exceed 500
- WHEN the slice is planned
- THEN it MUST be split into independently testable/revertible units
- AND pure-deletion content MUST be a separate review unit from authored edits

#### Scenario: Each slice is independently testable and revertible

- GIVEN a chain of stabilization slices
- WHEN any single slice is reverted
- THEN only that slice's files/behavior MUST be removed
- AND no cross-slice schema or state coupling MUST exist
