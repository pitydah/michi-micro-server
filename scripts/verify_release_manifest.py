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

def generate_manifest(tag: str, commit: str, image: str, platforms: list[str], output_file: str, digest: str | None = None) -> dict:
    if not tag:
        raise ValueError("Tag must not be empty")
    if not SHA_RE.match(commit):
        raise ValueError(f"Commit SHA must be a 40-character hex string: {commit}")
    if not image:
        raise ValueError("Image reference must not be empty")
    if not platforms:
        platforms = ["linux/amd64", "linux/arm64"]

    manifest = {
        "schema_version": "1.0.0",
        "tag": tag,
        "commit": commit,
        "image": image,
        "platforms": platforms,
        "digest": digest or "",
        "generated_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "publisher": "michi-micro-server-ci",
    }

    os.makedirs(os.path.dirname(os.path.abspath(output_file)), exist_ok=True)
    with open(output_file, "w", encoding="utf-8") as f:
        json.dump(manifest, f, indent=2)
        f.write("\n")

    print(f"Generated release manifest at {output_file}:")
    print(json.dumps(manifest, indent=2))
    return manifest

def verify_manifest(manifest_path: str, expected_tag: str | None = None, expected_commit: str | None = None, expected_image: str | None = None) -> bool:
    if not os.path.exists(manifest_path):
        print(f"ERROR: Manifest file not found: {manifest_path}", file=sys.stderr)
        return False

    try:
        with open(manifest_path, "r", encoding="utf-8") as f:
            manifest = json.load(f)
    except Exception as e:
        print(f"ERROR: Failed to parse manifest JSON: {e}", file=sys.stderr)
        return False

    errors = []
    required_keys = ["schema_version", "tag", "commit", "image", "platforms", "generated_at"]
    for key in required_keys:
        if key not in manifest:
            errors.append(f"Missing required key: {key}")

    if "commit" in manifest and not SHA_RE.match(manifest["commit"]):
        errors.append(f"Invalid commit SHA in manifest: {manifest['commit']}")

    if "platforms" in manifest:
        if not isinstance(manifest["platforms"], list) or len(manifest["platforms"]) == 0:
            errors.append("Platforms must be a non-empty list")
        else:
            required_platforms = {"linux/amd64", "linux/arm64"}
            found = set(manifest["platforms"])
            if not required_platforms.issubset(found):
                errors.append(f"Missing required multi-arch platforms: {required_platforms - found}")

    if expected_tag and manifest.get("tag") != expected_tag:
        errors.append(f"Tag mismatch: expected {expected_tag}, found {manifest.get('tag')}")

    if expected_commit and manifest.get("commit") != expected_commit:
        errors.append(f"Commit mismatch: expected {expected_commit}, found {manifest.get('commit')}")

    if expected_image and manifest.get("image") != expected_image:
        errors.append(f"Image mismatch: expected {expected_image}, found {manifest.get('image')}")

    if errors:
        print(f"ERROR: Manifest verification failed with {len(errors)} error(s):", file=sys.stderr)
        for err in errors:
            print(f"  - {err}", file=sys.stderr)
        return False

    print(f"SUCCESS: Manifest {manifest_path} verified successfully:")
    print(f"  Tag: {manifest.get('tag')}")
    print(f"  Commit: {manifest.get('commit')}")
    print(f"  Image: {manifest.get('image')}")
    print(f"  Platforms: {', '.join(manifest.get('platforms', []))}")
    return True

def main():
    parser = argparse.ArgumentParser(description="Generate and verify release-manifest.json")
    parser.add_argument("--generate", action="store_true", help="Generate manifest")
    parser.add_argument("--verify", action="store_true", help="Verify manifest")
    parser.add_argument("--manifest", default="target/release-manifest.json", help="Path to manifest JSON")
    parser.add_argument("--tag", help="Release tag (e.g. v1.0.0-rc.2)")
    parser.add_argument("--commit", help="Release commit SHA (40-char hex)")
    parser.add_argument("--image", help="Container image (e.g. ghcr.io/...:1.0.0-rc.2)")
    parser.add_argument("--platforms", default="linux/amd64,linux/arm64", help="Comma-separated platforms")
    parser.add_argument("--digest", help="Optional image digest")

    args = parser.parse_args()

    if args.generate:
        platforms = [p.strip() for p in args.platforms.split(",") if p.strip()]
        generate_manifest(
            tag=args.tag,
            commit=args.commit,
            image=args.image,
            platforms=platforms,
            output_file=args.manifest,
            digest=args.digest,
        )
        if not verify_manifest(args.manifest, expected_tag=args.tag, expected_commit=args.commit, expected_image=args.image):
            sys.exit(1)
        sys.exit(0)

    if args.verify:
        ok = verify_manifest(
            args.manifest,
            expected_tag=args.tag,
            expected_commit=args.commit,
            expected_image=args.image,
        )
        sys.exit(0 if ok else 1)

    parser.print_help()
    sys.exit(1)

if __name__ == "__main__":
    main()
