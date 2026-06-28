#!/usr/bin/env python3
"""Michi Music Player — CLI player using Michi Link v1.

Connects to Michi Micro Server, browses library, plays via mpv.

Usage:
    python player.py http://localhost:8096
"""

from __future__ import annotations

import asyncio
import json
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Optional

sys.path.insert(0, str(Path(__file__).parent.parent / "python-michi-client"))
from michi_client import MichiServerClient


def clear() -> None:
    os.system("cls" if os.name == "nt" else "clear")


def play_mpv(url: str, title: str = "Michi Player") -> subprocess.Popen:
    mpv = shutil.which("mpv")
    if not mpv:
        print("ERROR: mpv not found. Install mpv to play audio.")
        return subprocess.Popen([sys.executable, "-c", "pass"])
    return subprocess.Popen(
        ["mpv", "--no-video", f"--title={title}", url],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )


def fmt_dur(ms: Optional[int]) -> str:
    if not ms:
        return "--:--"
    total = ms // 1000
    return f"{total // 60}:{total % 60:02d}"


async def show_server_info(client: MichiServerClient) -> None:
    info = client.info
    if not info:
        return
    print(f"  Server: {info.name}")
    print(f"  Version: {info.version}")
    print(f"  API: {info.api_version}")
    print(f"  Server ID: {info.server_id}")
    print(f"  Features:")
    f = info.features
    print(f"    Library: {f.library}, Search: {f.search}, Streaming: {f.streaming}")
    print(f"    Playlists: {f.playlists}, Artwork: {f.artwork}")
    print(f"    WebSocket: {f.websocket}, Transcoding: {f.transcoding}")
    print()

    stats = await client.get_library_stats()
    print(f"  Library: {stats.get('tracks', '?')} tracks / {stats.get('albums', '?')} albums / {stats.get('artists', '?')} artists")


async def list_tracks(client: MichiServerClient, page: int = 0, page_size: int = 20) -> list[dict]:
    tracks = await client.get_tracks()
    start = page * page_size
    end = start + page_size
    page_tracks = tracks[start:end]

    if not page_tracks:
        print("  No more tracks.")
        return tracks

    for i, t in enumerate(page_tracks, start=start):
        title = t.get("title", "Unknown")
        artist = t.get("artist", "Unknown")
        album = t.get("album", "")
        dur = fmt_dur(t.get("duration_ms"))
        print(f"  [{i:4d}] {title:30s} {artist:20s} {album:20s} {dur:>6s}")

    print(f"\n  Page {page + 1}/{(len(tracks) - 1) // page_size + 1} ({len(tracks)} total)")
    return tracks


async def search_ui(client: MichiServerClient) -> None:
    query = input("  Search: ").strip()
    if not query:
        return
    results = await client.search_tracks(query)
    print(f"\n  Found {len(results)} results:\n")
    for t in results[:10]:
        print(f"  {t.get('title', '?')} — {t.get('artist', '?')}")


async def playlist_browser(client: MichiServerClient) -> None:
    if not client.has_feature("playlists"):
        print("  Playlists not available on this server.")
        input("  Press Enter to continue...")
        return

    playlists = await client.get_playlists()
    if not playlists:
        print("  No playlists.")
        input("  Press Enter to continue...")
        return

    for i, pl in enumerate(playlists):
        print(f"  [{i}] {pl.get('name', '?')} ({pl.get('track_count', 0)} tracks)")

    try:
        idx = int(input("\n  Select playlist: "))
        if idx < 0 or idx >= len(playlists):
            return
        pl_id = playlists[idx].get("id")
        tracks = await client.get_playlist_tracks(pl_id)
        print(f"\n  {playlists[idx]['name']} — {len(tracks)} tracks:\n")
        for t in tracks[:15]:
            print(f"  {t.get('title', '?')} — {t.get('artist', '?')}")
    except (ValueError, IndexError):
        pass

    input("\n  Press Enter to continue...")


async def main() -> None:
    if len(sys.argv) < 2:
        print(f"Usage: {sys.argv[0]} <server_url> [--token TOKEN]")
        sys.exit(1)

    server_url = sys.argv[1]
    token = None
    if "--token" in sys.argv:
        token = sys.argv[sys.argv.index("--token") + 1]

    client = MichiServerClient(server_url, token=token)

    # Connect
    print(f"Connecting to {server_url}...")
    try:
        await client.connect()
    except Exception as e:
        print(f"ERROR: {e}")
        sys.exit(1)

    current_page = 0
    mpv_process: Optional[subprocess.Popen] = None
    current_tracks: list[dict] = []
    selected_idx: Optional[int] = None

    while True:
        clear()
        print("=" * 70)
        print("  Michi Music Player — Michi Link v1")
        print("=" * 70)
        print()

        await show_server_info(client)
        print()

        print("  Commands:")
        print("    l          — List tracks")
        print("    n          — Next page")
        print("    p          — Previous page")
        print("    s          — Search")
        print("    [number]   — Play track by index")
        print("    x          — Stop playback")
        print("    pl         — Browse playlists")
        print("    q          — Quit")
        print()

        if current_tracks:
            print("  Current tracks:")
            print()
            current_tracks = await list_tracks(client, current_page)

        print()
        cmd = input("> ").strip().lower()

        if cmd == "q":
            if mpv_process:
                mpv_process.terminate()
            break
        elif cmd == "l":
            current_page = 0
            current_tracks = []
        elif cmd == "n":
            if current_tracks:
                current_page += 1
        elif cmd == "p":
            if current_page > 0:
                current_page -= 1
        elif cmd == "s":
            await search_ui(client)
            input("  Press Enter to continue...")
        elif cmd == "pl":
            await playlist_browser(client)
        elif cmd == "x":
            if mpv_process:
                mpv_process.terminate()
                mpv_process = None
        elif cmd.isdigit():
            idx = int(cmd)
            try:
                tracks = await client.get_tracks()
                if idx < len(tracks):
                    selected_idx = idx
                    t = tracks[idx]
                    url = client.stream_url(t["id"])
                    title = t.get("title", "Unknown")
                    artist = t.get("artist", "Unknown")
                    print(f"\n  Playing: {title} — {artist}")
                    if mpv_process:
                        mpv_process.terminate()
                    mpv_process = play_mpv(url, f"{title} — {artist}")
                    input("  Playing in mpv. Press Enter to return...")
            except Exception as e:
                print(f"  ERROR: {e}")
                input("  Press Enter to continue...")


if __name__ == "__main__":
    asyncio.run(main())
