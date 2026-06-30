/* ================================================================
   Michi Control UI — main application
   ================================================================ */

// ── API client ──────────────────────────────────────────────────
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
        const msg = body?.error?.message || body?.message || body?.error?.code || `HTTP ${res.status}`;
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
    if (opts.limit) params.push('limit=' + opts.limit);
    if (opts.offset) params.push('offset=' + (opts.offset || 0));
    if (params.length) url += '?' + params.join('&');
    return this.request(url);
  },
  search(q) { return this.request('/api/v1/search?q=' + encodeURIComponent(q)); },
  scan() { return this.request('/api/library/scan', { method: 'POST' }); },
  playlists() { return this.request('/api/v1/playlists'); },
  streamUrl(id) { return this.base + '/api/v1/stream/' + id; },
};

// ── State ───────────────────────────────────────────────────────
const State = {
  status: null,
  serverInfo: null,
  stats: null,
  tracks: [],
  allTracks: [],
  currentTrack: null,
  queue: [],
  polling: null,
  audio: null,
};

// ── Helpers ─────────────────────────────────────────────────────
function $(sel, ctx) { return (ctx || document).querySelector(sel); }
function $$(sel, ctx) { return (ctx || document).querySelectorAll(sel); }

function esc(s) {
  if (s === null || s === undefined) return '';
  const d = document.createElement('div');
  d.textContent = String(s);
  return d.innerHTML;
}

function fmtDur(ms) {
  if (!ms && ms !== 0) return '--:--';
  const t = Math.floor(ms / 1000);
  const m = Math.floor(t / 60);
  const sec = t % 60;
  return m + ':' + String(sec).padStart(2, '0');
}

function fmtDate(iso) {
  if (!iso) return '';
  const d = new Date(iso);
  return d.toLocaleDateString() + ' ' + d.toLocaleTimeString();
}

// ── Loading / Empty / Error states ──────────────────────────────
function renderLoading(container, lines) {
  if (!container) return;
  container.innerHTML = Array.from({ length: lines || 3 }, () =>
    '<div class="skeleton" style="height:32px;margin-bottom:6px">&nbsp;</div>'
  ).join('');
}

function renderEmpty(container, icon, title, msg) {
  if (!container) return;
  container.innerHTML =
    '<div class="empty-state">' +
    '<div class="icon">' + (icon || '📭') + '</div>' +
    '<p><strong>' + esc(title || 'Nothing here') + '</strong></p>' +
    '<p style="font-size:.78rem;margin-top:4px">' + esc(msg || '') + '</p>' +
    '</div>';
}

function renderError(container, message, retryFn) {
  if (!container) return;
  container.innerHTML =
    '<div class="empty-state">' +
    '<div class="icon">⚠️</div>' +
    '<p style="color:var(--error)">' + esc(message || 'An error occurred') + '</p>' +
    (retryFn ? '<button class="btn btn-sm btn-ghost" style="margin-top:8px" onclick="(' + retryFn + ')()">Retry</button>' : '') +
    '</div>';
}

// ── Toast ───────────────────────────────────────────────────────
function showToast(msg, isErr) {
  const el = $('#toast');
  if (!el) return;
  el.textContent = msg;
  el.style.borderColor = isErr ? '#EF4444' : '#22C55E';
  el.style.display = 'block';
  setTimeout(() => { el.style.display = 'none'; }, 3000);
}

// ── Navigation ──────────────────────────────────────────────────
function showSection(section) {
  $$('.nav-item').forEach(n => n.classList.remove('active'));
  const nav = $('.nav-item[data-section="' + section + '"]');
  if (nav) nav.classList.add('active');
  $$('.section-page').forEach(p => p.classList.add('hidden'));
  const page = $('#page-' + section);
  if (page) page.classList.remove('hidden');
}

// ── Init ────────────────────────────────────────────────────────
async function init() {
  showSection('dashboard');
  await Promise.all([loadStatus(), loadServerInfo(), loadStats(), loadTracks()]);
  State.polling = setInterval(() => { loadStatus(); loadStats(); }, 30000);
}

document.addEventListener('DOMContentLoaded', init);

// ── Status ──────────────────────────────────────────────────────
async function loadStatus() {
  try {
    State.status = await MichiAPI.status();
    renderStatus();
    renderStatusPage();
  } catch (e) { console.warn('status failed:', e.message); }
}

function renderStatus() {
  const s = State.status;
  if (!s) return;

  const dot = $('#server-status-dot');
  const lbl = $('#server-status-label');
  const ok = s.status === 'ok';
  if (dot) dot.className = 'server-status-dot ' + (ok ? 'online' : 'offline');
  if (lbl) lbl.textContent = ok ? 'Online' : 'Offline';

  const pill = $('#status-pill');
  if (pill) {
    pill.className = 'status-pill ' + (ok ? 'online' : 'offline');
    pill.innerHTML = '<span class="server-status-dot ' + (ok ? 'online' : 'offline') + '"></span>' + (ok ? 'Online' : 'Offline');
  }

  const sid = $('#sidebar-server-id');
  if (sid && s.server_id) sid.textContent = s.server_id.slice(0, 8) + '..';

  const suptime = $('#sidebar-uptime');
  if (suptime) {
    const h = Math.floor((s.uptime_seconds || 0) / 3600);
    const m = Math.floor(((s.uptime_seconds || 0) % 3600) / 60);
    suptime.textContent = (h || m) ? h + 'h ' + m + 'm' : 'Just started';
  }
}

function renderStatusPage() {
  const container = $('#status-content');
  if (!container) return;
  const s = State.status;
  if (!s) {
    renderError(container, 'Could not load server status', 'loadStatus');
    return;
  }
  container.innerHTML =
    '<div class="section-header"><span>Status</span><span class="badge ' + (s.status === 'ok' ? 'stable' : 'disabled') + '">' + esc(s.status) + '</span></div>' +
    '<div class="section-header"><span>Service</span><span>' + esc(s.name || 'No disponible') + '</span></div>' +
    '<div class="section-header"><span>Version</span><span>' + esc(s.version || 'No disponible') + '</span></div>' +
    '<div class="section-header"><span>Port</span><span>' + esc(s.port) + '</span></div>' +
    '<div class="section-header"><span>Uptime</span><span>' + fmtDur((s.uptime_seconds || 0) * 1000) + '</span></div>' +
    '<div class="section-header"><span>Server ID</span><span style="font-family:monospace;font-size:.78rem">' + esc(s.server_id || 'No disponible') + '</span></div>' +
    '<div class="section-header"><span>Database</span><span class="badge ' + (s.database === 'ok' ? 'stable' : 'disabled') + '">' + esc(s.database || 'No disponible') + '</span></div>' +
    '<div class="section-header"><span>Music Paths</span><span>' + esc(s.music_paths != null ? s.music_paths : 'No disponible') + '</span></div>';
}

// ── Server Info & Michi Link ────────────────────────────────────
async function loadServerInfo() {
  try {
    State.serverInfo = await MichiAPI.serverInfo();
    renderServerInfo();
  } catch (e) { console.warn('server info failed:', e.message); }
}

const FEATURE_LABELS = {
  library:     { label: 'Library',     stable: true },
  search:      { label: 'Search',      stable: true },
  streaming:   { label: 'Streaming',   stable: true },
  download:    { label: 'Download',    stable: true },
  artwork:     { label: 'Artwork',     stable: true },
  playlists:   { label: 'Playlists',   stable: true },
  sync_manifest: { label: 'Sync',      stable: true },
  import:      { label: 'Import',      stable: true },
  playback:    { label: 'Playback',    stable: true },
  queue:       { label: 'Queue',       stable: true },
  receivers:   { label: 'Receivers',   beta: true },
  rooms:       { label: 'Rooms',       future: true },
  events:      { label: 'Events',      beta: true },
  transcoding: { label: 'Transcoding', future: true },
  token_refresh: { label: 'Token Refresh', stable: true },
};

const FEATURE_CLASSES = {
  on:     { cls: 'stable', text: 'ON' },
  off:    { cls: 'disabled', text: 'OFF' },
  beta:   { cls: 'beta', text: 'BETA' },
  future: { cls: 'experimental', text: 'EXP' },
  nd:     { cls: 'disabled', text: 'N/D' },
};

function featureBadge(enabled, meta) {
  if (meta?.future && !enabled) return FEATURE_CLASSES.future;
  if (meta?.beta && enabled) return FEATURE_CLASSES.beta;
  if (enabled) return FEATURE_CLASSES.on;
  if (meta?.future) return FEATURE_CLASSES.future;
  if (meta?.beta) return FEATURE_CLASSES.beta;
  return FEATURE_CLASSES.off;
}

function renderServerInfo() {
  const info = State.serverInfo;
  if (!info) return;

  const ver = $('#sidebar-ver');
  if (ver) ver.textContent = info.michi_link_version || info.version || '--';

  const sid = $('#server-info-id');
  if (sid) sid.textContent = info.server_id || '--';

  const grid = $('#features-grid');
  if (!grid) return;
  grid.innerHTML = '';

  Object.keys(FEATURE_LABELS).forEach(key => {
    const meta = FEATURE_LABELS[key];
    const val = info.features && info.features[key];
    const fb = featureBadge(val, meta);
    const item = document.createElement('div');
    item.className = 'feature-item';
    item.innerHTML =
      '<span class="feature-dot ' + (val ? 'on' : 'off') + '"></span>' +
      esc(meta.label) +
      ' <span class="badge ' + fb.cls + '" style="margin-left:auto">' + fb.text + '</span>';
    grid.appendChild(item);
  });
}

// ── Stats / Dashboard ───────────────────────────────────────────
async function loadStats() {
  try {
    State.stats = await MichiAPI.libraryStats();
    renderStats();
  } catch (e) { console.warn('stats failed:', e.message); }
}

function renderStats() {
  const s = State.stats;
  const cd = $('#dashboard-cards');
  if (!cd) return;

  const tracks = s?.tracks ?? 'N/D';
  const albums = s?.albums ?? 'N/D';
  const artists = s?.artists ?? 'N/D';

  cd.innerHTML =
    '<div class="card"><div class="card-icon">🎵</div><div class="card-value">' + esc(tracks) + '</div><div class="card-label">Tracks</div></div>' +
    '<div class="card"><div class="card-icon">💿</div><div class="card-value">' + esc(albums) + '</div><div class="card-label">Albums</div></div>' +
    '<div class="card"><div class="card-icon">👤</div><div class="card-value">' + esc(artists) + '</div><div class="card-label">Artists</div></div>' +
    '<div class="card"><div class="card-icon">📋</div><div class="card-value">' + (State.serverInfo?.features?.playlists ? '✓' : '--') + '</div><div class="card-label">Playlists</div></div>';
}

// ── Tracks / Library ────────────────────────────────────────────
async function loadTracks() {
  try {
    const raw = await MichiAPI.tracks();
    State.tracks = raw.tracks || [];
    State.allTracks = State.tracks;
    updateTracksCount();
    renderTracks(State.tracks, 'tracks-table');
  } catch (e) { console.warn('tracks failed:', e.message); }
}

function updateTracksCount() {
  const el1 = $('#tracks-count');
  const el2 = $('#library-count');
  const total = State.stats?.tracks ?? State.allTracks.length;
  const text = 'Showing ' + Math.min(State.tracks.length, 50) + ' of ' + total + ' tracks';
  if (el1) el1.textContent = text;
  if (el2) el2.textContent = text;
}

function renderTracks(tracks, tableId) {
  const container = $('#' + (tableId || 'tracks-table'));
  if (!container) return;

  if (!tracks || tracks.length === 0) {
    renderEmpty(container, '🎵', 'Library empty', 'Scan your music library to get started.');
    return;
  }

  let html = '<table><thead><tr>' +
    '<th>#</th><th>Cover</th><th>Title</th><th>Artist</th><th>Album</th><th>Format</th><th>Duration</th><th></th>' +
    '</tr></thead><tbody>';

  const slice = tracks.slice(0, 50);
  slice.forEach((t, i) => {
    const realIdx = State.tracks.indexOf(t);
    const coverHtml = t.artwork_id
      ? '<img src="/api/artwork/' + t.artwork_id + '" alt="" style="width:32px;height:32px;border-radius:4px;object-fit:cover">'
      : '<span style="font-size:1rem">🎵</span>';
    html += '<tr>' +
      '<td style="color:var(--text-dim)">' + (i + 1) + '</td>' +
      '<td>' + coverHtml + '</td>' +
      '<td class="track-title">' + esc(t.title || 'Unknown') + '</td>' +
      '<td class="track-artist">' + esc(t.artist || '—') + '</td>' +
      '<td class="track-artist">' + esc(t.album || '—') + '</td>' +
      '<td><span class="badge format">' + esc(t.format || '?') + '</span></td>' +
      '<td style="color:var(--text-dim)">' + fmtDur(t.duration_ms) + '</td>' +
      '<td><button class="btn btn-sm btn-ghost" onclick="event.stopPropagation();playTrack(' + realIdx + ')">Play</button></td>' +
      '</tr>';
  });

  html += '</tbody></table>';
  container.innerHTML = html;
}

// ── Search ──────────────────────────────────────────────────────
async function handleSearch() {
  const q = $('#search-input')?.value.trim();
  if (!q) {
    State.tracks = State.allTracks;
    renderTracks(State.tracks, 'tracks-table');
    renderTracks(State.tracks, 'library-table');
    updateTracksCount();
    return;
  }
  try {
    const raw = await MichiAPI.search(q);
    State.tracks = raw.tracks || [];
    renderTracks(State.tracks, 'tracks-table');
    renderTracks(State.tracks, 'library-table');
    updateTracksCount();
    $('#tracks-count').textContent = State.tracks.length + ' results';
    $('#library-count').textContent = State.tracks.length + ' results';
    showToast('Found ' + State.tracks.length + ' results');
  } catch (e) { showToast(e.message, true); }
}

document.addEventListener('keydown', (e) => {
  if (e.key === 'Enter' && e.target.id === 'search-input') handleSearch();
});

// ── Scan ────────────────────────────────────────────────────────
async function handleScan() {
  const btn = $('#scan-btn');
  if (btn) { btn.disabled = true; btn.textContent = 'Scanning...'; }
  try {
    const r = await MichiAPI.scan();
    showToast('Scanned ' + r.scanned + ' tracks, saved ' + r.saved);
    await Promise.all([loadStats(), loadTracks()]);
  } catch (e) { showToast(e.message, true); }
  finally {
    if (btn) { btn.disabled = false; btn.textContent = 'Scan'; }
  }
}

// ── Playback ────────────────────────────────────────────────────
function getAudio() {
  if (!State.audio) {
    State.audio = document.getElementById('audio-player');
    if (!State.audio) {
      State.audio = document.createElement('audio');
      State.audio.id = 'audio-player';
      document.body.appendChild(State.audio);
    }
    State.audio.ontimeupdate = updatePlaybackProgress;
    State.audio.onended = onTrackEnd;
    State.audio.onerror = function () {
      showToast('Playback error: ' + (State.audio?.error?.message || 'unknown'), true);
    };
  }
  return State.audio;
}

function playTrack(idx) {
  const tracks = State.tracks;
  if (!tracks || idx < 0 || idx >= tracks.length) return;
  const t = tracks[idx];
  State.currentTrack = t;
  updateNowPlaying(t);
  updateMiniPlayer(t);

  const audio = getAudio();
  audio.src = MichiAPI.streamUrl(t.id);
  audio.play().catch(function (err) {
    showToast('Could not play: ' + err.message, true);
  });
}

function playPause() {
  const audio = getAudio();
  if (audio.paused) {
    if (!audio.src && State.currentTrack) {
      playTrack(State.tracks.indexOf(State.currentTrack));
      return;
    }
    audio.play().catch(function () {});
  } else {
    audio.pause();
  }
}

function onTrackEnd() {
  showToast('Track ended');
}

function updatePlaybackProgress() {
  const audio = getAudio();
  if (!audio || !audio.duration) return;

  const pct = (audio.currentTime / audio.duration) * 100;
  const fill1 = $('#np-progress-fill');
  const fill2 = $('#mini-progress-fill');
  if (fill1) fill1.style.width = pct + '%';
  if (fill2) fill2.style.width = pct + '%';

  const cur = $('#np-current');
  if (cur) cur.textContent = fmtDur(audio.currentTime * 1000);
}

function updateNowPlaying(t) {
  if (!t) return;
  const cover = $('#np-cover');
  if (cover) {
    cover.innerHTML = t.artwork_id
      ? '<img src="/api/artwork/' + t.artwork_id + '" alt="">'
      : '🎵';
  }
  const title = $('#np-title');
  const artist = $('#np-artist');
  if (title) title.textContent = t.title || 'Unknown';
  if (artist) artist.textContent = (t.artist || 'Unknown') + (t.album ? ' — ' + t.album : '');
  const fmt = $('#np-format');
  if (fmt) fmt.textContent = t.format || '';
  const dur = $('#np-duration');
  if (dur) dur.textContent = fmtDur(t.duration_ms);
}

function updateMiniPlayer(t) {
  const mp = $('#mini-player');
  if (!mp) return;
  mp.classList.remove('hidden');
  const cover = $('#minibar-cover');
  if (cover) {
    cover.innerHTML = t.artwork_id
      ? '<img src="/api/artwork/' + t.artwork_id + '" alt="">'
      : '🎵';
  }
  const title = $('#minibar-title');
  const artist = $('#minibar-artist');
  if (title) title.textContent = t.title || 'Unknown';
  if (artist) artist.textContent = t.artist || '—';
}

// ── Michi Link panel ────────────────────────────────────────────
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
