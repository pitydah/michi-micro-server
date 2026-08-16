#!/usr/bin/env bash
set -euo pipefail

BROKER_PORT="${1:-18885}"
SERVER_PORT="${2:-9098}"

echo "=== Running Mosquitto Real E2E Integration Test ==="
python3 tests/e2e/test_mosquitto_real.py --broker-port "$BROKER_PORT" --server-port "$SERVER_PORT"
