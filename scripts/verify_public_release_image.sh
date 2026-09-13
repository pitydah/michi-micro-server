#!/usr/bin/env bash
# Verifies that a published container image:
# 1. Matches expected build digest (REMOTE_INDEX_DIGEST == BUILD_DIGEST)
# 2. Has valid OCI multi-arch manifests for linux/amd64 and linux/arm64 without duplicates
# 3. Can be pulled anonymously without GHCR/registry credentials
# 4. Boots successfully and responds to /health/live
# 5. Emits structured ghcr-public-release-evidence.json
set -euo pipefail

IMAGE="${1:-}"
EXPECTED_DIGEST="${2:-}"
TEST_PORT="${3:-9095}"
EVIDENCE_OUTPUT="${4:-target/release-evidence/ghcr-public-release-evidence.json}"
TAG="${GITHUB_REF_NAME:-}"
COMMIT="${GITHUB_SHA:-}"

if [ -z "$IMAGE" ] || [ -z "$EXPECTED_DIGEST" ]; then
    echo "Usage: $0 <image-reference> <expected-build-digest> [test-port] [evidence-output]" >&2
    echo "Example: $0 ghcr.io/pitydah/michi-micro-server:1.0.0-rc.2 sha256:abcd... 9090" >&2
    exit 1
fi

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
echo "Expected Build Digest: $EXPECTED_DIGEST"
echo "========================================================================"

# Step 1: Strict credential isolation (logout to ensure anonymous inspection & pull)
echo "[1/5] Isolating credentials (docker logout ghcr.io)..."
docker logout ghcr.io >/dev/null 2>&1 || true

# Step 2: Fetch remote OCI image index without credentials
echo "[2/5] Fetching remote OCI index (docker buildx imagetools inspect --raw)..."
mkdir -p "$TMP_DIR"
RAW_MANIFEST="$TMP_DIR/raw_index.json"
docker buildx imagetools inspect --raw "$IMAGE" > "$RAW_MANIFEST"

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

# Step 6: Verify digest match, platforms, and emit structured evidence
echo "Recording release evidence and verifying index digest..."
python3 scripts/verify_public_release_image.py \
  --raw-manifest-file "$RAW_MANIFEST" \
  --expected-digest "$EXPECTED_DIGEST" \
  --tag "${TAG:-v1.0.0-rc.2}" \
  --commit "${COMMIT:-0000000000000000000000000000000000000000}" \
  --image "$IMAGE" \
  --output-evidence "$EVIDENCE_OUTPUT" \
  --anonymous-pull-verified \
  --runtime-health-verified

echo "========================================================================"
echo "PUBLIC RELEASE IMAGE VERIFICATION PASSED: $IMAGE"
echo "Evidence written to: $EVIDENCE_OUTPUT"
echo "========================================================================"
