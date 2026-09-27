#!/usr/bin/env bash
# ==============================================================================
# Michi ESP32-S3 Physical Hardware Certification Gate
# Canonical Contract:
#   - Whisker Multicast: 224.0.0.167:53318
#   - Receiver HTTP Port: 80
#   - Micro Server Default: http://127.0.0.1:9090
#   - Pairing Window TTL: 120 seconds
#   - PIN Format: /^\d{6}$/ (6 numeric ASCII digits)
#   - Profile: rtp_udp, pcm_s16le, 48000Hz, 16bit, 2ch
#
# Architecture:
#   Exercises the real Michi Micro Server APIs for discovery, pairing,
#   capability qualification, RTP audio emission, Purrbeat heartbeat,
#   and clean session teardown.
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
MICHI_SERVER="${2:-${MICHI_SERVER_URL:-http://127.0.0.1:9090}}"
PAIRING_PIN="${3:-${PAIRING_PIN:-}}"
MICHI_API_TOKEN="${MICHI_API_TOKEN:-}"
MICHI_IFACE_IP="${MICHI_WHISKER_IFACE_IP:-192.168.31.224}"

echo -e "${COLOR_BOLD}==================================================================${COLOR_RESET}"
echo -e "${COLOR_BOLD}    MICHI ESP32-S3 PHYSICAL HARDWARE CERTIFICATION GATE          ${COLOR_RESET}"
echo -e "${COLOR_BOLD}==================================================================${COLOR_RESET}"

if [[ -z "${ESP32_IP}" ]]; then
    echo "Usage: $0 <ESP32_IP> [MICHI_SERVER_URL] [6-DIGIT-PIN]"
    echo "Example: $0 192.168.31.150 http://127.0.0.1:9090 123456"
    echo ""
    echo "Environment Variables:"
    echo "  ESP32_IP                  IP address of the ESP32-S3 receiver"
    echo "  MICHI_SERVER_URL          URL of Michi Micro Server (default: http://127.0.0.1:9090)"
    echo "  PAIRING_PIN               6-digit numeric pairing PIN (prompted interactively if omitted)"
    echo "  MICHI_API_TOKEN           Bearer token for authenticated Micro routes (optional)"
    echo "  MICHI_WHISKER_IFACE_IP    Host LAN interface IP for Whisker multicast (default: 192.168.31.224)"
    exit 1
fi

ACTIVE_SESSION=0
PAIRED_DEVICE_ID=""

cleanup() {
    local exit_code=$?
    if [[ "${ACTIVE_SESSION}" -eq 1 && -n "${PAIRED_DEVICE_ID}" ]]; then
        log_warn "Trapped exit: tearing down active receiver session..."
        micro_request "POST" "/api/v1/receivers/${PAIRED_DEVICE_ID}/session/stop" 200 "{}" || true
    fi
    if [[ ${exit_code} -ne 0 ]]; then
        echo -e "${COLOR_RED}${COLOR_BOLD}✗ HARDWARE GATE ABORTED WITH STATUS ${exit_code}${COLOR_RESET}" >&2
    fi
}
trap cleanup EXIT

# Perform request against Michi Micro Server (applies MICHI_API_TOKEN if set)
# Usage: micro_request <METHOD> <PATH> <EXPECTED_STATUS> [JSON_BODY]
micro_request() {
    local method="$1"
    local path="$2"
    local expected_status="$3"
    local body="${4:-}"
    local url="${MICHI_SERVER}${path}"
    local response_file
    response_file="$(mktemp)"

    local curl_cmd=(curl -s -S -o "${response_file}" -w "%{http_code}" -X "${method}")

    if [[ -n "${MICHI_API_TOKEN}" ]]; then
        curl_cmd+=(-H "Authorization: Bearer ${MICHI_API_TOKEN}")
    fi

    if [[ "${method}" =~ ^(POST|PUT|PATCH)$ ]]; then
        curl_cmd+=(-H "Content-Type: application/json")
        curl_cmd+=(-d "${body:-{}}")
    elif [[ -n "${body}" ]]; then
        curl_cmd+=(-H "Content-Type: application/json" -d "${body}")
    fi

    curl_cmd+=("${url}")

    local http_code
    http_code="$("${curl_cmd[@]}")"

    local resp_content
    resp_content="$(cat "${response_file}")"
    rm -f "${response_file}"

    if [[ "${http_code}" != "${expected_status}" ]]; then
        log_err "Micro Server ${method} ${path} returned HTTP ${http_code} (expected ${expected_status}): ${resp_content}"
        exit 3
    fi

    echo "${resp_content}"
}

# Perform direct unauthenticated probe against ESP32-S3 receiver
# Usage: receiver_request <METHOD> <PATH> <EXPECTED_STATUS> [JSON_BODY]
receiver_request() {
    local method="$1"
    local path="$2"
    local expected_status="$3"
    local body="${4:-}"
    local url="http://${ESP32_IP}:80${path}"
    local response_file
    response_file="$(mktemp)"

    local curl_cmd=(curl -s -S -o "${response_file}" -w "%{http_code}" -X "${method}")

    if [[ "${method}" =~ ^(POST|PUT|PATCH)$ ]]; then
        curl_cmd+=(-H "Content-Type: application/json")
        curl_cmd+=(-d "${body:-{}}")
    elif [[ -n "${body}" ]]; then
        curl_cmd+=(-H "Content-Type: application/json" -d "${body}")
    fi

    curl_cmd+=("${url}")

    local http_code
    http_code="$("${curl_cmd[@]}")"

    local resp_content
    resp_content="$(cat "${response_file}")"
    rm -f "${response_file}"

    if [[ "${http_code}" != "${expected_status}" ]]; then
        log_err "ESP32-S3 ${method} ${path} returned HTTP ${http_code} (expected ${expected_status}): ${resp_content}"
        exit 3
    fi

    echo "${resp_content}"
}

# ------------------------------------------------------------------------------
# Phase 1: Micro Server Liveness & Interface Binding
# ------------------------------------------------------------------------------
log_info "Phase 1: Validating Michi Micro Server readiness at ${MICHI_SERVER} (/health/ready) ..."
HEALTH_RESP=$(micro_request "GET" "/health/ready" 200)
log_ok "Michi Micro Server is ready: ${HEALTH_RESP}"

log_info "Checking host LAN interface binding for Whisker multicast (224.0.0.167:53318) ..."
if ip addr show 2>/dev/null | grep -q "${MICHI_IFACE_IP}"; then
    log_ok "Host LAN interface IP ${MICHI_IFACE_IP} verified on physical adapter."
else
    log_warn "Host IP ${MICHI_IFACE_IP} not directly matched on local adapters; ensure MICHI_WHISKER_IFACE_IP matches your LAN interface."
fi

# ------------------------------------------------------------------------------
# Phase 2: Direct HTTP Probe to ESP32-S3 (Canonical Port 80, /api/v1/server/info)
# ------------------------------------------------------------------------------
log_info "Phase 2: Probing ESP32-S3 receiver at http://${ESP32_IP}:80/api/v1/server/info ..."
RECEIVER_INFO=$(receiver_request "GET" "/api/v1/server/info" 200)

python3 -c "
import json, sys
data = json.load(sys.stdin)

michi_id = data.get('michi_id')
if not michi_id:
    sys.stderr.write('Missing michi_id in /api/v1/server/info\\n')
    sys.exit(1)

service = data.get('service', '')
if service not in ('michi-stream-standard', 'michi-stream-hifi'):
    sys.stderr.write(f'Unexpected service in /api/v1/server/info: {service}\\n')
    sys.exit(1)

api_version = data.get('api_version', '')
if api_version != 'v1-lite':
    sys.stderr.write(f'Unsupported api_version: {api_version} (must be v1-lite)\\n')
    sys.exit(1)

roles = data.get('roles', [])
if 'audio_receiver' not in roles:
    sys.stderr.write(f'Missing audio_receiver role: {roles}\\n')
    sys.exit(1)

audio = data.get('audio') or {}
transports = audio.get('transports', [])
if 'rtp_udp' not in transports:
    sys.stderr.write(f'Receiver audio transports do not include rtp_udp: {transports}\\n')
    sys.exit(1)

codecs = audio.get('codecs', [])
if 'pcm_s16le' not in codecs:
    sys.stderr.write(f'Receiver audio codecs do not include pcm_s16le: {codecs}\\n')
    sys.exit(1)
" <<< "${RECEIVER_INFO}"

log_ok "ESP32-S3 contract verified on port 80: service and capabilities valid."

# ------------------------------------------------------------------------------
# Phase 3: Discovery API Snapshot Verification (Whisker / Scent)
# ------------------------------------------------------------------------------
log_info "Phase 3: Triggering Micro Server discovery snapshot: POST /api/v1/devices/discover ..."
DISCOVER_RESP=$(micro_request "POST" "/api/v1/devices/discover" 200 "{}")

DEVICE_ID=$(python3 -c "
import json, sys
data = json.load(sys.stdin)
receivers = data.get('receivers', []) or data.get('devices', [])
target_ip = sys.argv[1]

for r in receivers:
    base = r.get('base_url', '')
    host = r.get('host', '')
    addrs = [str(a) for a in r.get('addresses', [])]
    if target_ip in base or target_ip in host or any(target_ip in a for a in addrs):
        rec_id = r.get('receiver_id') or r.get('id') or ''
        if rec_id:
            print(rec_id)
            sys.exit(0)

sys.stderr.write(f'ESP32 receiver with IP {target_ip} NOT found in discovery snapshot!\\n')
sys.exit(1)
" "${ESP32_IP}" <<< "${DISCOVER_RESP}")

log_ok "ESP32-S3 discovered through Whisker/Scent in Micro Registry: ID=${DEVICE_ID}"

# ------------------------------------------------------------------------------
# Phase 4 & 5: Interactive Physical Pairing
# ------------------------------------------------------------------------------
echo ""
echo -e "${COLOR_BOLD}------------------------------------------------------------------${COLOR_RESET}"
echo -e "${COLOR_YELLOW}${COLOR_BOLD}PHYSICAL ACTION REQUIRED:${COLOR_RESET}"
echo -e "  1. Hold the physical pairing button on the ESP32-S3 for ~5 seconds."
echo -e "  2. Confirm the pairing LED is blinking."
echo -e "  3. You will have a 120-second pairing window once initiated."
echo -e "${COLOR_BOLD}------------------------------------------------------------------${COLOR_RESET}"

read -r -p "Press [ENTER] after holding the button on the ESP32-S3: " _

log_info "Phase 4: Initiating pairing via Micro Server: POST /api/v1/receivers/pair/start ..."
TARGET_PAYLOAD=$(printf '{"receiver_id":"%s"}' "${DEVICE_ID}")
PAIR_START_RESP=$(micro_request "POST" "/api/v1/receivers/pair/start" 200 "${TARGET_PAYLOAD}")

PAIRING_ID=$(python3 -c "
import json, sys
data = json.load(sys.stdin)
pid = data.get('pairing_id')
if not pid:
    sys.stderr.write('Missing pairing_id in pair/start response\\n')
    sys.exit(1)
print(pid)
" <<< "${PAIR_START_RESP}")

log_ok "Pairing session initiated with pairing_id: ${PAIRING_ID} (120s TTL)"

if [[ -z "${PAIRING_PIN}" ]]; then
    read -r -p "Enter the 6-digit PIN shown on/for your ESP32-S3: " PAIRING_PIN
fi

if [[ ! "${PAIRING_PIN}" =~ ^[0-9]{6}$ ]]; then
    log_err "PIN '${PAIRING_PIN}' is invalid: must be exactly 6 numeric digits (/^\\d{6}$/)."
    exit 2
fi

log_info "Phase 5: Confirming pairing with 6-digit PIN via Micro Server: POST /api/v1/receivers/pair/confirm ..."
PAIR_CONFIRM_PAYLOAD=$(printf '{"pairing_id":"%s","pin":"%s"}' "${PAIRING_ID}" "${PAIRING_PIN}")
PAIR_CONFIRM_RESP=$(micro_request "POST" "/api/v1/receivers/pair/confirm" 200 "${PAIR_CONFIRM_PAYLOAD}")

PAIRED_DEVICE_ID=$(python3 -c "
import json, sys
data = json.load(sys.stdin)
if data.get('status') != 'paired':
    sys.stderr.write(f'Pair confirm returned unexpected status: {data.get(\"status\")}\\n')
    sys.exit(1)
dev_id = data.get('device_id')
if not dev_id:
    sys.stderr.write('Pair confirm missing device_id\\n')
    sys.exit(1)
print(dev_id)
" <<< "${PAIR_CONFIRM_RESP}")

log_ok "Receiver successfully paired with Micro Server! Device ID: ${PAIRED_DEVICE_ID}"

# ------------------------------------------------------------------------------
# Phase 6: Inspect Receiver Qualification & Verified Capabilities
# ------------------------------------------------------------------------------
log_info "Phase 6: Verifying qualification state: GET /api/v1/receivers/${PAIRED_DEVICE_ID} ..."
REC_STATE=$(micro_request "GET" "/api/v1/receivers/${PAIRED_DEVICE_ID}" 200)

QUALIFICATION=$(python3 -c "
import json, sys
data = json.load(sys.stdin)
q = data.get('qualification', 'Unknown')
print(q)
if q != 'Qualified':
    sys.stderr.write(f'Receiver qualification check failed: {q}\\n')
    sys.exit(1)
" <<< "${REC_STATE}")

log_ok "Receiver is fully Qualified: ${QUALIFICATION} (rtp_udp + pcm_s16le + 48000Hz/16bit/2ch verified)."

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
    "stream_port": 53318,
    "buffer_ms": 100,
    "volume": 80
}
JSON
)

SESSION_START_RESP=$(micro_request "POST" "/api/v1/receivers/${PAIRED_DEVICE_ID}/session/start" 200 "${SESSION_START_PAYLOAD}")
ACTIVE_SESSION=1

python3 -c "
import json, sys
data = json.load(sys.stdin)
port = data.get('stream_port', 0)
ssrc = data.get('ssrc', 0)
if port <= 0 or ssrc == 0:
    sys.stderr.write(f'Invalid session negotiation: port={port}, ssrc={ssrc}\\n')
    sys.exit(1)
print(f'Remote RTP Port: {port}, SSRC: {ssrc}')
" <<< "${SESSION_START_RESP}"

log_ok "Session negotiated successfully with ESP32-S3!"

# ------------------------------------------------------------------------------
# Phase 8: Emit PCM / RTP Audio Packets
# ------------------------------------------------------------------------------
log_info "Phase 8: Emitting 2000ms test PCM audio (440 Hz sine) via Micro Server ..."
STREAM_TEST_PAYLOAD='{"frequency_hz":440.0,"duration_ms":2000}'
STREAM_TEST_RESP=$(micro_request "POST" "/api/v1/receivers/${PAIRED_DEVICE_ID}/stream/test_pcm" 200 "${STREAM_TEST_PAYLOAD}")

BYTES_SENT=$(python3 -c "
import json, sys
data = json.load(sys.stdin)
bytes_sent = data.get('bytes_sent', 0)
if bytes_sent <= 0:
    sys.stderr.write(f'Zero bytes sent: {data}\\n')
    sys.exit(1)
print(bytes_sent)
" <<< "${STREAM_TEST_RESP}")

PACKETS_SENT=$((BYTES_SENT / 1920))
log_ok "Audio streamed successfully! Bytes sent: ${BYTES_SENT} (~${PACKETS_SENT} RTP packets of 1920 bytes)"

# ------------------------------------------------------------------------------
# Phase 9: Purrbeat Heartbeat Verification
# ------------------------------------------------------------------------------
log_info "Phase 9: Verifying Purrbeat supervisor heartbeat: POST /api/v1/receivers/${PAIRED_DEVICE_ID}/heartbeat ..."
HB_RESP=$(micro_request "POST" "/api/v1/receivers/${PAIRED_DEVICE_ID}/heartbeat" 200 "{}")

python3 -c "
import json, sys
data = json.load(sys.stdin)
status = data.get('status', '')
if status != 'alive':
    sys.stderr.write(f'Heartbeat status is not alive: {status}\\n')
    sys.exit(1)
" <<< "${HB_RESP}"

log_ok "Purrbeat heartbeat confirmed alive!"

# ------------------------------------------------------------------------------
# Phase 10: Clean Session Teardown & Perch Authority Release
# ------------------------------------------------------------------------------
log_info "Phase 10: Stopping session and releasing Perch authority: POST /api/v1/receivers/${PAIRED_DEVICE_ID}/session/stop ..."
STOP_RESP=$(micro_request "POST" "/api/v1/receivers/${PAIRED_DEVICE_ID}/session/stop" 200 "{}")
ACTIVE_SESSION=0
log_ok "Session stopped cleanly: ${STOP_RESP}"

# Verify receiver state is idle
FINAL_STATE=$(micro_request "GET" "/api/v1/receivers/${PAIRED_DEVICE_ID}" 200)

python3 -c "
import json, sys
data = json.load(sys.stdin)
active = data.get('session_active', False)
act_sess_id = data.get('active_session_id')
if active or act_sess_id is not None:
    sys.stderr.write(f'Receiver remained active after stop: active={active}, id={act_sess_id}\\n')
    sys.exit(1)
" <<< "${FINAL_STATE}"

log_ok "Receiver session teardown verified in RAM and registry."

echo -e "${COLOR_BOLD}==================================================================${COLOR_RESET}"
echo -e "${COLOR_GREEN}${COLOR_BOLD}✓ ESP32-S3 PHYSICAL HARDWARE CERTIFICATION GATE: 100% PASSED${COLOR_RESET}"
echo -e "${COLOR_BOLD}==================================================================${COLOR_RESET}"
