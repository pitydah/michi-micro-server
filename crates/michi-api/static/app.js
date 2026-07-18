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
    container.innerHTML = '<div class="empty-state"><p style="color:var(--error)">Could not load server status</p></div>';
    return;
  }
  container.innerHTML =
    '<div class="status-item"><div class="icon"><svg viewBox="0 0 24 24" fill="none" stroke="var(--online)" stroke-width="1.5"><circle cx="12" cy="12" r="10"/><path d="M12 16v-4"/><path d="M12 8h.01"/></svg></div><div class="info"><div class="label">Status</div><div class="value"><span class="badge ' + (s.status === 'ok' ? 'stable' : 'disabled') + '">' + esc(s.status) + '</span></div></div></div>' +
    '<div class="status-item"><div class="icon"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5"><path d="M22 12h-4l-3 9L9 3l-3 9H2"/></svg></div><div class="info"><div class="label">Service</div><div class="value">' + esc(s.name || 'No disponible') + '</div></div></div>' +
    '<div class="status-item"><div class="icon"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5"><circle cx="12" cy="12" r="10"/><path d="M12 6v6l4 2"/></svg></div><div class="info"><div class="label">Uptime</div><div class="value">' + fmtDur((s.uptime_seconds || 0) * 1000) + '</div></div></div>' +
    '<div class="status-item"><div class="icon"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5"><rect x="2" y="3" width="20" height="14" rx="2"/><path d="M8 21h8"/><path d="M12 17v4"/></svg></div><div class="info"><div class="label">Version</div><div class="value">' + esc(s.version || 'No disponible') + '</div></div></div>' +
    '<div class="status-item"><div class="icon"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5"><rect x="2" y="2" width="20" height="8" rx="2"/><rect x="2" y="14" width="20" height="8" rx="2"/></svg></div><div class="info"><div class="label">Database</div><div class="value"><span class="badge ' + (s.database === 'ok' ? 'stable' : 'disabled') + '">' + esc(s.database || 'No disponible') + '</span></div></div></div>' +
    '<div class="status-item"><div class="icon"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5"><path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2"/><circle cx="12" cy="7" r="4"/></svg></div><div class="info"><div class="label">Server ID</div><div class="value" style="font-family:var(--font-mono);font-size:.75rem">' + esc(s.server_id || 'No disponible') + '</div></div></div>' +
    '<div class="status-item"><div class="icon"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5"><path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/></svg></div><div class="info"><div class="label">Music Paths</div><div class="value">' + esc(s.music_paths != null ? s.music_paths : 'No disponible') + '</div></div></div>';
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
    '<div class="card"><div class="card-icon"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5"><path d="M9 18V5l12-2v13"/><circle cx="6" cy="18" r="3"/><circle cx="18" cy="16" r="3"/></svg></div><div class="card-value">' + esc(tracks) + '</div><div class="card-label">Tracks</div></div>' +
    '<div class="card"><div class="card-icon"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5"><path d="M3 4h18"/><rect x="3" y="8" width="18" height="12" rx="2"/></svg></div><div class="card-value">' + esc(albums) + '</div><div class="card-label">Albums</div></div>' +
    '<div class="card"><div class="card-icon"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5"><path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2"/><circle cx="12" cy="7" r="4"/></svg></div><div class="card-value">' + esc(artists) + '</div><div class="card-label">Artists</div></div>' +
    '<div class="card"><div class="card-icon"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><polyline points="14 2 14 8 20 8"/><line x1="16" y1="13" x2="8" y2="13"/><line x1="16" y1="17" x2="8" y2="17"/></svg></div><div class="card-value">' + (State.serverInfo?.features?.playlists ? '✓' : '--') + '</div><div class="card-label">Playlists</div></div>';
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
      '<td><span class="badge format" data-format="' + esc(t.format || '').toLowerCase() + '">' + esc(t.format || '?') + '</span></td>' +
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
  updatePlayButtons();
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
  updatePlayButtons();
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

function updatePlayButtons() {
  const audio = getAudio();
  const isPlaying = !audio.paused;
  const playBtns = $$('[data-play-icon]');
  playBtns.forEach(btn => {
    btn.innerHTML = isPlaying
      ? '<svg viewBox="0 0 24 24" fill="currentColor" stroke="none"><rect x="6" y="4" width="4" height="16"/><rect x="14" y="4" width="4" height="16"/></svg>'
      : '<svg viewBox="0 0 24 24" fill="currentColor" stroke="none"><polygon points="5 3 19 12 5 21 5 3"/></svg>';
  });
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

// ── Playlists ────────────────────────────────────────────────────
async function loadPlaylists() {
  try {
    const raw = await MichiAPI.request('/api/v1/playlists');
    const playlists = raw.playlists || [];
    renderPlaylists(playlists);
    const el = $('#playlist-count');
    if (el) el.textContent = playlists.length + ' playlist(s)';

    const smartList = $('#smart-playlists-list');
    if (smartList) {
      const smartOnes = playlists.filter(function (p) { return p.description && p.description.indexOf('Smart playlist') === 0; });
      renderSmartList(smartOnes);
    }
  } catch (e) { console.warn('playlists failed:', e.message); }
}

function renderPlaylists(playlists) {
  const container = $('#playlists-list');
  if (!container) return;
  if (!playlists || playlists.length === 0) {
    renderEmpty(container, '📋', 'No playlists yet', 'Create a smart playlist to get started.');
    return;
  }
  container.innerHTML = playlists.map(function (p) {
    return '<div class="playlist-item">' +
      '<div class="info"><div class="name">' + esc(p.name) + '</div>' +
      (p.description ? '<div class="desc">' + esc(p.description) + '</div>' : '') +
      '</div>' +
      '<div class="meta">' + (p.track_count || 0) + ' tracks</div>' +
      '</div>';
  }).join('');
}

function renderSmartList(playlists) {
  const container = $('#smart-playlists-list');
  if (!container) return;
  if (!playlists || playlists.length === 0) {
    container.innerHTML = '<p style="color:var(--text-dim);font-size:.82rem;padding:12px 0">No smart playlists created yet.</p>';
    return;
  }
  container.innerHTML = playlists.map(function (p) {
    return '<div class="playlist-item">' +
      '<div class="info"><div class="name">' + esc(p.name) + '</div>' +
      '<div class="desc">' + esc(p.description || '') + '</div></div>' +
      '<div class="meta">' + (p.track_count || 0) + ' tracks</div>' +
      '</div>';
  }).join('');
}

function switchPlaylistTab(tab) {
  $$('.tab-btn').forEach(function (b) { b.classList.remove('active'); });
  var btn = $('.tab-btn[data-tab="' + tab + '"]');
  if (btn) btn.classList.add('active');
  $$('[id^="playlist-tab-"]').forEach(function (t) { t.classList.add('hidden'); });
  var pane = $('#playlist-tab-' + tab);
  if (pane) pane.classList.remove('hidden');

  if (tab === 'browse') loadPlaylists();
}

// Show/hide param row based on rule
document.addEventListener('DOMContentLoaded', function () {
  var ruleSelect = $('#smart-rule');
  if (ruleSelect) {
    ruleSelect.addEventListener('change', function () {
      var row = $('#smart-param-row');
      var label = $('#smart-param-label');
      var input = $('#smart-param');
      if (!row || !label || !input) return;
      var val = ruleSelect.value;
      if (val === 'by_genre') {
        row.style.display = 'flex';
        label.textContent = 'Genre';
        input.placeholder = 'e.g. Jazz';
      } else if (val === 'by_year') {
        row.style.display = 'flex';
        label.textContent = 'Year';
        input.placeholder = 'e.g. 2024';
      } else {
        row.style.display = 'none';
      }
    });
  }
});

async function createSmartPlaylist() {
  var name = $('#smart-name')?.value.trim();
  var rule = $('#smart-rule')?.value;
  var limit = parseInt($('#smart-limit')?.value || '50');
  if (!name) { showToast('Please enter a name', true); return; }

  var params = {};
  if (rule === 'by_genre' || rule === 'by_year') {
    var pval = $('#smart-param')?.value.trim();
    if (!pval) { showToast('Please enter a value', true); return; }
    params[rule === 'by_genre' ? 'genre' : 'year'] = rule === 'by_year' ? parseInt(pval) || 2024 : pval;
  }
  params.limit = limit;

  try {
    var resp = await MichiAPI.request('/api/v1/playlists/smart', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ name: name, rule: rule, params: params }),
    });
    showToast('Smart playlist "' + name + '" created with ' + resp.tracks_added + ' tracks');
    $('#smart-name').value = '';
    loadPlaylists();
    switchPlaylistTab('browse');
  } catch (e) { showToast(e.message, true); }
}

// ── Backup ───────────────────────────────────────────────────────
async function downloadBackup() {
  try {
    var data = await MichiAPI.request('/api/v1/backup', { timeout: 30000 });
    var blob = new Blob([JSON.stringify(data, null, 2)], { type: 'application/json' });
    var url = URL.createObjectURL(blob);
    var a = document.createElement('a');
    a.href = url;
    a.download = 'michi-backup-' + new Date().toISOString().slice(0, 10) + '.json';
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
    URL.revokeObjectURL(url);
    showToast('Backup downloaded!');
  } catch (e) { showToast(e.message, true); }
}

// Extend init to load playlists when navigating to playlists page
var _origShowSection = showSection;
showSection = function (section) {
  _origShowSection(section);
  if (section === 'playlists') {
    loadPlaylists();
  }
  if (section === 'settings') {
    loadCurrentState();
  }
};

// ── Settings ─────────────────────────────────────────────────────
function switchSettingsTab(tab) {
  $$('.settings-tab').forEach(function (b) { b.classList.remove('active'); });
  var btn = $('.settings-tab[data-stab="' + tab + '"]');
  if (btn) btn.classList.add('active');
  $$('[id^="stab-"]').forEach(function (t) { t.classList.add('hidden'); });
  var pane = $('#stab-' + tab);
  if (pane) pane.classList.remove('hidden');
}

async function loadCurrentState() {
  try {
    var data = await MichiAPI.request('/api/v1/playback/state');
    var el = $('#handoff-current-state');
    if (el) {
      el.textContent = JSON.stringify(data, null, 2);
    }
  } catch (e) {
    var el = $('#handoff-current-state');
    if (el) el.textContent = 'Could not load state: ' + e.message;
  }
}

// Upload
async function uploadFile() {
  var input = $('#settings-file-input');
  if (!input || !input.files || !input.files[0]) {
    showToast('Select a file first', true);
    return;
  }
  var file = input.files[0];
  var reader = new FileReader();
  reader.onload = async function (e) {
    var base64 = e.target.result.split(',')[1];
    try {
      var resp = await MichiAPI.request('/api/v1/sync/upload/file', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          filename: file.name,
          original_path: file.name,
          uploaded_by: 'webui',
          data_base64: base64,
        }),
        timeout: 60000,
      });
      var wrap = $('#upload-progress-wrap');
      var fill = $('#upload-progress-fill');
      var text = $('#upload-progress-text');
      if (wrap) wrap.classList.remove('hidden');
      if (fill) fill.style.width = '100%';
      if (text) text.textContent = 'Uploaded: ' + resp.hash.slice(0, 16) + '... (' + resp.size_bytes + ' bytes)';
      showToast('File uploaded: ' + resp.status);
      input.value = '';
    } catch (err) {
      showToast(err.message, true);
    }
  };
  reader.readAsDataURL(file);
}

// Playlist sync
async function syncPlaylist() {
  var name = $('#sync-playlist-name')?.value.trim();
  var tracksText = $('#sync-playlist-tracks')?.value.trim();
  if (!name) { showToast('Enter a playlist name', true); return; }
  var tracks = tracksText ? tracksText.split('\n').map(function (l) { return l.trim(); }).filter(Boolean) : [];
  try {
    var resp = await MichiAPI.request('/api/v1/sync/playlist', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ name: name, tracks: tracks }),
    });
    var el = $('#sync-playlist-result');
    if (el) {
      el.innerHTML = '<span style="color:var(--online)">✓ Created: ' + resp.playlist.name +
        ' (' + resp.tracks_added + ' tracks, ' + resp.tracks_missing.length + ' missing)</span>';
    }
    showToast('Playlist synced');
  } catch (e) { showToast(e.message, true); }
}

// Handoff
async function transferHandoff() {
  var trackId = $('#handoff-track-id')?.value.trim();
  var position = parseInt($('#handoff-position')?.value || '0');
  var playing = $('#handoff-playing')?.checked;
  if (!trackId) { showToast('Enter a track ID', true); return; }
  try {
    var resp = await MichiAPI.request('/api/v1/player/handoff', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        track_id: trackId,
        position_ms: position,
        playing: playing,
      }),
    });
    var el = $('#handoff-result');
    if (el) {
      el.innerHTML = '<span style="color:var(--online)">✓ Handoff accepted at ' + resp.position_ms + 'ms</span>';
    }
    loadCurrentState();
    showToast('Handoff transferred');
  } catch (e) { showToast(e.message, true); }
}

// Receivers
async function discoverDevices() {
  try {
    var resp = await MichiAPI.request('/api/v1/devices/discover', { method: 'POST', timeout: 10000 });
    var el = $('#discover-result');
    if (el) {
      var r = resp.receivers || [];
      if (r.length === 0) {
        el.innerHTML = '<span style="color:var(--text-dim)">No receivers found</span>';
      } else {
        el.innerHTML = r.map(function (rec) {
          return '<div style="padding:6px 0;border-bottom:1px solid var(--border-subtle);font-size:.78rem">' +
            '<span style="color:var(--text)">' + esc(rec.name || 'Unknown') + '</span> ' +
            '<span style="color:var(--text-dim);font-family:var(--font-mono);font-size:.7rem">' + esc(rec.host || '') + '</span>' +
            '</div>';
        }).join('');
      }
    }
  } catch (e) { showToast(e.message, true); }
}

async function createGroup() {
  var name = $('#group-name')?.value.trim();
  var receivers = $('#group-receivers')?.value.trim();
  if (!name) { showToast('Enter a group name', true); return; }
  var ids = receivers ? receivers.split(',').map(function (s) { return s.trim(); }).filter(Boolean) : [];
  try {
    var resp = await MichiAPI.request('/api/v1/receivers/groups', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ name: name, receiver_ids: ids }),
    });
    var el = $('#groups-list');
    if (el) {
      el.innerHTML = '<span style="color:var(--online)">✓ Group "' + resp.group.name + '" created</span>';
    }
    $('#group-name').value = '';
    $('#group-receivers').value = '';
    showToast('Group created');
  } catch (e) { showToast(e.message, true); }
}

// Webhook
async function setWebhook() {
  var url = $('#webhook-url')?.value.trim();
  if (!url) { showToast('Enter a webhook URL', true); return; }
  try {
    await MichiAPI.request('/api/v1/webhook', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ url: url }),
    });
    var el = $('#webhook-status');
    if (el) el.innerHTML = '<span style="color:var(--online)">✓ Webhook set</span>';
    showToast('Webhook configured');
  } catch (e) { showToast(e.message, true); }
}

async function testWebhook() {
  try {
    await MichiAPI.request('/api/v1/webhook/test', { method: 'POST', timeout: 10000 });
    var el = $('#webhook-status');
    if (el) el.innerHTML = '<span style="color:var(--online)">✓ Webhook fired</span>';
    showToast('Webhook tested');
  } catch (e) { showToast(e.message, true); }
}

async function deleteWebhook() {
  try {
    await MichiAPI.request('/api/v1/webhook', { method: 'DELETE' });
    var el = $('#webhook-status');
    if (el) el.innerHTML = '<span style="color:var(--text-dim)">Webhook cleared</span>';
    $('#webhook-url').value = '';
    showToast('Webhook removed');
  } catch (e) { showToast(e.message, true); }
}

// Backup
async function createSnapshot() {
  try {
    var resp = await MichiAPI.request('/api/v1/backup/snapshot', { method: 'POST' });
    var el = $('#snapshot-result');
    if (el) {
      var s = resp.snapshot || {};
      el.innerHTML = '<span style="color:var(--online)">✓ Snapshot: ' +
        (s.stats?.tracks || 0) + ' tracks, ' +
        (s.stats?.albums || 0) + ' albums, ' +
        (s.stats?.artists || 0) + ' artists</span>';
    }
    showToast('Snapshot created');
  } catch (e) { showToast(e.message, true); }
}

async function verifyIntegrity() {
  try {
    var resp = await MichiAPI.request('/api/v1/health/verify', { timeout: 30000 });
    var el = $('#integrity-result');
    if (el) {
      var ok = resp.status === 'ok';
      el.innerHTML = '<span style="color:' + (ok ? 'var(--online)' : 'var(--error)') + '">' +
        (ok ? '✓ All OK' : '⚠ Issues found') + ' — ' +
        resp.verified + ' verified, ' + resp.missing + ' missing, ' + resp.corrupt + ' corrupt</span>';
    }
  } catch (e) { showToast(e.message, true); }
}
