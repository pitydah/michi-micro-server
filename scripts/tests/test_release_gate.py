#!/usr/bin/env python3
import pytest
import sys
import os
import json
import hashlib

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

from verify_release_manifest import (
    generate_manifest,
    generate_manifest_from_evidence,
    validate_manifest_dict,
    verify_manifest,
)
from verify_public_release_image import parse_and_verify_image_index, generate_ghcr_evidence
import hashlib
import yaml

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
    assert len(manifest["platforms"]) == 2
    assert verify_manifest(out, expected_tag=tag, expected_commit=commit, expected_image=image, expected_digest=digest) is True

def test_release_manifest_rejects_missing_digest(tmp_path):
    out = str(tmp_path / "release-manifest.json")
    with pytest.raises(ValueError, match="Invalid or empty image digest"):
        generate_manifest(
            tag="v1.0.0-rc.2",
            commit="a" * 40,
            image="ghcr.io/pitydah/michi-micro-server:1.0.0-rc.2",
            platforms=["linux/amd64", "linux/arm64"],
            output_file=out,
            digest="",
        )

def test_release_manifest_rejects_invalid_commit(tmp_path):
    out = str(tmp_path / "release-manifest.json")
    with pytest.raises(ValueError, match="Invalid commit SHA"):
        generate_manifest(
            tag="v1.0.0-rc.2",
            commit="invalid_sha",
            image="ghcr.io/pitydah/michi-micro-server:1.0.0-rc.2",
            platforms=["linux/amd64", "linux/arm64"],
            output_file=out,
            digest="sha256:" + "b" * 64,
        )

def test_manifest_validation_negative_cases():
    valid_base = {
        "schema_version": "1.0.0",
        "tag": "v1.0.0-rc.2",
        "version": "1.0.0-rc.2",
        "repository": "pitydah/michi-micro-server",
        "commit": "a" * 40,
        "image": "ghcr.io/pitydah/michi-micro-server:1.0.0-rc.2",
        "digest": "sha256:" + "b" * 64,
        "platforms": [
            {"os": "linux", "architecture": "amd64", "digest": "sha256:" + "1" * 64},
            {"os": "linux", "architecture": "arm64", "digest": "sha256:" + "2" * 64},
        ],
        "generated_at": "2026-09-13T12:00:00Z",
        "publisher": "michi-ci",
    }
    assert validate_manifest_dict(valid_base) == []

    # Tag/version mismatch
    m = dict(valid_base, version="1.0.0-rc.3")
    errs = validate_manifest_dict(m)
    assert any("Version mismatch" in e for e in errs)

    # Image/version mismatch
    m = dict(valid_base, image="ghcr.io/pitydah/michi-micro-server:1.0.0-other")
    errs = validate_manifest_dict(m)
    assert any("Image tag mismatch" in e for e in errs)

    # Invalid digest
    m = dict(valid_base, digest="not-a-sha")
    errs = validate_manifest_dict(m)
    assert any("Invalid or empty image digest" in e for e in errs)

    # Empty digest
    m = dict(valid_base, digest="")
    errs = validate_manifest_dict(m)
    assert any("Invalid or empty image digest" in e for e in errs)

    # Platform missing
    m = dict(valid_base, platforms=[{"os": "linux", "architecture": "amd64", "digest": "sha256:" + "1" * 64}])
    errs = validate_manifest_dict(m)
    assert any("Platforms mismatch" in e for e in errs)

    # Duplicate platform
    m = dict(valid_base, platforms=[
        {"os": "linux", "architecture": "amd64", "digest": "sha256:" + "1" * 64},
        {"os": "linux", "architecture": "amd64", "digest": "sha256:" + "2" * 64},
    ])
    errs = validate_manifest_dict(m)
    assert any("Duplicate platform" in e for e in errs)

    # Unexpected architecture
    m = dict(valid_base, platforms=[
        {"os": "linux", "architecture": "amd64", "digest": "sha256:" + "1" * 64},
        {"os": "linux", "architecture": "s390x", "digest": "sha256:" + "2" * 64},
    ])
    errs = validate_manifest_dict(m)
    assert any("Platforms mismatch" in e for e in errs)

    # Platform digest invalid
    m = dict(valid_base, platforms=[
        {"os": "linux", "architecture": "amd64", "digest": "bad-digest"},
        {"os": "linux", "architecture": "arm64", "digest": "sha256:" + "2" * 64},
    ])
    errs = validate_manifest_dict(m)
    assert any("Invalid platform digest" in e for e in errs)

    # Unsupported schema
    m = dict(valid_base, schema_version="2.0.0")
    errs = validate_manifest_dict(m)
    assert any("Unsupported schema_version" in e for e in errs)

def _build_mock_index_json(manifests):
    return json.dumps({
        "schemaVersion": 2,
        "mediaType": "application/vnd.oci.image.index.v1+json",
        "manifests": manifests,
    })

def test_image_index_verification_positive():
    manifests = [
        {"platform": {"os": "linux", "architecture": "amd64"}, "digest": "sha256:" + "1" * 64},
        {"platform": {"os": "linux", "architecture": "arm64"}, "digest": "sha256:" + "2" * 64},
    ]
    raw = _build_mock_index_json(manifests)
    expected_digest = "sha256:" + hashlib.sha256(raw.encode("utf-8")).hexdigest()

    result = parse_and_verify_image_index(raw, expected_digest)
    assert result["digest_match"] is True
    assert result["remote_digest"] == expected_digest
    assert len(result["platforms"]) == 2

def test_image_index_verification_digest_mismatch():
    manifests = [
        {"platform": {"os": "linux", "architecture": "amd64"}, "digest": "sha256:" + "1" * 64},
        {"platform": {"os": "linux", "architecture": "arm64"}, "digest": "sha256:" + "2" * 64},
    ]
    raw = _build_mock_index_json(manifests)
    wrong_digest = "sha256:" + "f" * 64

    with pytest.raises(ValueError, match="Digest mismatch"):
        parse_and_verify_image_index(raw, wrong_digest)

def test_image_index_verification_missing_amd64():
    manifests = [
        {"platform": {"os": "linux", "architecture": "arm64"}, "digest": "sha256:" + "2" * 64},
    ]
    raw = _build_mock_index_json(manifests)
    expected_digest = "sha256:" + hashlib.sha256(raw.encode("utf-8")).hexdigest()

    with pytest.raises(ValueError, match="Image platforms mismatch"):
        parse_and_verify_image_index(raw, expected_digest)

def test_image_index_verification_missing_arm64():
    manifests = [
        {"platform": {"os": "linux", "architecture": "amd64"}, "digest": "sha256:" + "1" * 64},
    ]
    raw = _build_mock_index_json(manifests)
    expected_digest = "sha256:" + hashlib.sha256(raw.encode("utf-8")).hexdigest()

    with pytest.raises(ValueError, match="Image platforms mismatch"):
        parse_and_verify_image_index(raw, expected_digest)

def test_image_index_verification_duplicate_platform():
    manifests = [
        {"platform": {"os": "linux", "architecture": "amd64"}, "digest": "sha256:" + "1" * 64},
        {"platform": {"os": "linux", "architecture": "amd64"}, "digest": "sha256:" + "2" * 64},
        {"platform": {"os": "linux", "architecture": "arm64"}, "digest": "sha256:" + "3" * 64},
    ]
    raw = _build_mock_index_json(manifests)
    expected_digest = "sha256:" + hashlib.sha256(raw.encode("utf-8")).hexdigest()

    with pytest.raises(ValueError, match="Duplicate platform found"):
        parse_and_verify_image_index(raw, expected_digest)

def test_manifest_from_evidence_end_to_end(tmp_path):
    evid_path = str(tmp_path / "ghcr-evidence.json")
    manifest_out = str(tmp_path / "release-manifest.json")
    tag = "v1.0.0-rc.2"
    commit = "c" * 40
    image = "ghcr.io/pitydah/michi-micro-server:1.0.0-rc.2"
    digest = "sha256:" + "d" * 64

    manifests = [
        {"platform": {"os": "linux", "architecture": "amd64"}, "digest": "sha256:" + "1" * 64},
        {"platform": {"os": "linux", "architecture": "arm64"}, "digest": "sha256:" + "2" * 64},
    ]
    raw = _build_mock_index_json(manifests)
    raw_digest = "sha256:" + hashlib.sha256(raw.encode("utf-8")).hexdigest()
    parsed = parse_and_verify_image_index(raw, raw_digest)

    generate_ghcr_evidence(
        tag=tag,
        commit=commit,
        image=image,
        expected_digest=raw_digest,
        parsed_index=parsed,
        verified_anonymous_pull=True,
        runtime_health_verified=True,
        output_file=evid_path,
    )

    manifest = generate_manifest_from_evidence(
        evidence_path=evid_path,
        output_file=manifest_out,
        expected_tag=tag,
        expected_commit=commit,
    )

    assert manifest["tag"] == tag
    assert manifest["commit"] == commit
    assert manifest["digest"] == raw_digest
    assert len(manifest["platforms"]) == 2
    assert verify_manifest(manifest_out, expected_tag=tag, expected_commit=commit, expected_digest=raw_digest) is True

def test_ci_workflow_step_variable_isolation():
    ci_path = os.path.join(ROOT_DIR, ".github", "workflows", "ci.yml")
    with open(ci_path, "r", encoding="utf-8") as f:
        ci = yaml.safe_load(f)

    public_job = ci["jobs"]["ci-ghcr-public-release"]
    release_job = ci["jobs"]["release-github"]

    # In ci-ghcr-public-release, release_meta must be defined before verify step
    step_ids = [s.get("id") for s in public_job["steps"] if "id" in s]
    assert "release_meta" in step_ids

    # In release-github, release_meta must also be defined
    release_step_ids = [s.get("id") for s in release_job["steps"] if "id" in s]
    assert "release_meta" in release_step_ids

def test_soak_stability_contract_gate_specification():
    gates_path = os.path.join(ROOT_DIR, "release", "gates.json")
    with open(gates_path, "r", encoding="utf-8") as f:
        gates_data = json.load(f)

    gate = next((g for g in gates_data.get("gates", []) if g["id"] == "soak-stability-contract"), None)
    assert gate is not None, "soak-stability-contract must be defined in release/gates.json"
    assert "rc" in gate.get("required_for", []), "soak-stability-contract must be required for rc"
    assert "ga" in gate.get("required_for", []), "soak-stability-contract must be required for ga"
    assert "UNIT" in gate.get("accepted_evidence_classes", [])

    ci_path = os.path.join(ROOT_DIR, ".github", "workflows", "ci.yml")
    with open(ci_path, "r", encoding="utf-8") as f:
        ci = yaml.safe_load(f)

    assert "ci-soak-stability-contract" in ci["jobs"]
    soak_job = ci["jobs"]["ci-soak-stability-contract"]
    step_runs = [s.get("run", "") for s in soak_job["steps"]]
    assert any("soak-stability-contract" in r for r in step_runs)

