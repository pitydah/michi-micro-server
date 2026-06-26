#!/bin/bash
set -e

BINARY="${1:-target/release/michi-server}"
INSTALL_DIR="${2:-/usr/bin}"

if [ ! -f "$BINARY" ]; then
    echo "Binary not found at $BINARY"
    echo "Usage: $0 [path-to-binary] [install-dir]"
    echo ""
    echo "Run 'cargo build --release' first, or provide the path to the compiled binary."
    exit 1
fi

echo "Creating michi user and group..."
id -u michi &>/dev/null || useradd --system --user-group --create-home michi

echo "Creating required directories..."
mkdir -p /etc/michi
mkdir -p /var/cache/michi
mkdir -p /music

echo "Installing binary to $INSTALL_DIR/michi-server..."
cp "$BINARY" "$INSTALL_DIR/michi-server"
chmod 755 "$INSTALL_DIR/michi-server"

echo "Installing systemd service..."
cp "$(dirname "$0")/michi.service" /etc/systemd/system/michi.service
systemctl daemon-reload

echo "Enabling and starting michi.service..."
systemctl enable michi.service
systemctl start michi.service

echo ""
echo "=== Michi Micro Server installed successfully ==="
echo ""
echo "To set up a sample music directory:"
echo "  mkdir -p /music/sample && cp /path/to/your/music/* /music/sample/"
echo ""
echo "Check service status:"
echo "  systemctl status michi.service"
echo ""
echo "View logs:"
echo "  journalctl -u michi.service -f"
