#!/usr/bin/env bash
# Automated Runner for Michi Snapcast E2E Tests
# Spawns mock Snapserver with multi-client groups, runs E2E tests, and cleans up.

set -euo pipefail

SNAP_PORT="${1:-1780}"
SPAWNED_PIDS=""

cleanup() {
    if [ -n "$SPAWNED_PIDS" ]; then
        echo "Stopping Snapserver mock ($SPAWNED_PIDS)..."
        kill $SPAWNED_PIDS 2>/dev/null || true
        wait $SPAWNED_PIDS 2>/dev/null || true
    fi
}
trap cleanup EXIT INT TERM

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# Start Snapserver mock
echo "Starting Snapserver mock on port ${SNAP_PORT}..."
python3 "${PROJECT_ROOT}/scripts/snapserver_mock.py" --port "${SNAP_PORT}" &
SNAP_PID=$!
SPAWNED_PIDS="$SNAP_PID"

# Wait for Snapserver mock to be ready
for i in {1..20}; do
    if curl -sf "http://127.0.0.1:${SNAP_PORT}/health" > /dev/null 2>&1; then
        echo "Snapserver mock is ready."
        break
    fi
    sleep 0.2
done

# Run Python Snapcast E2E test suite
python3 "${PROJECT_ROOT}/tests/e2e/test_snapcast_e2e.py" --snapserver-url "http://127.0.0.1:${SNAP_PORT}"

echo "=== SNAPCAST E2E: SUCCESS ==="
