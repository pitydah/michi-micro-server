#!/usr/bin/env python3

import argparse
import json
import os
import platform
import subprocess
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
    return subprocess.check_output(
        ["git", "rev-parse", "HEAD"],
        text=True,
    ).strip()

def now_utc() -> str:
    return datetime.now(timezone.utc).isoformat()

def main() -> int:
    p = argparse.ArgumentParser()
    p.add_argument("--gate", required=True)
    p.add_argument("--class", dest="evidence_class", required=True)
    p.add_argument("--job-status", required=True)
    p.add_argument("--output", required=True)
    p.add_argument("--detail", default="")
    args = p.parse_args()

    if args.evidence_class not in VALID_CLASSES:
        raise SystemExit(
            f"invalid evidence class: {args.evidence_class}"
        )

    job_status = args.job_status.strip().lower()
    passed = job_status == "success"

    artifact = {
        "schema_version": 1,
        "gate_id": args.gate,
        "commit_sha": get_head_sha(),
        "evidence_class": args.evidence_class,
        "status": "PASS" if passed else "FAIL",
        "detail": args.detail or (
            f"GitHub Actions job "
            f"{os.getenv('GITHUB_JOB', 'unknown')} "
            f"finished with {job_status}"
        ),
        "exit_code": 0 if passed else 1,
        "finished_at": now_utc(),
        "producer": {
            "github_run_id": os.getenv("GITHUB_RUN_ID"),
            "github_run_attempt": os.getenv("GITHUB_RUN_ATTEMPT"),
            "github_job": os.getenv("GITHUB_JOB"),
            "github_workflow": os.getenv("GITHUB_WORKFLOW"),
            "github_ref": os.getenv("GITHUB_REF"),
            "github_ref_name": os.getenv("GITHUB_REF_NAME"),
            "github_ref_type": os.getenv("GITHUB_REF_TYPE"),
            "runner_name": os.getenv("RUNNER_NAME"),
            "runner_os": os.getenv("RUNNER_OS"),
            "runner_arch": os.getenv("RUNNER_ARCH"),
            "platform_machine": platform.machine(),
        },
    }

    path = Path(args.output)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        json.dumps(
            artifact,
            indent=2,
            sort_keys=True,
        ),
        encoding="utf-8",
    )

    print(
        f"{args.gate}: "
        f"{artifact['status']} "
        f"-> {path}"
    )

    # No debe esconder el resultado original del job.
    return 0

if __name__ == "__main__":
    raise SystemExit(main())
