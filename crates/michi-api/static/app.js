/* Michi Control UI — main application */

const MichiAPI = {
  base: '',

  async request(path, opts = {}) {
    const timeout = opts.timeout || 8000;
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), timeout);
    try {
      const res = await fetch(this.base + path, { ...opts, signal: controller.signal });
      clearTimeout(timer);
      if (!res.ok) {
        const body = await res.json().catch(() => ({}));
        const msg = body?.error?.message || body?.message || `HTTP ${res.status}`;
        throw new Error(msg);
      }
      return res.headers.get('content-type')?.includes('json') ? res.json() : res;
    } catch (e) {
      clearTimeout(timer);
      if (e.name === 'AbortError') throw new Error('Connection timeout');
      throw e;
    }
  },

  status() { return this.request('/api/status'); },
  serverInfo() { return this.request('/api/v1/server/info'); },
  libraryStats() { return this.request('/api/v1/library/stats'); },
  tracks(opts = {}) {
    let url = '/api/v1/tracks';
    const params = [];
    if (opts.limit) params.push(`limit=${opts.limit}`);
    if (opts.offset) params.push(`offset=${opts.offset}`);
    if (params.length) url += '?' + params.join('&');
    return this.request(url);
  },
  search(q) { return this.request(`/api/v1/search?q=${encodeURIComponent(q)}`); },
  scan() { return this.request('/api/library/scan', { method: 'POST' }); },
  playlists() { return this.request('/api/v1/playlists'); },
  streamUrl(id) { return this.base + `/api/v1/stream/${id}`; },
};

const State = {
  status: null,
  serverInfo: null,
  stats: null,
  tracks: [],
  allTracks: [],
  currentTrack: null,
  queue: [],
  polling: null,
};

function $(sel, ctx) { return (ctx || document).querySelector(sel); }
function $$(sel, ctx) { return (ctx || document).querySelectorAll(sel); }
function esc(s) { const d = document.createElement('div'); d.textContent = String(s ?? ''); return d.innerHTML; }
function fmtDur(ms) { if (!ms) return '--:--'; const t = Math.floor(ms / 1000); return Math.floor(t / 60) + ':' + String(t % 60).padStart(2, '0'); }
function fmtDate(iso) { if (!iso) return ''; const d = new Date(iso); return d.toLocaleDateString() + ' ' + d.toLocaleTimeString(); }

/* Toast */
function showToast(msg, isErr) {
  const el = $('#toast');
  if (!el) return;
  el.textContent = msg;
  el.style.borderColor = isErr ? '#EF4444' : '#22C55E';
  el.style.display = 'block';
  setTimeout(() => { el.style.display = 'none'; }, 3000);
}

/* Navigation */
function showSection(section) {
  $$('.nav-item').forEach(n => n.classList.remove('active'));
  const nav = $(`.nav-item[data-section="${section}"]`);
  if (nav) nav.classList.add('active');
  $$('.section-page').forEach(p => p.classList.add('hidden'));
  const page = $(`#page-${section}`);
  if (page) page.classList.remove('hidden');
}

/* Loading skeleton */
function skeleton(count = 3) {
  return Array.from({ length: count }, () => '<div class="skeleton" style="height:32px;margin-bottom:6px">&nbsp;</div>').join('');
}

/* Init */
async function init() {
  showSection('dashboard');
  await Promise.all([loadStatus(), loadServerInfo(), loadStats(), loadTracks()]);
  // Poll every 30s
  State.polling = setInterval(() => { loadStatus(); loadStats(); }, 30000);
}

/* Status */
async function loadStatus() {
  try {
    State.status = await MichiAPI.status();
    renderStatus();
  } catch (e) { console.warn('status failed:', e.message); }
}

function renderStatus() {
  const s = State.status;
  if (!s) return;
  // Update sidebar footer
  const dot = $('#server-status-dot');
  const lbl = $('#server-status-label');
  if (dot && lbl) {
    dot.className = 'server-status-dot ' + (s.status === 'ok' ? 'online' : 'offline');
    lbl.textContent = s.status === 'ok' ? 'Online' : 'Offline';
  }
  // Update topbar pill
  const pill = $('#status-pill');
  if (pill) {
    pill.className = 'status-pill ' + (s.status === 'ok' ? 'online' : 'offline');
    pill.innerHTML = `<span class="server-status-dot ${s.status === 'ok' ? 'online' : 'offline'}"></span>${s.status === 'ok' ? 'Online' : 'Offline'}`;
  }
  // Update sidebar info
  const sid = $('#sidebar-server-id');
  if (sid && s.server_id) sid.textContent = s.server_id.slice(0, 8) + '..';
  const suptime = $('#sidebar-uptime');
  if (suptime && s.uptime_seconds) {
    const h = Math.floor(s.uptime_seconds / 3600);
    const m = Math.floor((s.uptime_seconds % 3600) / 60);
    suptime.textContent = `${h}h ${m}m`;
  }
}

/* Server Info */
async function loadServerInfo() {
  try {
    State.serverInfo = await MichiAPI.serverInfo();
    renderServerInfo();
  } catch (e) { console.warn('server info failed:', e.message); }
}

function renderServerInfo() {
  const info = State.serverInfo;
  if (!info) return;
  // Sidebar version
  const ver = $('#sidebar-ver');
  if (ver) ver.textContent = info.version;
  // Server URL info
  const sid = $('#server-info-id');
  if (sid) sid.textContent = info.server_id;
  // Features
  Object.keys(info.features || {}).forEach(key => {
    const el = $(`#feature-${key}`);
    if (el) {
      el.innerHTML = `<span class="feature-dot ${info.features[key] ? 'on' : 'off'}"></span>${key}`;
    }
  });
}

/* Stats */
async function loadStats() {
  try {
    State.stats = await MichiAPI.libraryStats();
    renderStats();
  } catch (e) { console.warn('stats failed:', e.message); }
}

function renderStats() {
  const s = State.stats;
  if (!s) return;
  const map = { 'card-tracks': 'tracks', 'card-albums': 'albums', 'card-artists': 'artists' };
  Object.keys(map).forEach(id => {
    const el = $(`#${id}`);
    if (el) el.textContent = s[map[id]] ?? '--';
  });
  // Dashboard cards
  const cd = $('#dashboard-cards');
  if (cd) {
    cd.innerHTML = `
      <div class="card"><div class="card-icon">🎵</div><div class="card-value" id="card-tracks">${s.tracks ?? '?'}</div><div class="card-label">Tracks</div></div>
      <div class="card"><div class="card-icon">💿</div><div class="card-value" id="card-albums">${s.albums ?? '?'}</div><div class="card-label">Albums</div></div>
      <div class="card"><div class="card-icon">👤</div><div class="card-value" id="card-artists">${s.artists ?? '?'}</div><div class="card-label">Artists</div></div>
      <div class="card"><div class="card-icon">📋</div><div class="card-value" id="card-playlists">${State.serverInfo?.features?.playlists ? '--' : '--'}</div><div class="card-label">Playlists</div></div>
    `;
  }
}

/* Tracks */
async function loadTracks() {
  try {
    State.tracks = await MichiAPI.tracks();
    State.allTracks = State.tracks;
    renderTracks(State.tracks);
  } catch (e) { console.warn('tracks failed:', e.message); }
}

function renderTracks(tracks) {
  const container = $('#tracks-table');
  if (!container) return;
  if (!tracks || tracks.length === 0) {
    container.innerHTML = '<div class="empty-state"><div class="icon">🎵</div><p>Library empty — scan your music</p></div>';
    return;
  }
  let html = '<table><thead><tr><th>#</th><th>Title</th><th>Artist</th><th>Album</th><th>Format</th><th>Duration</th><th></th></tr></thead><tbody>';
  tracks.slice(0, 50).forEach((t, i) => {
    html += `<tr class="clickable" onclick="playTrack(${i})">
      <td style="color:var(--text-dim)">${i + 1}</td>
      <td class="track-title">${esc(t.title || 'Unknown')}</td>
      <td class="track-artist">${esc(t.artist || '—')}</td>
      <td class="track-artist">${esc(t.album || '—')}</td>
      <td><span class="badge format">${esc(t.format || '?')}</span></td>
      <td style="color:var(--text-dim)">${fmtDur(t.duration_ms)}</td>
      <td><button class="btn btn-sm btn-ghost" onclick="event.stopPropagation();playTrack(${i})">Play</button></td>
    </tr>`;
  });
  html += '</tbody></table>';
  container.innerHTML = html;
}

/* Search */
async function handleSearch() {
  const q = $('#search-input')?.value.trim();
  if (!q) { renderTracks(State.allTracks); return; }
  try {
    const results = await MichiAPI.search(q);
    renderTracks(results);
    showToast(`Found ${results.length} results`);
  } catch (e) { showToast(e.message, true); }
}

/* Scan */
async function handleScan() {
  const btn = $('#scan-btn');
  if (btn) { btn.disabled = true; btn.textContent = 'Scanning...'; }
  try {
    const r = await MichiAPI.scan();
    showToast(`Scanned ${r.scanned} tracks, saved ${r.saved}`);
    await Promise.all([loadStats(), loadTracks()]);
  } catch (e) { showToast(e.message, true); }
  finally { if (btn) { btn.disabled = false; btn.textContent = 'Scan'; } }
}

/* Playback */
function playTrack(idx) {
  const tracks = State.tracks;
  if (!tracks || idx < 0 || idx >= tracks.length) return;
  const t = tracks[idx];
  State.currentTrack = t;
  updateNowPlaying(t);
  updateMiniPlayer(t);
}

function updateNowPlaying(t) {
  if (!t) return;
  const cover = $('#np-cover');
  if (cover) {
    cover.innerHTML = t.artwork_id
      ? `<img src="/api/artwork/${t.artwork_id}" alt="">`
      : '🎵';
  }
  const title = $('#np-title');
  const artist = $('#np-artist');
  if (title) title.textContent = t.title || 'Unknown';
  if (artist) artist.textContent = (t.artist || 'Unknown') + (t.album ? ` — ${t.album}` : '');
  const fmt = $('#np-format');
  if (fmt) fmt.textContent = t.format || '';
  const dur = $('#np-duration');
  if (dur) dur.textContent = fmtDur(t.duration_ms);

  // Audio element
  let audio = $('#audio-player');
  if (!audio) {
    audio = document.createElement('audio');
    audio.id = 'audio-player';
    document.body.appendChild(audio);
  }
  audio.src = MichiAPI.streamUrl(t.id);
  audio.play().catch(() => {});
  audio.ontimeupdate = () => {
    const pct = audio.duration ? (audio.currentTime / audio.duration) * 100 : 0;
    const fill = $('#np-progress-fill');
    if (fill) fill.style.width = pct + '%';
    const cur = $('#np-current');
    if (cur) cur.textContent = fmtDur(audio.currentTime * 1000);
  };
}

function updateMiniPlayer(t) {
  if (!t) return;
  const cover = $('#minibar-cover');
  if (cover) cover.innerHTML = '🎵';
  const title = $('#minibar-title');
  const artist = $('#minibar-artist');
  if (title) title.textContent = t.title || 'Unknown';
  if (artist) artist.textContent = t.artist || 'Unknown';
}

/* Michi Link panel */
function testMichiLink() {
  loadStatus();
  loadServerInfo();
  loadStats();
  showToast('Michi Link connection tested!');
}

function copyServerUrl() {
  const inp = $('#server-url-input');
  if (!inp) return;
  inp.select();
  document.execCommand('copy');
  showToast('URL copied!');
}

/* Search on Enter */
document.addEventListener('keydown', (e) => {
  if (e.key === 'Enter' && e.target.id === 'search-input') {
    handleSearch();
  }
});

/* Init on load */
document.addEventListener('DOMContentLoaded', init);
