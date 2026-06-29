#!/usr/bin/env bash
# Run the receiver simulator E2E tests
#
# Prerequisites:
#   - receiver_sim.py running on ports 8080 (Standard) and 8081 (Hi-Fi)
#
# Usage:
#   ./scripts/test_receiver_e2e.sh              # default ports
#   ./scripts/test_receiver_e2e.sh 8080 8081     # custom ports
#   MICHI_RECEIVER_SIM_URL=http://... ./scripts/test_receiver_e2e.sh

set -euo pipefail

STD_PORT="${1:-8080}"
HIFI_PORT="${2:-8081}"

echo "=== Michi Micro Server - Receiver E2E Tests ==="
echo "Standard port: $STD_PORT"
echo "Hi-Fi port:    $HIFI_PORT"
echo ""

# Check if simulators are running
if ! curl -sf "http://127.0.0.1:${STD_PORT}/api/v1/receiver/info" > /dev/null 2>&1; then
    echo "ERROR: Standard simulator not running on port ${STD_PORT}"
    echo "  Start with: python3 receiver_sim.py --type standard --port ${STD_PORT}"
    exit 1
fi

if ! curl -sf "http://127.0.0.1:${HIFI_PORT}/api/v1/receiver/info" > /dev/null 2>&1; then
    echo "ERROR: Hi-Fi simulator not running on port ${HIFI_PORT}"
    echo "  Start with: python3 receiver_sim.py --type hifi --port ${HIFI_PORT}"
    exit 1
fi

echo "Both simulators are running."
echo ""

# Run tests
export MICHI_RECEIVER_SIM_URL="http://127.0.0.1:${STD_PORT}"
export MICHI_RECEIVER_SIM_HIFI_URL="http://127.0.0.1:${HIFI_PORT}"

echo "Running receiver simulator integration tests..."
cargo test --test receiver_simulator_integration -- --ignored 2>&1

echo ""
echo "=== Done ==="
