#!/usr/bin/env bash
# Automated Runner for Michi Home Assistant & MQTT E2E Tests
# Starts MQTT broker simulator, starts Michi Micro Server with MQTT enabled,
# verifies discovery, states, command execution, and reconnect resilience.

set -euo pipefail

MQTT_PORT="${1:-18883}"
ADMIN_PORT="${2:-18884}"
SERVER_PORT="${3:-9098}"

SPAWNED_PIDS=""

cleanup() {
    if [ -n "$SPAWNED_PIDS" ]; then
        echo "Cleaning up spawned processes ($SPAWNED_PIDS)..."
        kill -9 $SPAWNED_PIDS 2>/dev/null || true
    fi
}
trap cleanup EXIT INT TERM

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

echo "=== Michi Home Assistant & MQTT E2E Integration ==="
echo "MQTT Broker Port:  $MQTT_PORT"
echo "MQTT Admin Port:   $ADMIN_PORT"
echo "Server HTTP Port:  $SERVER_PORT"
echo ""

# Start MQTT Broker Simulator
echo "Starting MQTT Broker Simulator..."
python3 "${PROJECT_ROOT}/scripts/mqtt_broker_sim.py" --port "$MQTT_PORT" --admin-port "$ADMIN_PORT" &
BROKER_PID=$!
SPAWNED_PIDS="$BROKER_PID"

# Wait for broker admin API
for i in {1..20}; do
    if curl -sf "http://127.0.0.1:${ADMIN_PORT}/health" > /dev/null 2>&1; then
        echo "MQTT Broker is ready."
        break
    fi
    sleep 0.2
done

# Build / locate server binary
TARGET_BIN="${PROJECT_ROOT}/target/debug/michi-server"
if [ ! -f "$TARGET_BIN" ]; then
    echo "Building michi-server..."
    cargo build --bin michi-server
fi

# Prepare temporary test directories
mkdir -p /tmp/michi_ha_test /tmp/michi_ha_test/music /tmp/michi_ha_test/cache /tmp/michi_ha_test/config
rm -f /tmp/michi_ha_test/michi.db

# Run Michi Server with MQTT enabled
echo "Starting Michi Micro Server with MQTT enabled..."
MICHI_PORT="$SERVER_PORT" \
MICHI_AUTH_ENABLED="true" \
MICHI_AUTH_USERNAME="admin" \
MICHI_AUTH_PASSWORD="TestAdminPassword123!" \
MICHI_DATABASE_URL="sqlite:///tmp/michi_ha_test/michi.db" \
MICHI_MUSIC_PATHS="/tmp/michi_ha_test/music" \
MICHI_CACHE_PATH="/tmp/michi_ha_test/cache" \
MICHI_CONFIG_PATH="/tmp/michi_ha_test/config" \
MICHI_MQTT_ENABLED="true" \
MICHI_MQTT_HOST="127.0.0.1" \
MICHI_MQTT_PORT="$MQTT_PORT" \
"$TARGET_BIN" &
SERVER_PID=$!
SPAWNED_PIDS="$SPAWNED_PIDS $SERVER_PID"

# Wait for Michi Server HTTP endpoint
for i in {1..20}; do
    if curl -sf "http://127.0.0.1:${SERVER_PORT}/health/live" > /dev/null 2>&1; then
        echo "Michi Micro Server is ready."
        break
    fi
    sleep 0.2
done

# Run Home Assistant & MQTT E2E tests
python3 "${PROJECT_ROOT}/tests/e2e/test_homeassistant_e2e.py" \
    --admin-url "http://127.0.0.1:${ADMIN_PORT}" \
    --server-url "http://127.0.0.1:${SERVER_PORT}"

echo ""
echo "=== HOME ASSISTANT & MQTT E2E: SUCCESS ==="
