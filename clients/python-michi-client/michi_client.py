"""MichiServerClient — Python client for Michi Link v1.

Reference implementation for Michi Music Player integration.
Uses only the /api/v1 contract. No legacy endpoints.

Usage:
    client = MichiServerClient("http://192.168.1.50:8096")
    info = await client.connect()
    tracks = await client.get_tracks()
    results = await client.search_tracks("pink floyd")
"""

from __future__ import annotations

import asyncio
import json
from dataclasses import dataclass, field
from datetime import datetime
from typing import Optional
from urllib.parse import quote


@dataclass
class ServerFeatures:
    library: bool = False
    search: bool = False
    streaming: bool = False
    web_ui: bool = False
    playlists: bool = False
    artwork: bool = False
    sync: bool = False
    transcoding: bool = False
    websocket: bool = False


@dataclass
class ServerInfo:
    name: str = ""
    server_id: str = ""
    version: str = ""
    api_version: str = ""
    features: ServerFeatures = field(default_factory=ServerFeatures)


class V1Error(Exception):
    code: str
    message: str

    def __init__(self, code: str, message: str) -> None:
        super().__init__(f"[{code}] {message}")
        self.code = code
        self.message = message


class MichiServerClient:
    def __init__(
        self,
        server_url: str,
        token: Optional[str] = None,
        timeout: float = 10.0,
    ) -> None:
        self.server_url = server_url.rstrip("/")
        self.token = token
        self.timeout = timeout
        self.info: Optional[ServerInfo] = None
        self.last_seen: Optional[datetime] = None

    def _headers(self) -> dict[str, str]:
        h = {}
        if self.token:
            h["Authorization"] = f"Bearer {self.token}"
        return h

    def _handle_error(self, status: int, body: str) -> V1Error:
        try:
            data = json.loads(body)
            err = data.get("error", {})
            return V1Error(
                code=err.get("code", "UNKNOWN"),
                message=err.get("message", body),
            )
        except (json.JSONDecodeError, KeyError):
            return V1Error("UNKNOWN", body)

    async def connect(self) -> ServerInfo:
        """Discover the server via GET /api/v1/server/info."""
        import aiohttp

        url = f"{self.server_url}/api/v1/server/info"
        async with aiohttp.ClientSession() as session:
            async with session.get(url, timeout=self.timeout) as resp:
                if resp.status != 200:
                    body = await resp.text()
                    raise self._handle_error(resp.status, body)
                data = await resp.json()

        features = ServerFeatures(**data.get("features", {}))
        info = ServerInfo(
            name=data.get("name", ""),
            server_id=data.get("server_id", ""),
            version=data.get("version", ""),
            api_version=data.get("api_version", ""),
            features=features,
        )

        if info.api_version != "v1":
            raise V1Error(
                "VERSION_MISMATCH",
                f"expected v1, got {info.api_version}",
            )

        self.info = info
        self.last_seen = datetime.utcnow()
        return info

    async def login(self, username: str, password: str) -> dict:
        """Authenticate via POST /api/auth/login."""
        import aiohttp

        url = f"{self.server_url}/api/auth/login"
        async with aiohttp.ClientSession() as session:
            async with session.post(
                url,
                json={"username": username, "password": password},
                timeout=self.timeout,
            ) as resp:
                data = await resp.json()
                if resp.status != 200:
                    err = data.get("error", {})
                    raise V1Error(
                        code=err.get("code", "AUTH_ERROR"),
                        message=err.get("message", "login failed"),
                    )
                self.token = data.get("token")
                self.last_seen = datetime.utcnow()
                return data

    async def get_tracks(self) -> list[dict]:
        import aiohttp

        url = f"{self.server_url}/api/v1/tracks"
        async with aiohttp.ClientSession() as session:
            async with session.get(
                url, headers=self._headers(), timeout=self.timeout
            ) as resp:
                if resp.status != 200:
                    body = await resp.text()
                    raise self._handle_error(resp.status, body)
                return await resp.json()

    async def search_tracks(self, query: str) -> list[dict]:
        import aiohttp

        url = f"{self.server_url}/api/v1/search?q={quote(query)}"
        async with aiohttp.ClientSession() as session:
            async with session.get(
                url, headers=self._headers(), timeout=self.timeout
            ) as resp:
                if resp.status != 200:
                    body = await resp.text()
                    raise self._handle_error(resp.status, body)
                return await resp.json()

    async def get_track(self, track_id: str) -> dict:
        import aiohttp

        url = f"{self.server_url}/api/v1/tracks/{track_id}"
        async with aiohttp.ClientSession() as session:
            async with session.get(
                url, headers=self._headers(), timeout=self.timeout
            ) as resp:
                if resp.status != 200:
                    body = await resp.text()
                    raise self._handle_error(resp.status, body)
                return await resp.json()

    async def get_library_stats(self) -> dict:
        import aiohttp

        url = f"{self.server_url}/api/v1/library/stats"
        async with aiohttp.ClientSession() as session:
            async with session.get(
                url, headers=self._headers(), timeout=self.timeout
            ) as resp:
                if resp.status != 200:
                    body = await resp.text()
                    raise self._handle_error(resp.status, body)
                return await resp.json()

    async def get_status(self) -> dict:
        import aiohttp

        url = f"{self.server_url}/api/v1/status"
        async with aiohttp.ClientSession() as session:
            async with session.get(
                url, headers=self._headers(), timeout=self.timeout
            ) as resp:
                if resp.status != 200:
                    body = await resp.text()
                    raise self._handle_error(resp.status, body)
                return await resp.json()

    async def get_playlists(self) -> list[dict]:
        import aiohttp

        url = f"{self.server_url}/api/v1/playlists"
        async with aiohttp.ClientSession() as session:
            async with session.get(
                url, headers=self._headers(), timeout=self.timeout
            ) as resp:
                if resp.status != 200:
                    body = await resp.text()
                    raise self._handle_error(resp.status, body)
                return await resp.json()

    def stream_url(self, track_id: str) -> str:
        return f"{self.server_url}/api/v1/stream/{track_id}"

    def artwork_url(self, artwork_id: str) -> str:
        return f"{self.server_url}/api/v1/artwork/{artwork_id}"

    def has_feature(self, name: str) -> bool:
        if not self.info:
            return False
        return getattr(self.info.features, name, False)

    @property
    def is_authenticated(self) -> bool:
        return self.token is not None

    @property
    def server_id(self) -> Optional[str]:
        if not self.info:
            return None
        return self.info.server_id
