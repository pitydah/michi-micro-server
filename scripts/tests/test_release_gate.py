#!/usr/bin/env python3
import pytest
import sys
import os
import json
import hashlib
import subprocess

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


def test_build_rs_git_identity_tracking():
    """Verify build.rs watches HEAD, branch refs, and packed-refs via --git-path to prevent stale commit identity across worktrees."""
    build_rs_path = os.path.join(ROOT_DIR, "crates", "michi-api", "build.rs")
    assert os.path.exists(build_rs_path)
    with open(build_rs_path, "r", encoding="utf-8") as f:
        content = f.read()

    assert "cargo:rerun-if-env-changed=MICHI_BUILD_COMMIT" in content
    assert "rev-parse" in content
    assert "--git-path" in content
    assert "cargo:rerun-if-changed" in content
    assert "HEAD" in content
    assert "ref:" in content
    assert "packed-refs" in content


def test_soak_producer_provenance_validation(tmp_path):
    """Verify soak-24h gate strictly enforces authorized producer."""
    from release_evidence import validate_evidence_artifact, get_head_sha
    sha = get_head_sha()

    req = {
        "id": "soak-24h",
        "accepted_evidence_classes": ["LONG_SOAK"],
        "allowed_producers": [{"github_job": "soak-24h"}]
    }

    # 1. Valid producer passes
    valid_artifact = {
        "schema_version": 1,
        "gate_id": "soak-24h",
        "commit_sha": sha,
        "evidence_class": "LONG_SOAK",
        "status": "PASS",
        "exit_code": 0,
        "detail": "24h soak stable",
        "producer": {"github_job": "soak-24h"}
    }
    status, detail = validate_evidence_artifact(valid_artifact, sha, req)
    assert status == "PASS"

    # 2. Missing producer rejected as INVALID_EVIDENCE
    no_producer = dict(valid_artifact)
    del no_producer["producer"]
    status, detail = validate_evidence_artifact(no_producer, sha, req)
    assert status == "INVALID_EVIDENCE"
    assert "producer is not authorized" in detail

    # 3. Wrong producer rejected as INVALID_EVIDENCE
    wrong_producer = dict(valid_artifact, producer={"github_job": "unauthorized-job"})
    status, detail = validate_evidence_artifact(wrong_producer, sha, req)
    assert status == "INVALID_EVIDENCE"
    assert "producer is not authorized" in detail


def test_fetch_external_evidence_end_to_end(tmp_path):
    """Test fetch_external_release_evidence handling of valid, stale, wrong producer, and missing artifacts."""
    from fetch_external_release_evidence import ingest_evidence, fetch_artifact_from_local
    from release_evidence import get_head_sha
    sha = get_head_sha()

    req = {
        "id": "soak-24h",
        "accepted_evidence_classes": ["LONG_SOAK"],
        "allowed_producers": [{"github_job": "soak-24h"}]
    }

    source_dir = tmp_path / "source"
    output_dir = tmp_path / "output"
    source_dir.mkdir()
    output_dir.mkdir()

    # 1. Exact SHA + valid producer is accepted
    valid_data = {
        "schema_version": 1,
        "gate_id": "soak-24h",
        "commit_sha": sha,
        "evidence_class": "LONG_SOAK",
        "status": "PASS",
        "exit_code": 0,
        "detail": "Certified",
        "producer": {"github_job": "soak-24h"}
    }
    assert ingest_evidence("soak-24h", valid_data, sha, req, str(output_dir)) is True
    assert (output_dir / "soak-24h.json").exists()

    # 2. Stale SHA is rejected
    stale_data = dict(valid_data, commit_sha="0123456789abcdef0123456789abcdef01234567")
    assert ingest_evidence("soak-24h", stale_data, sha, req, str(output_dir)) is False

    # 3. Wrong producer is rejected
    wrong_prod = dict(valid_data, producer={"github_job": "wrong-job"})
    assert ingest_evidence("soak-24h", wrong_prod, sha, req, str(output_dir)) is False

    # 4. fetch_artifact_from_local finds by gate_id.json
    (source_dir / "soak-24h.json").write_text(json.dumps(valid_data))
    loaded = fetch_artifact_from_local(str(source_dir), "soak-24h", sha)
    assert loaded is not None
    assert loaded["gate_id"] == "soak-24h"


def test_fetch_external_evidence_cli_missing_flag(tmp_path):
    """Test fetch_external_release_evidence.py CLI fails on missing unless --allow-missing."""
    fetch_script = os.path.join(ROOT_DIR, "scripts", "fetch_external_release_evidence.py")
    empty_source = tmp_path / "empty_source"
    empty_source.mkdir()
    out_dir = tmp_path / "out"

    # Default (fail on missing)
    res_fail = subprocess.run(
        [sys.executable, fetch_script, "--source-dir", str(empty_source), "--output-dir", str(out_dir)],
        capture_output=True,
        text=True,
    )
    assert res_fail.returncode != 0
    assert "required artifact(s) missing" in res_fail.stderr

    # With --allow-missing
    res_allow = subprocess.run(
        [sys.executable, fetch_script, "--source-dir", str(empty_source), "--output-dir", str(out_dir), "--allow-missing"],
        capture_output=True,
        text=True,
    )
    assert res_allow.returncode == 0


def test_physical_and_soak_qualification_workflows():
    """Verify definitions and structural contracts of physical and soak qualification workflows."""
    workflows = [
        ("zimaos-physical.yml", "zimaos-physical", "casaos-zimaos-real", "release-evidence-casaos-zimaos-real"),
        ("rpi-physical.yml", "rpi-physical", "raspberry-pi-physical", "release-evidence-raspberry-pi-physical"),
        ("soak-24h.yml", "soak-24h", "soak-24h", "release-evidence-soak-24h"),
    ]

    for fname, expected_job, expected_gate, expected_artifact in workflows:
        wf_path = os.path.join(ROOT_DIR, ".github", "workflows", fname)
        assert os.path.exists(wf_path), f"Workflow {fname} does not exist"
        with open(wf_path, "r", encoding="utf-8") as f:
            wf = yaml.safe_load(f)

        assert expected_job in wf["jobs"], f"Job {expected_job} missing in {fname}"
        job_def = wf["jobs"][expected_job]
        steps = job_def.get("steps", [])

        # Verify upload artifact step matches expected name prefix
        upload_steps = [s for s in steps if "upload-artifact" in s.get("uses", "")]
        assert len(upload_steps) >= 1, f"Missing upload-artifact in {fname}"
        art_name = upload_steps[0].get("with", {}).get("name", "")
        assert expected_artifact in art_name, f"Artifact name {art_name} does not match {expected_artifact}"


def test_hardware_workflow_contract_contents():
    """Verify specific contractual directives in physical qualification workflows."""
    # 1. ZimaOS physical contract
    zima_path = os.path.join(ROOT_DIR, ".github", "workflows", "zimaos-physical.yml")
    with open(zima_path, "r", encoding="utf-8") as f:
        zima_raw = f.read()
    assert "--expected-commit" in zima_raw
    assert "--expected-version" in zima_raw
    assert "ZIMAOS_ADMIN_PASSWORD" in zima_raw or "ZIMAOS_ADMIN_TOKEN" in zima_raw
    assert "zimaos-physical" in zima_raw
    assert "attach_evidence_provenance.py" in zima_raw
    assert "zimaos-qualification" in zima_raw

    # 2. Raspberry Pi physical contract
    rpi_path = os.path.join(ROOT_DIR, ".github", "workflows", "rpi-physical.yml")
    with open(rpi_path, "r", encoding="utf-8") as f:
        rpi_raw = f.read()
    assert "self-hosted" in rpi_raw
    assert "arm64" in rpi_raw
    assert "rpi" in rpi_raw
    assert "/proc/device-tree/model" in rpi_raw
    assert "qualify_rpi_runtime.py" in rpi_raw
    assert "attach_evidence_provenance.py" in rpi_raw
    assert "rpi-physical" in rpi_raw
    assert "MICHI_BUILD_COMMIT" in rpi_raw
    assert "--expected-commit" in rpi_raw
    assert "--expected-version" in rpi_raw
    assert "--skip-model-check" not in rpi_raw


def test_attach_evidence_provenance_helper(tmp_path):
    """Test that attach_evidence_provenance.py preserves rich metrics while injecting canonical provenance."""
    test_file = tmp_path / "rich-evidence.json"
    initial_rich = {
        "runtime": {"version": "1.0.0-rc.2", "commit": "1234abcd", "deployment_platform": "zimaos"},
        "rss_bytes": 10485760,
        "thread_count": 8,
        "custom_check": "PASS",
        "status": "PASS",
    }
    test_file.write_text(json.dumps(initial_rich), encoding="utf-8")

    helper_path = os.path.join(ROOT_DIR, "scripts", "attach_evidence_provenance.py")
    res = subprocess.run(
        [
            sys.executable,
            helper_path,
            "--gate", "casaos-zimaos-real",
            "--class", "INTEGRATION_REAL",
            "--job-status", "success",
            "--producer-job", "zimaos-physical",
            "--commit-sha", "1234abcd",
            "--file", str(test_file),
        ],
        capture_output=True,
        text=True,
    )
    assert res.returncode == 0, f"attach_evidence_provenance failed:\n{res.stdout}\n{res.stderr}"

    updated = json.loads(test_file.read_text(encoding="utf-8"))
    assert updated["schema_version"] == 1
    assert updated["gate_id"] == "casaos-zimaos-real"
    assert updated["commit_sha"] == "1234abcd"
    assert updated["evidence_class"] == "INTEGRATION_REAL"
    assert updated["status"] == "PASS"
    assert updated["exit_code"] == 0
    assert updated["producer"]["github_job"] == "zimaos-physical"
    # Verify rich fields preserved intact
    assert updated["runtime"]["version"] == "1.0.0-rc.2"
    assert updated["runtime"]["deployment_platform"] == "zimaos"
    assert updated["rss_bytes"] == 10485760
    assert updated["thread_count"] == 8
    assert updated["custom_check"] == "PASS"


def test_rpi_device_tree_falsification(tmp_path):
    """Test qualify_rpi_runtime.py model checking accepts RPi 4/5 and rejects unauthorized hardware."""
    from qualify_rpi_runtime import get_rpi_model

    # 1. Valid RPi 4
    rpi4_file = tmp_path / "rpi4_model"
    rpi4_file.write_bytes(b"Raspberry Pi 4 Model B Rev 1.4\x00")
    assert "Raspberry Pi 4" in get_rpi_model(str(rpi4_file))

    # 2. Valid RPi 5
    rpi5_file = tmp_path / "rpi5_model"
    rpi5_file.write_bytes(b"Raspberry Pi 5 Model B Rev 1.0\x00")
    assert "Raspberry Pi 5" in get_rpi_model(str(rpi5_file))

    # 3. Unaccepted hardware
    unauth_file = tmp_path / "other_model"
    unauth_file.write_bytes(b"Rockchip RK3588 Board\x00")
    model = get_rpi_model(str(unauth_file))
    assert "Raspberry Pi 4" not in model and "Raspberry Pi 5" not in model


def test_full_synthetic_ga_matrix():
    """Build full valid synthetic evidence for all gates required in GA and verify PASS, then falsify 4 failure modes."""
    from release_evidence import get_head_sha
    sha = get_head_sha()

    reqs_path = os.path.join(ROOT_DIR, "release", "gates.json")
    with open(reqs_path, "r", encoding="utf-8") as f:
        reqs = json.load(f)

    # 1. Build valid synthetic artifacts for all 25 gates in release/gates.json
    artifacts = {}
    for gate in reqs.get("gates", []):
        gid = gate["id"]
        classes = gate.get("accepted_evidence_classes", ["INTEGRATION_REAL"])
        ev_class = classes[0]
        allowed_producers = gate.get("allowed_producers")

        producer = {"github_job": gid}
        if allowed_producers:
            producer = dict(allowed_producers[0])

        artifacts[gid] = {
            "schema_version": 1,
            "gate_id": gid,
            "commit_sha": sha,
            "evidence_class": ev_class,
            "status": "PASS",
            "exit_code": 0,
            "detail": f"Synthetic valid qualification for {gid}",
            "producer": producer,
        }

    # Verify GA full pass
    evaluated, blocked = aggregate_gates(reqs, artifacts, sha, "ga")
    assert blocked is False, f"Expected GA to pass, but was blocked:\n{evaluated}"
    assert len(evaluated) == len(reqs["gates"])
    assert all(e["status"] == "PASS" for e in evaluated)

    # Falsification 1: Stale Raspberry Pi physical artifact -> GA blocked
    stale_rpi_arts = dict(artifacts)
    stale_rpi_arts["raspberry-pi-physical"] = dict(
        stale_rpi_arts["raspberry-pi-physical"],
        commit_sha="0123456789abcdef0123456789abcdef01234567"
    )
    evaluated, blocked = aggregate_gates(reqs, stale_rpi_arts, sha, "ga")
    assert blocked is True
    rpi_eval = next(e for e in evaluated if e["id"] == "raspberry-pi-physical")
    assert rpi_eval["status"] == "STALE"

    # Falsification 2: Missing ZimaOS artifact -> GA blocked
    missing_zima_arts = dict(artifacts)
    del missing_zima_arts["casaos-zimaos-real"]
    evaluated, blocked = aggregate_gates(reqs, missing_zima_arts, sha, "ga")
    assert blocked is True
    zima_eval = next(e for e in evaluated if e["id"] == "casaos-zimaos-real")
    assert zima_eval["status"] == "NOT_RUN"

    # Falsification 3: Wrong soak producer -> GA blocked
    wrong_soak_arts = dict(artifacts)
    wrong_soak_arts["soak-24h"] = dict(
        wrong_soak_arts["soak-24h"],
        producer={"github_job": "unauthorized-soak-job"}
    )
    evaluated, blocked = aggregate_gates(reqs, wrong_soak_arts, sha, "ga")
    assert blocked is True
    soak_eval = next(e for e in evaluated if e["id"] == "soak-24h")
    assert soak_eval["status"] == "INVALID_EVIDENCE"

    # Falsification 4: Failed physical run -> GA blocked
    failed_physical_arts = dict(artifacts)
    failed_physical_arts["raspberry-pi-physical"] = dict(
        failed_physical_arts["raspberry-pi-physical"],
        status="FAIL",
        exit_code=1,
    )
    evaluated, blocked = aggregate_gates(reqs, failed_physical_arts, sha, "ga")
    assert blocked is True
    fail_eval = next(e for e in evaluated if e["id"] == "raspberry-pi-physical")
    assert fail_eval["status"] == "FAIL"


def test_rpi_qualifier_wav_generation(tmp_path):
    """Verify that qualify_rpi_runtime generates valid standard uncompressed PCM WAV files."""
    from qualify_rpi_runtime import create_mock_wav_file
    import wave

    wav_path = str(tmp_path / "test.wav")
    size = create_mock_wav_file(wav_path, duration_sec=0.5, sample_rate=44100)
    assert os.path.exists(wav_path)
    assert size > 0

    with wave.open(wav_path, "rb") as wf:
        assert wf.getnchannels() == 2
        assert wf.getsampwidth() == 2
        assert wf.getframerate() == 44100
        frames = wf.readframes(wf.getnframes())
        assert len(frames) == int(0.5 * 44100 * 2 * 2)


def test_rpi_qualifier_model_validation(tmp_path):
    """Verify Raspberry Pi hardware model check rules (RPi 4/5 accepted, other models rejected)."""
    from qualify_rpi_runtime import get_rpi_model

    # 1. RPi 4
    rpi4_file = tmp_path / "model_rpi4"
    rpi4_file.write_bytes(b"Raspberry Pi 4 Model B Rev 1.5\x00")
    assert "Raspberry Pi 4" in get_rpi_model(str(rpi4_file))

    # 2. RPi 5
    rpi5_file = tmp_path / "model_rpi5"
    rpi5_file.write_bytes(b"Raspberry Pi 5 Model B Rev 1.0\x00")
    assert "Raspberry Pi 5" in get_rpi_model(str(rpi5_file))

    # 3. Non-RPi model rejection in qualify_rpi_runtime execution
    orangepi_file = tmp_path / "model_orange"
    orangepi_file.write_bytes(b"Orange Pi 5 Plus\x00")
    assert "Orange Pi" in get_rpi_model(str(orangepi_file))

    evidence_file = str(tmp_path / "evidence_err.json")
    qual_script = os.path.join(ROOT_DIR, "scripts", "qualify_rpi_runtime.py")
    res = subprocess.run(
        [
            sys.executable,
            qual_script,
            "--model-path", str(orangepi_file),
            "--output-evidence", evidence_file,
        ],
        capture_output=True,
        text=True,
    )
    assert res.returncode != 0
    with open(evidence_file, "r", encoding="utf-8") as f:
        ev = json.load(f)
    assert ev["status"] == "FAIL"
    assert any("Unaccepted physical hardware model" in err for err in ev["errors"])


def test_rpi_evidence_constants_and_limits():
    """Verify strict qualification resource budget limits."""
    import qualify_rpi_runtime as rpi_mod

    assert rpi_mod.RSS_LIMIT_BYTES == 65 * 1024 * 1024  # 65 MB
    assert rpi_mod.THREAD_LIMIT == 16


def test_rpi_qualifier_mock_execution_and_assertions(tmp_path):
    """
    Test end-to-end qualify_rpi_runtime execution against a mock server executable.
    Verifies:
    - Canonical environment variables passed (and absence of legacy variables).
    - Database migration schema 49 verification.
    - Expected commit / version assertion enforcement (success and failure cases).
    - Correct recording of resource limits and metrics in rich evidence.
    """
    # 1. Create a mock michi-server executable in Python
    env_dump_path = str(tmp_path / "env_dump.json")
    mock_bin_path = str(tmp_path / "mock_michi_server.py")
    mock_code = f"""#!/usr/bin/env python3
import http.server
import json
import os
import signal
import socketserver
import sqlite3
import sys
import threading
import urllib.parse

# Dump environment
with open({json.dumps(env_dump_path)}, "w", encoding="utf-8") as f:
    json.dump(dict(os.environ), f)

# Initialize database at MICHI_DATABASE if present
db_url = os.environ.get("MICHI_DATABASE", "")
if db_url.startswith("sqlite://"):
    db_file = db_url[len("sqlite://"):]
    os.makedirs(os.path.dirname(db_file), exist_ok=True)
    conn = sqlite3.connect(db_file)
    conn.execute("CREATE TABLE IF NOT EXISTS _migrations (version INTEGER PRIMARY KEY, applied_at TEXT)")
    conn.execute("INSERT OR REPLACE INTO _migrations (version, applied_at) VALUES (49, '2026-09-21T00:00:00Z')")
    conn.commit()
    conn.close()

port = int(os.environ.get("MICHI_PORT", 9095))

class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self, format, *args):
        pass

    def do_GET(self):
        if self.path == "/health/live":
            self.send_response(200)
            self.end_headers()
            self.wfile.write(b"OK")
        elif self.path == "/api/v1/server/info":
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            self.wfile.write(json.dumps({{"version": "1.0.0", "commit": "mockcommit123"}}).encode("utf-8"))
        elif self.path == "/api/v1/tracks":
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            self.wfile.write(json.dumps([{{
                "id": "11111111-1111-1111-1111-111111111111",
                "title": "Smoke Track",
            }}]).encode("utf-8"))
        elif "/stream" in self.path:
            self.send_response(206)
            self.send_header("Content-Type", "audio/wav")
            self.send_header("Content-Range", "bytes 0-1023/1024")
            self.end_headers()
            self.wfile.write(b"RIFF" + b"\\x00" * 1020)
        else:
            self.send_response(404)
            self.end_headers()

    def do_POST(self):
        if self.path == "/api/v1/library/scan":
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            self.wfile.write(b"{{}}")
        else:
            self.send_response(404)
            self.end_headers()

socketserver.TCPServer.allow_reuse_address = True
httpd = socketserver.TCPServer(("127.0.0.1", port), Handler)
try:
    httpd.serve_forever()
finally:
    httpd.server_close()
"""
    with open(mock_bin_path, "w", encoding="utf-8") as f:
        f.write(mock_code)
    os.chmod(mock_bin_path, 0o755)

    qual_script = os.path.join(ROOT_DIR, "scripts", "qualify_rpi_runtime.py")
    test_port = 9188

    # Case A: Success run with matching expected-commit and expected-version
    ev_success_path = str(tmp_path / "ev_success.json")
    res = subprocess.run(
        [
            sys.executable,
            qual_script,
            "--binary", mock_bin_path,
            "--skip-model-check",
            "--port", str(test_port),
            "--expected-version", "1.0.0",
            "--expected-commit", "mockcommit123",
            "--output-evidence", ev_success_path,
        ],
        capture_output=True,
        text=True,
    )
    assert res.returncode == 0, f"Expected success but failed:\n{res.stdout}\n{res.stderr}"

    with open(ev_success_path, "r", encoding="utf-8") as f:
        ev_data = json.load(f)

    assert ev_data["status"] == "PASS"
    assert ev_data["runtime_version"] == "1.0.0"
    assert ev_data["runtime_commit"] == "mockcommit123"
    assert ev_data["expected_version"] == "1.0.0"
    assert ev_data["expected_commit"] == "mockcommit123"
    assert ev_data["database_schema_version"] == 49
    assert ev_data["health_result"] == "PASS"
    assert ev_data["stream_smoke_result"] == "PASS"
    assert ev_data["rss_limit_bytes"] == 65 * 1024 * 1024
    assert ev_data["thread_limit"] == 16

    # Verify canonical environment variables in dumped env
    with open(env_dump_path, "r", encoding="utf-8") as f:
        dumped_env = json.load(f)

    assert dumped_env.get("MICHI_PORT") == str(test_port)
    assert "MICHI_CONFIG_PATH" in dumped_env
    assert "MICHI_CACHE_PATH" in dumped_env
    assert "MICHI_MUSIC_PATH" in dumped_env
    assert "MICHI_MUSIC_PATHS" in dumped_env
    assert dumped_env.get("MICHI_DATABASE", "").startswith("sqlite://")
    assert dumped_env.get("MICHI_DEPLOYMENT_PLATFORM") == "rpi"

    # Assert absence of non-canonical / legacy environment variables
    for legacy_var in ["MICHI_CONFIG_DIR", "MICHI_CACHE_DIR", "MICHI_MUSIC_DIR", "MICHI_SERVER_PORT"]:
        assert legacy_var not in dumped_env, f"Legacy environment variable {legacy_var} must not be set"

    # Case B: Failure when expected-commit does not match
    ev_wrong_commit_path = str(tmp_path / "ev_wrong_commit.json")
    res_commit = subprocess.run(
        [
            sys.executable,
            qual_script,
            "--binary", mock_bin_path,
            "--skip-model-check",
            "--port", str(test_port + 1),
            "--expected-version", "1.0.0",
            "--expected-commit", "unmatched_commit_sha",
            "--output-evidence", ev_wrong_commit_path,
        ],
        capture_output=True,
        text=True,
    )
    assert res_commit.returncode != 0
    with open(ev_wrong_commit_path, "r", encoding="utf-8") as f:
        ev_commit = json.load(f)
    assert ev_commit["status"] == "FAIL"
    assert any("Runtime commit mismatch" in err for err in ev_commit["errors"])

    # Case C: Failure when expected-version does not match
    ev_wrong_ver_path = str(tmp_path / "ev_wrong_ver.json")
    res_ver = subprocess.run(
        [
            sys.executable,
            qual_script,
            "--binary", mock_bin_path,
            "--skip-model-check",
            "--port", str(test_port + 2),
            "--expected-version", "2.0.0-rc.99",
            "--expected-commit", "mockcommit123",
            "--output-evidence", ev_wrong_ver_path,
        ],
        capture_output=True,
        text=True,
    )
    assert res_ver.returncode != 0
    with open(ev_wrong_ver_path, "r", encoding="utf-8") as f:
        ev_ver = json.load(f)
    assert ev_ver["status"] == "FAIL"
    assert any("Runtime version mismatch" in err for err in ev_ver["errors"])



