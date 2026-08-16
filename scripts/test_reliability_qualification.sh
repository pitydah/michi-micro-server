#!/usr/bin/env bash
# Automated Master Reliability & Stress Qualification Runner
# Executes the full pre-v1.0.0 qualification battery and soak test monitor.

set -euo pipefail

SERVER_PORT="${1:-9091}"
BASE_DIR="/tmp/michi_rel_test"
CONFIG_DIR="${BASE_DIR}/config"
CACHE_DIR="${BASE_DIR}/cache"
MUSIC_DIR="${BASE_DIR}/music"

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

echo "=========================================================================="
echo "MICHI MICRO SERVER — MASTER RELIABILITY & STRESS QUALIFICATION RUNNER"
echo "Port:       $SERVER_PORT"
echo "Config Dir: $CONFIG_DIR"
echo "Music Dir:  $MUSIC_DIR"
echo "Cache Dir:  $CACHE_DIR"
echo "=========================================================================="

# 1. Clean Environment Setup
rm -rf "$BASE_DIR"
mkdir -p "$CONFIG_DIR" "$CACHE_DIR" "$MUSIC_DIR"

# 2. Locate / Build Binary
TARGET_BIN="${PROJECT_ROOT}/target/debug/michi-server"
if [ ! -f "$TARGET_BIN" ]; then
    echo "Building michi-server..."
    cargo build --bin michi-server
fi

# 3. Start Michi Server
echo "Starting Michi Micro Server..."
MICHI_PORT="$SERVER_PORT" \
MICHI_AUTH_USERNAME="admin" \
MICHI_AUTH_PASSWORD="admin123" \
MICHI_DATABASE="sqlite://${CONFIG_DIR}/michi.db" \
MICHI_MUSIC_PATH="$MUSIC_DIR" \
MICHI_MUSIC_PATHS="$MUSIC_DIR" \
MICHI_CACHE_PATH="$CACHE_DIR" \
MICHI_CONFIG_PATH="$CONFIG_DIR" \
"$TARGET_BIN" &
SERVER_PID=$!
SPAWNED_PIDS="$SERVER_PID"

# Wait for server readiness
for i in {1..25}; do
    if curl -sf "http://127.0.0.1:${SERVER_PORT}/health/live" > /dev/null 2>&1; then
        echo "Server is live and ready."
        break
    fi
    sleep 0.2
done

# 4. Run Master Reliability Qualification Battery
python3 "${PROJECT_ROOT}/tests/e2e/test_reliability_qualification.py" \
    --server-url "http://127.0.0.1:${SERVER_PORT}" \
    --config-dir "$CONFIG_DIR" \
    --music-dir "$MUSIC_DIR" \
    --username "admin" \
    --password "admin123"

# 5. Run Stability & Soak Telemetry Monitor
python3 "${PROJECT_ROOT}/scripts/soak_test.py" \
    --url "http://127.0.0.1:${SERVER_PORT}" \
    --pid "$SERVER_PID" \
    --config-dir "$CONFIG_DIR" \
    --duration-seconds 15 \
    --username "admin" \
    --password "admin123" \
    --report "${PROJECT_ROOT}/target/soak_report.json"

echo ""
echo "=========================================================================="
echo "=== MASTER RELIABILITY QUALIFICATION & STABILITY MONITOR: SUCCESS ==="
echo "=========================================================================="
