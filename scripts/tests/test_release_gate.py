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

def test_blocked_external_blocks_when_required():
    reqs = {
        "gates": [
            {
                "id": "raspberry-pi-physical",
                "required_for": ["ga"],
                "accepted_evidence_classes": ["PHYSICAL_HARDWARE"]
            }
        ]
    }
    artifacts = {
        "raspberry-pi-physical": {
            "schema_version": 1,
            "gate_id": "raspberry-pi-physical",
            "commit_sha": "aaaa",
            "evidence_class": "PHYSICAL_HARDWARE",
            "status": "BLOCKED_EXTERNAL",
            "exit_code": 0,
            "detail": "waiting on physical runner"
        }
    }
    evaluated, has_blocking_failure = aggregate_gates(reqs, artifacts, "aaaa", "ga")
    assert has_blocking_failure is True
    assert evaluated[0]["status"] == "BLOCKED_EXTERNAL"

from verify_release_manifest import generate_manifest, verify_manifest

def test_release_manifest_valid(tmp_path):
    out = str(tmp_path / "release-manifest.json")
    tag = "v1.0.0-rc.2"
    commit = "a" * 40
    image = "ghcr.io/pitydah/michi-micro-server:1.0.0-rc.2"
    digest = "sha256:" + "b" * 64
    manifest = generate_manifest(
        tag=tag,
        commit=commit,
        image=image,
        platforms=["linux/amd64", "linux/arm64"],
        output_file=out,
        digest=digest,
    )
    assert manifest["tag"] == tag
    assert manifest["commit"] == commit
    assert manifest["digest"] == digest
    assert verify_manifest(out, expected_tag=tag, expected_commit=commit, expected_image=image, expected_digest=digest) is True

def test_release_manifest_rejects_missing_digest(tmp_path):
    out = str(tmp_path / "release-manifest.json")
    with pytest.raises(ValueError, match="Digest must be a valid sha256:hex64 string"):
        generate_manifest(
            tag="v1.0.0-rc.2",
            commit="a" * 40,
            image="ghcr.io/pitydah/michi-micro-server:1.0.0-rc.2",
            platforms=["linux/amd64"],
            output_file=out,
            digest="",
        )

def test_release_manifest_rejects_invalid_commit(tmp_path):
    out = str(tmp_path / "release-manifest.json")
    with pytest.raises(ValueError, match="Commit SHA must be a 40-character hex string"):
        generate_manifest(
            tag="v1.0.0-rc.2",
            commit="invalid_sha",
            image="ghcr.io/pitydah/michi-micro-server:1.0.0-rc.2",
            platforms=["linux/amd64"],
            output_file=out,
            digest="sha256:" + "b" * 64,
        )

