use axum::response::Html;

pub async fn root_handler() -> Html<&'static str> {
    Html(HTML)
}

const HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Michi Micro Server</title>
    <style>
        * { box-sizing: border-box; margin: 0; padding: 0; }
        body { font-family: system-ui, -apple-system, sans-serif; background: #1a1a2e; color: #e0e0e0; padding: 20px; }
        .container { max-width: 960px; margin: 0 auto; }
        h1 { color: #e94560; margin-bottom: 8px; }
        h2 { color: #e94560; margin: 24px 0 12px; font-size: 1.2rem; }
        .status-card { background: #16213e; border-radius: 8px; padding: 16px; margin-bottom: 16px; display: flex; gap: 24px; flex-wrap: wrap; }
        .status-item { display: flex; flex-direction: column; }
        .status-label { font-size: 0.75rem; color: #888; text-transform: uppercase; letter-spacing: 0.5px; }
        .status-value { font-size: 1.1rem; color: #4ecca3; }
        .btn { background: #e94560; color: #fff; border: none; padding: 10px 20px; border-radius: 6px; cursor: pointer; font-size: 0.9rem; }
        .btn:hover { background: #d63851; }
        .btn:disabled { opacity: 0.5; cursor: not-allowed; }
        .btn-scan { margin-bottom: 16px; }
        .loading { color: #888; font-style: italic; padding: 8px 0; }
        .error { color: #e94560; padding: 8px 0; }
        table { width: 100%; border-collapse: collapse; background: #16213e; border-radius: 8px; overflow: hidden; }
        th, td { padding: 10px 12px; text-align: left; border-bottom: 1px solid #0f3460; }
        th { background: #0f3460; color: #4ecca3; font-size: 0.75rem; text-transform: uppercase; letter-spacing: 0.5px; }
        tr:hover { background: #1a2744; }
        .play-btn { background: none; border: 1px solid #4ecca3; color: #4ecca3; padding: 4px 12px; border-radius: 4px; cursor: pointer; font-size: 0.8rem; }
        .play-btn:hover { background: #4ecca3; color: #1a1a2e; }
        .now-playing { background: #16213e; border-radius: 8px; padding: 16px; margin-bottom: 16px; display: none; }
        .now-playing.visible { display: block; }
        .now-playing h3 { color: #4ecca3; margin-bottom: 8px; }
        audio { width: 100%; margin-top: 8px; }
        .toast { position: fixed; top: 20px; right: 20px; background: #16213e; border: 1px solid #4ecca3; border-radius: 8px; padding: 12px 20px; display: none; z-index: 100; }
    </style>
</head>
<body>
    <div class="container">
        <h1>Michi Micro Server</h1>

        <div class="status-card" id="status">
            <div class="status-item"><span class="status-label">Status</span><span class="status-value" id="status-value">—</span></div>
            <div class="status-item"><span class="status-label">Version</span><span class="status-value" id="version-value">—</span></div>
            <div class="status-item"><span class="status-label">Port</span><span class="status-value" id="port-value">—</span></div>
            <div class="status-item"><span class="status-label">Tracks</span><span class="status-value" id="tracks-value">—</span></div>
            <div class="status-item"><span class="status-label">Albums</span><span class="status-value" id="albums-value">—</span></div>
            <div class="status-item"><span class="status-label">Artists</span><span class="status-value" id="artists-value">—</span></div>
        </div>

        <button class="btn btn-scan" id="scan-btn" onclick="scanLibrary()">Scan Library</button>

        <div class="now-playing" id="now-playing">
            <h3 id="now-title">Now Playing</h3>
            <audio id="audio-player" controls></audio>
        </div>

        <h2>Tracks</h2>
        <div id="tracks-container">
            <div class="loading" id="tracks-loading">Loading tracks...</div>
        </div>
    </div>

    <div class="toast" id="toast"></div>

    <script>
        async function api(url, opts) {
            const res = await fetch(url, opts);
            if (!res.ok) {
                const body = await res.json().catch(() => ({ message: res.statusText }));
                throw new Error(body.message || `HTTP ${res.status}`);
            }
            return res.json();
        }

        function showToast(msg, isErr) {
            const t = document.getElementById('toast');
            t.textContent = msg;
            t.style.borderColor = isErr ? '#e94560' : '#4ecca3';
            t.style.display = 'block';
            setTimeout(() => { t.style.display = 'none'; }, 3000);
        }

        async function loadStatus() {
            try {
                const s = await api('/api/status');
                document.getElementById('status-value').textContent = s.status;
                document.getElementById('version-value').textContent = s.version;
                document.getElementById('port-value').textContent = s.port;
            } catch (e) {
                document.getElementById('status-value').textContent = 'error';
                showToast('Failed to load status: ' + e.message, true);
            }
        }

        async function loadStats() {
            try {
                const s = await api('/api/library/stats');
                document.getElementById('tracks-value').textContent = s.tracks;
                document.getElementById('albums-value').textContent = s.albums;
                document.getElementById('artists-value').textContent = s.artists;
            } catch (e) {
                showToast('Failed to load stats: ' + e.message, true);
            }
        }

        function fmtDur(ms) {
            if (!ms) return '—';
            const total = Math.floor(ms / 1000);
            const m = Math.floor(total / 60);
            const s = total % 60;
            return m + ':' + s.toString().padStart(2, '0');
        }

        async function loadTracks() {
            const container = document.getElementById('tracks-container');
            try {
                const tracks = await api('/api/tracks');
                if (tracks.length === 0) {
                    container.innerHTML = '<p style="color:#888;">No tracks in library. Scan your music directory.</p>';
                    return;
                }
                let html = '<table><thead><tr><th>Title</th><th>Artist</th><th>Album</th><th>Format</th><th>Duration</th><th></th></tr></thead><tbody>';
                for (const t of tracks) {
                    html += '<tr>' +
                        '<td>' + esc(t.title || '—') + '</td>' +
                        '<td>' + esc(t.artist || '—') + '</td>' +
                        '<td>' + esc(t.album || '—') + '</td>' +
                        '<td>' + esc(t.format) + '</td>' +
                        '<td>' + fmtDur(t.duration_ms) + '</td>' +
                        '<td><button class="play-btn" onclick="playTrack(\'' + t.id + '\',\'' + escAttr(t.title || 'Track') + '\')">Play</button></td>' +
                        '</tr>';
                }
                html += '</tbody></table>';
                container.innerHTML = html;
            } catch (e) {
                container.innerHTML = '<div class="error">Failed to load tracks: ' + esc(e.message) + '</div>';
            }
        }

        function esc(s) {
            const div = document.createElement('div');
            div.textContent = s;
            return div.innerHTML;
        }

        function escAttr(s) {
            return s.replace(/'/g, "\\'");
        }

        async function scanLibrary() {
            const btn = document.getElementById('scan-btn');
            btn.disabled = true;
            btn.textContent = 'Scanning...';
            try {
                const result = await api('/api/library/scan', { method: 'POST' });
                showToast('Scanned ' + result.scanned + ' tracks, saved ' + result.saved);
                await loadStats();
                await loadTracks();
            } catch (e) {
                showToast('Scan failed: ' + e.message, true);
            } finally {
                btn.disabled = false;
                btn.textContent = 'Scan Library';
            }
        }

        function playTrack(id, title) {
            const np = document.getElementById('now-playing');
            np.classList.add('visible');
            document.getElementById('now-title').textContent = 'Now Playing: ' + title;
            const audio = document.getElementById('audio-player');
            audio.src = '/api/stream/' + id;
            audio.play().catch(function(e) {
                showToast('Playback failed: ' + e.message, true);
            });
        }

        loadStatus();
        loadStats();
        loadTracks();
    </script>
</body>
</html>"#;
