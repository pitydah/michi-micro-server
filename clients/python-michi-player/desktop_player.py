#!/usr/bin/env python3
"""Michi Music Player — PySide6 Desktop GUI.

Connects to Michi Micro Server via Michi Link v1.
Requires: PySide6, aiohttp, qasync

Usage:
    python desktop_player.py http://localhost:8096
"""

from __future__ import annotations

import asyncio
import shutil
import subprocess
import sys
from pathlib import Path
from typing import Optional

sys.path.insert(0, str(Path(__file__).parent.parent / "python-michi-client"))
from michi_client import MichiServerClient, V1Error

try:
    from PySide6.QtCore import Qt, Slot
    from PySide6.QtGui import QAction
    from PySide6.QtWidgets import (
        QApplication,
        QHBoxLayout,
        QLabel,
        QLineEdit,
        QListWidget,
        QListWidgetItem,
        QMainWindow,
        QMenuBar,
        QMessageBox,
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


class Tabs:
    TRACKS = 0
    SEARCH = 1
    PLAYLISTS = 2


class MichiPlayerWindow(QMainWindow):
    def __init__(self, client: MichiServerClient, server_url: str) -> None:
        super().__init__()
        self.client = client
        self.server_url = server_url
        self.setWindowTitle(f"Michi Music Player — {client.info.name if client.info else 'Connecting...'}")
        self.resize(1000, 650)

        self._mpv_process: Optional[subprocess.Popen] = None
        self._all_tracks: list[dict] = []

        self._build_ui()
        self._build_menu()

        self.load_data()

    def _build_ui(self) -> None:
        central = QWidget()
        self.setCentralWidget(central)
        layout = QVBoxLayout(central)

        # Tabs
        self.tabs = QTabWidget()
        self.track_list = QListWidget()
        self.track_list.itemDoubleClicked.connect(self.play_selected)
        self.search_list = QListWidget()
        self.search_list.itemDoubleClicked.connect(self.play_selected)
        self.playlist_list = QListWidget()
        self.playlist_list.itemClicked.connect(self.load_playlist_tracks)

        self.tabs.addTab(self.track_list, "Tracks")
        self.tabs.addTab(self.search_list, "Search")
        self.tabs.addTab(self.playlist_list, "Playlists")

        layout.addWidget(self.tabs)

        # Player controls
        controls = QHBoxLayout()
        self.now_label = QLabel("Not playing")
        controls.addWidget(self.now_label, 1)

        self.play_btn = QPushButton("Play")
        self.play_btn.clicked.connect(self.play_selected)
        controls.addWidget(self.play_btn)

        self.stop_btn = QPushButton("Stop")
        self.stop_btn.clicked.connect(self.stop_playback)
        controls.addWidget(self.stop_btn)

        layout.addLayout(controls)

        # Search bar
        search_bar = QHBoxLayout()
        self.search_input = QLineEdit()
        self.search_input.setPlaceholderText("Search tracks...")
        self.search_input.returnPressed.connect(self.do_search)
        search_bar.addWidget(self.search_input, 1)

        self.refresh_btn = QPushButton("Refresh")
        self.refresh_btn.clicked.connect(self.load_data)
        search_bar.addWidget(self.refresh_btn)

        layout.addLayout(search_bar)

        # Status bar
        self.status = QStatusBar()
        self.setStatusBar(self.status)
        self.status.showMessage(f"Connected to {self.server_url}")

    def _build_menu(self) -> None:
        menu = self.menuBar()
        file_menu = menu.addMenu("&File")
        quit_action = QAction("Quit", self)
        quit_action.triggered.connect(self.close)
        file_menu.addAction(quit_action)

        server_menu = menu.addMenu("&Server")
        info_action = QAction("Server Info", self)
        info_action.triggered.connect(self.show_server_info)
        server_menu.addAction(info_action)

    def show_server_info(self) -> None:
        info = self.client.info
        if not info:
            return
        msg = (
            f"Server: {info.name}\n"
            f"Version: {info.version}\n"
            f"API: {info.api_version}\n"
            f"ID: {info.server_id}\n"
            f"Features:\n"
            f"  Library: {info.features.library}\n"
            f"  Search: {info.features.search}\n"
            f"  Streaming: {info.features.streaming}\n"
            f"  Playlists: {info.features.playlists}\n"
            f"  Artwork: {info.features.artwork}\n"
            f"  WebSocket: {info.features.websocket}"
        )
        QMessageBox.information(self, "Server Info", msg)

    def load_data(self) -> None:
        self.load_tracks()
        self.load_playlists()

    def load_tracks(self) -> None:
        async def _load() -> None:
            self.track_list.clear()
            self.status.showMessage("Loading tracks...")
            try:
                self._all_tracks = await self.client.get_tracks()
                for t in self._all_tracks:
                    title = t.get("title", "Unknown")
                    artist = t.get("artist", "Unknown")
                    album = t.get("album", "")
                    item = QListWidgetItem(f"{title} — {artist}")
                    if album:
                        item.setText(f"{title} — {artist} | {album}")
                    item.setData(Qt.UserRole, t)
                    self.track_list.addItem(item)
                self.status.showMessage(
                    f"{len(self._all_tracks)} tracks loaded"
                )
            except Exception as e:
                self.status.showMessage(f"Error loading tracks: {e}")

        asyncio.ensure_future(_load())

    def load_playlists(self) -> None:
        if not self.client.has_feature("playlists"):
            return
        async def _load() -> None:
            self.playlist_list.clear()
            try:
                playlists = await self.client.get_playlists()
                for pl in playlists:
                    item = QListWidgetItem(
                        f"{pl.get('name', '?')} ({pl.get('track_count', 0)} tracks)"
                    )
                    item.setData(Qt.UserRole, pl)
                    self.playlist_list.addItem(item)
            except Exception:
                pass
        asyncio.ensure_future(_load())

    def load_playlist_tracks(self, item: QListWidgetItem) -> None:
        pl = item.data(Qt.UserRole)
        if not pl:
            return
        async def _load() -> None:
            self.status.showMessage(f"Loading playlist: {pl.get('name', '')}...")
            try:
                tracks = await self.client.get_playlist_tracks(pl["id"])
                self.track_list.clear()
                for t in tracks:
                    title = t.get("title", "Unknown")
                    artist = t.get("artist", "Unknown")
                    litem = QListWidgetItem(f"{title} — {artist}")
                    litem.setData(Qt.UserRole, t)
                    self.track_list.addItem(litem)
                self.tabs.setCurrentIndex(Tabs.TRACKS)
                self.status.showMessage(
                    f"Playlist: {pl.get('name', '')} ({len(tracks)} tracks)"
                )
            except Exception as e:
                self.status.showMessage(f"Error: {e}")
        asyncio.ensure_future(_load())

    def do_search(self) -> None:
        query = self.search_input.text().strip()
        if not query:
            return

        async def _search() -> None:
            self.search_list.clear()
            self.status.showMessage(f"Searching: {query}...")
            try:
                results = await self.client.search_tracks(query)
                for t in results:
                    title = t.get("title", "Unknown")
                    artist = t.get("artist", "Unknown")
                    item = QListWidgetItem(f"{title} — {artist}")
                    item.setData(Qt.UserRole, t)
                    self.search_list.addItem(item)
                self.tabs.setCurrentIndex(Tabs.SEARCH)
                self.status.showMessage(f"Found {len(results)} results for '{query}'")
            except Exception as e:
                self.status.showMessage(f"Search error: {e}")

        asyncio.ensure_future(_search())

    def play_selected(self) -> None:
        tab = self.tabs.currentIndex()
        source = [self.track_list, self.search_list, self.playlist_list]
        if tab >= len(source):
            return
        items = source[tab].selectedItems()
        if not items:
            self.status.showMessage("No track selected")
            return
        t = items[0].data(Qt.UserRole)
        track_id = t.get("id")
        title = t.get("title", "Unknown")
        artist = t.get("artist", "Unknown")
        if not track_id:
            return
        url = self.client.stream_url(track_id)
        self.stop_playback()
        mpv = shutil.which("mpv")
        if not mpv:
            self.status.showMessage("Install mpv for playback")
            self.now_label.setText(f"mpv required — {title} — {artist}")
            return
        self._mpv_process = subprocess.Popen(
            ["mpv", "--no-video", f"--title=Michi — {title}", url],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        self.now_label.setText(f"Playing: {title} — {artist}")
        self.status.showMessage(f"Playing: {title} — {artist}")

    def stop_playback(self) -> None:
        if self._mpv_process:
            self._mpv_process.terminate()
            self._mpv_process = None
            self.now_label.setText("Stopped")

    def closeEvent(self, event) -> None:
        self.stop_playback()
        super().closeEvent(event)


def main() -> None:
    if not HAS_PYSIDE:
        print("ERROR: PySide6 not installed.")
        print("Install: pip install PySide6 aiohttp")
        sys.exit(1)

    server_url = sys.argv[1] if len(sys.argv) > 1 else "http://localhost:8096"
    token = None
    if "--token" in sys.argv:
        token = sys.argv[sys.argv.index("--token") + 1]

    client = MichiServerClient(server_url, token=token)

    # Run connect synchronously before starting Qt
    async def connect() -> MichiServerClient:
        await client.connect()
        return client

    try:
        loop = asyncio.get_event_loop()
        loop.run_until_complete(connect())
    except Exception as e:
        print(f"ERROR connecting to {server_url}: {e}")
        sys.exit(1)

    app = QApplication(sys.argv)
    window = MichiPlayerWindow(client, server_url)
    window.show()
    sys.exit(app.exec())


if __name__ == "__main__":
    main()
