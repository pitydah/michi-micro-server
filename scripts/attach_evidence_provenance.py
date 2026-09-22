#!/usr/bin/env python3
"""
Evidence Provenance Attacher for Michi Micro Server Release Gates.

Loads an existing rich evidence artifact (e.g. produced by verify_zimaos_install.py or
qualify_rpi_runtime.py), attaches standard release gate provenance metadata (schema_version,
gate_id, commit_sha, evidence_class, producer, exit_code, status, finished_at), preserves
all tool-specific rich metrics and fields, and writes the resulting JSON atomically.
"""

import argparse
import json
import os
import platform
import subprocess
import sys
import tempfile
from datetime import datetime, timezone
from pathlib import Path

VALID_CLASSES = {
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

def get_head_sha() -> str:
    try:
        return subprocess.check_output(
            ["git", "rev-parse", "HEAD"],
            text=True,
        ).strip()
    except Exception:
        return os.getenv("GITHUB_SHA", "0" * 40)

def now_utc() -> str:
    return datetime.now(timezone.utc).isoformat()

def main() -> int:
    parser = argparse.ArgumentParser(description="Attach GitHub Actions provenance to evidence JSON")
    parser.add_argument("--gate", required=True, help="Release gate identifier (e.g. casaos-zimaos-real)")
    parser.add_argument("--class", dest="evidence_class", required=True, help="Evidence class")
    parser.add_argument("--job-status", required=True, help="GitHub Actions job status (success, failure, cancelled)")
    parser.add_argument("--file", required=True, help="Path to evidence JSON file to update or create")
    parser.add_argument("--commit-sha", default=None, help="Target commit SHA (default: git rev-parse HEAD)")
    parser.add_argument("--producer-job", default=None, help="Explicit producer job name override")
    parser.add_argument("--detail", default=None, help="Optional description or detail message")
    args = parser.parse_args()

    if args.evidence_class not in VALID_CLASSES:
        print(f"ERROR: invalid evidence class: {args.evidence_class}", file=sys.stderr)
        sys.exit(1)

    file_path = Path(args.file)
    existing_data = {}
    if file_path.exists():
        try:
            with open(file_path, "r", encoding="utf-8") as f:
                existing_data = json.load(f)
            if not isinstance(existing_data, dict):
                existing_data = {}
        except Exception as e:
            print(f"WARNING: Could not parse existing evidence file {file_path}: {e}", file=sys.stderr)
            existing_data = {}

    job_status = args.job_status.strip().lower()
    passed = (job_status == "success") and (existing_data.get("status") != "FAIL")
    commit_sha = args.commit_sha or existing_data.get("commit_sha") or get_head_sha()

    producer_job = (
        args.producer_job
        or os.getenv("GITHUB_JOB")
        or (existing_data.get("producer", {}).get("github_job") if isinstance(existing_data.get("producer"), dict) else None)
        or args.gate
    )

    # Build producer object preserving any existing producer details
    producer = {}
    if isinstance(existing_data.get("producer"), dict):
        producer.update(existing_data["producer"])

    producer.update({
        "github_run_id": os.getenv("GITHUB_RUN_ID") or producer.get("github_run_id"),
        "github_run_attempt": os.getenv("GITHUB_RUN_ATTEMPT") or producer.get("github_run_attempt"),
        "github_job": producer_job,
        "github_workflow": os.getenv("GITHUB_WORKFLOW") or producer.get("github_workflow"),
        "github_ref": os.getenv("GITHUB_REF") or producer.get("github_ref"),
        "github_ref_name": os.getenv("GITHUB_REF_NAME") or producer.get("github_ref_name"),
        "github_ref_type": os.getenv("GITHUB_REF_TYPE") or producer.get("github_ref_type"),
        "runner_name": os.getenv("RUNNER_NAME") or producer.get("runner_name"),
        "runner_os": os.getenv("RUNNER_OS") or producer.get("runner_os"),
        "runner_arch": os.getenv("RUNNER_ARCH") or producer.get("runner_arch"),
        "platform_machine": platform.machine(),
    })

    # Start with existing rich data so that all nested fields are preserved
    artifact = dict(existing_data)

    artifact["schema_version"] = 1
    artifact["gate_id"] = args.gate
    artifact["commit_sha"] = commit_sha
    artifact["evidence_class"] = args.evidence_class
    artifact["status"] = "PASS" if passed else "FAIL"
    artifact["exit_code"] = 0 if passed else 1
    artifact["finished_at"] = now_utc()
    artifact["producer"] = producer

    if args.detail:
        artifact["detail"] = args.detail
    elif "detail" not in artifact:
        artifact["detail"] = f"Job {producer_job} finished with {job_status}"

    file_path.parent.mkdir(parents=True, exist_ok=True)
    
    # Write atomically
    temp_dir = file_path.parent
    with tempfile.NamedTemporaryFile("w", dir=temp_dir, delete=False, encoding="utf-8") as tf:
        json.dump(artifact, tf, indent=2, sort_keys=True)
        temp_name = tf.name

    os.replace(temp_name, str(file_path))
    print(f"✓ Attached provenance to {args.gate} ({artifact['status']}) -> {file_path}")
    return 0

if __name__ == "__main__":
    sys.exit(main())
