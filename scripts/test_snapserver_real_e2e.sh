#!/usr/bin/env bash
set -euo pipefail

SNAP_PORT="${1:-1781}"

echo "=== Running Snapserver Real E2E Integration Test ==="
python3 tests/e2e/test_snapserver_real.py --port "$SNAP_PORT"
