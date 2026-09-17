#!/usr/bin/env python3
"""
Release Manifest Generator & Validator for Michi Micro Server.

Binds release tag, commit SHA, GHCR image, multi-arch platforms, and image digests
into a verified release-manifest.json artifact attached to GitHub releases.
"""

import argparse
import datetime
import json
import os
import re
import sys

SHA_RE = re.compile(r"^[0-9a-fA-F]{40}$")
DIGEST_RE = re.compile(r"^sha256:[0-9a-fA-F]{64}$")
TAG_RE = re.compile(r"^v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$")

CANONICAL_PLATFORMS = {"linux/amd64", "linux/arm64"}
SUPPORTED_SCHEMA_VERSIONS = {"1.0.0"}


def validate_manifest_dict(
    manifest: dict,
    expected_tag: str | None = None,
    expected_commit: str | None = None,
    expected_image: str | None = None,
    expected_digest: str | None = None,
) -> list[str]:
    errors = []

    required_keys = [
        "schema_version",
        "tag",
        "version",
        "repository",
        "commit",
        "image",
        "platforms",
        "digest",
        "generated_at",
        "publisher",
    ]
    for key in required_keys:
        if key not in manifest:
            errors.append(f"Missing required key: {key}")

    schema_version = manifest.get("schema_version")
    if schema_version not in SUPPORTED_SCHEMA_VERSIONS:
        errors.append(f"Unsupported schema_version: {schema_version!r}")

    tag = manifest.get("tag", "")
    if not isinstance(tag, str) or not TAG_RE.match(tag):
        errors.append(f"Invalid release tag (must match {TAG_RE.pattern}): {tag!r}")

    version = manifest.get("version", "")
    if not isinstance(version, str) or not version:
        errors.append("Version must be a non-empty string")
    elif tag and version != tag.lstrip("v"):
        errors.append(f"Version mismatch: expected {tag.lstrip('v')}, found {version}")

    commit = manifest.get("commit", "")
    if not isinstance(commit, str) or not SHA_RE.match(commit):
        errors.append(f"Invalid commit SHA (must be 40-character hex): {commit!r}")

    repository = manifest.get("repository", "")
    if repository != "pitydah/michi-micro-server":
        errors.append(f"Unexpected repository: {repository!r}")

    image = manifest.get("image", "")
    expected_image_prefix = f"ghcr.io/{repository}:"
    if not isinstance(image, str) or not image.startswith(expected_image_prefix):
        errors.append(f"Image must belong to {expected_image_prefix}, found: {image!r}")
    elif version and not image.endswith(f":{version}"):
        errors.append(f"Image tag mismatch: image {image} does not end with version :{version}")

    digest = manifest.get("digest", "")
    if not isinstance(digest, str) or not DIGEST_RE.match(digest):
        errors.append(f"Invalid or empty image digest: {digest!r}")

    platforms = manifest.get("platforms")
    if not isinstance(platforms, list):
        errors.append("Platforms must be a list of platform objects")
    else:
        seen_keys = []
        for idx, p in enumerate(platforms):
            if not isinstance(p, dict):
                errors.append(f"Platform item #{idx} is not an object: {p!r}")
                continue
            os_name = p.get("os")
            arch = p.get("architecture")
            p_digest = p.get("digest")

            if not os_name or not arch:
                errors.append(f"Platform item #{idx} missing os or architecture")
                continue

            key = f"{os_name}/{arch}"
            if key in seen_keys:
                errors.append(f"Duplicate platform in manifest: {key}")
            seen_keys.append(key)

            if not p_digest or not DIGEST_RE.match(p_digest):
                errors.append(f"Invalid platform digest for {key}: {p_digest!r}")

        seen_set = set(seen_keys)
        if seen_set != CANONICAL_PLATFORMS:
            errors.append(
                f"Platforms mismatch: expected strictly {sorted(CANONICAL_PLATFORMS)}, found {sorted(seen_set)}"
            )

    # Optional expected values check
    if expected_tag and tag != expected_tag:
        errors.append(f"Tag mismatch: expected {expected_tag}, found {tag}")
    if expected_commit and commit != expected_commit:
        errors.append(f"Commit mismatch: expected {expected_commit}, found {commit}")
    if expected_image and image != expected_image:
        errors.append(f"Image mismatch: expected {expected_image}, found {image}")
    if expected_digest and digest != expected_digest:
        errors.append(f"Digest mismatch: expected {expected_digest}, found {digest}")

    return errors


def generate_manifest_from_evidence(
    evidence_path: str,
    output_file: str,
    expected_tag: str | None = None,
    expected_commit: str | None = None,
) -> dict:
    if not os.path.exists(evidence_path):
        raise ValueError(f"Evidence file not found: {evidence_path}")

    with open(evidence_path, "r", encoding="utf-8") as f:
        evid = json.load(f)

    if not evid.get("digest_match"):
        raise ValueError("Evidence does not have digest_match == True")
    if not evid.get("verified_anonymous_pull"):
        raise ValueError("Evidence does not have verified_anonymous_pull == True")
    if not evid.get("runtime_health_verified"):
        raise ValueError("Evidence does not have runtime_health_verified == True")
    if evid.get("build_digest") != evid.get("remote_digest"):
        raise ValueError(f"Evidence build_digest ({evid.get('build_digest')}) != remote_digest ({evid.get('remote_digest')})")

    tag = evid.get("tag")
    commit = evid.get("commit")
    image = evid.get("image")
    digest = evid.get("remote_digest")
    platforms = evid.get("platforms")
    version = evid.get("version") or (tag[1:] if tag and tag.startswith("v") else tag)
    repo = evid.get("repository", "pitydah/michi-micro-server")

    if expected_tag and tag != expected_tag:
        raise ValueError(f"Evidence tag {tag} does not match expected {expected_tag}")
    if expected_commit and commit != expected_commit:
        raise ValueError(f"Evidence commit {commit} does not match expected {expected_commit}")

    manifest = {
        "schema_version": "1.0.0",
        "tag": tag,
        "version": version,
        "repository": repo,
        "commit": commit,
        "image": image,
        "digest": digest,
        "platforms": platforms,
        "generated_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "publisher": "michi-micro-server-ci",
    }

    errors = validate_manifest_dict(manifest, expected_tag, expected_commit)
    if errors:
        raise ValueError(f"Generated manifest failed validation: {'; '.join(errors)}")

    os.makedirs(os.path.dirname(os.path.abspath(output_file)), exist_ok=True)
    with open(output_file, "w", encoding="utf-8") as f:
        json.dump(manifest, f, indent=2)
        f.write("\n")

    print(f"Generated release manifest from verified evidence at {output_file}:")
    print(json.dumps(manifest, indent=2))
    return manifest


def generate_manifest(
    tag: str,
    commit: str,
    image: str,
    platforms: list,
    output_file: str,
    digest: str,
    repository: str = "pitydah/michi-micro-server",
) -> dict:
    version = tag.lstrip("v") if tag else ""

    # Normalize platforms if provided as list of strings
    normalized_platforms = []
    for p in platforms:
        if isinstance(p, dict):
            normalized_platforms.append(p)
        elif isinstance(p, str):
            parts = p.split("/")
            if len(parts) == 2:
                normalized_platforms.append({
                    "os": parts[0],
                    "architecture": parts[1],
                    "digest": digest,
                })
            else:
                normalized_platforms.append(p)

    manifest = {
        "schema_version": "1.0.0",
        "tag": tag,
        "version": version,
        "repository": repository,
        "commit": commit,
        "image": image,
        "digest": digest,
        "platforms": normalized_platforms,
        "generated_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "publisher": "michi-micro-server-ci",
    }

    errors = validate_manifest_dict(manifest)
    if errors:
        raise ValueError(f"Manifest validation failed: {'; '.join(errors)}")

    os.makedirs(os.path.dirname(os.path.abspath(output_file)), exist_ok=True)
    with open(output_file, "w", encoding="utf-8") as f:
        json.dump(manifest, f, indent=2)
        f.write("\n")

    print(f"Generated release manifest at {output_file}:")
    print(json.dumps(manifest, indent=2))
    return manifest


def verify_manifest(
    manifest_path: str,
    expected_tag: str | None = None,
    expected_commit: str | None = None,
    expected_image: str | None = None,
    expected_digest: str | None = None,
) -> bool:
    if not os.path.exists(manifest_path):
        print(f"ERROR: Manifest file not found: {manifest_path}", file=sys.stderr)
        return False

    try:
        with open(manifest_path, "r", encoding="utf-8") as f:
            manifest = json.load(f)
    except Exception as e:
        print(f"ERROR: Failed to parse manifest JSON: {e}", file=sys.stderr)
        return False

    errors = validate_manifest_dict(
        manifest,
        expected_tag=expected_tag,
        expected_commit=expected_commit,
        expected_image=expected_image,
        expected_digest=expected_digest,
    )

    if errors:
        print(f"ERROR: Manifest verification failed with {len(errors)} error(s):", file=sys.stderr)
        for err in errors:
            print(f"  - {err}", file=sys.stderr)
        return False

    print(f"SUCCESS: Manifest {manifest_path} verified successfully:")
    print(f"  Tag: {manifest.get('tag')}")
    print(f"  Version: {manifest.get('version')}")
    print(f"  Commit: {manifest.get('commit')}")
    print(f"  Image: {manifest.get('image')}")
    plat_str = ", ".join(f"{p['os']}/{p['architecture']}" for p in manifest.get("platforms", []))
    print(f"  Platforms: {plat_str}")
    print(f"  Digest: {manifest.get('digest')}")
    return True


def main():
    parser = argparse.ArgumentParser(description="Generate and verify release-manifest.json")
    parser.add_argument("--generate", action="store_true", help="Generate manifest")
    parser.add_argument("--verify", action="store_true", help="Verify manifest")
    parser.add_argument("--from-evidence", help="Generate manifest directly from verified GHCR evidence JSON")
    parser.add_argument("--manifest", default="target/release-manifest.json", help="Path to manifest JSON")
    parser.add_argument("--tag", help="Release tag (e.g. v1.0.0-rc.2)")
    parser.add_argument("--commit", help="Release commit SHA (40-char hex)")
    parser.add_argument("--image", help="Container image (e.g. ghcr.io/pitydah/michi-micro-server:1.0.0-rc.2)")
    parser.add_argument("--platforms", default="linux/amd64,linux/arm64", help="Comma-separated platforms")
    parser.add_argument("--digest", help="Image digest (sha256:...)")

    args = parser.parse_args()

    if args.generate:
        if args.from_evidence:
            try:
                generate_manifest_from_evidence(
                    evidence_path=args.from_evidence,
                    output_file=args.manifest,
                    expected_tag=args.tag,
                    expected_commit=args.commit,
                )
                if not verify_manifest(
                    args.manifest,
                    expected_tag=args.tag,
                    expected_commit=args.commit,
                    expected_image=args.image,
                    expected_digest=args.digest,
                ):
                    sys.exit(1)
                sys.exit(0)
            except Exception as e:
                print(f"ERROR: {e}", file=sys.stderr)
                sys.exit(1)

        if not args.tag or not args.commit or not args.image or not args.digest:
            print("ERROR: --generate requires (--from-evidence) OR (--tag, --commit, --image, and --digest)", file=sys.stderr)
            sys.exit(1)

        raw_platforms = [p.strip() for p in args.platforms.split(",") if p.strip()]
        platform_objs = [
            {"os": p.split("/")[0], "architecture": p.split("/")[1], "digest": args.digest}
            for p in raw_platforms if "/" in p
        ]

        try:
            generate_manifest(
                tag=args.tag,
                commit=args.commit,
                image=args.image,
                platforms=platform_objs,
                output_file=args.manifest,
                digest=args.digest,
            )
            if not verify_manifest(
                args.manifest,
                expected_tag=args.tag,
                expected_commit=args.commit,
                expected_image=args.image,
                expected_digest=args.digest,
            ):
                sys.exit(1)
            sys.exit(0)
        except Exception as e:
            print(f"ERROR: {e}", file=sys.stderr)
            sys.exit(1)

    if args.verify:
        ok = verify_manifest(
            args.manifest,
            expected_tag=args.tag,
            expected_commit=args.commit,
            expected_image=args.image,
            expected_digest=args.digest,
        )
        sys.exit(0 if ok else 1)

    parser.print_help()
    sys.exit(1)


if __name__ == "__main__":
    main()
