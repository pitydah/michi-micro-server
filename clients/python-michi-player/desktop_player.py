#!/usr/bin/env python3
"""Michi Music Player — PySide6 Desktop GUI (skeleton).

Connects to Michi Micro Server via Michi Link v1.
Requires: PySide6, aiohttp, qasync

Usage:
    python desktop_player.py http://localhost:8096
"""

from __future__ import annotations

import asyncio
import subprocess
import shutil
import sys
from pathlib import Path
from typing import Optional

sys.path.insert(0, str(Path(__file__).parent.parent / "python-michi-client"))
from michi_client import MichiServerClient

try:
    from PySide6.QtCore import QTimer, Qt, Signal
    from PySide6.QtGui import QFont
    from PySide6.QtWidgets import (
        QApplication,
        QHBoxLayout,
        QLabel,
        QLineEdit,
        QListWidget,
        QListWidgetItem,
        QMainWindow,
        QPushButton,
        QSplitter,
        QStatusBar,
        QTabWidget,
        QVBoxLayout,
        QWidget,
    )
    HAS_PYSIDE = True
except ImportError:
    HAS_PYSIDE = False


class ServerConnectDialog(QWidget):
    def __init__(self, client: MichiServerClient) -> None:
        super().__init__()
        self.client = client
        self.setWindowTitle("Connect to Michi")
        self.resize(400, 200)

        layout = QVBoxLayout()
        layout.addWidget(QLabel("Server URL:"))
        self.url_input = QLineEdit("http://localhost:8096")
        layout.addWidget(self.url_input)

        self.token_input = QLineEdit()
        self.token_input.setPlaceholderText("Auth token (optional)")
        layout.addWidget(self.token_input)

        self.btn = QPushButton("Connect")
        self.btn.clicked.connect(self.do_connect)
        layout.addWidget(self.btn)

        self.status_label = QLabel("")
        layout.addWidget(self.status_label)

        self.setLayout(layout)

    def do_connect(self) -> None:
        self.btn.setEnabled(False)
        self.status_label.setText("Connecting...")

        async def connect() -> None:
            try:
                url = self.url_input.text().strip()
                token = self.token_input.text().strip() or None
                if token:
                    self.client.token = token
                await self.client.connect()
                info = self.client.info
                if info:
                    self.status_label.setText(f"Connected: {info.name} v{info.version}")
                    QTimer.singleShot(500, self.accept)
                else:
                    self.status_label.setText("Failed: no server info")
                    self.btn.setEnabled(True)
            except Exception as e:
                self.status_label.setText(f"Error: {e}")
                self.btn.setEnabled(True)

        asyncio.ensure_future(connect())

    def accept(self) -> None:
        window = MichiPlayerWindow(self.client)
        window.show()
        self.parent_win = window
        self.close()


class MichiPlayerWindow(QMainWindow):
    def __init__(self, client: MichiServerClient) -> None:
        super().__init__()
        self.client = client
        self.setWindowTitle("Michi Music Player")
        self.resize(900, 600)

        central = QWidget()
        self.setCentralWidget(central)
        layout = QVBoxLayout(central)

        # Tabs
        self.tabs = QTabWidget()
        self.track_list = QListWidget()
        self.search_list = QListWidget()
        self.playlist_list = QListWidget()

        self.tabs.addTab(self.track_list, "Tracks")
        self.tabs.addTab(self.search_list, "Search")
        self.tabs.addTab(self.playlist_list, "Playlists")

        layout.addWidget(self.tabs)

        # Controls
        controls = QHBoxLayout()
        self.search_input = QLineEdit()
        self.search_input.setPlaceholderText("Search...")
        self.search_input.returnPressed.connect(self.do_search)
        controls.addWidget(self.search_input)

        self.play_btn = QPushButton("Play")
        self.play_btn.clicked.connect(self.play_selected)
        controls.addWidget(self.play_btn)

        self.stop_btn = QPushButton("Stop")
        self.stop_btn.clicked.connect(self.stop_playback)
        controls.addWidget(self.stop_btn)

        self.refresh_btn = QPushButton("Refresh")
        self.refresh_btn.clicked.connect(self.load_tracks)
        controls.addWidget(self.refresh_btn)

        layout.addLayout(controls)

        # Status bar
        self.status = QStatusBar()
        self.setStatusBar(self.status)

        self.mpv_process: Optional[subprocess.Popen] = None

        # Load initial data
        self.load_tracks()

    def load_tracks(self) -> None:
        async def _load() -> None:
            self.status.showMessage("Loading tracks...")
            try:
                tracks = await self.client.get_tracks()
                info = self.client.info
                if info:
                    stats = await self.client.get_library_stats()
                    self.status.showMessage(
                        f"{info.name} — {stats.get('tracks', len(tracks))} tracks"
                    )
                self.track_list.clear()
                for t in tracks:
                    title = t.get("title", "Unknown")
                    artist = t.get("artist", "Unknown")
                    item = QListWidgetItem(f"{title} — {artist}")
                    item.setData(Qt.UserRole, t)
                    self.track_list.addItem(item)
            except Exception as e:
                self.status.showMessage(f"Error: {e}")

        asyncio.ensure_future(_load())

    def do_search(self) -> None:
        query = self.search_input.text().strip()
        if not query:
            return

        async def _search() -> None:
            self.status.showMessage(f"Searching '{query}'...")
            try:
                results = await self.client.search_tracks(query)
                self.search_list.clear()
                for t in results:
                    title = t.get("title", "Unknown")
                    artist = t.get("artist", "Unknown")
                    item = QListWidgetItem(f"{title} — {artist}")
                    item.setData(Qt.UserRole, t)
                    self.search_list.addItem(item)
                self.tabs.setCurrentIndex(1)
                self.status.showMessage(f"Found {len(results)} results")
            except Exception as e:
                self.status.showMessage(f"Error: {e}")

        asyncio.ensure_future(_search())

    def play_selected(self) -> None:
        tab = self.tabs.currentIndex()
        if tab == 0:
            items = self.track_list.selectedItems()
        elif tab == 1:
            items = self.search_list.selectedItems()
        elif tab == 2:
            items = self.playlist_list.selectedItems()
        else:
            return

        if not items:
            self.status.showMessage("No track selected")
            return

        t = items[0].data(Qt.UserRole)
        if not t:
            return

        track_id = t.get("id")
        title = t.get("title", "Unknown")
        artist = t.get("artist", "Unknown")
        url = self.client.stream_url(track_id)

        self.stop_playback()
        mpv = shutil.which("mpv")
        if not mpv:
            self.status.showMessage("mpv not found — install mpv for playback")
            return

        self.mpv_process = subprocess.Popen(
            ["mpv", "--no-video", f"--title=Michi — {title} — {artist}", url],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        self.status.showMessage(f"Playing: {title} — {artist}")

    def stop_playback(self) -> None:
        if self.mpv_process:
            self.mpv_process.terminate()
            self.mpv_process = None


def main() -> None:
    if not HAS_PYSIDE:
        print("PySide6 not installed. Install with: pip install PySide6 aiohttp qasync")
        print("Then run: python desktop_player.py http://localhost:8096")
        sys.exit(1)

    server_url = sys.argv[1] if len(sys.argv) > 1 else "http://localhost:8096"
    token = None
    if "--token" in sys.argv:
        token = sys.argv[sys.argv.index("--token") + 1]

    client = MichiServerClient(server_url, token=token)

    app = QApplication(sys.argv)
    dialog = ServerConnectDialog(client)
    dialog.show()
    sys.exit(app.exec())


if __name__ == "__main__":
    main()
