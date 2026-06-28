# Michi Music Player — CLI Reference

A reference CLI player for Michi Micro Server using Michi Link v1.

## Requirements

- Python 3.8+
- mpv (for audio playback)
- aiohttp

## Install

```bash
pip install -r requirements.txt
```

## Usage

```bash
# Connect without auth
python player.py http://192.168.1.50:8096

# Connect with auth token
python player.py http://192.168.1.50:8096 --token abc123
```

## Commands

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

## How it works

Uses `MichiServerClient` from `clients/python-michi-client/michi_client.py`
which implements the full Michi Link v1 contract.
