#!/usr/bin/env python3
"""
Release Evidence Taxonomy and Validation Helpers for Michi Micro Server.

Validates evidence payload schemas, evidence classes, and SHA provenance.
"""

import datetime
import json
import os
import subprocess
from typing import Dict, Any, Tuple, Optional

VALID_EVIDENCE_CLASSES = {
    "STATIC_ANALYSIS",
    "UNIT",
    "CONTRACT_MOCK",
    "CONTRACT_SIMULATOR",
    "INTEGRATION_REAL",
    "CONTAINER_NATIVE",
    "EMULATED_ARCH",
    "PHYSICAL_HARDWARE",
    "LONG_SOAK",
}

VALID_STATUSES = {"PASS", "FAIL", "BLOCKED_EXTERNAL", "NOT_RUN", "STALE", "INVALID_EVIDENCE"}

def now_utc_iso() -> str:
    return datetime.datetime.now(datetime.timezone.utc).isoformat()

def get_head_sha() -> str:
    try:
        return subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()
    except Exception:
        return "UNKNOWN_SHA"

def validate_evidence_artifact(data: Dict[str, Any], current_sha: str, accepted_classes: list) -> Tuple[str, str]:
    """
    Evaluates an evidence artifact against current commit SHA and accepted classes.
    Returns (status, reason_detail).
    """
    if not isinstance(data, dict):
        return "INVALID_EVIDENCE", "Artifact payload is not a valid JSON object"

    if data.get("schema_version") != 1:
        return "INVALID_EVIDENCE", f"Unsupported schema_version: {data.get('schema_version')}"

    artifact_sha = data.get("commit_sha")
    if not artifact_sha:
        return "INVALID_EVIDENCE", "Missing commit_sha in artifact"

    # Verify commit provenance
    if artifact_sha != current_sha and current_sha != "UNKNOWN_SHA":
        return "STALE", f"Artifact SHA ({artifact_sha[:8]}) differs from HEAD ({current_sha[:8]})"

    ev_class = data.get("evidence_class")
    if ev_class not in VALID_EVIDENCE_CLASSES:
        return "INVALID_EVIDENCE", f"Invalid evidence_class: {ev_class}"

    if ev_class not in accepted_classes:
        return "INVALID_EVIDENCE", f"Evidence class {ev_class} not accepted (requires one of {accepted_classes})"

    raw_status = data.get("status")
    if raw_status not in {"PASS", "FAIL", "BLOCKED_EXTERNAL"}:
        return "INVALID_EVIDENCE", f"Invalid raw status: {raw_status}"

    if raw_status == "PASS":
        exit_code = data.get("exit_code", 0)
        if exit_code != 0:
            return "FAIL", f"Status declared PASS but exit_code is {exit_code}"
        return "PASS", data.get("detail", "Evidence certified")
    elif raw_status == "BLOCKED_EXTERNAL":
        return "BLOCKED_EXTERNAL", data.get("detail", "Blocked by external prerequisite")
    else:
        return "FAIL", data.get("detail", f"Execution failed (exit code {data.get('exit_code')})")
