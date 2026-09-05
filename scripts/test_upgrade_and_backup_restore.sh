#!/usr/bin/env bash
set -euo pipefail

PORT="${1:-9097}"
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TMP_DIR="$(mktemp -d /tmp/michi_upgrade_restore_XXXXXX)"

cleanup() {
    if [[ -n "${SERVER_PID:-}" ]] && kill -0 "$SERVER_PID" 2>/dev/null; then
        kill "$SERVER_PID" 2>/dev/null || true
        wait "$SERVER_PID" 2>/dev/null || true
    fi
    rm -rf "$TMP_DIR"
}
trap cleanup EXIT

echo "=== Running Upgrade & Backup/Restore Qualification Gate ==="
echo "Working directory: $TMP_DIR"

CFG_DIR="$TMP_DIR/config"
CACHE_DIR="$TMP_DIR/cache"
MUSIC_DIR="$TMP_DIR/music"
mkdir -p "$CFG_DIR" "$CACHE_DIR" "$MUSIC_DIR"

# 1. Start Server on clean database
export MICHI_PORT="$PORT"
export MICHI_CONFIG_PATH="$CFG_DIR"
export MICHI_CACHE_PATH="$CACHE_DIR"
export MICHI_MUSIC_PATH="$MUSIC_DIR"
export MICHI_DATABASE="sqlite://$CFG_DIR/michi.db"
export MICHI_AUTH_USERNAME="admin"
export MICHI_AUTH_PASSWORD="Password123!"
export MICHI_AUTH_ENABLED="true"
export RUST_LOG="error"

"$ROOT_DIR/target/release/michi-server" > /dev/null 2>&1 &
SERVER_PID=$!

# Wait for healthy
for i in $(seq 1 40); do
    if curl -fsS "http://127.0.0.1:$PORT/health/live" >/dev/null 2>&1; then
        break
    fi
    sleep 0.2
done
curl -fsS "http://127.0.0.1:$PORT/health/live" >/dev/null

# Authenticate
LOGIN_RESP=$(curl -fsS -X POST "http://127.0.0.1:$PORT/api/auth/login" \
    -H "Content-Type: application/json" \
    -d '{"username":"admin","password":"Password123!"}')
TOKEN=$(echo "$LOGIN_RESP" | python3 -c 'import sys, json; print(json.load(sys.stdin)["token"])')

# Create a sample playlist
PL_RESP=$(curl -fsS -X POST "http://127.0.0.1:$PORT/api/v1/playlists" \
    -H "Authorization: Bearer $TOKEN" \
    -H "Content-Type: application/json" \
    -d '{"name":"Upgrade Test Playlist"}')
PL_ID=$(echo "$PL_RESP" | python3 -c 'import sys, json; print(json.load(sys.stdin)["playlist"]["id"])')

# 2. Export full backup
BACKUP_FILE="$TMP_DIR/backup.json"
curl -fsS -X GET "http://127.0.0.1:$PORT/api/v1/backup/download" \
    -H "Authorization: Bearer $TOKEN" > "$BACKUP_FILE"

if [[ ! -s "$BACKUP_FILE" ]]; then
    echo "FAIL: Backup file is empty"
    exit 1
fi
echo "✓ Backup exported successfully ($(wc -c < "$BACKUP_FILE") bytes)"

# 3. Stop Server
kill "$SERVER_PID"
wait "$SERVER_PID" 2>/dev/null || true
SERVER_PID=""

# 4. Wipe DB file to simulate clean machine / disaster recovery
rm -f "$CFG_DIR/michi.db"*

# 5. Restart server
"$ROOT_DIR/target/release/michi-server" > /dev/null 2>&1 &
SERVER_PID=$!

for i in $(seq 1 40); do
    if curl -fsS "http://127.0.0.1:$PORT/health/live" >/dev/null 2>&1; then
        break
    fi
    sleep 0.2
done

# Authenticate on clean instance
LOGIN_RESP=$(curl -fsS -X POST "http://127.0.0.1:$PORT/api/auth/login" \
    -H "Content-Type: application/json" \
    -d '{"username":"admin","password":"Password123!"}')
NEW_TOKEN=$(echo "$LOGIN_RESP" | python3 -c 'import sys, json; print(json.load(sys.stdin)["token"])')

# 6. Restore from backup
RESTORE_RESP=$(curl -fsS -X POST "http://127.0.0.1:$PORT/api/v1/backup/restore?force=true" \
    -H "Authorization: Bearer $NEW_TOKEN" \
    -H "Content-Type: application/json" \
    --data-binary @"$BACKUP_FILE")

echo "$RESTORE_RESP" | python3 -c 'import sys, json; data = json.load(sys.stdin); assert data.get("status") in ["ok", "completed", "restored"] or "imported" in str(data) or "counts" in str(data)'

# Verify playlist restored
PLAYLISTS_RESP=$(curl -fsS -X GET "http://127.0.0.1:$PORT/api/v1/playlists" \
    -H "Authorization: Bearer $NEW_TOKEN")
echo "$PLAYLISTS_RESP" | python3 -c 'import sys, json; data = json.load(sys.stdin); pls = data.get("playlists", data); assert any(p["name"] == "Upgrade Test Playlist" for p in pls)'

echo "✓ Migration, backup & restore qualification completed PASS."
