#!/usr/bin/env bash
# Verifies that a published container image:
# 1. Has valid OCI multi-arch manifests for linux/amd64 and linux/arm64
# 2. Can be pulled anonymously without GHCR/registry credentials
# 3. Boots successfully and responds to /health/live
set -euo pipefail

IMAGE="${1:-}"
if [ -z "$IMAGE" ]; then
    echo "Usage: $0 <image-reference>" >&2
    echo "Example: $0 ghcr.io/pitydah/michi-micro-server:1.0.0-rc.1" >&2
    exit 1
fi

TEST_PORT="${2:-9095}"
CONTAINER_NAME="michi-public-smoke-$$"
TMP_DIR="/tmp/michi-public-smoke-$$"

cleanup() {
    echo "Cleaning up smoke test container and temporary directories..."
    docker rm -f "$CONTAINER_NAME" >/dev/null 2>&1 || true
    rm -rf "$TMP_DIR" >/dev/null 2>&1 || true
}
trap cleanup EXIT INT TERM

echo "========================================================================"
echo "VERIFYING PUBLIC RELEASE IMAGE: $IMAGE"
echo "========================================================================"

# Step 1: Strict credential isolation (logout to ensure anonymous pull)
echo "[1/5] Isolating credentials (docker logout ghcr.io)..."
docker logout ghcr.io >/dev/null 2>&1 || true

# Step 2: Inspect remote manifest using buildx imagetools (queries registry directly)
echo "[2/5] Inspecting multi-arch manifest list..."
MANIFEST_OUTPUT=$(docker buildx imagetools inspect "$IMAGE")
echo "$MANIFEST_OUTPUT"

if ! echo "$MANIFEST_OUTPUT" | grep -q "linux/amd64"; then
    echo "ERROR: Image $IMAGE manifest list is missing linux/amd64 architecture!" >&2
    exit 1
fi

if ! echo "$MANIFEST_OUTPUT" | grep -q "linux/arm64"; then
    echo "ERROR: Image $IMAGE manifest list is missing linux/arm64 architecture!" >&2
    exit 1
fi
echo "  ✓ Multi-arch manifest verified: linux/amd64 and linux/arm64 present."

# Step 3: Anonymous pull
echo "[3/5] Performing anonymous docker pull..."
docker pull "$IMAGE"
echo "  ✓ Anonymous pull succeeded."

# Step 4: Run smoke test container
echo "[4/5] Executing smoke container test..."
mkdir -p "$TMP_DIR/config" "$TMP_DIR/cache" "$TMP_DIR/music"

docker run -d \
  --name "$CONTAINER_NAME" \
  --network host \
  -e MICHI_PORT="$TEST_PORT" \
  -e MICHI_AUTH_USERNAME=admin \
  -e MICHI_AUTH_PASSWORD=test-release-password \
  -e MICHI_CONFIG_PATH=/config \
  -e MICHI_CACHE_PATH=/cache \
  -e MICHI_MUSIC_PATH=/music \
  -e MICHI_DATABASE=sqlite:///config/michi.db \
  -v "$TMP_DIR/config":/config \
  -v "$TMP_DIR/cache":/cache \
  -v "$TMP_DIR/music":/music:ro \
  "$IMAGE"

# Step 5: Verify health endpoint
echo "[5/5] Polling /health/live on port $TEST_PORT..."
READY=0
for i in {1..30}; do
    if curl -sf "http://127.0.0.1:${TEST_PORT}/health/live" >/dev/null 2>&1; then
        READY=1
        break
    fi
    sleep 1
done

if [ "$READY" -ne 1 ]; then
    echo "ERROR: Container failed to become healthy within 30 seconds!" >&2
    docker logs "$CONTAINER_NAME" >&2 || true
    exit 1
fi

HEALTH_RESP=$(curl -s "http://127.0.0.1:${TEST_PORT}/health/live")
echo "  ✓ Health response: $HEALTH_RESP"

echo "========================================================================"
echo "PUBLIC RELEASE IMAGE VERIFICATION PASSED: $IMAGE"
echo "========================================================================"
