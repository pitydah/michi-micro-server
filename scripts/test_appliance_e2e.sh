#!/usr/bin/env bash
# Appliance Qualification E2E Test Runner
# Validates Raspberry Pi 4/5, CasaOS, and ZimaOS operational requirements:
# - Clean install
# - Permissions on /music, /config, /cache
# - Historical SQLite DB upgrade (v1/v2 schema -> v1.0.0)
# - Container restart & simulated reboot persistence
# - Audio streaming over HTTP Range requests

set -euo pipefail

SERVER_PORT="${1:-9092}"
BASE_DIR="/tmp/michi_appliance_test"
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

echo "========================================================================"
echo "MICHI MICRO SERVER — APPLIANCE E2E QUALIFICATION (RPi / CasaOS / ZimaOS)"
echo "Port:       $SERVER_PORT"
echo "Config Dir: $CONFIG_DIR"
echo "Music Dir:  $MUSIC_DIR"
echo "Cache Dir:  $CACHE_DIR"
echo "========================================================================"

# 1. Clean Environment Setup
rm -rf "$BASE_DIR"
mkdir -p "$CONFIG_DIR" "$CACHE_DIR" "$MUSIC_DIR"

# 2. Historical Database Migration Simulation
# Create a historical SQLite database with schema version 10 and dummy track data
echo "Preparing historical SQLite database (v0.1.0 schema)..."
sqlite3 "${CONFIG_DIR}/michi.db" <<'EOF'
CREATE TABLE _migrations (
    version INTEGER PRIMARY KEY,
    applied_at TEXT NOT NULL
);
INSERT INTO _migrations (version, applied_at) VALUES 
(1, datetime('now')), (2, datetime('now')), (3, datetime('now')), 
(4, datetime('now')), (5, datetime('now')), (6, datetime('now')), 
(7, datetime('now')), (8, datetime('now')), (9, datetime('now')), (10, datetime('now'));

CREATE TABLE tracks (
    id TEXT PRIMARY KEY,
    title TEXT,
    artist TEXT,
    album TEXT,
    album_artist TEXT,
    duration_ms INTEGER,
    file_path TEXT NOT NULL UNIQUE,
    format TEXT NOT NULL DEFAULT 'unknown',
    sample_rate INTEGER,
    bit_depth INTEGER,
    channels INTEGER,
    artwork_id TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

INSERT INTO tracks (id, title, artist, album, file_path, format, created_at, updated_at) 
VALUES ('hist-track-001', 'Historical Ballad', 'Vintage Artist', 'First Edition', '/music/historical_track.flac', 'flac', datetime('now'), datetime('now'));

CREATE TABLE users (
    id TEXT PRIMARY KEY,
    username TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    is_admin INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
EOF

echo "Historical database prepared with 1 track and 1 user."

# 3. Locate / Build Binary
TARGET_BIN="${PROJECT_ROOT}/target/debug/michi-server"
if [ ! -f "$TARGET_BIN" ]; then
    echo "Building michi-server..."
    cargo build --bin michi-server
fi

# 4. Start Michi Server with Historical Database
echo "Starting Michi Micro Server to execute automatic migrations..."
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

# Wait for server readiness (generous window: cold start generates the
# Ed25519 identity, migrates the DB and only then binds the listener).
for i in {1..60}; do
    if curl -sf "http://127.0.0.1:${SERVER_PORT}/health/live" > /dev/null 2>&1; then
        echo "Server is live and ready."
        break
    fi
    sleep 0.5
done
if ! curl -sf "http://127.0.0.1:${SERVER_PORT}/health/live" > /dev/null 2>&1; then
    echo "ERROR: server did not become ready within 30s" >&2
    exit 1
fi

# 5. Run Appliance Qualification Test Suite
python3 "${PROJECT_ROOT}/tests/e2e/test_appliance_qualification.py" \
    --server-url "http://127.0.0.1:${SERVER_PORT}" \
    --config-dir "$CONFIG_DIR" \
    --music-dir "$MUSIC_DIR" \
    --username "admin" \
    --password "admin123"

# 6. Test Container Restart & Reboot Simulation
echo "Simulating server restart / host reboot..."
kill -9 "$SERVER_PID" 2>/dev/null || true
sleep 1.0

echo "Restarting Michi Micro Server from persisted storage..."
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

for i in {1..60}; do
    if curl -sf "http://127.0.0.1:${SERVER_PORT}/health/live" > /dev/null 2>&1; then
        echo "Server restarted successfully from persisted database."
        break
    fi
    sleep 0.5
done
if ! curl -sf "http://127.0.0.1:${SERVER_PORT}/health/live" > /dev/null 2>&1; then
    echo "ERROR: server did not become ready after restart within 30s" >&2
    exit 1
fi

# Re-run full qualification on restarted server to verify total persistence
python3 "${PROJECT_ROOT}/tests/e2e/test_appliance_qualification.py" \
    --server-url "http://127.0.0.1:${SERVER_PORT}" \
    --config-dir "$CONFIG_DIR" \
    --music-dir "$MUSIC_DIR" \
    --username "admin" \
    --password "admin123"

echo ""
echo "========================================================================"
echo "=== APPLIANCE E2E QUALIFICATION (RPi 4/5, CasaOS, ZimaOS): SUCCESS ==="
echo "========================================================================"
