#!/usr/bin/env bash
# Canonical pre-commit quality gate script for Michi Micro Server.
# Must be 100% green before any git push.

set -euo pipefail

echo "=== [1/5] Formatting check (cargo fmt) ==="
cargo fmt --all
cargo fmt --all -- --check
git diff --check

echo "=== [2/5] Workspace compilation check (cargo check) ==="
cargo check --workspace --all-targets

echo "=== [3/5] Unit and integration tests (cargo test) ==="
cargo test --workspace

echo "=== [4/5] Linter enforcement (cargo clippy) ==="
cargo clippy --workspace --all-targets --all-features -- -D warnings

echo "=== [5/5] Release binary build (cargo build --release) ==="
cargo build --release --bin michi-server

echo "=== QUALITY GATE PASSED: ALL GATES GREEN ==="
