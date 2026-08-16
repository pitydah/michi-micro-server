#!/usr/bin/env bash
set -euo pipefail

MICRO_PORT="${1:-9099}"
STREAM_STD_PORT="${2:-55438}"
STREAM_HIFI_PORT="${3:-55439}"

echo "=== Running Three-Way Ecosystem E2E Integration Suite ==="
python3 tests/e2e/test_three_way_ecosystem.py \
  --micro-port "$MICRO_PORT" \
  --stream-std-port "$STREAM_STD_PORT" \
  --stream-hifi-port "$STREAM_HIFI_PORT"
