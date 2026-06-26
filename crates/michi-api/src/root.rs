use axum::response::Html;

pub async fn root_handler() -> Html<&'static str> {
    Html(HTML)
}

const HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <link rel="manifest" href="/manifest.json">
    <title>Michi Micro Server</title>
    <style>
        * { box-sizing: border-box; margin: 0; padding: 0; }
        :root { --bg: #1a1a2e; --bg2: #16213e; --bg3: #0f3460; --fg: #e0e0e0; --fg2: #888; --accent: #e94560; --green: #4ecca3; --border: #0f3460; --hover: #1a2744; }
        .light { --bg: #f5f5f5; --bg2: #ffffff; --bg3: #e0e0e0; --fg: #222; --fg2: #777; --accent: #c0392b; --green: #27ae60; --border: #ddd; --hover: #f0f0f0; }
        body { font-family: system-ui, -apple-system, sans-serif; background: var(--bg); color: var(--fg); padding: 20px; transition: background .3s, color .3s; }
        .login-container { max-width: 360px; margin: 80px auto; background: var(--bg2); border-radius: 12px; padding: 32px; text-align: center; }
        .login-container h1 { color: var(--accent); margin-bottom: 24px; }
        .login-container input { width: 100%; padding: 12px 16px; margin-bottom: 12px; border-radius: 6px; border: 1px solid var(--border); background: var(--bg); color: var(--fg); font-size: .95rem; }
        .login-container input:focus { outline: none; border-color: var(--green); }
        .login-container .btn { width: 100%; margin-top: 8px; }
        .login-error { color: var(--accent); margin-top: 12px; font-size: .85rem; }
        .container { max-width: 960px; margin: 0 auto; }
        .header { display: flex; align-items: center; gap: 12px; margin-bottom: 8px; flex-wrap: wrap; }
        .header h1 { color: var(--accent); flex: 1; }
        .theme-btn { background: none; border: 1px solid var(--fg2); color: var(--fg2); padding: 4px 10px; border-radius: 4px; cursor: pointer; font-size: .8rem; }
        h2 { color: var(--accent); margin: 16px 0 12px; font-size: 1.2rem; }
        .count { color: var(--fg2); font-size: .85rem; font-weight: normal; }
        .bar { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; margin-bottom: 16px; }
        .bar input { flex: 1; min-width: 160px; padding: 10px 14px; border-radius: 6px; border: 1px solid var(--border); background: var(--bg2); color: var(--fg); font-size: .9rem; }
        .bar input:focus { outline: none; border-color: var(--green); }
        .btn { background: var(--accent); color: #fff; border: none; padding: 10px 20px; border-radius: 6px; cursor: pointer; font-size: .85rem; }
        .btn:hover { filter: brightness(1.1); }
        .btn:disabled { opacity: .5; cursor: not-allowed; }
        .btn-sm { padding: 6px 12px; font-size: .8rem; }
        .btn-secondary { background: var(--bg3); color: var(--fg); }
        .btn-danger { background: #b91c1c; color: #fff; }
        .btn-icon { background: none; border: none; color: var(--fg2); cursor: pointer; padding: 2px 6px; font-size: .8rem; }
        .btn-icon:hover { color: var(--green); }
        .tabs { display: flex; gap: 4px; margin-bottom: 16px; flex-wrap: wrap; }
        .tab { padding: 8px 18px; border-radius: 6px 6px 0 0; cursor: pointer; background: var(--bg3); color: var(--fg2); font-size: .85rem; border: none; }
        .tab:hover { filter: brightness(1.2); }
        .tab.active { background: var(--bg2); color: var(--green); }
        .tab-content { display: none; }
        .tab-content.active { display: block; }
        .status-card { background: var(--bg2); border-radius: 8px; padding: 16px; margin-bottom: 16px; display: flex; gap: 24px; flex-wrap: wrap; }
        .status-item { display: flex; flex-direction: column; }
        .status-label { font-size: .75rem; color: var(--fg2); text-transform: uppercase; letter-spacing: .5px; }
        .status-value { font-size: 1.1rem; color: var(--green); }
        .player { background: var(--bg2); border-radius: 8px; padding: 16px; margin-bottom: 16px; display: none; gap: 16px; }
        .player.visible { display: flex; }
        .player .info { flex: 1; min-width: 0; }
        .player .cover { width: 120px; height: 120px; border-radius: 6px; object-fit: cover; background: var(--bg3); flex-shrink: 0; }
        .player h3 { color: var(--green); margin-bottom: 2px; font-size: 1rem; }
        .player .sub { color: var(--fg2); font-size: .85rem; margin-bottom: 4px; }
        .player .meta { color: var(--fg2); font-size: .8rem; margin-bottom: 4px; }
        .player audio { width: 100%; }
        .player .controls { display: flex; align-items: center; gap: 8px; margin-top: 4px; }
        .player .controls input[type=range] { flex: 1; max-width: 120px; accent-color: var(--green); }
        .player .shortcuts-hint { color: var(--fg2); font-size: .7rem; margin-top: 4px; }
        table { width: 100%; border-collapse: collapse; background: var(--bg2); border-radius: 8px; overflow: hidden; }
        th, td { padding: 8px 10px; text-align: left; border-bottom: 1px solid var(--border); }
        th { background: var(--bg3); color: var(--green); font-size: .75rem; text-transform: uppercase; letter-spacing: .5px; }
        tr:hover { background: var(--hover); }
        .clickable { cursor: pointer; }
        .clickable:hover { color: var(--green); }
        .play-btn { background: none; border: 1px solid var(--green); color: var(--green); padding: 4px 12px; border-radius: 4px; cursor: pointer; font-size: .8rem; }
        .play-btn:hover { background: var(--green); color: var(--bg); }
        .q-btn { background: none; border: none; color: var(--fg2); cursor: pointer; padding: 2px 6px; font-size: .75rem; }
        .q-btn:hover { color: var(--green); }
        .edit-inline { cursor: text; border-bottom: 1px dashed var(--fg2); }
        .edit-inline:focus { outline: none; border-bottom-color: var(--green); background: var(--bg3); border-radius: 2px; }
        .empty { color: var(--fg2); padding: 16px 0; }
        .loading { color: var(--fg2); font-style: italic; padding: 8px 0; }
        .error { color: var(--accent); padding: 8px 0; }
        .toast { position: fixed; top: 20px; right: 20px; background: var(--bg2); border: 1px solid var(--green); border-radius: 8px; padding: 12px 20px; display: none; z-index: 100; }
        .playlist-form { display: flex; gap: 8px; margin-bottom: 12px; }
        .playlist-form input { flex: 1; padding: 8px 12px; border-radius: 6px; border: 1px solid var(--border); background: var(--bg2); color: var(--fg); font-size: .85rem; }
        .q-remove { color: var(--accent); cursor: pointer; font-size: .8rem; }
        .drag-over { outline: 2px dashed var(--green); outline-offset: -2px; }
        .drag-handle { cursor: grab; color: var(--fg2); font-size: 1rem; user-select: none; }
        .drag-handle:active { cursor: grabbing; }
        @media (max-width: 600px) {
            .bar input { min-width: 100%; } .status-card { gap: 12px; } th, td { padding: 6px; font-size: .8rem; }
            .player { flex-direction: column; } .player .cover { width: 80px; height: 80px; }
            .play-btn, .q-btn, .btn-sm { font-size: .7rem; padding: 4px 8px; }
        }
    </style>
</head>
<body>
    <div class="login-container" id="login-container" style="display:none">
        <h1>Michi Micro Server</h1>
        <input type="text" id="login-username" placeholder="Username" autocomplete="username">
        <input type="password" id="login-password" placeholder="Password" autocomplete="current-password">
        <button class="btn" onclick="doLogin()">Sign In</button>
        <button class="btn btn-secondary" id="register-btn" style="display:none;margin-top:8px" onclick="doRegister()">Register</button>
        <div class="login-error" id="login-error"></div>
    </div>
    <div class="container" id="app-container" style="display:none">
        <div class="header">
            <h1>Michi Micro Server</h1>
            <span id="user-info" style="color:var(--fg2);font-size:.85rem"></span>
            <button class="theme-btn" id="theme-btn" onclick="toggleTheme()">🌙</button>
            <span id="online-indicator" style="font-size:1.2rem;cursor:default" title="Online status"></span>
            <button class="btn btn-sm btn-secondary" id="logout-btn" style="display:none" onclick="doLogout()">Logout</button>
        </div>
        <div class="status-card" id="status"></div>
        <div class="player" id="player">
            <img class="cover" id="now-cover" src="" alt="">
            <div class="info">
                <h3 id="now-title">Now Playing</h3>
                <div class="sub" id="now-sub"></div>
                <div class="meta" id="now-meta"></div>
                <audio id="audio-player" preload="auto"></audio>
                <div class="controls">
                    <span style="font-size:.75rem;color:var(--fg2)">Vol</span>
                    <input type="range" id="vol-slider" min="0" max="1" step="0.05" value="0.8">
                    <span id="vol-pct" style="font-size:.75rem;color:var(--fg2);min-width:2.5em">80%</span>
                    <label style="color:var(--fg2);font-size:.7rem;margin-left:8px">
                        <input type="checkbox" id="transcode-toggle" onchange="onTranscodeToggle()"> MP3
                    </label>
                    <span style="color:var(--fg2);font-size:.7rem;margin-left:auto">␣ play · ← → seek · N next · +/− vol</span>
                </div>
            </div>
        </div>
        <div class="bar">
            <input type="text" id="search-input" placeholder="Search tracks..." enterkeyhint="search" aria-label="Search tracks">
            <button class="btn btn-secondary" onclick="doSearch()">Search</button>
            <button class="btn btn-secondary" onclick="scanLibrary()">Scan</button>
            <button class="btn btn-danger btn-sm" onclick="clearLibrary()">Clear</button>
        </div>
        <div class="tabs">
            <button class="tab active" data-tab="tracks" onclick="switchTab('tracks')">Tracks</button>
            <button class="tab" data-tab="albums" onclick="switchTab('albums')">Albums</button>
            <button class="tab" data-tab="artists" onclick="switchTab('artists')">Artists</button>
            <button class="tab" data-tab="queue" onclick="switchTab('queue')">Queue</button>
            <button class="tab" data-tab="playlists" onclick="switchTab('playlists')">Playlists</button>
            <button class="tab" data-tab="history" onclick="switchTab('history')">History</button>
            <button class="tab" data-tab="offline" onclick="switchTab('offline')">Offline</button>
        </div>
        <div class="tab-content active" id="tab-tracks">
            <h2 id="tracks-heading">Tracks <span class="count" id="tracks-count"></span></h2>
            <div id="tracks-container"><div class="loading">Loading tracks...</div></div>
        </div>
        <div class="tab-content" id="tab-albums">
            <h2>Albums <span class="count" id="albums-count"></span></h2>
            <div id="albums-container"><div class="loading">Loading albums...</div></div>
        </div>
        <div class="tab-content" id="tab-artists">
            <h2>Artists <span class="count" id="artists-count"></span></h2>
            <div id="artists-container"><div class="loading">Loading artists...</div></div>
        </div>
        <div class="tab-content" id="tab-queue">
            <h2>Queue <span class="count" id="queue-count"></span></h2>
            <div id="queue-container"><div class="empty">Queue is empty. Use "Q" or "Next" on tracks to add.</div></div>
        </div>
        <div class="tab-content" id="tab-playlists">
            <h2>Playlists <span class="count" id="playlists-count"></span></h2>
            <div class="playlist-form">
                <input type="text" id="playlist-name-input" placeholder="New playlist name...">
                <button class="btn btn-sm btn-secondary" onclick="createPlaylist()">Create</button>
            </div>
            <details style="margin-bottom:12px">
                <summary style="cursor:pointer;color:var(--fg2);font-size:.85rem">Import M3U</summary>
                <div style="margin-top:8px;display:flex;flex-direction:column;gap:8px">
                    <input type="text" id="import-name-input" placeholder="Playlist name..." style="padding:8px 12px;border-radius:6px;border:1px solid var(--border);background:var(--bg2);color:var(--fg);font-size:.85rem">
                    <textarea id="import-content-input" rows="6" placeholder="Paste M3U content here..." style="padding:8px 12px;border-radius:6px;border:1px solid var(--border);background:var(--bg2);color:var(--fg);font-size:.85rem;font-family:monospace;resize:vertical"></textarea>
                    <button class="btn btn-sm" onclick="importM3U()">Import</button>
                </div>
            </details>
            <div id="playlists-container"><div class="loading">Loading playlists...</div></div>
        </div>
        <div class="tab-content" id="tab-history">
            <h2>Play History <span class="count" id="history-count"></span></h2>
            <div id="history-container"><div class="loading">Loading history...</div></div>
        </div>
        <div class="tab-content" id="tab-offline">
            <h2>Offline Tracks <span class="count" id="offline-count"></span></h2>
            <div id="offline-container"><div class="loading">Loading offline tracks...</div></div>
        </div>
    </div>
    <div class="toast" id="toast"></div>
    <div id="share-modal" style="display:none;position:fixed;top:0;left:0;width:100%;height:100%;background:rgba(0,0,0,0.5);z-index:200;align-items:center;justify-content:center">
        <div style="background:var(--bg2);border-radius:12px;padding:24px;max-width:420px;width:90%">
            <h3 style="color:var(--green);margin-bottom:12px">Share Playlist</h3>
            <input id="share-link-input" type="text" readonly style="width:100%;padding:8px 12px;border-radius:6px;border:1px solid var(--border);background:var(--bg);color:var(--fg);margin-bottom:8px;font-size:.85rem">
            <div style="display:flex;gap:8px;margin-bottom:8px">
                <button class="btn btn-sm" onclick="copyShareLink()">Copy Link</button>
                <button class="btn btn-sm btn-danger" id="disable-share-btn" onclick="disableShare()">Disable</button>
                <button class="btn btn-sm btn-secondary" onclick="closeShareModal()">Close</button>
            </div>
        </div>
    </div>
    <script>
        let allTracks = [];
        let currentTracks = [];
        let currentPlayIndex = -1;
        let currentPlaylists = [];
        let offlineTracks = [];
        let queue = [];
        let queueContext = null;

        let authToken = localStorage.getItem('michi-token');
        let currentUser = null;

        function authHeaders(headers) {
            const h = headers || {};
            if (authToken) h['Authorization'] = 'Bearer ' + authToken;
            return h;
        }

        async function api(url, opts) {
            const o = opts || {};
            o.headers = authHeaders(o.headers);
            const res = await fetch(url, o);
            if (res.status === 401 && authToken) {
                authToken = null;
                localStorage.removeItem('michi-token');
                await checkAuth();
                throw new Error('Session expired');
            }
            if (!res.ok) {
                const body = await res.json().catch(() => ({ message: res.statusText }));
                throw new Error(body.message || `HTTP ${res.status}`);
            }
            return res.json();
        }

        function showToast(msg, isErr) {
            const t = document.getElementById('toast');
            t.textContent = msg;
            t.style.borderColor = isErr ? 'var(--accent, #e94560)' : 'var(--green, #4ecca3)';
            t.style.display = 'block';
            setTimeout(() => { t.style.display = 'none'; }, 3000);
        }

        function setTrackCount(n) {
            document.getElementById('tracks-count').textContent = n + ' track' + (n !== 1 ? 's' : '');
        }

        function esc(s) {
            const d = document.createElement('div');
            d.textContent = String(s);
            return d.innerHTML;
        }

        function fmtDur(ms) {
            if (!ms && ms !== 0) return '\u2014';
            const total = Math.floor(ms / 1000);
            return Math.floor(total / 60) + ':' + (total % 60).toString().padStart(2, '0');
        }

        function switchTab(name) {
            document.querySelectorAll('.tab').forEach(function(t) { t.classList.remove('active'); });
            document.querySelectorAll('.tab-content').forEach(function(c) { c.classList.remove('active'); });
            document.querySelector('.tab[data-tab="' + name + '"]').classList.add('active');
            document.getElementById('tab-' + name).classList.add('active');
            if (name === 'albums') loadAlbums();
            if (name === 'artists') loadArtists();
            if (name === 'queue') renderQueue();
            if (name === 'playlists') loadPlaylists();
            if (name === 'history') loadHistory();
            if (name === 'offline') loadOfflineTracks();
        }

        function playlistOptions() {
            return currentPlaylists.map(function(p) { return '<option value="' + p.id + '">' + esc(p.name) + '</option>'; }).join('');
        }

        function renderTracks(tracks) {
            currentTracks = tracks;
            const container = document.getElementById('tracks-container');
            const heading = document.getElementById('tracks-heading');
            setTrackCount(tracks.length);
            if (tracks.length === 0) {
                container.innerHTML = '<div class="empty">No tracks found.</div>';
                return;
            }
            let html = '<table><thead><tr><th>Title</th><th>Artist</th><th>Album</th><th>Dur.</th><th></th><th></th><th></th><th></th></tr></thead><tbody>';
            for (let i = 0; i < tracks.length; i++) {
                const t = tracks[i];
                const fmt = t.format === 'unknown' ? '' : t.format;
                html += '<tr>' +
                    '<td><span class="edit-inline" data-field="title" data-id="' + t.id + '" data-val="' + esc(t.title || '') + '">' + esc(t.title || '\u2014') + '</span></td>' +
                    '<td><span class="edit-inline" data-field="artist" data-id="' + t.id + '" data-val="' + esc(t.artist || '') + '">' + esc(t.artist || '\u2014') + '</span></td>' +
                    '<td>' + esc(t.album || '\u2014') + ' <span style="color:var(--fg2);font-size:.8rem">' + esc(fmt) + '</span></td>' +
                    '<td style="color:var(--fg2)">' + fmtDur(t.duration_ms) + '</td>' +
                    '<td><button class="play-btn" data-idx="' + i + '">Play</button></td>' +
                    '<td><button class="q-btn" onclick="queueNext(\'' + t.id + '\',' + i + ')" title="Play next">Next</button> <button class="q-btn" onclick="queueAdd(\'' + t.id + '\',' + i + ')" title="Add to queue">+Q</button></td>' +
                    '<td><select class="q-btn" onchange="addTrackToPlaylist(this, \'' + t.id + '\)"><option value="">PL</option>' + playlistOptions() + '</select></td>' +
                    '<td><button class="q-btn offline-btn" data-id="' + t.id + '" onclick="toggleOffline(this)">DL</button></td></tr>';
            }
            html += '</tbody></table>';
            container.innerHTML = html;
            container.querySelectorAll('.play-btn').forEach(function(btn) {
                btn.addEventListener('click', function() { playTrack(tracks, parseInt(this.dataset.idx)); });
            });
            attachInlineEdit();
            document.querySelectorAll('.offline-btn').forEach(function(btn) {
                isOffline(btn.dataset.id).then(function(off) {
                    if (off) btn.textContent = 'RM';
                });
            });
        }

        function attachInlineEdit() {
            document.querySelectorAll('.edit-inline').forEach(function(el) {
                el.addEventListener('dblclick', function() {
                    const val = this.dataset.val;
                    this.innerHTML = '<input type="text" value="' + esc(val) + '" style="width:100%;background:var(--bg3);border:1px solid var(--green);color:var(--fg);border-radius:3px;padding:2px 4px;font:inherit">';
                    const inp = this.querySelector('input');
                    inp.focus();
                    inp.select();
                    inp.addEventListener('blur', function() { saveInlineEdit(el); });
                    inp.addEventListener('keydown', function(e) { if (e.key === 'Enter') { e.preventDefault(); inp.blur(); } if (e.key === 'Escape') { el.textContent = val || '\u2014'; } });
                });
            });
        }

        async function saveInlineEdit(el) {
            const inp = el.querySelector('input');
            if (!inp) return;
            const newVal = inp.value.trim();
            const id = el.dataset.id;
            const field = el.dataset.field;
            if (newVal === el.dataset.val) { el.textContent = newVal || '\u2014'; return; }
            try {
                const body = {};
                body[field] = newVal || null;
                await api('/api/tracks/' + id, { method: 'PUT', body: JSON.stringify(body), headers: { 'Content-Type': 'application/json' } });
                el.dataset.val = newVal;
                el.textContent = newVal || '\u2014';
                showToast('Updated ' + field);
            } catch (e) {
                showToast('Edit failed: ' + e.message, true);
                el.textContent = el.dataset.val || '\u2014';
            }
        }

        async function addTrackToPlaylist(sel, trackId) {
            const plId = sel.value;
            if (!plId) return;
            try {
                await api('/api/playlists/' + plId + '/tracks/' + trackId, { method: 'POST' });
                showToast('Added to playlist');
            } catch (e) {
                showToast(e.message, true);
            }
            sel.selectedIndex = 0;
        }

        async function removeFromPlaylist(trackId) {
            if (!queueContext || queueContext.type !== 'playlist') return;
            const plId = queueContext.id;
            try {
                await api('/api/playlists/' + plId + '/tracks/' + trackId, { method: 'DELETE' });
                showToast('Removed from playlist');
                showPlaylistTracks(plId, queueContext.name);
            } catch (e) {
                showToast(e.message, true);
            }
        }

        // Sync: push playback state to server
        async function pushPlaybackState() {
            const audio = document.getElementById('audio-player');
            const player = document.getElementById('player');
            if (!player.classList.contains('visible')) return;

            // Find track_id from current source
            let trackId = null;
            const src = audio.getAttribute('src');
            if (src) {
                const parts = src.split('/');
                trackId = parts[parts.length - 1] || null;
            }

            try {
                await fetch('/api/playback/state', {
                    method: 'POST',
                    body: JSON.stringify({
                        track_id: trackId,
                        position_ms: Math.floor(audio.currentTime * 1000),
                        playing: !audio.paused,
                        volume: audio.volume,
                    }),
                    headers: authHeaders({ 'Content-Type': 'application/json' }),
                });
            } catch (e) { /* ignore push errors */ }
        }

        // Debounced push
        let pushTimer = null;
        function schedulePushState() {
            if (pushTimer) clearTimeout(pushTimer);
            pushTimer = setTimeout(pushPlaybackState, 500);
        }

        async function recordPlay(track) {
            try {
                await fetch('/api/playback/record', {
                    method: 'POST',
                    body: JSON.stringify({
                        track_id: track.id,
                        duration_ms: track.duration_ms,
                    }),
                    headers: authHeaders({ 'Content-Type': 'application/json' }),
                });
            } catch (e) { /* ignore record errors */ }
        }

        async function loadTracks() {
            try {
                allTracks = await api('/api/tracks');
                document.getElementById('tracks-heading').textContent = 'Tracks ';
                renderTracks(allTracks);
                await loadPlaylists();
            } catch (e) {
                document.getElementById('tracks-container').innerHTML = '<div class="error">Failed: ' + esc(e.message) + '</div>';
            }
        }

        async function doSearch() {
            const q = document.getElementById('search-input').value.trim();
            const container = document.getElementById('tracks-container');
            const heading = document.getElementById('tracks-heading');
            if (!q) { heading.textContent = 'Tracks '; renderTracks(allTracks); return; }
            container.innerHTML = '<div class="loading">Searching...</div>';
            heading.textContent = 'Search: "' + esc(q) + '" ';
            queueContext = null;
            try {
                const results = await api('/api/search?q=' + encodeURIComponent(q));
                renderTracks(results);
            } catch (e) {
                container.innerHTML = '<div class="error">Search failed: ' + esc(e.message) + '</div>';
            }
        }

        document.getElementById('search-input').addEventListener('keydown', function(e) {
            if (e.key === 'Enter') doSearch().catch(function(err) { showToast(err.message, true); });
        });

        async function scanLibrary() {
            const btn = document.getElementById('scan-btn');
            btn.disabled = true; btn.textContent = 'Scanning...';
            try {
                const r = await api('/api/library/scan', { method: 'POST' });
                showToast('Scanned ' + r.scanned + ' tracks, saved ' + r.saved);
                await loadStats(); await loadTracks();
            } catch (e) { showToast('Scan failed: ' + e.message, true); }
            finally { btn.disabled = false; btn.textContent = 'Scan'; }
        }

        async function clearLibrary() {
            if (!confirm('Delete all tracks? This cannot be undone.')) return;
            const btn = document.getElementById('clear-btn');
            btn.disabled = true;
            try {
                const r = await api('/api/library/tracks', { method: 'DELETE' });
                showToast('Deleted ' + r.deleted + ' tracks');
                await loadStats(); await loadTracks(); stopPlayback();
            } catch (e) { showToast('Clear failed: ' + e.message, true); }
            finally { btn.disabled = false; }
        }

        function stopPlayback() {
            const audio = document.getElementById('audio-player');
            audio.pause(); audio.removeAttribute('src');
            document.getElementById('player').classList.remove('visible');
            document.getElementById('now-cover').src = '';
            currentPlayIndex = -1;
        }

        function playTrack(tracks, idx) {
            if (idx < 0 || idx >= tracks.length) return;
            currentPlayIndex = idx;
            currentTracks = tracks;
            const t = tracks[idx];
            const player = document.getElementById('player');
            player.classList.add('visible');
            document.getElementById('now-title').textContent = t.title || 'Unknown Track';
            document.getElementById('now-sub').textContent = (t.artist || 'Unknown Artist') + ' \u2014 ' + (t.album || 'Unknown Album');
            const fmt = t.format === 'unknown' ? '' : t.format;
            document.getElementById('now-meta').textContent = [fmt, fmtDur(t.duration_ms)].filter(Boolean).join(' | ');
            const cover = document.getElementById('now-cover');
            cover.src = t.artwork_id ? '/api/artwork/' + encodeURIComponent(t.artwork_id) : '';
            const audio = document.getElementById('audio-player');
            if (navigator.onLine === false) {
                getOfflineTrackUrl(t.id).then(function(url) {
                    audio.src = url;
                    audio.play().catch(function(e) { showToast('Playback failed: ' + e.message, true); });
                }).catch(function() {
                    showToast('Track not available offline', true);
                });
            } else {
                audio.src = '/api/stream/' + encodeURIComponent(t.id) + (document.getElementById('transcode-toggle').checked ? '?format=mp3' : '');
                audio.play().catch(function(e) { showToast('Playback failed: ' + e.message, true); });
            }
            audio.onended = function() { recordPlay(t); autoNext(); };
        }

        function autoNext() {
            if (queue.length > 0) {
                const next = queue.shift();
                renderQueue();
                playTrack(next.tracks, next.idx);
                return;
            }
            if (currentTracks.length > 1 && currentPlayIndex >= 0 && currentPlayIndex < currentTracks.length - 1) {
                playTrack(currentTracks, currentPlayIndex + 1);
            }
        }

        function queueAdd(trackId, idx) {
            const track = currentTracks.find(function(t) { return t.id === trackId; });
            if (!track) return;
            queue.push({ tracks: currentTracks, idx: idx, track: track });
            renderQueue();
            showToast('Added to queue');
        }

        function queueNext(trackId, idx) {
            const track = currentTracks.find(function(t) { return t.id === trackId; });
            if (!track) return;
            queue.unshift({ tracks: currentTracks, idx: idx, track: track });
            renderQueue();
            showToast('Will play next');
        }

        function queueRemove(i) {
            queue.splice(i, 1);
            renderQueue();
        }

        function renderQueue() {
            const container = document.getElementById('queue-container');
            document.getElementById('queue-count').textContent = queue.length + ' queued';
            if (queue.length === 0) {
                container.innerHTML = '<div class="empty">Queue is empty. Click "Q" or "Next" on any track.</div>';
                return;
            }
            let html = '<table><thead><tr><th>Title</th><th>Artist</th><th>Album</th><th>Dur.</th><th></th></tr></thead><tbody>';
            for (let i = 0; i < queue.length; i++) {
                const t = queue[i].track;
                html += '<tr><td>' + esc(t.title || '\u2014') + '</td><td>' + esc(t.artist || '\u2014') + '</td><td>' + esc(t.album || '\u2014') + '</td><td style="color:var(--fg2)">' + fmtDur(t.duration_ms) + '</td><td><span class="q-remove" onclick="queueRemove(' + i + ')">\u2716</span></td></tr>';
            }
            html += '</tbody></table>';
            container.innerHTML = html;
        }

        async function loadStatus() {
            try {
                const s = await api('/api/status');
                document.getElementById('status').innerHTML =
                    '<div class="status-item"><span class="status-label">Status</span><span class="status-value">' + esc(s.status) + '</span></div>' +
                    '<div class="status-item"><span class="status-label">Version</span><span class="status-value">' + esc(s.version) + '</span></div>' +
                    '<div class="status-item"><span class="status-label">Port</span><span class="status-value">' + esc(s.port) + '</span></div>' +
                    '<div class="status-item"><span class="status-label">Library Paths</span><span class="status-value">' + esc(s.music_paths) + '</span></div>';
            } catch (e) {
                document.getElementById('status').innerHTML = '<div class="error">Failed: ' + esc(e.message) + '</div>';
            }
        }

        async function loadStats() {
            try {
                const s = await api('/api/library/stats');
                const el = document.getElementById('status');
                if (el) el.innerHTML +=
                    '<div class="status-item"><span class="status-label">Tracks</span><span class="status-value">' + esc(s.tracks) + '</span></div>' +
                    '<div class="status-item"><span class="status-label">Albums</span><span class="status-value">' + esc(s.albums) + '</span></div>' +
                    '<div class="status-item"><span class="status-label">Artists</span><span class="status-value">' + esc(s.artists) + '</span></div>';
            } catch (e) { showToast('Stats failed: ' + e.message, true); }
        }

        async function loadAlbums() {
            const container = document.getElementById('albums-container');
            try {
                const albums = await api('/api/albums');
                document.getElementById('albums-count').textContent = albums.length + ' album' + (albums.length !== 1 ? 's' : '');
                if (albums.length === 0) { container.innerHTML = '<div class="empty">No albums found.</div>'; return; }
                let html = '<table><thead><tr><th>Album</th><th>Artist</th><th>Tr.</th><th></th></tr></thead><tbody>';
                for (const a of albums) {
                    html += '<tr><td class="clickable" onclick="showAlbumTracks(\'' + esc(a.album) + '\')">' + esc(a.album) + '</td><td>' + esc(a.album_artist || '\u2014') + '</td><td>' + a.track_count + '</td><td><button class="play-btn" onclick="playAlbum(\'' + esc(a.album) + '\')">Play</button></td></tr>';
                }
                container.innerHTML = html + '</tbody></table>';
            } catch (e) { container.innerHTML = '<div class="error">Failed: ' + esc(e.message) + '</div>'; }
        }

        async function showAlbumTracks(album) {
            switchTab('tracks');
            const container = document.getElementById('tracks-container');
            const heading = document.getElementById('tracks-heading');
            container.innerHTML = '<div class="loading">Loading...</div>';
            heading.textContent = 'Album: "' + esc(album) + '" ';
            queueContext = null;
            try {
                const tracks = await api('/api/albums/' + encodeURIComponent(album));
                renderTracks(tracks);
            } catch (e) { container.innerHTML = '<div class="error">Failed: ' + esc(e.message) + '</div>'; }
        }

        async function playAlbum(album) {
            try {
                const tracks = await api('/api/albums/' + encodeURIComponent(album));
                if (tracks.length > 0) playTrack(tracks, 0);
            } catch (e) { showToast('Failed: ' + e.message, true); }
        }

        async function loadArtists() {
            const container = document.getElementById('artists-container');
            try {
                const artists = await api('/api/artists');
                document.getElementById('artists-count').textContent = artists.length + ' artist' + (artists.length !== 1 ? 's' : '');
                if (artists.length === 0) { container.innerHTML = '<div class="empty">No artists found.</div>'; return; }
                let html = '<table><thead><tr><th>Artist</th><th>Tr.</th><th></th></tr></thead><tbody>';
                for (const a of artists) {
                    html += '<tr><td class="clickable" onclick="showArtistTracks(\'' + esc(a.artist) + '\')">' + esc(a.artist || 'Unknown') + '</td><td>' + a.track_count + '</td><td><button class="play-btn" onclick="playArtist(\'' + esc(a.artist) + '\')">Play</button></td></tr>';
                }
                container.innerHTML = html + '</tbody></table>';
            } catch (e) { container.innerHTML = '<div class="error">Failed: ' + esc(e.message) + '</div>'; }
        }

        async function showArtistTracks(artist) {
            switchTab('tracks');
            const container = document.getElementById('tracks-container');
            const heading = document.getElementById('tracks-heading');
            container.innerHTML = '<div class="loading">Loading...</div>';
            heading.textContent = 'Artist: "' + esc(artist) + '" ';
            queueContext = null;
            try {
                const tracks = await api('/api/artists/' + encodeURIComponent(artist));
                renderTracks(tracks);
            } catch (e) { container.innerHTML = '<div class="error">Failed: ' + esc(e.message) + '</div>'; }
        }

        async function playArtist(artist) {
            try {
                const tracks = await api('/api/artists/' + encodeURIComponent(artist));
                if (tracks.length > 0) playTrack(tracks, 0);
            } catch (e) { showToast('Failed: ' + e.message, true); }
        }

        async function loadHistory() {
            const container = document.getElementById('history-container');
            try {
                const entries = await api('/api/history?limit=100');
                document.getElementById('history-count').textContent = entries.length + ' play' + (entries.length !== 1 ? 's' : '');
                if (entries.length === 0) { container.innerHTML = '<div class="empty">No play history yet.</div>'; return; }
                let html = '<table><thead><tr><th>Track</th><th>Artist</th><th>Album</th><th>Played</th><th>Dur.</th></tr></thead><tbody>';
                for (const e of entries) {
                    const played = new Date(e.played_at);
                    const timeStr = played.toLocaleTimeString();
                    const dateStr = played.toLocaleDateString();
                    html += '<tr>' +
                        '<td>' + esc(e.title || '\u2014') + '</td>' +
                        '<td>' + esc(e.artist || '\u2014') + '</td>' +
                        '<td>' + esc(e.album || '\u2014') + '</td>' +
                        '<td style="color:var(--fg2);font-size:.8rem" title="' + dateStr + '">' + esc(timeStr) + '</td>' +
                        '<td style="color:var(--fg2)">' + fmtDur(e.track_duration_ms) + '</td></tr>';
                }
                container.innerHTML = html + '</tbody></table>';
            } catch (e) { container.innerHTML = '<div class="error">Failed: ' + esc(e.message) + '</div>'; }
        }

        async function loadPlaylists() {
            try {
                currentPlaylists = await api('/api/playlists');
                const container = document.getElementById('playlists-container');
                document.getElementById('playlists-count').textContent = currentPlaylists.length + ' playlist' + (currentPlaylists.length !== 1 ? 's' : '');
                if (currentPlaylists.length === 0) { container.innerHTML = '<div class="empty">No playlists yet.</div>'; return; }
                let html = '<table><thead><tr><th>Name</th><th>Tr.</th><th></th><th></th><th></th></tr></thead><tbody>';
                for (const p of currentPlaylists) {
                    html += '<tr><td class="clickable" onclick="showPlaylistTracks(\'' + p.id + '\', \'' + esc(p.name) + '\')">' + esc(p.name) + '</td><td>' + p.track_count + '</td><td><button class="play-btn" onclick="playPlaylist(\'' + p.id + '\')">Play</button></td><td><button class="btn btn-sm btn-secondary" onclick="exportPlaylist(\'' + p.id + '\', \'' + esc(p.name) + '\')">Export</button></td><td><button class="btn btn-sm btn-secondary" style="background:var(--green);color:var(--bg)" onclick="showShareModal(\'' + p.id + '\')">Share</button></td><td><button class="btn btn-sm btn-danger" onclick="deletePlaylist(\'' + p.id + '\')">Del</button></td></tr>';
                }
                container.innerHTML = html + '</tbody></table>';
            } catch (e) { document.getElementById('playlists-container').innerHTML = '<div class="error">Failed: ' + esc(e.message) + '</div>'; }
        }

        async function createPlaylist() {
            const input = document.getElementById('playlist-name-input');
            const name = input.value.trim();
            if (!name) { showToast('Name required', true); return; }
            try {
                await api('/api/playlists', { method: 'POST', body: JSON.stringify({ name: name }), headers: { 'Content-Type': 'application/json' } });
                input.value = ''; showToast('Playlist created'); await loadPlaylists();
            } catch (e) { showToast(e.message, true); }
        }

        async function deletePlaylist(id) {
            if (!confirm('Delete this playlist?')) return;
            try {
                await api('/api/playlists/' + id, { method: 'DELETE' });
                showToast('Playlist deleted'); await loadPlaylists();
            } catch (e) { showToast(e.message, true); }
        }

        async function exportPlaylist(id, name) {
            try {
                const res = await fetch('/api/playlists/' + id + '/export', { headers: authHeaders() });
                if (!res.ok) throw new Error('Export failed');
                const text = await res.text();
                const blob = new Blob([text], { type: 'audio/x-mpegurl' });
                const url = URL.createObjectURL(blob);
                const a = document.createElement('a');
                a.href = url;
                a.download = (name || 'playlist') + '.m3u';
                document.body.appendChild(a);
                a.click();
                document.body.removeChild(a);
                URL.revokeObjectURL(url);
                showToast('Exported ' + name);
            } catch (e) { showToast(e.message, true); }
        }

        async function importM3U() {
            const name = document.getElementById('import-name-input').value.trim();
            const content = document.getElementById('import-content-input').value.trim();
            if (!name) { showToast('Playlist name required', true); return; }
            if (!content) { showToast('M3U content required', true); return; }
            try {
                const r = await api('/api/playlists/import', { method: 'POST', body: JSON.stringify({ name: name, content: content }), headers: { 'Content-Type': 'application/json' } });
                showToast('Imported ' + r.matched + '/' + r.total + ' tracks');
                document.getElementById('import-name-input').value = '';
                document.getElementById('import-content-input').value = '';
                await loadPlaylists();
            } catch (e) { showToast(e.message, true); }
        }

        async function showPlaylistTracks(id, name) {
            switchTab('tracks');
            const container = document.getElementById('tracks-container');
            const heading = document.getElementById('tracks-heading');
            container.innerHTML = '<div class="loading">Loading...</div>';
            heading.textContent = 'Playlist: "' + esc(name) + '" ';
            queueContext = { type: 'playlist', id: id, name: name };
            try {
                const tracks = await api('/api/playlists/' + id + '/tracks');
                currentTracks = tracks;
                setTrackCount(tracks.length);
                if (tracks.length === 0) { container.innerHTML = '<div class="empty">No tracks in playlist.</div>'; return; }
                let html = '<table><thead><tr><th></th><th>Title</th><th>Artist</th><th>Album</th><th>Dur.</th><th></th><th></th></tr></thead><tbody>';
                for (let i = 0; i < tracks.length; i++) {
                    const t = tracks[i];
                    html += '<tr draggable="true" ondragstart="dragStart(event,' + i + ')" ondragend="dragEnd(event)" ondragover="dragOver(event)" ondragleave="dragLeave(event)" ondrop="dropTrack(event,' + i + ')">' +
                        '<td class="drag-handle">&#9776;</td>' +
                        '<td>' + esc(t.title || '\u2014') + '</td><td>' + esc(t.artist || '\u2014') + '</td><td>' + esc(t.album || '\u2014') + '</td><td style="color:var(--fg2)">' + fmtDur(t.duration_ms) + '</td>' +
                        '<td><button class="play-btn" onclick="playTrack(currentTracks,' + i + ')">Play</button></td>' +
                        '<td><button class="q-btn" onclick="removeFromPlaylist(\'' + t.id + '\')" style="color:var(--accent)">Remove</button></td></tr>';
                }
                container.innerHTML = html + '</tbody></table>';
            } catch (e) { container.innerHTML = '<div class="error">Failed: ' + esc(e.message) + '</div>'; }
        }

        // Drag & drop for playlist track reordering
        let dragIndex = -1;
        function dragStart(e, idx) {
            dragIndex = idx;
            e.dataTransfer.effectAllowed = 'move';
            e.dataTransfer.setData('text/plain', String(idx));
        }
        function dragEnd(e) {
            e.currentTarget.classList.remove('drag-over');
            document.querySelectorAll('.drag-over').forEach(function(el) { el.classList.remove('drag-over'); });
        }
        function dragOver(e) {
            e.preventDefault();
            e.dataTransfer.dropEffect = 'move';
            e.currentTarget.classList.add('drag-over');
        }
        function dragLeave(e) {
            if (!e.currentTarget.contains(e.relatedTarget)) {
                e.currentTarget.classList.remove('drag-over');
            }
        }
        function dropTrack(e, idx) {
            e.preventDefault();
            e.currentTarget.classList.remove('drag-over');
            if (dragIndex === idx || dragIndex < 0) return;
            const plId = queueContext ? queueContext.id : null;
            const plName = queueContext ? queueContext.name : '';
            if (!plId) return;
            const tracks = currentTracks.slice();
            const moved = tracks.splice(dragIndex, 1)[0];
            tracks.splice(idx, 0, moved);
            api('/api/playlists/' + plId + '/reorder', {
                method: 'PUT',
                body: JSON.stringify({ track_ids: tracks.map(function(t) { return t.id; }) }),
                headers: { 'Content-Type': 'application/json' }
            }).then(function() {
                showPlaylistTracks(plId, plName);
            }).catch(function(e) {
                showToast('Reorder failed: ' + e.message, true);
                showPlaylistTracks(plId, plName);
            });
        }

        async function playPlaylist(id) {
            try {
                const tracks = await api('/api/playlists/' + id + '/tracks');
                if (tracks.length > 0) { currentTracks = tracks; playTrack(tracks, 0); }
            } catch (e) { showToast('Failed: ' + e.message, true); }
        }

        // Volume
        function initVolume() {
            const slider = document.getElementById('vol-slider');
            const pct = document.getElementById('vol-pct');
            const audio = document.getElementById('audio-player');
            const saved = localStorage.getItem('michi-volume');
            if (saved) { slider.value = saved; audio.volume = parseFloat(saved); pct.textContent = Math.round(parseFloat(saved) * 100) + '%'; }
            slider.addEventListener('input', function() {
                const v = parseFloat(this.value);
                audio.volume = v;
                pct.textContent = Math.round(v * 100) + '%';
                localStorage.setItem('michi-volume', v);
            });
        }

        // Theme
        function initTheme() {
            const saved = localStorage.getItem('michi-theme');
            const btn = document.getElementById('theme-btn');
            if (saved === 'light') { document.body.classList.add('light'); btn.textContent = '\u2600\uFE0F'; }
        }

        function toggleTheme() {
            const btn = document.getElementById('theme-btn');
            document.body.classList.toggle('light');
            const isLight = document.body.classList.contains('light');
            localStorage.setItem('michi-theme', isLight ? 'light' : 'dark');
            btn.textContent = isLight ? '\u2600\uFE0F' : '\U0001F319';
        }

        // Keyboard shortcuts
        document.addEventListener('keydown', function(e) {
            const tag = e.target.tagName;
            if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT') return;
            const audio = document.getElementById('audio-player');
            const player = document.getElementById('player');
            if (!player.classList.contains('visible')) return;
            switch (e.key) {
                case ' ':
                    e.preventDefault();
                    if (audio.paused) audio.play().catch(function(){}); else audio.pause();
                    break;
                case 'ArrowLeft':
                    e.preventDefault();
                    if (audio.currentTime > 5) audio.currentTime -= 5; else audio.currentTime = 0;
                    break;
                case 'ArrowRight':
                    e.preventDefault();
                    audio.currentTime = Math.min(audio.currentTime + 5, audio.duration || 0);
                    break;
                case 'n': case 'N':
                    e.preventDefault(); autoNext(); break;
                case 'p': case 'P':
                    e.preventDefault();
                    if (currentPlayIndex > 0) playTrack(currentTracks, currentPlayIndex - 1);
                    break;
                case '+': case '=':
                    e.preventDefault(); adjustVol(0.05); break;
                case '-': case '_':
                    e.preventDefault(); adjustVol(-0.05); break;
                case 'm': case 'M':
                    e.preventDefault();
                    const slider = document.getElementById('vol-slider');
                    if (audio.volume > 0) { slider.dataset.prev = audio.volume; audio.volume = 0; slider.value = 0; }
                    else { const prev = parseFloat(slider.dataset.prev) || 0.8; audio.volume = prev; slider.value = prev; }
                    document.getElementById('vol-pct').textContent = Math.round(audio.volume * 100) + '%';
                    localStorage.setItem('michi-volume', audio.volume);
                    break;
            }
        });

        function adjustVol(delta) {
            const audio = document.getElementById('audio-player');
            const slider = document.getElementById('vol-slider');
            const v = Math.max(0, Math.min(1, audio.volume + delta));
            audio.volume = v; slider.value = v;
            document.getElementById('vol-pct').textContent = Math.round(v * 100) + '%';
            localStorage.setItem('michi-volume', v);
        }

        function onTranscodeToggle() {
            const player = document.getElementById('player');
            if (player.classList.contains('visible') && currentPlayIndex >= 0) {
                playTrack(currentTracks, currentPlayIndex);
            }
        }

        // WebSocket
        function connectWs() {
            const proto = location.protocol === 'https:' ? 'wss:' : 'ws:';
            const ws = new WebSocket(proto + '//' + location.host + '/api/ws');
            ws.onmessage = function(ev) {
                try {
                    const msg = JSON.parse(ev.data);
                    if (msg.type === 'scan_start') {
                        showToast('Scan started...');
                    } else if (msg.type === 'scan_done') {
                        showToast('Scanned ' + msg.scanned + ' tracks, saved ' + msg.saved);
                        loadStats();
                        loadTracks();
                    } else if (msg.type === 'library_updated') {
                        loadStats();
                        loadTracks();
                    } else if (msg.type === 'playlist_updated') {
                        loadPlaylists();
                    } else if (msg.type === 'sync_state') {
                        // Another room changed playback state
                        const audio = document.getElementById('audio-player');
                        const fmtSuffix = document.getElementById('transcode-toggle').checked ? '?format=mp3' : '';
                        if (msg.playing && msg.track_id && audio.getAttribute('src') !== '/api/stream/' + msg.track_id + fmtSuffix) {
                            // Remote track change - try to play it
                            const newSrc = '/api/stream/' + msg.track_id + fmtSuffix;
                            audio.src = newSrc;
                            audio.currentTime = (msg.position_ms || 0) / 1000;
                            audio.volume = msg.volume != null ? msg.volume : audio.volume;
                            if (msg.playing) audio.play().catch(function(){});
                            // Update display
                            fetch('/api/tracks/' + msg.track_id, { headers: authHeaders() }).then(function(r) { return r.json(); }).then(function(t) {
                                if (t && t.title) {
                                    document.getElementById('player').classList.add('visible');
                                    document.getElementById('now-title').textContent = t.title || 'Unknown';
                                    document.getElementById('now-sub').textContent = (t.artist || '') + ' \u2014 ' + (t.album || '');
                                    document.getElementById('now-cover').src = t.artwork_id ? '/api/artwork/' + t.artwork_id : '';
                                }
                            }).catch(function(){});
                        } else if (msg.playing != null) {
                            if (msg.playing && audio.paused) audio.play().catch(function(){});
                            else if (!msg.playing && !audio.paused) audio.pause();
                            if (msg.position_ms != null) audio.currentTime = msg.position_ms / 1000;
                            if (msg.volume != null) audio.volume = msg.volume;
                        }
                    }
                } catch (e) { /* ignore parse errors */ }
            };
            ws.onclose = function() {
                setTimeout(connectWs, 3000);
            };
            ws.onerror = function() {
                ws.close();
            };
        }

        function updateUserDisplay() {
            const infoEl = document.getElementById('user-info');
            const registerBtn = document.getElementById('register-btn');
            if (currentUser) {
                infoEl.textContent = currentUser.username + (currentUser.is_admin ? ' (admin)' : '');
            } else {
                infoEl.textContent = '';
            }
            if (registerBtn) {
                registerBtn.style.display = currentUser ? 'none' : '';
            }
        }

        // Auth
        async function checkAuth() {
            try {
                const res = await fetch('/api/auth/check');
                const status = await res.json();
                const loginEl = document.getElementById('login-container');
                const appEl = document.getElementById('app-container');
                const logoutBtn = document.getElementById('logout-btn');
                if (status.enabled && !status.authenticated) {
                    loginEl.style.display = 'block';
                    appEl.style.display = 'none';
                    currentUser = null;
                    document.getElementById('register-btn').style.display = '';
                    updateUserDisplay();
                } else {
                    loginEl.style.display = 'none';
                    appEl.style.display = 'block';
                    if (status.enabled) {
                        logoutBtn.style.display = 'inline-block';
                        if (status.username) {
                            currentUser = { username: status.username, is_admin: status.is_admin };
                        }
                    }
                    updateUserDisplay();
                    initVolume();
                    initTheme();
                    connectWs();
                    loadStatus();
                    loadStats();
                    loadTracks();
                }
            } catch (e) {
                document.getElementById('login-container').style.display = 'block';
                document.getElementById('app-container').style.display = 'none';
                currentUser = null;
                updateUserDisplay();
            }
        }

        async function doLogin() {
            const username = document.getElementById('login-username').value.trim();
            const password = document.getElementById('login-password').value;
            const errorEl = document.getElementById('login-error');
            errorEl.textContent = '';
            if (!username || !password) { errorEl.textContent = 'Fill in both fields'; return; }
            try {
                const res = await fetch('/api/auth/login', {
                    method: 'POST',
                    body: JSON.stringify({ username, password }),
                    headers: { 'Content-Type': 'application/json' },
                });
                if (!res.ok) {
                    const err = await res.json().catch(() => ({ message: 'Login failed' }));
                    throw new Error(err.message || 'Login failed');
                }
                const data = await res.json();
                authToken = data.token;
                localStorage.setItem('michi-token', data.token);
                currentUser = { username: data.username, is_admin: data.is_admin };
                await checkAuth();
            } catch (e) {
                errorEl.textContent = e.message;
            }
        }

        async function doRegister() {
            const username = document.getElementById('login-username').value.trim();
            const password = document.getElementById('login-password').value;
            const errorEl = document.getElementById('login-error');
            if (!username || !password) { errorEl.textContent = 'Fill in both fields'; return; }
            try {
                const res = await fetch('/api/auth/register', {
                    method: 'POST',
                    body: JSON.stringify({ username, password }),
                    headers: { 'Content-Type': 'application/json' },
                });
                if (!res.ok) {
                    const err = await res.json().catch(() => ({ message: 'Registration failed' }));
                    throw new Error(err.message || 'Registration failed');
                }
                const data = await res.json();
                authToken = data.token;
                localStorage.setItem('michi-token', data.token);
                currentUser = { username: data.username, is_admin: data.is_admin };
                await checkAuth();
            } catch (e) {
                errorEl.textContent = e.message;
            }
        }

        async function doLogout() {
            try {
                await fetch('/api/auth/logout', { method: 'POST', headers: authHeaders() });
            } catch (e) { /* ignore */ }
            authToken = null;
            currentUser = null;
            localStorage.removeItem('michi-token');
            await checkAuth();
        }

        // Allow pressing Enter in login fields
        document.getElementById('login-username').addEventListener('keydown', function(e) {
            if (e.key === 'Enter') document.getElementById('login-password').focus();
        });
        document.getElementById('login-password').addEventListener('keydown', function(e) {
            if (e.key === 'Enter') doLogin();
        });

        // Offline storage

        function openOfflineDB() {
            return new Promise(function(resolve, reject) {
                var req = indexedDB.open('michi-offline', 1);
                req.onupgradeneeded = function(e) {
                    var db = e.target.result;
                    db.createObjectStore('tracks', { keyPath: 'id' });
                };
                req.onsuccess = function(e) { resolve(e.target.result); };
                req.onerror = function(e) { reject(e.target.error); };
            });
        }

        async function downloadTrack(trackId) {
            var headers = authHeaders();
            var url = '/api/stream/' + encodeURIComponent(trackId) + (document.getElementById('transcode-toggle').checked ? '?format=mp3' : '');
            var res = await fetch(url, { headers: headers });
            if (!res.ok) throw new Error('Download failed');
            var buffer = await res.arrayBuffer();
            var track = (currentTracks || []).find(function(t) { return t.id === trackId; }) || allTracks.find(function(t) { return t.id === trackId; });
            var db = await openOfflineDB();
            var tx = db.transaction('tracks', 'readwrite');
            var store = tx.objectStore('tracks');
            store.put({
                id: trackId,
                audio: buffer,
                title: track ? track.title : null,
                artist: track ? track.artist : null,
                album: track ? track.album : null,
                artwork_id: track ? track.artwork_id : null,
                duration_ms: track ? track.duration_ms : null,
            });
            return new Promise(function(resolve, reject) {
                tx.oncomplete = function() { resolve(); };
                tx.onerror = function(e) { reject(e.target.error); };
            });
        }

        function removeOfflineTrack(trackId) {
            return openOfflineDB().then(function(db) {
                var tx = db.transaction('tracks', 'readwrite');
                var store = tx.objectStore('tracks');
                store.delete(trackId);
                return new Promise(function(resolve, reject) {
                    tx.oncomplete = function() { resolve(); };
                    tx.onerror = function(e) { reject(e.target.error); };
                });
            });
        }

        function isOffline(trackId) {
            return openOfflineDB().then(function(db) {
                return new Promise(function(resolve) {
                    var tx = db.transaction('tracks', 'readonly');
                    var store = tx.objectStore('tracks');
                    var req = store.get(trackId);
                    req.onsuccess = function() { resolve(!!req.result); };
                    req.onerror = function() { resolve(false); };
                });
            });
        }

        function getOfflineTrackUrl(trackId) {
            return openOfflineDB().then(function(db) {
                return new Promise(function(resolve, reject) {
                    var tx = db.transaction('tracks', 'readonly');
                    var store = tx.objectStore('tracks');
                    var req = store.get(trackId);
                    req.onsuccess = function() {
                        if (req.result) {
                            var blob = new Blob([req.result.audio]);
                            var url = URL.createObjectURL(blob);
                            resolve(url);
                        } else {
                            reject(new Error('Track not found offline'));
                        }
                    };
                    req.onerror = function(e) { reject(e.target.error); };
                });
            });
        }

        function getAllOfflineTracks() {
            return openOfflineDB().then(function(db) {
                return new Promise(function(resolve, reject) {
                    var tx = db.transaction('tracks', 'readonly');
                    var store = tx.objectStore('tracks');
                    var req = store.getAll();
                    req.onsuccess = function() {
                        var tracks = (req.result || []).map(function(r) {
                            return { id: r.id, title: r.title, artist: r.artist, album: r.album, artwork_id: r.artwork_id, duration_ms: r.duration_ms };
                        });
                        resolve(tracks);
                    };
                    req.onerror = function(e) { reject(e.target.error); };
                });
            });
        }

        async function toggleOffline(btn) {
            var trackId = btn.dataset.id;
            var off = await isOffline(trackId);
            if (off) {
                await removeOfflineTrack(trackId);
                btn.textContent = 'DL';
                showToast('Removed offline track');
            } else {
                btn.textContent = '...';
                try {
                    await downloadTrack(trackId);
                    btn.textContent = 'RM';
                    showToast('Track saved offline');
                } catch (e) {
                    btn.textContent = 'DL';
                    showToast('Offline save failed: ' + e.message, true);
                }
            }
        }

        async function loadOfflineTracks() {
            var container = document.getElementById('offline-container');
            try {
                offlineTracks = await getAllOfflineTracks();
                document.getElementById('offline-count').textContent = offlineTracks.length + ' track' + (offlineTracks.length !== 1 ? 's' : '');
                if (offlineTracks.length === 0) {
                    container.innerHTML = '<div class="empty">No offline tracks. Download tracks from the Tracks tab.</div>';
                    return;
                }
                var html = '<table><thead><tr><th>Title</th><th>Artist</th><th>Album</th><th>Dur.</th><th></th><th></th></tr></thead><tbody>';
                for (var i = 0; i < offlineTracks.length; i++) {
                    var t = offlineTracks[i];
                    html += '<tr>' +
                        '<td>' + esc(t.title || '\u2014') + '</td>' +
                        '<td>' + esc(t.artist || '\u2014') + '</td>' +
                        '<td>' + esc(t.album || '\u2014') + '</td>' +
                        '<td style="color:var(--fg2)">' + fmtDur(t.duration_ms) + '</td>' +
                        '<td><button class="play-btn" onclick="playOfflineTrack(\'' + t.id + '\')">Play</button></td>' +
                        '<td><button class="btn btn-sm btn-danger" onclick="removeOfflineTrackConfirm(\'' + t.id + '\')">Delete</button></td></tr>';
                }
                container.innerHTML = html + '</tbody></table>';
            } catch (e) {
                container.innerHTML = '<div class="error">Failed: ' + esc(e.message) + '</div>';
            }
        }

        function playOfflineTrack(trackId) {
            var idx = offlineTracks.findIndex(function(ot) { return ot.id === trackId; });
            if (idx < 0) { showToast('Track not found', true); return; }
            var fakeTracks = offlineTracks.map(function(ot) {
                return { id: ot.id, title: ot.title, artist: ot.artist, album: ot.album, duration_ms: ot.duration_ms, format: '', artwork_id: ot.artwork_id };
            });
            playTrack(fakeTracks, idx);
        }

        function removeOfflineTrackConfirm(trackId) {
            if (!confirm('Remove this track from offline storage?')) return;
            removeOfflineTrack(trackId).then(function() {
                showToast('Removed offline track');
                loadOfflineTracks();
                document.querySelectorAll('.offline-btn').forEach(function(btn) {
                    if (btn.dataset.id === trackId) btn.textContent = 'DL';
                });
            }).catch(function(e) {
                showToast('Failed to remove: ' + e.message, true);
            });
        }

        function updateOnlineStatus() {
            var el = document.getElementById('online-indicator');
            if (el) el.textContent = navigator.onLine ? '\uD83D\uDCF6' : '\uD83D\uDCF4';
        }

        let currentSharePlaylistId = null;

        function showShareModal(playlistId) {
            currentSharePlaylistId = playlistId;
            api('/api/playlists/' + playlistId + '/share').then(function(info) {
                if (info.share_code) {
                    var url = window.location.origin + info.share_url;
                    document.getElementById('share-link-input').value = url;
                    document.getElementById('disable-share-btn').style.display = 'inline-block';
                } else {
                    document.getElementById('share-link-input').value = 'Not shared yet. Click Share to generate a link.';
                    document.getElementById('disable-share-btn').style.display = 'none';
                }
                document.getElementById('share-modal').style.display = 'flex';
            }).catch(function(e) {
                showToast(e.message, true);
            });
        }

        function closeShareModal() {
            document.getElementById('share-modal').style.display = 'none';
            currentSharePlaylistId = null;
        }

        function copyShareLink() {
            var inp = document.getElementById('share-link-input');
            inp.select();
            document.execCommand('copy');
            showToast('Link copied!');
        }

        async function disableShare() {
            if (!currentSharePlaylistId) return;
            try {
                await api('/api/playlists/' + currentSharePlaylistId + '/share', { method: 'DELETE' });
                document.getElementById('share-link-input').value = 'Sharing disabled.';
                document.getElementById('disable-share-btn').style.display = 'none';
                await loadPlaylists();
                showToast('Sharing disabled');
            } catch (e) {
                showToast(e.message, true);
            }
        }

        // Enable sharing when modal opens and no share exists
        document.getElementById('share-modal').addEventListener('click', function(e) {
            if (e.target === this) closeShareModal();
        });

        async function enableShare() {
            if (!currentSharePlaylistId) return;
            try {
                var info = await api('/api/playlists/' + currentSharePlaylistId + '/share', { method: 'POST' });
                var url = window.location.origin + info.share_url;
                document.getElementById('share-link-input').value = url;
                document.getElementById('disable-share-btn').style.display = 'inline-block';
                await loadPlaylists();
                showToast('Sharing enabled!');
            } catch (e) {
                showToast(e.message, true);
            }
        }

        // Auto-enable share if not shared yet
        var origShowShareModal = showShareModal;
        showShareModal = function(id) {
            api('/api/playlists/' + id + '/share').then(function(info) {
                if (!info.share_code) {
                    api('/api/playlists/' + id + '/share', { method: 'POST' }).then(function(newInfo) {
                        currentSharePlaylistId = id;
                        var url = window.location.origin + newInfo.share_url;
                        document.getElementById('share-link-input').value = url;
                        document.getElementById('disable-share-btn').style.display = 'inline-block';
                        document.getElementById('share-modal').style.display = 'flex';
                        loadPlaylists();
                    }).catch(function(e) { showToast(e.message, true); });
                } else {
                    origShowShareModal(id);
                }
            }).catch(function(e) { showToast(e.message, true); });
        };

        // Hook audio events to push sync state
        const audioEl = document.getElementById('audio-player');
        audioEl.addEventListener('play', schedulePushState);
        audioEl.addEventListener('pause', schedulePushState);
        audioEl.addEventListener('seeked', schedulePushState);

        checkAuth();

        // Service worker registration
        if ('serviceWorker' in navigator) {
            navigator.serviceWorker.register('/sw.js').catch(function(err) {
                console.warn('SW registration failed:', err);
            });
        }

        // Online status
        updateOnlineStatus();
        window.addEventListener('online', updateOnlineStatus);
        window.addEventListener('offline', updateOnlineStatus);
    </script>
</body>
</html>"#;
