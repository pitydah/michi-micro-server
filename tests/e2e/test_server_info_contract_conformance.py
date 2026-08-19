#!/usr/bin/env python3
"""
Michi Link contract conformance gate for GET /api/v1/server/info.

Validates the live server's /api/v1/server/info response against the
canonical Michi Link v1 contract (roles, api_version, features shape).

This is the "capability truth gate" + "contract drift gate" (program sections 24-25).
It MUST fail if the server advertises non-canonical roles or a wrong api_version.

Usage:
    pytest tests/e2e/test_server_info_contract_conformance.py
    # or with a custom URL:
    MICHI_SERVER_URL=http://127.0.0.1:9092 pytest tests/e2e/test_server_info_contract_conformance.py
"""

import json
import os
import urllib.request

import pytest

SERVER_URL = os.environ.get("MICHI_SERVER_URL", "http://127.0.0.1:8096")

# --- Canonical Michi Link v1 constants (from michi-link tag michi-link-v1.0.0-alpha.1) ---

CANONICAL_API_VERSIONS = {"v1", "v1-lite"}

CANONICAL_ROLE_ENUM = {
    "desktop_player",
    "library_master",
    "sync_host",
    "music_server",
    "library_host",
    "playback_host",
    "mobile_player",
    "remote_controller",
    "sync_client",
    "audio_receiver",
}

# Micro-server is allowed to advertise ONLY this subset.
CANONICAL_MICRO_SERVER_ROLES = {
    "music_server",
    "library_host",
    "playback_host",
}

# Required top-level fields per server-info.schema.json.
REQUIRED_FIELDS = {"service", "name", "version", "api_version", "roles", "auth", "features"}


def _fetch_server_info():
    """Fetch /api/v1/server/info from the live server."""
    url = f"{SERVER_URL}/api/v1/server/info"
    try:
        with urllib.request.urlopen(url, timeout=5) as resp:
            assert resp.status == 200, f"expected 200, got {resp.status}"
            return json.loads(resp.read())
    except Exception as exc:
        pytest.fail(f"cannot fetch {url}: {exc}")


@pytest.fixture(scope="module")
def server_info():
    return _fetch_server_info()


class TestServerInfoConformance:
    """Contract drift gate: server/info MUST conform to Michi Link v1."""

    def test_required_fields_present(self, server_info):
        """All required top-level fields must be present."""
        missing = REQUIRED_FIELDS - set(server_info.keys())
        assert not missing, f"missing required fields: {missing}"

    def test_api_version_canonical(self, server_info):
        """api_version must be v1 or v1-lite (never a semantic version)."""
        av = server_info["api_version"]
        assert av in CANONICAL_API_VERSIONS, (
            f"api_version={av!r} is not canonical; must be one of {CANONICAL_API_VERSIONS}"
        )

    def test_roles_within_canonical_enum(self, server_info):
        """Every advertised role must exist in the canonical Role enum."""
        roles = server_info["roles"]
        non_canonical = set(roles) - CANONICAL_ROLE_ENUM
        assert not non_canonical, (
            f"non-canonical roles: {sorted(non_canonical)}; "
            f"canonical enum: {sorted(CANONICAL_ROLE_ENUM)}"
        )

    def test_micro_server_roles_subset(self, server_info):
        """Micro-server may only advertise {music_server, library_host, playback_host}."""
        roles = set(server_info["roles"])
        extra = roles - CANONICAL_MICRO_SERVER_ROLES
        assert not extra, (
            f"micro-server advertises roles outside its canonical subset: {sorted(extra)}; "
            f"allowed: {sorted(CANONICAL_MICRO_SERVER_ROLES)}"
        )

    def test_service_is_micro_server(self, server_info):
        """service field must identify as michi-micro-server."""
        assert server_info["service"] == "michi-micro-server"

    def test_features_is_object(self, server_info):
        """features must be a JSON object (boolean map)."""
        features = server_info["features"]
        assert isinstance(features, dict), f"features must be a dict, got {type(features)}"

    def test_auth_block_present(self, server_info):
        """auth block must have required, strategy, token_refresh."""
        auth = server_info["auth"]
        assert "required" in auth
        assert "strategy" in auth
        assert "token_refresh" in auth

    def test_no_phantom_michi_link_version(self, server_info):
        """michi_link_version MUST NOT exist (api_version is the sole authority)."""
        assert "michi_link_version" not in server_info, (
            "michi_link_version is a phantom field; api_version is the sole contract authority"
        )
