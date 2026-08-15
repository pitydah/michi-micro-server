#!/usr/bin/env bash
# CI contract gate: build and boot a real michi-server on isolated paths, wait
# for public /health/live (never the protected /api/status), run the Player<->Micro
# contract test, then tear down deterministically.
#
# Any failure — missing prerequisite, build error, boot failure, health-wait
# timeout, or contract FAIL — exits non-zero. Nothing is skipped. Credentials
# are throwaway per-run and never logged.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

TARGET_DIR="${CARGO_TARGET_DIR:-$REPO_ROOT/target}"
SERVER_BIN="$TARGET_DIR/release/michi-server"

# Deterministic, collision-avoiding scratch port (PID-derived within a reserved
# band); overridable via MICHI_CONTRACT_PORT.
PORT="${MICHI_CONTRACT_PORT:-$((18100 + ($$ % 900)))}"

# Isolated per-run state dirs.
RUN_DIR="$(mktemp -d "${TMPDIR:-/tmp}/michi-contract-gate.XXXXXX")"
CONFIG_DIR="$RUN_DIR/config"
CACHE_DIR="$RUN_DIR/cache"
MUSIC_DIR="$RUN_DIR/music"
SERVER_LOG="$RUN_DIR/server.log"
CONTRACT_LOG="$RUN_DIR/contract.log"

# Persistent failure log (survives RUN_DIR cleanup so CI can upload it).
FAILURE_LOG="${MICHI_FAILURE_LOG:-$RUN_DIR/failure.log}"

# Throwaway per-run admin credentials (never echoed or logged).
ADMIN_USER="ci-contract"
ADMIN_PASS="ci-$(date +%s)-$RANDOM$RANDOM"

SERVER_PID=""

fail() {
    echo "❌ ci-contract-gate: $*" >&2
    {
        echo "ci-contract-gate FAILURE: $*"
        if [[ -s "$SERVER_LOG" ]]; then
            echo "--- server log (tail) ---"
            tail -n 50 "$SERVER_LOG"
        fi
    } >>"$FAILURE_LOG" 2>/dev/null || true
    exit 1
}

cleanup() {
    if [[ -n "$SERVER_PID" ]] && kill -0 "$SERVER_PID" 2>/dev/null; then
        echo "ci-contract-gate: stopping server (pid $SERVER_PID)…" >&2
        kill "$SERVER_PID" 2>/dev/null || true
        for _ in $(seq 1 20); do
            kill -0 "$SERVER_PID" 2>/dev/null || break
            sleep 0.5
        done
        kill -9 "$SERVER_PID" 2>/dev/null || true
    fi
    rm -rf "$RUN_DIR"
}
trap cleanup EXIT

command -v python3 >/dev/null 2>&1 || fail "python3 is required but not found on PATH"
command -v curl >/dev/null 2>&1 || fail "curl is required but not found on PATH"
command -v cargo >/dev/null 2>&1 || fail "cargo is required but not found on PATH"

if [[ ! -x "$SERVER_BIN" ]]; then
    echo "ci-contract-gate: server binary not found — building release binary…"
    cargo build --release -p michi-server
fi
[[ -x "$SERVER_BIN" ]] || fail "server binary missing after build at $SERVER_BIN"

mkdir -p "$CONFIG_DIR" "$CACHE_DIR" "$MUSIC_DIR"

echo "ci-contract-gate: booting server on port $PORT (isolated paths under $RUN_DIR)…"
MICHI_PORT="$PORT" \
    MICHI_CONFIG_PATH="$CONFIG_DIR" \
    MICHI_CACHE_PATH="$CACHE_DIR" \
    MICHI_MUSIC_PATH="$MUSIC_DIR" \
    MICHI_DATABASE="sqlite://$CONFIG_DIR/michi.db" \
    MICHI_AUTH_USERNAME="$ADMIN_USER" \
    MICHI_AUTH_PASSWORD="$ADMIN_PASS" \
    "$SERVER_BIN" >"$SERVER_LOG" 2>&1 &
SERVER_PID=$!

if ! kill -0 "$SERVER_PID" 2>/dev/null; then
    fail "server process exited immediately after boot; see server log"
fi

echo "ci-contract-gate: waiting for /health/live on 127.0.0.1:$PORT (≤60s)…"
deadline=$((SECONDS + 60))
healthy=0
while ((SECONDS < deadline)); do
    if curl -fsS "http://127.0.0.1:$PORT/health/live" >/dev/null 2>&1; then
        healthy=1
        break
    fi
    if ! kill -0 "$SERVER_PID" 2>/dev/null; then
        break
    fi
    sleep 1
done
if [[ "$healthy" -ne 1 ]]; then
    fail "server did not become healthy on /health/live within 60s"
fi

# The contract test enforces a 5-second per-request timeout internally.
echo "ci-contract-gate: running contract test against http://127.0.0.1:$PORT …"
set +e
MICHI_AUTH_USERNAME="$ADMIN_USER" MICHI_AUTH_PASSWORD="$ADMIN_PASS" \
    python3 tests/e2e/test_player_micro_contract_compatibility.py \
    --url "http://127.0.0.1:$PORT" >"$CONTRACT_LOG" 2>&1
CONTRACT_RC=$?
set -e

if [[ "$CONTRACT_RC" -ne 0 ]]; then
    {
        echo "ci-contract-gate FAILURE: contract test exited $CONTRACT_RC"
        cat "$CONTRACT_LOG"
    } >>"$FAILURE_LOG"
    cat "$CONTRACT_LOG" >&2
    exit "$CONTRACT_RC"
fi

cat "$CONTRACT_LOG"
echo "ci-contract-gate: PASS (contract OK, exit 0)"
