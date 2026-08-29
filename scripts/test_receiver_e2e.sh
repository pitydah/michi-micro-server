#!/usr/bin/env bash
# Automated Runner for Michi Receiver Simulator Integration Tests
# Spawns Standard and Hi-Fi simulators, executes the 14 tests, and cleans up.

set -euo pipefail

STD_PORT="${1:-8080}"
HIFI_PORT="${2:-8081}"

SPAWNED_PIDS=""

cleanup() {
    if [ -n "$SPAWNED_PIDS" ]; then
        echo "Stopping spawned receiver simulators ($SPAWNED_PIDS)..."
        kill $SPAWNED_PIDS 2>/dev/null || true
        wait $SPAWNED_PIDS 2>/dev/null || true
    fi
}
trap cleanup EXIT INT TERM

echo "=== Michi Micro Server - Receiver E2E Tests ==="
echo "Standard port: $STD_PORT"
echo "Hi-Fi port:    $HIFI_PORT"
echo ""

# Find simulator script
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SIM_SCRIPT="${SCRIPT_DIR}/receiver_sim.py"

# Start Standard simulator if not running
if ! curl -sf "http://127.0.0.1:${STD_PORT}/api/v1/server/info" > /dev/null 2>&1; then
    echo "Starting Standard receiver simulator on port ${STD_PORT}..."
    python3 "$SIM_SCRIPT" --type standard --port "${STD_PORT}" &
    STD_PID=$!
    SPAWNED_PIDS="$SPAWNED_PIDS $STD_PID"
fi

# Start Hi-Fi simulator if not running
if ! curl -sf "http://127.0.0.1:${HIFI_PORT}/api/v1/server/info" > /dev/null 2>&1; then
    echo "Starting Hi-Fi receiver simulator on port ${HIFI_PORT}..."
    python3 "$SIM_SCRIPT" --type hifi --port "${HIFI_PORT}" &
    HIFI_PID=$!
    SPAWNED_PIDS="$SPAWNED_PIDS $HIFI_PID"
fi

# Wait for both simulators to be healthy (max 10s)
for i in {1..20}; do
    STD_OK=0
    HIFI_OK=0
    if curl -sf "http://127.0.0.1:${STD_PORT}/api/v1/server/info" > /dev/null 2>&1; then
        STD_OK=1
    fi
    if curl -sf "http://127.0.0.1:${HIFI_PORT}/api/v1/server/info" > /dev/null 2>&1; then
        HIFI_OK=1
    fi
    if [ "$STD_OK" -eq 1 ] && [ "$HIFI_OK" -eq 1 ]; then
        echo "Both simulators are healthy and ready."
        break
    fi
    sleep 0.5
done

if [ "$STD_OK" -ne 1 ] || [ "$HIFI_OK" -ne 1 ]; then
    echo "ERROR: Receiver simulators failed to start within timeout."
    exit 1
fi

export MICHI_RECEIVER_SIM_URL="http://127.0.0.1:${STD_PORT}"
export MICHI_RECEIVER_SIM_HIFI_URL="http://127.0.0.1:${HIFI_PORT}"

echo ""
echo "Running all receiver simulator integration tests..."
cargo test -p michi-receivers --test receiver_simulator_integration -- --ignored --test-threads=1

echo ""
echo "Running production ReceiverAudioSink data plane certification test..."
cargo test -p michi-api test_autonomous_playback_to_production_receiver_sink_e2e -- --ignored --test-threads=1

echo ""
echo "=== RECEIVER E2E TESTS: ALL PASSED ==="
