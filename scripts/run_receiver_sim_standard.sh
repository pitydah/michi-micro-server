#!/usr/bin/env bash
# Start the Michi Music Stream Simulator (Standard) on port 8080
set -euo pipefail

SIM_PATH="${MICHI_STREAM_SIM_PATH:-/home/cristian/michi-music-stream/simulator/receiver_sim.py}"
PORT="${MICHI_SIM_STANDARD_PORT:-8080}"

if [ ! -f "$SIM_PATH" ]; then
    echo "ERROR: Simulator not found at $SIM_PATH"
    echo "Set MICHI_STREAM_SIM_PATH or clone pitydah/michi-music-stream"
    exit 1
fi

echo "Starting Standard Receiver Simulator on port $PORT..."
exec python3 "$SIM_PATH" --type standard --port "$PORT"
