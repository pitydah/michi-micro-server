#!/usr/bin/env bash
# Start the Michi Music Stream Simulator (Standard) on port 8080
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKSPACE_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
SIM_PATH="${MICHI_STREAM_SIM_PATH:-${WORKSPACE_ROOT}/vendor/michi-music-stream/simulator/receiver_sim.py}"
PORT="${MICHI_SIM_STANDARD_PORT:-8080}"

if [ ! -f "$SIM_PATH" ]; then
    SIM_PATH="${SCRIPT_DIR}/receiver_sim.py"
fi

if [ ! -f "$SIM_PATH" ]; then
    echo "ERROR: Simulator not found at $SIM_PATH"
    echo "Ensure submodules are initialized (git submodule update --init --recursive)"
    exit 1
fi

echo "Starting Standard Receiver Simulator from $SIM_PATH on port $PORT..."
exec python3 "$SIM_PATH" --type standard --port "$PORT"
