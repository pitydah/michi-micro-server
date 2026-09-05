#!/usr/bin/env bash
set -euo pipefail

PORT="${1:-9097}"
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TMP_DIR="$(mktemp -d /tmp/michi_upgrade_restore_XXXXXX)"
OLD_SRC="$TMP_DIR/old-src"
OLD_TARGET="$TMP_DIR/old-target"

cleanup() {
    if [[ -n "${SERVER_PID:-}" ]] && kill -0 "$SERVER_PID" 2>/dev/null; then
        kill "$SERVER_PID" 2>/dev/null || true
        wait "$SERVER_PID" 2>/dev/null || true
    fi
    if [ -d "$OLD_SRC" ]; then
        git worktree remove --force "$OLD_SRC" 2>/dev/null || true
    fi
    rm -rf "$TMP_DIR"
}
trap cleanup EXIT

echo "=== Running Upgrade (v0.2.0 -> Current) & Backup/Restore Qualification Gate ==="
echo "Working directory: $TMP_DIR"

CFG_DIR="$TMP_DIR/config"
CACHE_DIR="$TMP_DIR/cache"
MUSIC_DIR="$TMP_DIR/music"
mkdir -p "$CFG_DIR" "$CACHE_DIR" "$MUSIC_DIR"

# 1. Build old version (v0.2.0)
OLD_VERSION_TAG="${MICHI_UPGRADE_FROM_TAG:-v0.2.0}"
echo "Building historical release ${OLD_VERSION_TAG}..."
git worktree add --detach "$OLD_SRC" "$OLD_VERSION_TAG"
git -C "$OLD_SRC" submodule update --init --recursive || true
cargo build --release --manifest-path "$OLD_SRC/Cargo.toml" --target-dir "$OLD_TARGET" --bin michi-server

OLD_BIN="$OLD_TARGET/release/michi-server"
NEW_BIN="$ROOT_DIR/target/release/michi-server"

# Build new binary if missing
if [ ! -f "$NEW_BIN" ]; then
    echo "Building current release candidate binary..."
    cargo build --release --bin michi-server
fi

# 2. Run historical server (v0.2.0) on clean database
export MICHI_PORT="$PORT"
export MICHI_CONFIG_PATH="$CFG_DIR"
export MICHI_CACHE_PATH="$CACHE_DIR"
export MICHI_MUSIC_PATH="$MUSIC_DIR"
export MICHI_DATABASE="sqlite://$CFG_DIR/michi.db"
export MICHI_AUTH_USERNAME="admin"
export MICHI_AUTH_PASSWORD="Password123!"
export MICHI_AUTH_ENABLED="true"
export RUST_LOG="error"

"$OLD_BIN" > /dev/null 2>&1 &
SERVER_PID=$!

for i in $(seq 1 40); do
    if curl -fsS "http://127.0.0.1:$PORT/health/live" >/dev/null 2>&1; then
        break
    fi
    sleep 0.25
done
curl -fsS "http://127.0.0.1:$PORT/health/live" >/dev/null

# Authenticate on v0.2.0
LOGIN_RESP=$(curl -fsS -X POST "http://127.0.0.1:$PORT/api/auth/login" \
    -H "Content-Type: application/json" \
    -d '{"username":"admin","password":"Password123!"}')
TOKEN=$(echo "$LOGIN_RESP" | python3 -c 'import sys, json; print(json.load(sys.stdin)["token"])')

# Create a sample playlist in v0.2.0
curl -fsS -X POST "http://127.0.0.1:$PORT/api/v1/playlists" \
    -H "Authorization: Bearer $TOKEN" \
    -H "Content-Type: application/json" \
    -d '{"name":"Legacy v0.2.0 Playlist"}' > /dev/null

echo "✓ Legacy v0.2.0 database populated with sample data."

# Graceful shutdown of old binary
kill "$SERVER_PID"
wait "$SERVER_PID" 2>/dev/null || true
SERVER_PID=""

# 3. Start Current Release Candidate on the SAME database
echo "Starting current release candidate on migrated database..."
"$NEW_BIN" > /dev/null 2>&1 &
SERVER_PID=$!

for i in $(seq 1 40); do
    if curl -fsS "http://127.0.0.1:$PORT/health/live" >/dev/null 2>&1; then
        break
    fi
    sleep 0.25
done
curl -fsS "http://127.0.0.1:$PORT/health/live" >/dev/null

# Authenticate on current instance
LOGIN_RESP=$(curl -fsS -X POST "http://127.0.0.1:$PORT/api/auth/login" \
    -H "Content-Type: application/json" \
    -d '{"username":"admin","password":"Password123!"}')
NEW_TOKEN=$(echo "$LOGIN_RESP" | python3 -c 'import sys, json; print(json.load(sys.stdin)["token"])')

# Verify playlist persisted across migration
PLAYLISTS_RESP=$(curl -fsS -X GET "http://127.0.0.1:$PORT/api/v1/playlists" \
    -H "Authorization: Bearer $NEW_TOKEN")
echo "$PLAYLISTS_RESP" | python3 -c 'import sys, json; data = json.load(sys.stdin); pls = data.get("playlists", data); assert any(p["name"] == "Legacy v0.2.0 Playlist" for p in pls)'
echo "✓ Schema migration v0.2.0 -> Current passed and preserved legacy playlist."

# SQLite integrity verification
INTEGRITY="$(sqlite3 "$CFG_DIR/michi.db" 'PRAGMA integrity_check;')"
if [ "$INTEGRITY" != "ok" ]; then
    echo "FAIL: SQLite PRAGMA integrity_check=$INTEGRITY"
    exit 1
fi
echo "✓ SQLite PRAGMA integrity_check is ok."

# 4. Disaster Recovery: Export Backup from Migrated Instance
BACKUP_FILE="$TMP_DIR/backup.json"
curl -fsS -X GET "http://127.0.0.1:$PORT/api/v1/backup/download" \
    -H "Authorization: Bearer $NEW_TOKEN" > "$BACKUP_FILE"

if [[ ! -s "$BACKUP_FILE" ]]; then
    echo "FAIL: Backup file is empty"
    exit 1
fi
echo "✓ Backup exported successfully ($(wc -c < "$BACKUP_FILE") bytes)"

# 5. Stop Server, Wipe DB, Restart & Restore
kill "$SERVER_PID"
wait "$SERVER_PID" 2>/dev/null || true
SERVER_PID=""

rm -f "$CFG_DIR/michi.db"*

"$NEW_BIN" > /dev/null 2>&1 &
SERVER_PID=$!

for i in $(seq 1 40); do
    if curl -fsS "http://127.0.0.1:$PORT/health/live" >/dev/null 2>&1; then
        break
    fi
    sleep 0.25
done

LOGIN_RESP=$(curl -fsS -X POST "http://127.0.0.1:$PORT/api/auth/login" \
    -H "Content-Type: application/json" \
    -d '{"username":"admin","password":"Password123!"}')
RESTORE_TOKEN=$(echo "$LOGIN_RESP" | python3 -c 'import sys, json; print(json.load(sys.stdin)["token"])')

# Restore from backup
RESTORE_RESP=$(curl -fsS -X POST "http://127.0.0.1:$PORT/api/v1/backup/restore?force=true" \
    -H "Authorization: Bearer $RESTORE_TOKEN" \
    -H "Content-Type: application/json" \
    --data-binary @"$BACKUP_FILE")

echo "$RESTORE_RESP" | python3 -c 'import sys, json; data = json.load(sys.stdin); assert data.get("status") in ["ok", "completed", "restored"] or "imported" in str(data) or "counts" in str(data)'

# Verify playlist restored after wipe
PLAYLISTS_RESP=$(curl -fsS -X GET "http://127.0.0.1:$PORT/api/v1/playlists" \
    -H "Authorization: Bearer $RESTORE_TOKEN")
echo "$PLAYLISTS_RESP" | python3 -c 'import sys, json; data = json.load(sys.stdin); pls = data.get("playlists", data); assert any(p["name"] == "Legacy v0.2.0 Playlist" for p in pls)'

echo "=== UPGRADE (v0.2.0 -> Current) & DISASTER RECOVERY: SUCCESS ==="
