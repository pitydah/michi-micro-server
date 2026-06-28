# Michi Music Player — CLI & Desktop

A reference player for Michi Micro Server using Michi Link v1.

## Requirements

- Python 3.8+
- mpv (for audio playback)
- aiohttp

### Desktop GUI (optional)
- PySide6: `pip install PySide6`

## Install

```bash
pip install -r requirements.txt
```

## CLI Usage

```bash
# Connect without auth
python player.py http://192.168.1.50:8096

# Connect with auth token
python player.py http://192.168.1.50:8096 --token abc123
```

## Desktop GUI Usage

```bash
# Requires PySide6
pip install PySide6
python desktop_player.py http://192.168.1.50:8096
```

## CLI Commands

| Key | Action |
|-----|--------|
| `l` | List tracks (first page) |
| `n` | Next page |
| `p` | Previous page |
| `s` | Search |
| `[number]` | Play track by index |
| `x` | Stop playback |
| `pl` | Browse playlists |
| `q` | Quit |

## Desktop GUI Features

- Browse tracks, search, playlists
- Double-click to play via mpv
- Server info dialog
- Stop/play controls
- Status bar with connection info

## How it works

Uses `MichiServerClient` from `clients/python-michi-client/michi_client.py`
which implements the full Michi Link v1 contract.
