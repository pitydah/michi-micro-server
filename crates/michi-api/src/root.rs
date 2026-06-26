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

        .bar { display: flex; gap: 12px; align-items: center; flex-wrap: wrap; margin-bottom: 16px; }
        .bar input { flex: 1; min-width: 180px; padding: 10px 14px; border-radius: 6px; border: 1px solid #0f3460; background: #16213e; color: #e0e0e0; font-size: 0.9rem; }
        .bar input:focus { outline: none; border-color: #4ecca3; }
        .btn { background: #e94560; color: #fff; border: none; padding: 10px 20px; border-radius: 6px; cursor: pointer; font-size: 0.9rem; }
        .btn:hover { background: #d63851; }
        .btn:disabled { opacity: 0.5; cursor: not-allowed; }
        .btn-secondary { background: #0f3460; }
        .btn-secondary:hover { background: #1a4a8a; }

        .status-card { background: #16213e; border-radius: 8px; padding: 16px; margin-bottom: 16px; display: flex; gap: 24px; flex-wrap: wrap; }
        .status-item { display: flex; flex-direction: column; }
        .status-label { font-size: 0.75rem; color: #888; text-transform: uppercase; letter-spacing: 0.5px; }
        .status-value { font-size: 1.1rem; color: #4ecca3; }

        .player { background: #16213e; border-radius: 8px; padding: 16px; margin-bottom: 16px; display: none; }
        .player.visible { display: block; }
        .player h3 { color: #4ecca3; margin-bottom: 4px; font-size: 1rem; }
        .player .sub { color: #888; font-size: 0.85rem; margin-bottom: 8px; }
        .player audio { width: 100%; }

        table { width: 100%; border-collapse: collapse; background: #16213e; border-radius: 8px; overflow: hidden; }
        th, td { padding: 10px 12px; text-align: left; border-bottom: 1px solid #0f3460; }
        th { background: #0f3460; color: #4ecca3; font-size: 0.75rem; text-transform: uppercase; letter-spacing: 0.5px; }
        tr:hover { background: #1a2744; }
        .play-btn { background: none; border: 1px solid #4ecca3; color: #4ecca3; padding: 4px 12px; border-radius: 4px; cursor: pointer; font-size: 0.8rem; }
        .play-btn:hover { background: #4ecca3; color: #1a1a2e; }
        .empty { color: #888; padding: 16px 0; }
        .loading { color: #888; font-style: italic; padding: 8px 0; }
        .error { color: #e94560; padding: 8px 0; }
        .toast { position: fixed; top: 20px; right: 20px; background: #16213e; border: 1px solid #4ecca3; border-radius: 8px; padding: 12px 20px; display: none; z-index: 100; }
        .sr-only { position: absolute; width: 1px; height: 1px; overflow: hidden; clip: rect(0,0,0,0); }
    </style>
</head>
<body>
    <div class="container">
        <h1>Michi Micro Server</h1>

        <div class="status-card" id="status"></div>

        <div class="bar">
            <input type="text" id="search-input" placeholder="Search tracks by title, artist or album..." enterkeyhint="search">
            <button class="btn btn-secondary" id="search-btn" onclick="doSearch()">Search</button>
            <button class="btn" id="scan-btn" onclick="scanLibrary()">Scan</button>
        </div>

        <div class="player" id="player">
            <h3 id="now-title">Now Playing</h3>
            <div class="sub" id="now-sub"></div>
            <audio id="audio-player" controls></audio>
        </div>

        <h2 id="tracks-heading">Tracks</h2>
        <div id="tracks-container"><div class="loading">Loading tracks...</div></div>
    </div>

    <div class="toast" id="toast"></div>

    <script>
        let allTracks = [];
        let currentPlayIndex = -1;

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
                document.getElementById('status').innerHTML =
                    '<div class="status-item"><span class="status-label">Status</span><span class="status-value">' + esc(s.status) + '</span></div>' +
                    '<div class="status-item"><span class="status-label">Version</span><span class="status-value">' + esc(s.version) + '</span></div>' +
                    '<div class="status-item"><span class="status-label">Port</span><span class="status-value">' + esc(s.port) + '</span></div>';
            } catch (e) {
                document.getElementById('status').innerHTML = '<div class="error">Failed: ' + esc(e.message) + '</div>';
            }
        }

        async function loadStats() {
            try {
                const s = await api('/api/library/stats');
                const el = document.getElementById('status');
                if (el) {
                    el.innerHTML +=
                        '<div class="status-item"><span class="status-label">Tracks</span><span class="status-value">' + s.tracks + '</span></div>' +
                        '<div class="status-item"><span class="status-label">Albums</span><span class="status-value">' + s.albums + '</span></div>' +
                        '<div class="status-item"><span class="status-label">Artists</span><span class="status-value">' + s.artists + '</span></div>';
                }
            } catch (e) {
                showToast('Stats failed: ' + e.message, true);
            }
        }

        function fmtDur(ms) {
            if (!ms && ms !== 0) return '\u2014';
            const total = Math.floor(ms / 1000);
            const m = Math.floor(total / 60);
            const s = total % 60;
            return m + ':' + s.toString().padStart(2, '0');
        }

        function esc(s) {
            const d = document.createElement('div');
            d.textContent = String(s);
            return d.innerHTML;
        }

        function renderTracks(tracks) {
            const container = document.getElementById('tracks-container');
            const heading = document.getElementById('tracks-heading');
            if (tracks.length === 0) {
                container.innerHTML = '<div class="empty">No tracks found.</div>';
                return;
            }
            let html = '<table><thead><tr><th>Title</th><th>Artist</th><th>Album</th><th>Dur.</th><th></th></tr></thead><tbody>';
            for (let i = 0; i < tracks.length; i++) {
                const t = tracks[i];
                const fmt = t.format === 'unknown' ? '' : t.format;
                html += '<tr>' +
                    '<td>' + esc(t.title || '\u2014') + '</td>' +
                    '<td>' + esc(t.artist || '\u2014') + '</td>' +
                    '<td>' + esc(t.album || '\u2014') + ' <span style="color:#666;font-size:0.8rem">' + esc(fmt) + '</span></td>' +
                    '<td style="color:#888">' + fmtDur(t.duration_ms) + '</td>' +
                    '<td><button class="play-btn" data-idx="' + i + '">Play</button></td>' +
                    '</tr>';
            }
            html += '</tbody></table>';
            container.innerHTML = html;

            container.querySelectorAll('.play-btn').forEach(function(btn) {
                btn.addEventListener('click', function() {
                    const idx = parseInt(this.dataset.idx);
                    playTrack(tracks, idx);
                });
            });
        }

        async function loadTracks() {
            try {
                allTracks = await api('/api/tracks');
                document.getElementById('tracks-heading').textContent = 'Tracks';
                renderTracks(allTracks);
            } catch (e) {
                document.getElementById('tracks-container').innerHTML = '<div class="error">Failed: ' + esc(e.message) + '</div>';
            }
        }

        async function doSearch() {
            const q = document.getElementById('search-input').value.trim();
            const container = document.getElementById('tracks-container');
            const heading = document.getElementById('tracks-heading');

            if (!q) {
                heading.textContent = 'Tracks';
                renderTracks(allTracks);
                return;
            }

            container.innerHTML = '<div class="loading">Searching...</div>';
            heading.textContent = 'Search: "' + esc(q) + '"';

            try {
                const results = await api('/api/search?q=' + encodeURIComponent(q));
                renderTracks(results);
            } catch (e) {
                container.innerHTML = '<div class="error">Search failed: ' + esc(e.message) + '</div>';
            }
        }

        document.getElementById('search-input').addEventListener('keydown', function(e) {
            if (e.key === 'Enter') doSearch();
        });

        async function scanLibrary() {
            const btn = document.getElementById('scan-btn');
            btn.disabled = true;
            btn.textContent = 'Scanning...';
            try {
                const r = await api('/api/library/scan', { method: 'POST' });
                showToast('Scanned ' + r.scanned + ' tracks, saved ' + r.saved);
                await loadStats();
                await loadTracks();
            } catch (e) {
                showToast('Scan failed: ' + e.message, true);
            } finally {
                btn.disabled = false;
                btn.textContent = 'Scan';
            }
        }

        function playTrack(tracks, idx) {
            if (idx < 0 || idx >= tracks.length) return;
            currentPlayIndex = idx;
            const t = tracks[idx];
            const player = document.getElementById('player');
            player.classList.add('visible');
            document.getElementById('now-title').textContent = t.title || 'Unknown Track';
            document.getElementById('now-sub').textContent = (t.artist || 'Unknown Artist') + ' \u2014 ' + (t.album || 'Unknown Album');
            const audio = document.getElementById('audio-player');
            audio.src = '/api/stream/' + t.id;
            audio.play().catch(function(e) {
                showToast('Playback failed: ' + e.message, true);
            });
            audio.onended = function() {
                // Auto-advance to next track if we have a track list loaded
                if (tracks.length > 1 && currentPlayIndex >= 0 && currentPlayIndex < tracks.length - 1) {
                    playTrack(tracks, currentPlayIndex + 1);
                }
            };
        }

        loadStatus();
        loadStats();
        loadTracks();
    </script>
</body>
</html>"#;
