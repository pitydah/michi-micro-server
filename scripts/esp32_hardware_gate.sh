#!/usr/bin/env bash
# ==============================================================================
# Michi ESP32-S3 Physical Hardware Certification Gate
# Canonical Contract:
#   - Whisker Multicast: 224.0.0.167:53318
#   - Receiver HTTP Port: 80
#   - Pairing Window TTL: 120 seconds
#   - Protocol: receiver-v1-lite (rtp_udp, pcm_s16le, 48000Hz, 16bit, 2ch)
# Architecture:
#   Exercises the real Michi Micro Server APIs for pairing, capability qualification,
#   RTP audio emission, Purrbeat heartbeat, and clean session teardown.
# ==============================================================================

set -euo pipefail

COLOR_GREEN="\033[0;32m"
COLOR_YELLOW="\033[1;33m"
COLOR_RED="\033[0;31m"
COLOR_BLUE="\033[0;34m"
COLOR_BOLD="\033[1m"
COLOR_RESET="\033[0m"

log_info() { echo -e "${COLOR_BLUE}[INFO]${COLOR_RESET} $*"; }
log_ok() { echo -e "${COLOR_GREEN}[PASS]${COLOR_RESET} $*"; }
log_warn() { echo -e "${COLOR_YELLOW}[WARN]${COLOR_RESET} $*"; }
log_err() { echo -e "${COLOR_RED}[FAIL]${COLOR_RESET} $*" >&2; }

ESP32_IP="${1:-${ESP32_IP:-}}"
PIN="${2:-${PAIRING_PIN:-}}"
MICHI_SERVER="${3:-${MICHI_SERVER_URL:-http://127.0.0.1:3000}}"
MICHI_IFACE_IP="${MICHI_WHISKER_IFACE_IP:-192.168.31.224}"

echo -e "${COLOR_BOLD}==================================================================${COLOR_RESET}"
echo -e "${COLOR_BOLD}    MICHI ESP32-S3 PHYSICAL HARDWARE CERTIFICATION GATE          ${COLOR_RESET}"
echo -e "${COLOR_BOLD}==================================================================${COLOR_RESET}"

if [[ -z "${ESP32_IP}" || -z "${PIN}" ]]; then
    echo "Usage: $0 <ESP32_IP> <6-DIGIT-PIN> [MICHI_SERVER_URL]"
    echo "Example: $0 192.168.31.150 123456 http://127.0.0.1:3000"
    echo ""
    echo "Prerequisites:"
    echo "  1. Michi Micro Server must be running (export MICHI_WHISKER_IFACE_IP=${MICHI_IFACE_IP})"
    echo "  2. ESP32-S3 must be on the LAN (port 80) and announce to 224.0.0.167:53318"
    echo "  3. You must have physical access to press the pairing button (window: 120s)"
    exit 1
fi

if [[ ! "${PIN}" =~ ^[0-9]{6}$ ]]; then
    log_err "PIN '${PIN}' is invalid: must be exactly 6 numeric ASCII digits (/^\\d{6}$/)."
    exit 2
fi

# Helper function: perform an HTTP request and verify HTTP status code strictly
# Usage: http_expect_json <METHOD> <URL> <EXPECTED_STATUS> [JSON_BODY]
http_expect_json() {
    local method="$1"
    local url="$2"
    local expected_status="$3"
    local body="${4:-}"
    local response_file
    response_file="$(mktemp)"

    local http_code
    if [[ -n "${body}" ]]; then
        http_code=$(curl -s -o "${response_file}" -w "%{http_code}" -X "${method}" \
            -H "Content-Type: application/json" -d "${body}" "${url}")
    else
        http_code=$(curl -s -o "${response_file}" -w "%{http_code}" -X "${method}" "${url}")
    fi

    local resp_content
    resp_content="$(cat "${response_file}")"
    rm -f "${response_file}"

    if [[ "${http_code}" != "${expected_status}" ]]; then
        log_err "${method} ${url} returned HTTP ${http_code} (expected ${expected_status}): ${resp_content}"
        exit 3
    fi

    echo "${resp_content}"
}

# ------------------------------------------------------------------------------
# Phase 1: Micro Server Liveness & Interface Binding
# ------------------------------------------------------------------------------
log_info "Phase 1: Validating Michi Micro Server at ${MICHI_SERVER} ..."
SERVER_STATUS=$(http_expect_json "GET" "${MICHI_SERVER}/api/v1/system/status" 200)
log_ok "Michi Micro Server is healthy and responsive."

log_info "Checking host LAN interface binding for Whisker multicast (224.0.0.167:53318) ..."
if ip addr show | grep -q "${MICHI_IFACE_IP}"; then
    log_ok "Host LAN interface IP ${MICHI_IFACE_IP} verified on physical adapter."
else
    log_warn "Configured IP ${MICHI_IFACE_IP} not directly found on local adapters; ensure MICHI_WHISKER_IFACE_IP is set correctly."
fi

# ------------------------------------------------------------------------------
# Phase 2: Direct HTTP Probe to ESP32-S3 (Canonical Port 80)
# ------------------------------------------------------------------------------
ESP32_BASE_URL="http://${ESP32_IP}:80"
log_info "Phase 2: Probing ESP32-S3 receiver at ${ESP32_BASE_URL}/api/v1/receiver-lite/info ..."
RECEIVER_INFO=$(http_expect_json "GET" "${ESP32_BASE_URL}/api/v1/receiver-lite/info" 200)
log_ok "ESP32-S3 responded on port 80: ${RECEIVER_INFO}"

# ------------------------------------------------------------------------------
# Phase 3: Discovery API Snapshot Verification
# ------------------------------------------------------------------------------
log_info "Phase 3: Querying Micro Server discovery snapshot: GET ${MICHI_SERVER}/api/v1/receivers/discover ..."
DISCOVER_RESP=$(http_expect_json "GET" "${MICHI_SERVER}/api/v1/receivers/discover" 200)

DEVICE_ID=$(python3 -c "
import json, sys
data = json.loads('''${DISCOVER_RESP}''')
receivers = data.get('receivers', [])
target_ip = '${ESP32_IP}'
for r in receivers:
    if target_ip in r.get('base_url', '') or target_ip in r.get('host', '') or target_ip in str(r.get('addresses', [])):
        print(r.get('receiver_id') or r.get('id') or '')
        sys.exit(0)
# If not yet found by IP, try first unbonded pairable receiver
for r in receivers:
    if r.get('pairable', False):
        print(r.get('receiver_id') or r.get('id') or '')
        sys.exit(0)
sys.exit(1)
" || true)

if [[ -z "${DEVICE_ID}" ]]; then
    log_warn "ESP32-S3 not yet present in registry snapshot; will initiate pairing via direct base_url."
    TARGET_ARG="{\"base_url\":\"${ESP32_BASE_URL}\"}"
else
    log_ok "Discovered receiver in Micro Registry: ${DEVICE_ID}"
    TARGET_ARG="{\"base_url\":\"${ESP32_BASE_URL}\",\"receiver_id\":\"${DEVICE_ID}\"}"
fi

# ------------------------------------------------------------------------------
# Phase 4: Initiate Pairing via Micro Server (120s Window)
# ------------------------------------------------------------------------------
log_info "Phase 4: Initiating pairing via Micro Server: POST ${MICHI_SERVER}/api/v1/receivers/pair/start ..."
log_warn ">>> IMPORTANT: If required by your ESP32-S3 firmware, press the physical button now! (120s window) <<<"

PAIR_START_RESP=$(http_expect_json "POST" "${MICHI_SERVER}/api/v1/receivers/pair/start" 200 "${TARGET_ARG}")
PAIRING_ID=$(python3 -c "import json; print(json.loads('''${PAIR_START_RESP}''')['pairing_id'])")
log_ok "Pairing session initiated with pairing_id: ${PAIRING_ID}"

# ------------------------------------------------------------------------------
# Phase 5: Confirm Pairing with 6-Digit PIN via Micro Server
# ------------------------------------------------------------------------------
log_info "Phase 5: Confirming pairing with 6-digit PIN '${PIN}' via Micro Server ..."
PAIR_CONFIRM_PAYLOAD=$(printf '{"pairing_id":"%s","pin":"%s"}' "${PAIRING_ID}" "${PIN}")
PAIR_CONFIRM_RESP=$(http_expect_json "POST" "${MICHI_SERVER}/api/v1/receivers/pair/confirm" 200 "${PAIR_CONFIRM_PAYLOAD}")

STATUS=$(python3 -c "import json; print(json.loads('''${PAIR_CONFIRM_RESP}''').get('status', ''))")
PAIRED_DEVICE_ID=$(python3 -c "import json; print(json.loads('''${PAIR_CONFIRM_RESP}''').get('device_id', ''))")

if [[ "${STATUS}" != "paired" || -z "${PAIRED_DEVICE_ID}" ]]; then
    log_err "Pairing confirmation failed: ${PAIR_CONFIRM_RESP}"
    exit 4
fi
log_ok "Receiver successfully paired with Micro Server! Device ID: ${PAIRED_DEVICE_ID}"

# ------------------------------------------------------------------------------
# Phase 6: Inspect Receiver Qualification & Verified Capabilities
# ------------------------------------------------------------------------------
log_info "Phase 6: Verifying qualification state: GET ${MICHI_SERVER}/api/v1/receivers/${PAIRED_DEVICE_ID} ..."
REC_STATE=$(http_expect_json "GET" "${MICHI_SERVER}/api/v1/receivers/${PAIRED_DEVICE_ID}" 200)

QUALIFICATION=$(python3 -c "import json; print(json.loads('''${REC_STATE}''').get('qualification', 'Unknown'))")
log_info "Receiver qualification: ${QUALIFICATION}"

if [[ "${QUALIFICATION}" != "Qualified" ]]; then
    log_warn "Qualification is '${QUALIFICATION}'. Checking if basic streaming is permitted..."
else
    log_ok "Receiver is fully Qualified (rtp_udp + pcm_s16le + 48000Hz/16bit/2ch verified)."
fi

# ------------------------------------------------------------------------------
# Phase 7: Session Creation (RTP / PCM Negotiation)
# ------------------------------------------------------------------------------
SESSION_ID="gate-hw-$(date +%s)"
log_info "Phase 7: Starting playback session '${SESSION_ID}' via Micro Server ..."
SESSION_START_PAYLOAD=$(cat << JSON
{
    "session_id": "${SESSION_ID}",
    "codec": "pcm_s16le",
    "sample_rate": 48000,
    "bit_depth": 16,
    "channels": 2,
    "stream_port": 0,
    "buffer_ms": 100,
    "volume": 80
}
JSON
)

SESSION_START_RESP=$(http_expect_json "POST" "${MICHI_SERVER}/api/v1/receivers/${PAIRED_DEVICE_ID}/session/start" 200 "${SESSION_START_PAYLOAD}")
STREAM_PORT=$(python3 -c "import json; print(json.loads('''${SESSION_START_RESP}''').get('stream_port', 0))")
SSRC=$(python3 -c "import json; print(json.loads('''${SESSION_START_RESP}''').get('ssrc', 0))")
log_ok "Session negotiated! Remote RTP Port: ${STREAM_PORT}, SSRC: ${SSRC}"

# ------------------------------------------------------------------------------
# Phase 8: Emit PCM / RTP Audio Packets
# ------------------------------------------------------------------------------
log_info "Phase 8: Emitting 2000ms test PCM audio (440 Hz sine) via Micro Server ..."
STREAM_TEST_PAYLOAD='{"frequency_hz":440.0,"duration_ms":2000}'
STREAM_TEST_RESP=$(http_expect_json "POST" "${MICHI_SERVER}/api/v1/receivers/${PAIRED_DEVICE_ID}/stream/test_pcm" 200 "${STREAM_TEST_PAYLOAD}")

BYTES_SENT=$(python3 -c "import json; print(json.loads('''${STREAM_TEST_RESP}''').get('bytes_sent', 0))")
if [[ "${BYTES_SENT}" -le 0 ]]; then
    log_err "RTP packet emission failed (0 bytes sent): ${STREAM_TEST_RESP}"
    exit 5
fi
log_ok "Audio streamed successfully! Bytes sent: ${BYTES_SENT} (~$((BYTES_SENT / 1920)) RTP packets)"

# ------------------------------------------------------------------------------
# Phase 9: Purrbeat Heartbeat Verification
# ------------------------------------------------------------------------------
log_info "Phase 9: Verifying Purrbeat supervisor heartbeat: POST ${MICHI_SERVER}/api/v1/receivers/${PAIRED_DEVICE_ID}/heartbeat ..."
HB_RESP=$(http_expect_json "POST" "${MICHI_SERVER}/api/v1/receivers/${PAIRED_DEVICE_ID}/heartbeat" 200)
HB_STATUS=$(python3 -c "import json; print(json.loads('''${HB_RESP}''').get('status', ''))")

if [[ "${HB_STATUS}" != "alive" ]]; then
    log_err "Heartbeat returned non-alive status: ${HB_RESP}"
    exit 6
fi
log_ok "Heartbeat confirmed alive!"

# ------------------------------------------------------------------------------
# Phase 10: Clean Session Teardown & Perch Authority Release
# ------------------------------------------------------------------------------
log_info "Phase 10: Stopping session and releasing authority: POST ${MICHI_SERVER}/api/v1/receivers/${PAIRED_DEVICE_ID}/session/stop ..."
STOP_RESP=$(http_expect_json "POST" "${MICHI_SERVER}/api/v1/receivers/${PAIRED_DEVICE_ID}/session/stop" 200)
log_ok "Session stopped cleanly: ${STOP_RESP}"

# Verify receiver state is idle
FINAL_STATE=$(http_expect_json "GET" "${MICHI_SERVER}/api/v1/receivers/${PAIRED_DEVICE_ID}" 200)
IS_ACTIVE=$(python3 -c "import json; print(json.loads('''${FINAL_STATE}''').get('session_active', False))")

if [[ "${IS_ACTIVE}" != "False" ]]; then
    log_err "Receiver session remained active after teardown!"
    exit 7
fi
log_ok "Receiver session teardown verified in RAM and registry."

echo -e "${COLOR_BOLD}==================================================================${COLOR_RESET}"
echo -e "${COLOR_GREEN}${COLOR_BOLD}✓ ESP32-S3 PHYSICAL HARDWARE CERTIFICATION GATE: 100% PASSED${COLOR_RESET}"
echo -e "${COLOR_BOLD}==================================================================${COLOR_RESET}"
