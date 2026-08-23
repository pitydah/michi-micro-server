#!/bin/sh
set -e

# Support PUID and PGID for Docker volume permission alignment (ZimaOS / CasaOS / NAS)
PUID=${PUID:-1000}
PGID=${PGID:-1000}

if [ "$(id -u)" = "0" ]; then
    # Adjust group ID if needed
    if [ "$(id -g michi 2>/dev/null)" != "$PGID" ]; then
        groupmod -o -g "$PGID" michi 2>/dev/null || true
    fi

    # Adjust user ID if needed
    if [ "$(id -u michi 2>/dev/null)" != "$PUID" ]; then
        usermod -o -u "$PUID" -g "$PGID" michi 2>/dev/null || true
    fi

    # Ensure /config and /cache are writable by michi
    mkdir -p /config /cache /music
    chown -R michi:michi /config /cache 2>/dev/null || true

    exec gosu michi michi-server "$@"
fi

exec michi-server "$@"
