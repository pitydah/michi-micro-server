#!/usr/bin/env bash
# Start the Michi Music Stream Simulator (Hi-Fi) on port 8081
set -euo pipefail

SIM_PATH="${MICHI_STREAM_SIM_PATH:-/home/cristian/michi-music-stream/simulator/receiver_sim.py}"
PORT="${MICHI_SIM_HIFI_PORT:-8081}"

if [ ! -f "$SIM_PATH" ]; then
    echo "ERROR: Simulator not found at $SIM_PATH"
    echo "Set MICHI_STREAM_SIM_PATH or clone pitydah/michi-music-stream"
    exit 1
fi

echo "Starting Hi-Fi Receiver Simulator on port $PORT..."
exec python3 "$SIM_PATH" --type hifi --port "$PORT"
