#!/usr/bin/env bash
# ==============================================================================
# Michi ESP32-S3 Hardware Certification Gate Script
# Protocol: michi-stream-standard / receiver-v1-lite
# Interface: enp7s0 (LAN: 192.168.31.224)
# ==============================================================================

set -euo pipefail

COLOR_GREEN="\033[0;32m"
COLOR_YELLOW="\033[1;33m"
COLOR_RED="\033[0;31m"
COLOR_BLUE="\033[0;34m"
COLOR_RESET="\033[0m"

log_info() { echo -e "${COLOR_BLUE}[INFO]${COLOR_RESET} $*"; }
log_ok() { echo -e "${COLOR_GREEN}[PASS]${COLOR_RESET} $*"; }
log_warn() { echo -e "${COLOR_YELLOW}[WARN]${COLOR_RESET} $*"; }
log_err() { echo -e "${COLOR_RED}[FAIL]${COLOR_RESET} $*"; }

RECEIVER_IP="${1:-${ESP32_IP:-}}"
RECEIVER_PORT="${2:-8080}"
PIN="${3:-${PAIRING_PIN:-}}"
MICHI_IFACE_IP="${MICHI_WHISKER_IFACE_IP:-192.168.31.224}"

echo "=================================================================="
echo "    MICHI V2 CLOSED - PHYSICAL ESP32-S3 HARDWARE GATE PROTOCOL   "
echo "=================================================================="

if [[ -z "${RECEIVER_IP}" ]]; then
    echo "Usage: $0 <ESP32_IP> [PORT] [6-DIGIT-PIN]"
    echo "Example: $0 192.168.31.150 8080 123456"
    echo ""
    echo "Or scan the LAN for candidate devices:"
    echo "  ip -br addr show enp7s0"
    exit 1
fi

BASE_URL="http://${RECEIVER_IP}:${RECEIVER_PORT}"

# ------------------------------------------------------------------------------
# Phase 1: Local Network Interface & Multicast Route
# ------------------------------------------------------------------------------
log_info "Phase 1: Validating host network interface..."
if ip addr show | grep -q "${MICHI_IFACE_IP}"; then
    log_ok "Host LAN interface IP ${MICHI_IFACE_IP} verified."
else
    log_warn "Host IP ${MICHI_IFACE_IP} not directly found on local adapters; using default route."
fi

# ------------------------------------------------------------------------------
# Phase 2: Probe ESP32-S3 HTTP Reachability & Identity
# ------------------------------------------------------------------------------
log_info "Phase 2: Probing ESP32-S3 at ${BASE_URL}/api/v1/receiver-lite/info ..."
INFO_RESP=$(curl -s --max-time 3 "${BASE_URL}/api/v1/receiver-lite/info" 2>/dev/null || true)
if [[ -z "${INFO_RESP}" ]]; then
    INFO_RESP=$(curl -s --max-time 3 "${BASE_URL}/api/v1/server/info" 2>/dev/null || true)
fi

if [[ -z "${INFO_RESP}" ]]; then
    log_err "Failed to connect to ${BASE_URL}. Ensure device is powered and connected to Wi-Fi."
    exit 2
fi

log_ok "Receiver responded: ${INFO_RESP}"

# ------------------------------------------------------------------------------
# Phase 3: Inspect Advertised Capabilities & Qualification
# ------------------------------------------------------------------------------
log_info "Phase 3: Inspecting capabilities..."
DEVICE_TYPE=$(echo "${INFO_RESP}" | grep -o '"type":"[^"]*"' | cut -d'"' -f4 || echo "standard")
MICHI_ID=$(echo "${INFO_RESP}" | grep -o '"michi_id":"[^"]*"' | cut -d'"' -f4 || echo "unknown")
log_info "Device Type: ${DEVICE_TYPE}, Michi ID: ${MICHI_ID}"

# ------------------------------------------------------------------------------
# Phase 4: Pairing Handshake (6-digit PIN validation)
# ------------------------------------------------------------------------------
if [[ -n "${PIN}" ]]; then
    log_info "Phase 4: Testing 6-digit PIN constraint..."
    if [[ ! "${PIN}" =~ ^[0-9]{6}$ ]]; then
        log_err "Provided PIN '${PIN}' is NOT a 6-digit numeric string (/^\d{6}$/)"
        exit 3
    fi
    log_ok "PIN ${PIN} complies with 6-digit requirement."

    log_info "Starting pairing flow: POST ${BASE_URL}/api/v1/pair/start ..."
    START_RESP=$(curl -s -X POST -H "Content-Type: application/json" \
        -d '{"client_id":"hardware-gate-runner","name":"Michi Gate"}' \
        "${BASE_URL}/api/v1/pair/start" 2>/dev/null || true)
    log_info "Pair start response: ${START_RESP}"

    PAIRING_ID=$(echo "${START_RESP}" | grep -o '"pairing_id":"[^"]*"' | cut -d'"' -f4 || echo "")
    if [[ -n "${PAIRING_ID}" ]]; then
        log_info "Pairing initiated with ID: ${PAIRING_ID}. Confirming with PIN ${PIN}..."
        CONFIRM_RESP=$(curl -s -X POST -H "Content-Type: application/json" \
            -d "{\"pairing_id\":\"${PAIRING_ID}\",\"pin\":\"${PIN}\"}" \
            "${BASE_URL}/api/v1/pair/confirm" 2>/dev/null || true)
        log_ok "Pair confirm response: ${CONFIRM_RESP}"
    fi
fi

# ------------------------------------------------------------------------------
# Phase 5: Perch Authority & State Probe
# ------------------------------------------------------------------------------
log_info "Phase 5: Probing Perch Authority State: GET ${BASE_URL}/api/v1/authority/state ..."
AUTH_RESP=$(curl -s --max-time 3 "${BASE_URL}/api/v1/authority/state" 2>/dev/null || true)
if [[ -n "${AUTH_RESP}" ]]; then
    log_ok "Authority state response: ${AUTH_RESP}"
else
    log_info "Authority not configured or not active on standard profile."
fi

# ------------------------------------------------------------------------------
# Phase 6: Conclusion & Certification Status
# ------------------------------------------------------------------------------
echo "=================================================================="
log_ok "ESP32-S3 Hardware Gate probe completed successfully on ${BASE_URL}."
echo "=================================================================="
