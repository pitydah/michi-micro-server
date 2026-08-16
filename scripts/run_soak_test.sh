#!/usr/bin/env bash
# Long-Term Soak Test Runner (24h, 48h, 72h)
# Monitors memory RSS, CPU, file descriptors, WAL files, and child processes under load.
# Usage:
#   bash scripts/run_soak_test.sh 24 9089   # 24 hours soak test
#   bash scripts/run_soak_test.sh 48 9089   # 48 hours soak test
#   bash scripts/run_soak_test.sh 72 9089   # 72 hours soak test

set -euo pipefail

DURATION_HOURS="${1:-24}"
SERVER_PORT="${2:-9089}"
BASE_DIR="/tmp/michi_soak_${DURATION_HOURS}h"
CONFIG_DIR="${BASE_DIR}/config"
CACHE_DIR="${BASE_DIR}/cache"
MUSIC_DIR="${BASE_DIR}/music"

SPAWNED_PIDS=""

cleanup() {
    if [ -n "$SPAWNED_PIDS" ]; then
        echo "Cleaning up soak test process ($SPAWNED_PIDS)..."
        kill -9 $SPAWNED_PIDS 2>/dev/null || true
    fi
}
trap cleanup EXIT INT TERM

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

echo "=========================================================================="
echo "MICHI MICRO SERVER — LONG-TERM SOAK TEST (${DURATION_HOURS} HOURS)"
echo "Port:           $SERVER_PORT"
echo "Duration:       $DURATION_HOURS hours"
echo "Config Dir:     $CONFIG_DIR"
echo "Music Dir:      $MUSIC_DIR"
echo "Report:         ${PROJECT_ROOT}/target/soak_report_${DURATION_HOURS}h.json"
echo "=========================================================================="

mkdir -p "$CONFIG_DIR" "$CACHE_DIR" "$MUSIC_DIR"

TARGET_BIN="${PROJECT_ROOT}/target/release/michi-server"
if [ ! -f "$TARGET_BIN" ]; then
    echo "Building release binary for soak test..."
    cargo build --release --bin michi-server
fi

echo "Starting Michi Micro Server in release mode..."
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

for i in {1..25}; do
    if curl -sf "http://127.0.0.1:${SERVER_PORT}/health/live" > /dev/null 2>&1; then
        echo "Server is live and ready."
        break
    fi
    sleep 0.2
done

python3 "${PROJECT_ROOT}/scripts/soak_test.py" \
    --url "http://127.0.0.1:${SERVER_PORT}" \
    --pid "$SERVER_PID" \
    --config-dir "$CONFIG_DIR" \
    --duration-hours "$DURATION_HOURS" \
    --sample-interval 10.0 \
    --username "admin" \
    --password "admin123" \
    --report "${PROJECT_ROOT}/target/soak_report_${DURATION_HOURS}h.json"

echo "=== SOAK TEST (${DURATION_HOURS}h) COMPLETE ==="
