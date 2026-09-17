#!/usr/bin/env python3
"""
Public GHCR Image Index Parser and Evidence Generator.

Validates:
1. Expected build digest matches the remote image index digest byte-for-byte.
2. The remote OCI/Docker index strictly contains linux/amd64 and linux/arm64 without duplicates.
3. Every architecture has a valid sha256 platform manifest digest.
4. Generates a machine-readable evidence file adhering to the release provenance schema.
"""

import argparse
import datetime
import hashlib
import json
import os
import re
import sys

SHA_RE = re.compile(r"^[0-9a-fA-F]{40}$")
DIGEST_RE = re.compile(r"^sha256:[0-9a-fA-F]{64}$")
TAG_RE = re.compile(r"^v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$")

CANONICAL_PLATFORMS = {"linux/amd64", "linux/arm64"}


def parse_and_verify_image_index(raw_manifest_str: str, expected_digest: str) -> dict:
    if not expected_digest or not DIGEST_RE.match(expected_digest):
        raise ValueError(f"Expected digest must be a valid sha256:hex64 string, got: {expected_digest!r}")

    # Compute sha256 over raw index payload bytes
    computed_digest = "sha256:" + hashlib.sha256(raw_manifest_str.encode("utf-8")).hexdigest()
    if computed_digest.lower() != expected_digest.lower():
        raise ValueError(
            f"Digest mismatch: expected build digest {expected_digest}, but remote index digest is {computed_digest}"
        )

    try:
        data = json.loads(raw_manifest_str)
    except Exception as e:
        raise ValueError(f"Failed to parse raw manifest JSON: {e}")

    if not isinstance(data, dict):
        raise ValueError("Raw manifest must be a JSON object")

    manifests = data.get("manifests")
    if not isinstance(manifests, list) or len(manifests) == 0:
        raise ValueError("Raw manifest must contain a non-empty 'manifests' list")

    platforms_found = []
    seen_platforms = set()

    for item in manifests:
        if not isinstance(item, dict):
            continue

        # Skip attestation manifests
        ann = item.get("annotations") or {}
        if ann.get("vnd.docker.reference.type") == "attestation-manifest":
            continue

        platform = item.get("platform") or {}
        os_name = platform.get("os")
        arch = platform.get("architecture")

        if not os_name or not arch or os_name == "unknown" or arch == "unknown":
            continue

        plat_key = f"{os_name}/{arch}"
        if plat_key in seen_platforms:
            raise ValueError(f"Duplicate platform found in image index: {plat_key}")
        seen_platforms.add(plat_key)

        p_digest = item.get("digest")
        if not p_digest or not DIGEST_RE.match(p_digest):
            raise ValueError(f"Invalid platform digest for {plat_key}: {p_digest!r}")

        platforms_found.append({
            "os": os_name,
            "architecture": arch,
            "digest": p_digest,
        })

    if seen_platforms != CANONICAL_PLATFORMS:
        raise ValueError(
            f"Image platforms mismatch: expected strictly {sorted(CANONICAL_PLATFORMS)}, but found {sorted(seen_platforms)}"
        )

    return {
        "remote_digest": computed_digest,
        "build_digest": expected_digest,
        "digest_match": True,
        "platforms": sorted(platforms_found, key=lambda p: (p["os"], p["architecture"])),
    }


def generate_ghcr_evidence(
    tag: str,
    commit: str,
    image: str,
    expected_digest: str,
    parsed_index: dict,
    verified_anonymous_pull: bool,
    runtime_health_verified: bool,
    output_file: str,
    repository: str = "pitydah/michi-micro-server",
) -> dict:
    if not TAG_RE.match(tag):
        raise ValueError(f"Invalid release tag: {tag!r}")
    if not SHA_RE.match(commit):
        raise ValueError(f"Invalid commit SHA (must be 40-char hex): {commit!r}")
    if not image or not image.startswith(f"ghcr.io/{repository}:"):
        raise ValueError(f"Image must belong to ghcr.io/{repository}, got: {image!r}")

    version = tag[1:]
    expected_image_tag = f"ghcr.io/{repository}:{version}"
    if image != expected_image_tag:
        raise ValueError(f"Image tag mismatch: expected {expected_image_tag}, got {image}")

    if not verified_anonymous_pull:
        raise ValueError("Anonymous pull must be verified as True")
    if not runtime_health_verified:
        raise ValueError("Runtime health must be verified as True")

    evidence = {
        "schema_version": 1,
        "tag": tag,
        "version": version,
        "repository": repository,
        "image": image,
        "build_digest": expected_digest,
        "remote_digest": parsed_index["remote_digest"],
        "digest_match": parsed_index["digest_match"],
        "platforms": parsed_index["platforms"],
        "verified_anonymous_pull": verified_anonymous_pull,
        "runtime_health_verified": runtime_health_verified,
        "commit": commit,
        "timestamp": datetime.datetime.now(datetime.timezone.utc).isoformat(),
    }

    os.makedirs(os.path.dirname(os.path.abspath(output_file)), exist_ok=True)
    with open(output_file, "w", encoding="utf-8") as f:
        json.dump(evidence, f, indent=2)
        f.write("\n")

    print(f"Recorded GHCR public release evidence to {output_file}:")
    print(json.dumps(evidence, indent=2))
    return evidence


def main():
    parser = argparse.ArgumentParser(description="Verify GHCR public release image index and record evidence")
    parser.add_argument("--raw-manifest-file", required=True, help="File containing raw output from buildx imagetools inspect --raw")
    parser.add_argument("--expected-digest", required=True, help="Expected build digest (sha256:...)")
    parser.add_argument("--tag", required=True, help="Git tag (e.g. v1.0.0-rc.2)")
    parser.add_argument("--commit", required=True, help="Git commit SHA (40-char hex)")
    parser.add_argument("--image", required=True, help="Full container image reference")
    parser.add_argument("--output-evidence", required=True, help="Output evidence JSON path")
    parser.add_argument("--anonymous-pull-verified", action="store_true", default=False)
    parser.add_argument("--runtime-health-verified", action="store_true", default=False)
    parser.add_argument("--repository", default="pitydah/michi-micro-server")

    args = parser.parse_args()

    with open(args.raw_manifest_file, "r", encoding="utf-8") as f:
        raw_manifest_str = f.read()

    try:
        parsed = parse_and_verify_image_index(raw_manifest_str, args.expected_digest)
        generate_ghcr_evidence(
            tag=args.tag,
            commit=args.commit,
            image=args.image,
            expected_digest=args.expected_digest,
            parsed_index=parsed,
            verified_anonymous_pull=args.anonymous_pull_verified,
            runtime_health_verified=args.runtime_health_verified,
            output_file=args.output_evidence,
            repository=args.repository,
        )
    except Exception as e:
        print(f"ERROR: {e}", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
