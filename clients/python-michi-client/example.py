"""Example: use MichiServerClient to browse and stream."""

import asyncio
from michi_client import MichiServerClient


async def main() -> None:
    client = MichiServerClient("http://localhost:8096")

    # Discover server
    info = await client.connect()
    print(f"Connected to {info.name} (v{info.version})")
    print(f"Server ID: {info.server_id}")
    print(f"Features: library={info.features.library} search={info.features.search}")

    # Browse library
    tracks = await client.get_tracks()
    print(f"Library has {len(tracks)} tracks")

    for t in tracks[:5]:
        title = t.get("title", "Unknown")
        artist = t.get("artist", "Unknown")
        print(f"  {title} — {artist}")

    # Search
    if len(tracks) > 0:
        results = await client.search_tracks("test")
        print(f"Search found {len(results)} results")

    # Stream URL
    if len(tracks) > 0:
        tid = tracks[0].get("id")
        url = client.stream_url(tid)
        print(f"Stream URL: {url}")

    # Stats
    stats = await client.get_library_stats()
    print(f"Stats: {stats}")


if __name__ == "__main__":
    asyncio.run(main())
