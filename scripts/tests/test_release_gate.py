#!/usr/bin/env python3
import pytest
import sys
import os

ROOT_DIR = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
sys.path.insert(0, os.path.join(ROOT_DIR, "scripts"))

from release_evidence import validate_evidence_artifact
from generate_release_gate import aggregate_gates

def test_same_sha_pass():
    art = {
        "schema_version": 1,
        "gate_id": "rust-quality",
        "commit_sha": "aaaa",
        "evidence_class": "STATIC_ANALYSIS",
        "status": "PASS",
        "exit_code": 0,
        "detail": "clean build"
    }
    requirement = {
        "accepted_evidence_classes": ["STATIC_ANALYSIS"]
    }
    status, detail = validate_evidence_artifact(art, current_sha="aaaa", requirement=requirement)
    assert status == "PASS"

def test_stale_sha_rejected():
    art = {
        "schema_version": 1,
        "gate_id": "rust-quality",
        "commit_sha": "aaaa",
        "evidence_class": "STATIC_ANALYSIS",
        "status": "PASS",
        "exit_code": 0,
    }
    requirement = {
        "accepted_evidence_classes": ["STATIC_ANALYSIS"]
    }
    status, _ = validate_evidence_artifact(art, current_sha="bbbb", requirement=requirement)
    assert status == "STALE"

def test_invalid_class_rejected():
    art = {
        "schema_version": 1,
        "gate_id": "generic-linux-appliance",
        "commit_sha": "aaaa",
        "evidence_class": "CONTRACT_MOCK",
        "status": "PASS",
        "exit_code": 0,
    }
    requirement = {
        "accepted_evidence_classes": ["INTEGRATION_REAL"]
    }
    status, _ = validate_evidence_artifact(art, current_sha="aaaa", requirement=requirement)
    assert status == "INVALID_EVIDENCE"

def test_missing_required_blocks():
    reqs = {
        "gates": [
            {
                "id": "rust-quality",
                "required_for": ["rc", "ga"],
                "accepted_evidence_classes": ["STATIC_ANALYSIS"]
            }
        ]
    }
    artifacts = {}
    evaluated, has_blocking_failure = aggregate_gates(reqs, artifacts, "aaaa", "rc")
    assert has_blocking_failure is True
    assert evaluated[0]["status"] == "NOT_RUN"

def test_missing_optional_does_not_block():
    reqs = {
        "gates": [
            {
                "id": "soak-24h",
                "required_for": ["ga"],
                "accepted_evidence_classes": ["LONG_SOAK"]
            }
        ]
    }
    artifacts = {}
    evaluated, has_blocking_failure = aggregate_gates(reqs, artifacts, "aaaa", "rc")
    assert has_blocking_failure is False
    assert evaluated[0]["status"] == "NOT_RUN"

def test_ga_mode_requires_ga_gates():
    reqs = {
        "gates": [
            {
                "id": "soak-24h",
                "required_for": ["ga"],
                "accepted_evidence_classes": ["LONG_SOAK"]
            }
        ]
    }
    artifacts = {}
    evaluated, has_blocking_failure = aggregate_gates(reqs, artifacts, "aaaa", "ga")
    assert has_blocking_failure is True
    assert evaluated[0]["status"] == "NOT_RUN"

def test_fail_status_blocks():
    reqs = {
        "gates": [
            {
                "id": "rust-quality",
                "required_for": ["rc"],
                "accepted_evidence_classes": ["STATIC_ANALYSIS"]
            }
        ]
    }
    artifacts = {
        "rust-quality": {
            "schema_version": 1,
            "gate_id": "rust-quality",
            "commit_sha": "aaaa",
            "evidence_class": "STATIC_ANALYSIS",
            "status": "FAIL",
            "exit_code": 1
        }
    }
    evaluated, has_blocking_failure = aggregate_gates(reqs, artifacts, "aaaa", "rc")
    assert has_blocking_failure is True
    assert evaluated[0]["status"] == "FAIL"

def test_unauthorized_producer_rejected():
    art = {
        "schema_version": 1,
        "gate_id": "raspberry-pi-physical",
        "commit_sha": "aaaa",
        "evidence_class": "PHYSICAL_HARDWARE",
        "status": "PASS",
        "exit_code": 0,
        "producer": {
            "github_job": "unauthorized-job"
        }
    }
    requirement = {
        "accepted_evidence_classes": ["PHYSICAL_HARDWARE"],
        "allowed_producers": [
            {"github_job": "rpi-physical"}
        ]
    }
    status, _ = validate_evidence_artifact(art, current_sha="aaaa", requirement=requirement)
    assert status == "INVALID_EVIDENCE"
