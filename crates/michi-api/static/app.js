/* ================================================================
   Michi Control UI — Truthful Functional Conformance WebUI
   ================================================================ */

// ── API client ──────────────────────────────────────────────────
const MichiAPI = {
  base: '',

  async request(path, opts = {}) {
    const timeout = opts.timeout || 12000;
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), timeout);
    const method = (opts.method || 'GET').toUpperCase();
    const headers = new Headers(opts.headers || {});
    let body = opts.body;

    if (['POST', 'PUT', 'PATCH'].includes(method) && !(body instanceof FormData)) {
      if (!headers.has('Content-Type')) {
        headers.set('Content-Type', 'application/json');
      }
      if (body === undefined || body === null) {
        body = '{}';
      } else if (typeof body !== 'string') {
        body = JSON.stringify(body);
      }
    }

    try {
      const res = await fetch(this.base + path, {
        ...opts,
        method,
        headers,
        body,
        credentials: 'same-origin',
        signal: controller.signal,
      });
      clearTimeout(timer);

      if (res.status === 401 && !path.startsWith('/api/auth/')) {
        AuthSession.setUnauthenticated();
        teardownProtected();
        ConnectionStatus.setAuthRequired();
      }

      if (!res.ok) {
        const errBody = await res.json().catch(() => ({}));
        const msg = errBody?.error?.message || errBody?.message || errBody?.error?.code || `HTTP ${res.status}`;
        const err = new Error(msg);
        err.status = res.status;
        err.details = errBody?.error?.details || errBody;
        throw err;
      }
      const ct = res.headers.get('content-type') || '';
      return ct.includes('json') ? res.json() : res;
    } catch (e) {
      clearTimeout(timer);
      if (e.name === 'AbortError') {
        const timeoutErr = new Error('Connection timeout');
        timeoutErr.name = 'TimeoutError';
        throw timeoutErr;
      }
      throw e;
    }
  },

  // Auth
  authCheck() { return this.request('/api/auth/check'); },
  login(username, password) {
    return this.request('/api/auth/login', {
      method: 'POST',
      body: { username, password }
    });
  },
  register(username, password) {
    return this.request('/api/auth/register', {
      method: 'POST',
      body: { username, password }
    });
  },
  logout() { return this.request('/api/auth/logout', { method: 'POST' }); },

  // System & Status
  status() { return this.request('/api/v1/status'); },
  serverInfo() { return this.request('/api/v1/server/info'); },
  capabilities() { return this.request('/api/v1/capabilities'); },
  libraryStats() { return this.request('/api/v1/library/stats'); },
  dashboard() { return this.request('/api/v1/home/dashboard'); },

  // Library & Tracks
  tracks(opts = {}) {
    let url = '/api/v1/tracks';
    const params = [];
    if (opts.limit) params.push('limit=' + opts.limit);
    if (opts.offset) params.push('offset=' + (opts.offset || 0));
    if (params.length) url += '?' + params.join('&');
    return this.request(url);
  },
  track(id) { return this.request('/api/v1/tracks/' + id); },
  rateTrack(id, rating) {
    return this.request('/api/v1/rate/' + id, {
      method: 'POST',
      body: { rating: Number(rating) }
    });
  },
  starTrack(id, starred) {
    return this.request('/api/v1/star/' + id, {
      method: 'POST',
      body: { starred: !!starred }
    });
  },
  starredTracks() { return this.request('/api/v1/starred'); },
  bookmarks(opts = {}) {
    let url = '/api/v1/bookmarks';
    const params = [];
    if (opts.limit) params.push('limit=' + opts.limit);
    if (opts.offset) params.push('offset=' + (opts.offset || 0));
    if (params.length) url += '?' + params.join('&');
    return this.request(url);
  },
  addBookmark(track_id, position_ms, duration_ms, finished, device_id) {
    return this.request('/api/v1/bookmarks', {
      method: 'POST',
      body: { track_id, position_ms, duration_ms: duration_ms || 0, finished: !!finished, device_id }
    });
  },
  deleteBookmark(track_id) { return this.request('/api/v1/bookmarks/' + track_id, { method: 'DELETE' }); },
  duplicates() { return this.request('/api/v1/library/duplicates'); },
  artistInsights(name) { return this.request('/api/v1/artists/' + encodeURIComponent(name) + '/insights'); },
  albumHealth(name) { return this.request('/api/v1/albums/' + encodeURIComponent(name) + '/health'); },
  search(q) { return this.request('/api/v1/search?q=' + encodeURIComponent(q)); },
  searchAdvanced(q) { return this.request('/api/v1/search/advanced?q=' + encodeURIComponent(q)); },
  scan() { return this.request('/api/v1/library/scan', { method: 'POST' }); },
  artworkUrl(id) { return this.base + '/api/v1/artwork/' + id; },
  streamUrl(id) { return this.base + '/api/v1/stream/' + id; },
  downloadUrl(id) { return this.base + '/api/v1/download/' + id; },
  downloadTrack(id) { return this.request('/api/v1/download/' + id); },

  // Playlists
  playlists() { return this.request('/api/v1/playlists'); },
  playlist(id) { return this.request('/api/v1/playlists/' + id); },
  playlistTracks(id) { return this.request('/api/v1/playlists/' + id + '/tracks'); },
  createPlaylist(name, description) {
    return this.request('/api/v1/playlists', {
      method: 'POST',
      body: { name, description }
    });
  },
  updatePlaylist(id, name, description) {
    return this.request('/api/v1/playlists/' + id, {
      method: 'PUT',
      body: { name, description }
    });
  },
  deletePlaylist(id) { return this.request('/api/v1/playlists/' + id, { method: 'DELETE' }); },
  addPlaylistTrack(playlist_id, track_id) {
    return this.request('/api/v1/playlists/' + playlist_id + '/tracks/' + track_id, { method: 'POST' });
  },
  removePlaylistTrack(playlist_id, track_id) {
    return this.request('/api/v1/playlists/' + playlist_id + '/tracks/' + track_id, { method: 'DELETE' });
  },
  reorderPlaylist(id, track_ids) {
    return this.request('/api/v1/playlists/' + id + '/reorder', {
      method: 'PUT',
      body: { track_ids }
    });
  },
  smartPlaylist(name, rule, params) {
    return this.request('/api/v1/playlists/smart', {
      method: 'POST',
      body: { name, rule, params }
    });
  },

  // Playback & Queue
  playbackState() { return this.request('/api/v1/playback/state'); },
  playbackOutput() { return this.request('/api/v1/playback/output'); },
  setPlaybackOutput(body) { return this.request('/api/v1/playback/output', { method: 'PUT', body }); },
  getPlaybackOutput() { return this.playbackOutput(); },
  handoff(body) { return this.request('/api/v1/playback/handoff', { method: 'POST', body }); },
  seekPlayback(position_ms) { return this.request('/api/v1/playback/seek', { method: 'POST', body: { position_ms } }); },
  playbackControl(body) {
    return this.request('/api/v1/playback/control', {
      method: 'POST',
      body
    });
  },
  queue() { return this.request('/api/v1/queue'); },
  addQueueItems(track_ids, name) {
    return this.request('/api/v1/queue/items', {
      method: 'POST',
      body: { track_ids, name }
    });
  },
  jumpQueue(index, queue_id) {
    return this.request('/api/v1/queue/jump', {
      method: 'POST',
      body: { index, queue_id }
    });
  },
  reorderQueue(item_ids, queue_id) {
    return this.request('/api/v1/queue/reorder', {
      method: 'PUT',
      body: { item_ids, queue_id }
    });
  },
  deleteQueue(queue_id) { return this.request('/api/v1/queue/' + queue_id, { method: 'DELETE' }); },
  transferQueue(track_ids, current_index, position_ms, source) {
    return this.request('/api/v1/queue/transfer', {
      method: 'POST',
      body: { track_ids, current_index, position_ms, source: source || 'webui' }
    });
  },
  saveQueue(name, track_ids) {
    return this.request('/api/v1/queue/save', {
      method: 'POST',
      body: { name, track_ids }
    });
  },
  savedQueues() { return this.request('/api/v1/queue/saved'); },

  // History
  history(opts = {}) {
    let url = '/api/v1/history';
    const params = [];
    if (opts.limit) params.push('limit=' + opts.limit);
    if (opts.offset) params.push('offset=' + (opts.offset || 0));
    if (params.length) url += '?' + params.join('&');
    return this.request(url);
  },
  historyStats() { return this.request('/api/v1/history/stats'); },
  exportHistory() { return this.request('/api/v1/history/export', { timeout: 20000 }); },
  clearHistory() { return this.request('/api/v1/history', { method: 'DELETE' }); },

  // Ecosystem & Link
  linkDevices() { return this.request('/api/v1/link/devices'); },
  revokeLinkDevice(device_id) {
    return this.request('/api/v1/devices/revoke', {
      method: 'POST',
      body: { device_id }
    });
  },
  qrPair(server_url) {
    return this.request('/api/v1/pair/qr', {
      method: 'POST',
      body: { server_url }
    });
  },
  qrStatus(code) { return this.request('/api/v1/pair/qr/' + code + '/status'); },
  receivers() { return this.request('/api/v1/receivers'); },
  getReceivers() { return this.receivers(); },
  discoverDevices() { return this.request('/api/v1/devices/discover', { method: 'POST', timeout: 10000 }); },
  startReceiverPair(body) { return this.request('/api/v1/receivers/pair/start', { method: 'POST', body }); },
  confirmReceiverPair(body) { return this.request('/api/v1/receivers/pair/confirm', { method: 'POST', body }); },

  // Rooms & Chains
  roomGroups() { return this.request('/api/v1/rooms/groups'); },
  getRoomGroups() { return this.roomGroups(); },
  createRoomGroup(body) { return this.request('/api/v1/rooms/groups', { method: 'POST', body }); },
  activateRoomGroup(id) { return this.request('/api/v1/rooms/groups/' + id + '/activate', { method: 'POST' }); },
  deactivateRoomGroup(id) { return this.request('/api/v1/rooms/groups/' + id + '/deactivate', { method: 'POST' }); },
  deleteRoomGroup(id) { return this.request('/api/v1/rooms/groups/' + id, { method: 'DELETE' }); },
  playRoom(id, track_id) { return this.request('/api/v1/rooms/' + id + '/play', { method: 'POST', body: { track_id } }); },
  chains() { return this.request('/api/v1/chains'); },
  getChains() { return this.chains(); },
  chain(id) { return this.request('/api/v1/chains/' + id); },
  createChain(name) { return this.request('/api/v1/chains', { method: 'POST', body: { name } }); },
  addChainLink(chainId, body) { return this.request('/api/v1/chains/' + chainId + '/links', { method: 'POST', body }); },
  removeChainLink(chainId, linkId) { return this.request('/api/v1/chains/' + chainId + '/links/' + linkId, { method: 'DELETE' }); },
  reorderChainLinks(chainId, link_ids) { return this.request('/api/v1/chains/' + chainId + '/links/reorder', { method: 'POST', body: { link_ids } }); },
  updateChainLink(chainId, linkId, body) { return this.request('/api/v1/chains/' + chainId + '/links/' + linkId, { method: 'PUT', body }); },
  playChain(chainId) { return this.request('/api/v1/chains/' + chainId + '/play', { method: 'POST' }); },
  stopChain(chainId) { return this.request('/api/v1/chains/' + chainId + '/stop', { method: 'POST' }); },
  setChainVolume(chainId, volume) { return this.request('/api/v1/chains/' + chainId + '/volume', { method: 'POST', body: { volume } }); },

  // Sources (Broadcast & Radio)
  sources() { return this.request('/api/v1/sources'); },
  addSource(url) { return this.request('/api/v1/sources', { method: 'POST', body: { url }, timeout: 15000 }); },
  deleteSource(id) { return this.request('/api/v1/sources/' + id, { method: 'DELETE' }); },
  sourceEpisodes(id) { return this.request('/api/v1/sources/' + id + '/episodes'); },
  updateEpisode(id, position_ms, played) {
    return this.request('/api/v1/episodes/' + id, {
      method: 'PUT',
      body: { position_ms: position_ms || 0, played: !!played }
    });
  },

  // Sync API
  syncUploadInit(filename, file_size, content_hash) {
    return this.request('/api/v1/sync/upload/init', {
      method: 'POST',
      body: { filename, file_size: file_size || 0, content_hash: content_hash || '' }
    });
  },
  syncUploadChunk(id, chunk_index, total_chunks, dataBase64, chunk_hash) {
    return this.request('/api/v1/sync/upload/' + id + '/chunk', {
      method: 'POST',
      body: { chunk_index, total_chunks, data: dataBase64, chunk_hash: chunk_hash || '' }
    });
  },
  syncUploadStatus(id) { return this.request('/api/v1/sync/upload/' + id + '/status'); },
  syncUploadFinalize(id, file_size, content_hash) {
    return this.request('/api/v1/sync/upload/' + id + '/finalize', {
      method: 'POST',
      body: { file_size: file_size || 0, content_hash: content_hash || '' }
    });
  },
  syncPlaylist(name, track_ids) {
    return this.request('/api/v1/playlists', {
      method: 'POST',
      body: { name, description: 'Synced playlist', track_ids }
    });
  },

  // Settings & Webhooks
  settings() { return this.request('/api/v1/settings'); },
  updateSettings(body) { return this.request('/api/v1/settings', { method: 'PUT', body }); },
  setWebhook(url) { return this.request('/api/v1/webhook', { method: 'POST', body: { url } }); },
  testWebhook() { return this.request('/api/v1/webhook/test', { method: 'POST', timeout: 10000 }); },
  deleteWebhook() { return this.request('/api/v1/webhook', { method: 'DELETE' }); },

  // Backup & Storage
  backupSnapshot() { return this.request('/api/v1/backup/snapshot', { method: 'POST' }); },
  backupVerify() { return this.request('/api/v1/backup/verify', { timeout: 30000 }); },
  downloadBackup() { return this.request('/api/v1/backup/download', { timeout: 30000 }); },
  restoreBackup(body) { return this.request('/api/v1/backup/restore', { method: 'POST', body, timeout: 60000 }); },

  // Diagnostics & Health
  health() { return this.request('/health'); },
  serverCapabilities() { return this.request('/api/v1/server/capabilities'); },
  diagnostics() { return this.request('/api/v1/diagnostics'); },
  selfTest() { return this.request('/api/v1/health/self-test', { timeout: 30000 }); },
  mountHealth() { return this.request('/api/v1/health/mounts'); },
  storageHealth() { return this.request('/api/v1/health/storage'); },
  configValidate() { return this.request('/api/v1/config/validate'); },
  jobs() { return this.request('/api/v1/jobs'); },
  job(id) { return this.request('/api/v1/jobs/' + id); },
  cancelJob(id) { return this.request('/api/v1/jobs/' + id + '/cancel', { method: 'POST' }); },
  modules() { return this.request('/api/v1/modules'); },
  toggleModule(name, enabled) { return this.request('/api/v1/modules/' + name, { method: 'POST', body: { name, enabled: !!enabled } }); },
  auditLog() { return this.request('/api/v1/audit/log'); },
  changeJournal() { return this.request('/api/v1/changes'); },

  // Import Workflow
  importPreflight(tracks) { return this.request('/api/v1/import/preflight', { method: 'POST', body: { tracks } }); },
  createImportSession(total_tracks, total_playlists) {
    return this.request('/api/v1/import/session', {
      method: 'POST',
      body: { total_tracks: total_tracks || 0, total_playlists: total_playlists || 0 }
    });
  },
  importSessionStatus(sessionId) { return this.request('/api/v1/import/session/' + sessionId + '/status'); },
  importUpload(sessionId, filename, dataBase64, hash) {
    return this.request('/api/v1/import/session/' + sessionId + '/upload', {
      method: 'POST',
      body: {
        filename,
        data: dataBase64,
        hash
      },
      timeout: 60000
    });
  },
  commitImportSession(sessionId) { return this.request('/api/v1/import/commit/' + sessionId, { method: 'POST' }); },
  rollbackImportSession(sessionId) { return this.request('/api/v1/import/rollback/' + sessionId, { method: 'POST' }); },
};

// ── Connection Status State Machine ──────────────────────────────
const ConnectionStatus = {
  state: 'checking', // 'checking' | 'online' | 'degraded' | 'auth_required' | 'offline'

  update(statusData) {
    if (!statusData) {
      this.state = 'offline';
    } else if (statusData.status === 'ok') {
      if (statusData.database === 'connected') {
        this.state = 'online';
      } else {
        this.state = 'degraded';
      }
    } else {
      this.state = 'degraded';
    }
    this.render();
  },

  setAuthRequired() {
    this.state = 'auth_required';
    this.render();
  },

  setOffline() {
    this.state = 'offline';
    this.render();
  },

  render() {
    const dot = $('#server-status-dot');
    const lbl = $('#server-status-label');
    const pill = $('#status-pill');
    const pillText = $('#status-pill-text');

    const labels = {
      checking: 'Checking...',
      online: 'Online',
      degraded: 'Degraded',
      auth_required: 'Auth Required',
      offline: 'Offline',
    };

    const cssClass = {
      checking: 'checking',
      online: 'online',
      degraded: 'degraded',
      auth_required: 'auth-required',
      offline: 'offline',
    }[this.state] || 'offline';

    const text = labels[this.state] || 'Offline';

    if (dot) dot.className = 'server-status-dot ' + cssClass;
    if (lbl) lbl.textContent = text;
    if (pill) pill.className = 'status-pill ' + cssClass;
    if (pillText) pillText.textContent = text;
  }
};

// ── Auth Session State Machine ──────────────────────────────────
const AuthSession = {
  state: 'checking', // 'checking' | 'anonymous' | 'authenticated' | 'disabled'
  user: null,
  registrationAllowed: false,

  async check() {
    try {
      const resp = await MichiAPI.authCheck();
      this.registrationAllowed = !!resp.registration_allowed;
      if (resp.authenticated === true) {
        this.state = 'authenticated';
        this.user = {
          id: resp.id,
          username: resp.username || 'User',
          is_admin: !!resp.is_admin
        };
      } else if (resp.enabled === false) {
        this.state = 'disabled';
        this.user = null;
      } else {
        this.state = 'anonymous';
        this.user = null;
      }
    } catch (e) {
      this.state = 'anonymous';
      this.user = null;
    }
    this.render();
    return this.state;
  },

  setUnauthenticated() {
    this.state = 'anonymous';
    this.user = null;
    this.render();
  },

  render() {
    const btn = $('#auth-user-btn');
    const label = $('#auth-btn-label');
    const formContainer = $('#auth-form-container');
    const sessionContainer = $('#auth-session-container');
    const currentUserEl = $('#auth-current-username');
    const regTab = $('#auth-tab-register');

    if (regTab) {
      regTab.style.display = this.registrationAllowed ? 'inline-block' : 'none';
    }

    if (this.state === 'authenticated') {
      if (label) label.textContent = this.user?.username || 'Account';
      if (formContainer) formContainer.style.display = 'none';
      if (sessionContainer) sessionContainer.style.display = 'block';
      if (currentUserEl) currentUserEl.textContent = this.user?.username || 'User';
    } else if (this.state === 'disabled') {
      if (label) label.textContent = 'Auth Disabled';
      if (formContainer) formContainer.style.display = 'none';
      if (sessionContainer) sessionContainer.style.display = 'block';
      if (currentUserEl) currentUserEl.textContent = 'Auth Disabled (Public View - Protected API Blocked)';
    } else {
      if (label) label.textContent = 'Sign In';
      if (formContainer) formContainer.style.display = 'block';
      if (sessionContainer) sessionContainer.style.display = 'none';
    }
  }
};

// ── i18n ────────────────────────────────────────────────────────
var _i18n = {};
var _currentLang = localStorage.getItem('michi_lang') || navigator.language.split('-')[0] || 'en';

function t(key, vars) {
  var val = _i18n[key];
  if (val === undefined || val === null) return key;
  if (vars) {
    for (var k in vars) {
      val = val.replace('{' + k + '}', vars[k]);
    }
  }
  return val;
}

async function loadI18n(lang) {
  _currentLang = lang || _currentLang;
  localStorage.setItem('michi_lang', _currentLang);
  document.documentElement.lang = _currentLang;
  try {
    var resp = await fetch('/static/i18n/' + _currentLang);
    if (resp.ok) {
      _i18n = await resp.json();
    } else {
      var fallback = await fetch('/static/i18n/en.json');
      _i18n = await fallback.json();
    }
  } catch (e) {
    _i18n = {};
  }
  applyI18n();
}

function applyI18n() {
  document.querySelectorAll('[data-i18n]').forEach(function (el) {
    var key = el.getAttribute('data-i18n');
    var text = t(key);
    if (el.tagName === 'INPUT' || el.tagName === 'TEXTAREA') {
      el.placeholder = text;
    } else {
      el.textContent = text;
    }
  });
  var searchInput = $('#search-input');
  if (searchInput) searchInput.placeholder = t('search_placeholder');
}

function setLanguage(lang) {
  loadI18n(lang).then(function () {
    var sel = $('#lang-select');
    if (sel) sel.value = lang;
    showToast(t('toast.language_set', {lang: lang.toUpperCase()}));
  });
}

// ── State ───────────────────────────────────────────────────────
const State = {
  status: null,
  serverInfo: null,
  stats: null,
  dashboard: null,
  tracks: [],
  allTracks: [],
  currentTrack: null,
  queue: [],
  polling: null,
  audio: null,
  eventWs: null,
};
window.State = State;

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

function fmtBytes(bytes) {
  if (bytes === null || bytes === undefined) return '--';
  if (bytes === 0) return '0 B';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return parseFloat((bytes / Math.pow(k, i)).toFixed(1)) + ' ' + sizes[i];
}

function fmtDate(iso) {
  if (!iso) return '';
  const d = new Date(iso);
  return d.toLocaleDateString() + ' ' + d.toLocaleTimeString();
}

function renderLoading(container, lines) {
  if (!container) return;
  container.innerHTML = Array.from({ length: lines || 3 }, () =>
    '<div class="skeleton" style="height:32px;margin-bottom:6px">&nbsp;</div>'
  ).join('');
}

function renderEmpty(container, icon, title, msg) {
  if (!container) return;
  container.innerHTML =
    '<div class="empty-state mascot">' +
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

function showToast(msg, isErr) {
  const el = $('#toast');
  if (!el) return;
  el.textContent = msg;
  el.className = 'toast ' + (isErr ? 'toast-error' : 'toast-success');
  el.style.display = 'block';
  setTimeout(() => {
    el.classList.add('toast-hiding');
    setTimeout(() => { el.style.display = 'none'; el.classList.remove('toast-hiding'); }, 200);
  }, 3000);
}

// ── Modals ───────────────────────────────────────────────────────
var _modalCallback = null;
var _modalPreviousFocus = null;

function showModal(title, message, confirmText, callback) {
  var overlay = $('#modal-overlay');
  var titleEl = $('#modal-title');
  var msgEl = $('#modal-message');
  var btn = $('#modal-confirm-btn');
  if (!overlay) return;
  titleEl.textContent = title;
  msgEl.textContent = message;
  btn.textContent = confirmText || t('common.confirm');
  _modalCallback = callback;
  _modalPreviousFocus = document.activeElement;
  btn.onclick = function () {
    var fn = _modalCallback;
    closeModal();
    if (typeof fn === 'function') fn();
  };
  overlay.classList.remove('hidden');
  requestAnimationFrame(function () { btn.focus(); });
}

function closeModal() {
  var overlay = $('#modal-overlay');
  if (overlay) overlay.classList.add('hidden');
  _modalCallback = null;
  if (_modalPreviousFocus && typeof _modalPreviousFocus.focus === 'function') {
    _modalPreviousFocus.focus();
  }
  _modalPreviousFocus = null;
}

function openAuthModal() {
  var overlay = $('#auth-overlay');
  if (overlay) overlay.classList.remove('hidden');
  AuthSession.render();
}

function closeAuthModal() {
  var overlay = $('#auth-overlay');
  if (overlay) overlay.classList.add('hidden');
  var errEl = $('#auth-error-msg');
  if (errEl) errEl.style.display = 'none';
}

var _authMode = 'login';
function switchAuthTab(mode) {
  _authMode = mode;
  var logTab = $('#auth-tab-login');
  var regTab = $('#auth-tab-register');
  var submitBtn = $('#auth-submit-btn');
  if (logTab) logTab.classList.toggle('active', mode === 'login');
  if (regTab) regTab.classList.toggle('active', mode === 'register');
  if (submitBtn) submitBtn.textContent = mode === 'login' ? 'Sign In' : 'Register';
}

async function submitAuth() {
  var username = $('#auth-username')?.value.trim();
  var password = $('#auth-password')?.value;
  var errEl = $('#auth-error-msg');
  if (errEl) errEl.style.display = 'none';

  if (!username || !password) {
    if (errEl) { errEl.textContent = 'Username and password required'; errEl.style.display = 'block'; }
    return;
  }

  try {
    if (_authMode === 'login') {
      await MichiAPI.login(username, password);
      showToast('Signed in successfully');
    } else {
      await MichiAPI.register(username, password);
      showToast('Registered and signed in');
    }
    closeAuthModal();
    await AuthSession.check();
    await bootstrapPublic();
    await bootstrapProtected();
  } catch (err) {
    if (errEl) {
      errEl.textContent = err.message || 'Authentication failed';
      errEl.style.display = 'block';
    }
  }
}

async function handleLogout() {
  try {
    await MichiAPI.logout();
    showToast('Signed out');
    closeAuthModal();
    AuthSession.setUnauthenticated();
    teardownProtected();
    await bootstrapPublic();
  } catch (e) {
    showToast(e.message, true);
  }
}

function openTrackDetailModal(idx) {
  var t = State.tracks[idx];
  if (!t) return;
  var overlay = $('#track-detail-modal');
  var body = $('#td-modal-body');
  if (!overlay || !body) return;

  body.innerHTML =
    '<div style="display:flex;gap:16px;align-items:flex-start;margin-bottom:12px">' +
    (t.artwork_id ? '<img src="/api/v1/artwork/' + t.artwork_id + '" alt="" style="width:80px;height:80px;border-radius:6px;object-fit:cover">' : '<div style="width:80px;height:80px;background:var(--bg-card);border-radius:6px;display:flex;align-items:center;justify-content:center;font-size:2rem">🎵</div>') +
    '<div><h3 style="margin:0 0 4px 0">' + esc(t.title || 'Unknown Title') + '</h3>' +
    '<p style="color:var(--text-2);margin:0">' + esc(t.artist || 'Unknown Artist') + '</p>' +
    '<p style="color:var(--text-3);font-size:.78rem;margin:2px 0 0 0">' + esc(t.album || 'Unknown Album') + '</p>' +
    '</div></div>' +
    '<div class="panel" style="margin-top:12px">' +
    '<div class="panel-row"><span class="panel-label">ID</span><span class="panel-mono">' + esc(t.id) + '</span></div>' +
    '<div class="panel-row"><span class="panel-label">Format</span><span>' + esc(t.format || 'Unknown') + '</span></div>' +
    '<div class="panel-row"><span class="panel-label">Duration</span><span>' + fmtDur(t.duration_ms) + '</span></div>' +
    '<div class="panel-row"><span class="panel-label">Sample Rate</span><span>' + (t.sample_rate ? t.sample_rate + ' Hz' : 'N/D') + '</span></div>' +
    '<div class="panel-row"><span class="panel-label">Bit Depth</span><span>' + (t.bit_depth ? t.bit_depth + '-bit' : 'N/D') + '</span></div>' +
    '<div class="panel-row"><span class="panel-label">Channels</span><span>' + (t.channels ? t.channels + ' ch' : 'N/D') + '</span></div>' +
    '<div class="panel-row"><span class="panel-label">Rating</span><span>' + (t.rating ? '★'.repeat(t.rating) : 'Unrated') + '</span></div>' +
    '<div class="panel-row"><span class="panel-label">Starred</span><span>' + (t.starred ? '⭐ Starred' : 'No') + '</span></div>' +
    '<div class="panel-row"><span class="panel-label">File Size</span><span>' + fmtBytes(t.file_size) + '</span></div>' +
    '<div class="panel-row"><span class="panel-label">Content Hash</span><span class="panel-mono">' + esc(t.content_hash || 'N/D') + '</span></div>' +
    '</div>';

  overlay.classList.remove('hidden');
}

function closeTrackDetailModal() {
  var overlay = $('#track-detail-modal');
  if (overlay) overlay.classList.add('hidden');
}

document.addEventListener('keydown', function (e) {
  if (e.key === 'Escape') {
    closeModal();
    closeAuthModal();
    closeTrackDetailModal();
  }
});

// ── Theme / Shell ───────────────────────────────────────────────
function resolvedTheme(theme) {
  if (theme === 'system') {
    return window.matchMedia('(prefers-color-scheme: light)').matches ? 'light' : 'dark';
  }
  return theme === 'light' ? 'light' : 'dark';
}

function setTheme(theme, persist) {
  var selected = ['dark', 'light', 'system'].includes(theme) ? theme : 'dark';
  document.documentElement.dataset.theme = resolvedTheme(selected);
  document.documentElement.dataset.themePreference = selected;
  document.querySelector('meta[name="theme-color"]')?.setAttribute(
    'content',
    resolvedTheme(selected) === 'light' ? '#EFF2F4' : '#090B10'
  );
  $$('.theme-options [data-theme-option]').forEach(function (button) {
    button.classList.toggle('active', button.dataset.themeOption === selected);
  });
  localStorage.setItem('michi_theme', selected);
  if (persist) saveSetting('theme', selected);
}

function toggleTheme() {
  var current = document.documentElement.dataset.theme;
  setTheme(current === 'light' ? 'dark' : 'light', true);
}

function toggleNavigation(force) {
  var open = typeof force === 'boolean' ? force : !document.body.classList.contains('nav-open');
  document.body.classList.toggle('nav-open', open);
  var button = $('#mobile-menu-btn');
  if (button) button.setAttribute('aria-expanded', String(open));
}

// ── Navigation ──────────────────────────────────────────────────
function showSection(section) {
  $$('.nav-item').forEach(n => {
    n.classList.remove('active');
    n.removeAttribute('aria-current');
  });
  const nav = $('.nav-item[data-section="' + section + '"]');
  if (nav) {
    nav.classList.add('active');
    nav.setAttribute('aria-current', 'page');
    var title = $('#current-section-title');
    var label = nav.querySelector('span');
    if (title && label) title.textContent = label.textContent;
  }
  $$('.section-page').forEach(p => {
    p.classList.add('hidden');
    p.classList.remove('fade-in');
  });
  const page = $('#page-' + section);
  if (page) {
    page.classList.remove('hidden');
    void page.offsetHeight;
    page.classList.add('fade-in');
  }
  toggleNavigation(false);

  if (AuthSession.state !== 'authenticated') return;

  // Lazy loaders for sections
  if (section === 'playlists') loadPlaylists();
  if (section === 'settings') { loadSettings(); setTimeout(loadRoomGroups, 100); }
  if (section === 'michilink') setTimeout(loadEcosystemDevices, 200);
  if (section === 'history') { _historyOffset = 0; loadHistory(); }
  if (section === 'chains') { _currentChainId = null; loadChains(); }
  if (section === 'broadcast') loadSources();
}

// ── Bootstrap Authority ──────────────────────────────────────────
async function bootstrapPublic() {
  await Promise.allSettled([loadStatus(), loadServerInfo()]);
}

async function bootstrapProtected() {
  if (AuthSession.state !== 'authenticated') {
    teardownProtected();
    return;
  }
  await Promise.allSettled([
    loadDashboard(),
    loadTracks(),
    loadCanonicalPlaybackState(),
    loadCanonicalQueue()
  ]);

  if (!State.polling) {
    State.polling = setInterval(function () {
      if (!document.hidden && AuthSession.state === 'authenticated') {
        loadStatus();
        loadDashboard();
      }
    }, 30000);
  }

  if (!ServerPlayback.pollTimer) {
    ServerPlayback.pollTimer = setInterval(function () {
      if (!document.hidden && AuthSession.state === 'authenticated' && ServerPlayback.outputTarget === 'server') {
        loadCanonicalPlaybackState();
      }
    }, 3000);
  }

  setupLiveEvents();
}

function teardownProtected() {
  if (State.polling) { clearInterval(State.polling); State.polling = null; }
  if (ServerPlayback.pollTimer) { clearInterval(ServerPlayback.pollTimer); ServerPlayback.pollTimer = null; }
  if (State.eventWs) {
    try { State.eventWs.close(); } catch (e) {}
    State.eventWs = null;
  }
  State.dashboard = null;
  State.tracks = [];
  State.allTracks = [];
  State.currentTrack = null;
  State.queue = [];
  renderDashboard();
  renderTracks([], 'tracks-table');
  renderTracks([], 'library-table');
  renderQueue([], 0);
}

async function init() {
  setTheme(localStorage.getItem('michi_theme') || 'dark', false);
  await loadI18n();
  updateServerUrlDisplay();
  showSection('dashboard');

  await bootstrapPublic();
  const authState = await AuthSession.check();
  if (authState === 'authenticated') {
    await bootstrapProtected();
  } else {
    teardownProtected();
  }

  if ('serviceWorker' in navigator) {
    navigator.serviceWorker.register('/sw.js').catch(function () {});
  }
}

document.addEventListener('DOMContentLoaded', init);

// ── Live Events (WebSocket / SSE) ────────────────────────────────
function setupLiveEvents() {
  if (State.eventWs) return;
  try {
    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    const wsUrl = protocol + '//' + window.location.host + '/api/v1/events';
    State.eventWs = new WebSocket(wsUrl);

    State.eventWs.onmessage = function (ev) {
      try {
        const data = JSON.parse(ev.data);
        if (data.type === 'playback_state_changed' || data.event === 'playback') {
          loadCanonicalPlaybackState();
        } else if (data.type === 'queue_changed' || data.event === 'queue') {
          loadCanonicalQueue();
        } else if (data.type === 'playlist_updated' || data.event === 'playlist') {
          loadPlaylists();
        } else if (data.type === 'scan_completed' || data.event === 'library') {
          loadDashboard();
          loadTracks();
        }
      } catch (e) {}
    };

    State.eventWs.onclose = function () {
      State.eventWs = null;
      if (AuthSession.state !== 'anonymous') {
        setTimeout(setupLiveEvents, 10000);
      }
    };
  } catch (e) {}
}

// ── Status & Connection State Machine ────────────────────────────
async function loadStatus() {
  try {
    State.status = await MichiAPI.status();
    ConnectionStatus.update(State.status);
    renderStatus();
    renderStatusPage();
  } catch (e) {
    ConnectionStatus.setOffline();
    renderOfflineStatus();
  }
}

function renderStatus() {
  const s = State.status;
  if (!s) { renderOfflineStatus(); return; }

  const sid = $('#sidebar-server-id');
  if (sid && s.server_id) sid.textContent = s.server_id.slice(0, 8) + '..';

  const suptime = $('#sidebar-uptime');
  if (suptime) {
    const h = Math.floor((s.uptime_seconds || 0) / 3600);
    const m = Math.floor(((s.uptime_seconds || 0) % 3600) / 60);
    suptime.textContent = (h || m) ? h + 'h ' + m + 'm' : '<1m';
  }

  const db = $('#sidebar-db-status');
  if (db) db.textContent = s.database === 'connected' ? 'OK' : 'ERR';
}

function renderOfflineStatus() {
  const db = $('#sidebar-db-status');
  if (db) db.textContent = 'ERR';
}

function renderStatusPage() {
  const container = $('#status-content');
  if (!container) return;
  const s = State.status;
  if (!s) {
    container.innerHTML = '<div class="empty-state"><p style="color:var(--error)">' + t('error.could_not_load_status') + '</p></div>';
    return;
  }
  container.innerHTML =
    '<div class="status-item"><div class="icon"><svg viewBox="0 0 24 24" fill="none" stroke="var(--online)" stroke-width="1.5"><circle cx="12" cy="12" r="10"/><path d="M12 16v-4"/><path d="M12 8h.01"/></svg></div><div class="info"><div class="label">Status</div><div class="value"><span class="badge ' + (s.status === 'ok' ? 'stable' : 'disabled') + '">' + esc(s.status) + '</span></div></div></div>' +
    '<div class="status-item"><div class="icon"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5"><path d="M22 12h-4l-3 9L9 3l-3 9H2"/></svg></div><div class="info"><div class="label">Service</div><div class="value">' + esc(s.name || 'Michi Micro Server') + '</div></div></div>' +
    '<div class="status-item"><div class="icon"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5"><circle cx="12" cy="12" r="10"/><path d="M12 6v6l4 2"/></svg></div><div class="info"><div class="label">Uptime</div><div class="value">' + fmtDur((s.uptime_seconds || 0) * 1000) + '</div></div></div>' +
    '<div class="status-item"><div class="icon"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5"><rect x="2" y="3" width="20" height="14" rx="2"/><path d="M8 21h8"/><path d="M12 17v4"/></svg></div><div class="info"><div class="label">Version</div><div class="value">' + esc(s.version || 'No disponible') + '</div></div></div>' +
    '<div class="status-item"><div class="icon"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5"><rect x="2" y="2" width="20" height="8" rx="2"/><rect x="2" y="14" width="20" height="8" rx="2"/></svg></div><div class="info"><div class="label">Database</div><div class="value"><span class="badge ' + (s.database === 'connected' ? 'stable' : 'disabled') + '">' + esc(s.database || 'No disponible') + '</span></div></div></div>' +
    '<div class="status-item"><div class="icon"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5"><path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2"/><circle cx="12" cy="7" r="4"/></svg></div><div class="info"><div class="label">Server ID</div><div class="value" style="font-family:var(--font-mono);font-size:.75rem">' + esc(s.server_id || 'No disponible') + '</div></div></div>' +
    '<div class="status-item"><div class="icon"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5"><path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/></svg></div><div class="info"><div class="label">Music Paths</div><div class="value">' + esc((s.music_paths || []).join(', ') || 'No disponible') + '</div></div></div>';
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

function hasServerFeature(featureName) {
  var info = State.serverInfo;
  if (!info || !info.features) return true;
  var feats = info.features;
  if (typeof feats === 'object' && !Array.isArray(feats)) {
    return feats[featureName] === true;
  }
  if (Array.isArray(feats)) {
    return feats.some(function (f) {
      return f === featureName || (f && f.name === featureName && f.enabled !== false);
    });
  }
  return false;
}

function featureBadge(enabled, meta) {
  if (meta?.future && !enabled) return { cls: 'experimental', text: 'EXP' };
  if (meta?.beta && enabled) return { cls: 'beta', text: 'BETA' };
  if (enabled) return { cls: 'stable', text: 'ON' };
  return { cls: 'disabled', text: 'OFF' };
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

// ── Dashboard ───────────────────────────────────────────────────
async function loadDashboard() {
  if (AuthSession.state !== 'authenticated') return;
  try {
    State.dashboard = await MichiAPI.dashboard();
    renderDashboard();
  } catch (e) { console.warn('dashboard failed:', e.message); }
}

function renderDashboard() {
  const d = State.dashboard;
  const cd = $('#dashboard-cards');
  if (!cd) return;

  if (!d) {
    cd.innerHTML = '<div class="empty-state"><div class="icon">📊</div><p><strong>' + (AuthSession.state !== 'authenticated' ? 'Sign in to access dashboard' : t('error.could_not_load_dashboard')) + '</strong></p></div>';
    return;
  }

  const lib = d.library || {};
  const health = d.health || {};
  const play = d.playback || {};

  const nowTitle = $('#dashboard-now-title');
  const nowDetail = $('#dashboard-now-detail');
  const nowState = $('#dashboard-now-state');
  if (nowTitle) nowTitle.textContent = play.has_current ? (play.title || 'Playing') : 'No track selected';
  if (nowDetail) nowDetail.textContent = play.has_current ? (play.artist || play.album || '') : 'Choose a track from your library to begin.';
  if (nowState) nowState.textContent = play.has_current ? (play.state || 'Playing') : 'Ready to listen';

  function fmtHours(ms) {
    if (!ms && ms !== 0) return '—';
    const h = Math.floor(ms / 3600000);
    return h + 'h';
  }

  function val(v) { return v !== null && v !== undefined ? v : '—'; }

  cd.innerHTML =
    '<div class="card" style="animation-delay:0ms"><div class="card-icon"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5"><path d="M9 18V5l12-2v13"/><circle cx="6" cy="18" r="3"/><circle cx="18" cy="16" r="3"/></svg></div><div class="card-value">' + val(lib.tracks) + '</div><div class="card-label">Tracks</div></div>' +
    '<div class="card" style="animation-delay:40ms"><div class="card-icon"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5"><path d="M3 4h18"/><rect x="3" y="8" width="18" height="12" rx="2"/></svg></div><div class="card-value">' + val(lib.albums) + '</div><div class="card-label">Albums</div></div>' +
    '<div class="card" style="animation-delay:80ms"><div class="card-icon"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5"><path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2"/><circle cx="12" cy="7" r="4"/></svg></div><div class="card-value">' + val(lib.artists) + '</div><div class="card-label">Artists</div></div>' +
    '<div class="card" style="animation-delay:120ms"><div class="card-icon"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5"><path d="M22 12h-4l-3 9L9 3l-3 9H2"/></svg></div><div class="card-value">' + fmtHours(lib.total_duration_ms) + '</div><div class="card-label">Duration</div></div>' +
    '<div class="card" style="animation-delay:160ms"><div class="card-icon"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5"><path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2"/><circle cx="12" cy="7" r="4"/></svg></div><div class="card-value">' + val(health.missing_files) + '</div><div class="card-label">Missing files ' + (health.missing_files !== undefined ? (health.missing_files === 0 ? '<span class="badge stable">Healthy</span>' : '<span class="badge error">Needs attention</span>') : '') + '</div></div>';

  var meta = $('#dashboard-meta');
  if (meta) {
    var statusText = {
      online: '● Online',
      degraded: '▲ Degraded',
      auth_required: '🔒 Auth Required',
      checking: '◌ Checking...',
      offline: '● Offline',
    }[ConnectionStatus.state] || '● Offline';
    meta.textContent = 'Server ' + statusText +
      ' · v' + (State.serverInfo?.version || State.status?.version || '?') +
      (lib.tracks !== undefined ? ' · ' + lib.tracks + ' tracks' : '');
  }
}

// ── Tracks & Library ────────────────────────────────────────────
async function loadTracks() {
  if (AuthSession.state !== 'authenticated') return;
  try {
    const raw = await MichiAPI.tracks({ limit: 100 });
    State.tracks = raw.tracks || [];
    State.allTracks = State.tracks;
    updateTracksCount();
    renderTracks(State.tracks, 'tracks-table');
    renderTracks(State.tracks, 'library-table');
  } catch (e) { console.warn('tracks failed:', e.message); }
}

function updateTracksCount() {
  const el1 = $('#tracks-count');
  const el2 = $('#library-meta');
  const total = State.dashboard?.library?.tracks ?? State.allTracks.length;
  const text = 'Showing ' + Math.min(State.tracks.length, 100) + ' of ' + total + ' tracks';
  if (el1) el1.textContent = text;
  if (el2) el2.textContent = total + ' tracks · ' + (State.dashboard?.library?.albums || '?') + ' albums · ' + (State.dashboard?.library?.artists || '?') + ' artists';
}

function renderTracks(tracks, tableId) {
  const container = $('#' + (tableId || 'tracks-table'));
  if (!container) return;

  if (!tracks || tracks.length === 0) {
    renderEmpty(container, '🎵', AuthSession.state !== 'authenticated' ? 'Authentication Required' : 'Library empty', AuthSession.state !== 'authenticated' ? 'Please sign in to view library.' : 'Scan or import music files to populate library.');
    return;
  }

  let html = '<table><thead><tr>' +
    '<th>#</th><th>Cover</th><th>Title</th><th>Artist</th><th>Album</th><th>Format</th><th>Rating</th><th>Duration</th><th></th>' +
    '</tr></thead><tbody>';

  tracks.slice(0, 100).forEach((t, i) => {
    const realIdx = State.tracks.indexOf(t);
    const coverHtml = t.artwork_id
      ? '<img src="/api/v1/artwork/' + t.artwork_id + '" alt="" style="width:32px;height:32px;border-radius:4px;object-fit:cover">'
      : '<span style="font-size:1rem">🎵</span>';

    const starIcon = t.starred ? '⭐' : '☆';
    const starsHtml = '<span class="star-rating" style="cursor:pointer" onclick="event.stopPropagation();toggleStar(' + realIdx + ')" title="Toggle Star">' + starIcon + '</span>';

    html += '<tr onclick="openTrackDetailModal(' + realIdx + ')" style="cursor:pointer">' +
      '<td style="color:var(--text-dim)">' + (i + 1) + '</td>' +
      '<td>' + coverHtml + '</td>' +
      '<td class="track-title">' + esc(t.title || 'Unknown') + '</td>' +
      '<td class="track-artist">' + esc(t.artist || '—') + '</td>' +
      '<td class="track-artist">' + esc(t.album || '—') + '</td>' +
      '<td><span class="badge format" data-format="' + esc(t.format || '').toLowerCase() + '">' + esc(t.format || '?') + '</span></td>' +
      '<td>' + starsHtml + '</td>' +
      '<td style="color:var(--text-dim)">' + fmtDur(t.duration_ms) + '</td>' +
      '<td style="white-space:nowrap">' +
      '<button class="btn btn-sm btn-ghost" onclick="event.stopPropagation();playTrack(' + realIdx + ')">Play</button>' +
      '<button class="btn btn-sm btn-ghost" onclick="event.stopPropagation();addToQueue(' + realIdx + ')" style="margin-left:4px" aria-label="Add to queue">+Q</button>' +
      '</td>' +
      '</tr>';
  });

  html += '</tbody></table>';
  container.innerHTML = html;
}

async function toggleStar(idx) {
  const t = State.tracks[idx];
  if (!t) return;
  const next = !t.starred;
  try {
    await MichiAPI.starTrack(t.id, next);
    t.starred = next;
    renderTracks(State.tracks, 'tracks-table');
    renderTracks(State.tracks, 'library-table');
    showToast(next ? 'Starred' : 'Unstarred');
  } catch (e) {
    showToast(e.message, true);
  }
}

// ── Search ──────────────────────────────────────────────────────
function isAdvancedQuery(q) {
  return /(artist|album|genre|format|year|rating):/.test(q);
}

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
    const raw = isAdvancedQuery(q)
      ? await MichiAPI.searchAdvanced(q)
      : await MichiAPI.search(q);
    State.tracks = raw.tracks || [];
    renderTracks(State.tracks, 'tracks-table');
    renderTracks(State.tracks, 'library-table');
    updateTracksCount();
    $('#tracks-count').textContent = State.tracks.length + ' results';
    showSection('library');
    showToast(t('toast.found_results', {n: State.tracks.length}));
  } catch (e) { showToast(e.message, true); }
}

document.addEventListener('keydown', (e) => {
  if (e.key === 'Enter' && e.target.id === 'search-input') handleSearch();
});

// ── Scan ────────────────────────────────────────────────────────
async function handleScan() {
  try {
    const r = await MichiAPI.scan();
    showToast(t('toast.scanned', {n: r.scanned, s: r.saved}));
    await Promise.all([loadDashboard(), loadTracks()]);
  } catch (e) { showToast(e.message, true); }
}

// ── Canonical Playback & Up Next Queue ───────────────────────────
var ServerPlayback = {
  state: 'paused',
  track_id: null,
  current_track: null,
  position_ms: 0,
  duration_ms: 0,
  volume: 80,
  shuffle: false,
  repeat: 'off',
  playing: false,
  outputTarget: 'browser', // 'browser' | 'server'
  remoteTargetName: null,  // Room or Chain name if selected
  pollTimer: null,
};

function toggleOutputTarget() {
  showOutputSelectorModal();
}

function updateOutputRoutingBadge() {
  var badge = $('#np-target-badge');
  if (!badge) return;
  if (ServerPlayback.outputTarget === 'browser') {
    badge.textContent = 'Output: This Browser';
    badge.className = 'badge format';
  } else if (ServerPlayback.remoteTargetName) {
    badge.textContent = 'Output: ' + ServerPlayback.remoteTargetName;
    badge.className = 'badge stable';
  } else {
    badge.textContent = 'Server: No output selected';
    badge.className = 'badge beta';
  }
}

async function showOutputSelectorModal() {
  closeOutputSelectorModal();
  try {
    var [recResp, groupResp, chainResp, curOut] = await Promise.all([
      MichiAPI.getReceivers().catch(function() { return { receivers: [] }; }),
      MichiAPI.getRoomGroups().catch(function() { return { groups: [] }; }),
      MichiAPI.getChains().catch(function() { return { chains: [] }; }),
      MichiAPI.getPlaybackOutput().catch(function() { return { output: null }; }),
    ]);

    var receivers = recResp.receivers || [];
    var groups = groupResp.groups || [];
    var chains = chainResp.chains || [];
    var activeSel = curOut.output;

    var modal = document.createElement('div');
    modal.className = 'modal-backdrop';
    modal.id = 'output-selector-modal';
    modal.style.display = 'flex';
    modal.innerHTML = '<div class="modal-card" style="max-width:440px">' +
      '<div class="modal-header"><h3>Select Playback Output</h3><button class="modal-close" onclick="closeOutputSelectorModal()">&times;</button></div>' +
      '<div class="modal-body" style="display:flex;flex-direction:column;gap:12px;max-height:60vh;overflow-y:auto">' +
        '<div style="font-weight:600;font-size:0.85rem;color:var(--text-2);margin-top:4px">Local Output</div>' +
        '<label style="display:flex;align-items:center;gap:8px;padding:8px;border-radius:6px;cursor:pointer;background:' + (ServerPlayback.outputTarget === 'browser' ? 'var(--bg-card-hover)' : 'transparent') + '">' +
          '<input type="radio" name="output-target" value="browser" ' + (ServerPlayback.outputTarget === 'browser' ? 'checked' : '') + ' onchange="selectLocalBrowserOutput()">' +
          '<div><div style="font-weight:500">This Browser (Web Audio)</div><div style="font-size:0.75rem;color:var(--text-3)">Play audio locally in this browser</div></div>' +
        '</label>' +
        '<div style="font-weight:600;font-size:0.85rem;color:var(--text-2);margin-top:8px">Receivers</div>' +
        (receivers.length === 0 ? '<div style="font-size:0.8rem;color:var(--text-3);padding:4px 8px">No paired receivers found</div>' : receivers.map(function(r) {
          var isSel = ServerPlayback.outputTarget === 'server' && activeSel && activeSel.kind === 'receiver' && activeSel.id === r.id;
          return '<label style="display:flex;align-items:center;gap:8px;padding:8px;border-radius:6px;cursor:pointer;background:' + (isSel ? 'var(--bg-card-hover)' : 'transparent') + '">' +
            '<input type="radio" name="output-target" value="receiver:' + r.id + '" ' + (isSel ? 'checked' : '') + ' onchange="selectServerOutputTarget(\'receiver\', \'' + r.id + '\', \'' + esc(r.name) + '\')">' +
            '<div><div style="font-weight:500">' + esc(r.name) + '</div><div style="font-size:0.75rem;color:var(--text-3)">' + esc(r.device_type || 'Receiver') + ' &bull; ' + (r.online ? 'Online' : 'Offline') + '</div></div>' +
          '</label>';
        }).join('')) +
        '<div style="font-weight:600;font-size:0.85rem;color:var(--text-2);margin-top:8px">Room Groups</div>' +
        (groups.length === 0 ? '<div style="font-size:0.8rem;color:var(--text-3);padding:4px 8px">No room groups defined</div>' : groups.map(function(g) {
          var isSel = ServerPlayback.outputTarget === 'server' && activeSel && activeSel.kind === 'room_group' && activeSel.id === g.id;
          return '<label style="display:flex;align-items:center;gap:8px;padding:8px;border-radius:6px;cursor:pointer;background:' + (isSel ? 'var(--bg-card-hover)' : 'transparent') + '">' +
            '<input type="radio" name="output-target" value="room_group:' + g.id + '" ' + (isSel ? 'checked' : '') + ' onchange="selectServerOutputTarget(\'room_group\', \'' + g.id + '\', \'' + esc(g.name) + '\')">' +
            '<div><div style="font-weight:500">' + esc(g.name) + '</div><div style="font-size:0.75rem;color:var(--text-3)">' + (g.receivers ? g.receivers.length : 0) + ' speakers</div></div>' +
          '</label>';
        }).join('')) +
        '<div style="font-weight:600;font-size:0.85rem;color:var(--text-2);margin-top:8px">Chains</div>' +
        (chains.length === 0 ? '<div style="font-size:0.8rem;color:var(--text-3);padding:4px 8px">No audio chains defined</div>' : chains.map(function(c) {
          var isSel = ServerPlayback.outputTarget === 'server' && activeSel && activeSel.kind === 'chain' && activeSel.id === c.id;
          return '<label style="display:flex;align-items:center;gap:8px;padding:8px;border-radius:6px;cursor:pointer;background:' + (isSel ? 'var(--bg-card-hover)' : 'transparent') + '">' +
            '<input type="radio" name="output-target" value="chain:' + c.id + '" ' + (isSel ? 'checked' : '') + ' onchange="selectServerOutputTarget(\'chain\', \'' + c.id + '\', \'' + esc(c.name) + '\')">' +
            '<div><div style="font-weight:500">' + esc(c.name) + '</div><div style="font-size:0.75rem;color:var(--text-3)">' + (c.receivers ? c.receivers.length : 0) + ' nodes</div></div>' +
          '</label>';
        }).join('')) +
      '</div>' +
      '<div class="modal-footer"><button class="btn btn-secondary" onclick="closeOutputSelectorModal()">Close</button></div>' +
    '</div>';

    document.body.appendChild(modal);
  } catch (e) {
    showToast('Failed to load output targets: ' + e.message, true);
  }
}

function closeOutputSelectorModal() {
  var el = $('#output-selector-modal');
  if (el) el.remove();
}

function selectLocalBrowserOutput() {
  ServerPlayback.outputTarget = 'browser';
  ServerPlayback.remoteTargetName = null;
  updateOutputRoutingBadge();
  closeOutputSelectorModal();
  showToast('Output: This Browser');
}

async function selectServerOutputTarget(kind, id, name) {
  try {
    await MichiAPI.setPlaybackOutput({ kind: kind, id: id });
    ServerPlayback.outputTarget = 'server';
    ServerPlayback.remoteTargetName = name;
    updateOutputRoutingBadge();
    closeOutputSelectorModal();
    showToast('Output: ' + name);
  } catch (e) {
    showToast('Failed to select output: ' + e.message, true);
  }
}

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
      showToast(t('error.playback_error', {msg: State.audio?.error?.message || 'unknown'}), true);
    };
  }
  return State.audio;
}

async function playTrack(idx) {
  const tracks = State.tracks;
  if (!tracks || idx < 0 || idx >= tracks.length) return;
  const t = tracks[idx];

  if (ServerPlayback.outputTarget === 'browser') {
    State.currentTrack = t;
    updateNowPlaying(t);
    updateMiniPlayer(t);
    const audio = getAudio();
    audio.src = MichiAPI.streamUrl(t.id);
    audio.play().catch(function (err) {
      showToast(t('error.could_not_play', {msg: err.message}), true);
    });
    updatePlayButtons();
    return;
  }

  // Canonical server playback
  try {
    await MichiAPI.playbackControl({
      command: 'play',
      track_id: t.id,
      position_ms: 0
    });
    await loadCanonicalPlaybackState();
  } catch (err) {
    showToast('Server playback failed: ' + err.message, true);
    if (err.message && err.message.includes('NO_OUTPUT_SELECTED')) {
      showOutputSelectorModal();
    }
  }
}

async function playPause() {
  if (ServerPlayback.outputTarget === 'browser') {
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
    return;
  }

  // Canonical server toggle
  try {
    await MichiAPI.playbackControl({ command: 'toggle' });
    await loadCanonicalPlaybackState();
  } catch (err) {
    showToast('Server playback control failed: ' + err.message, true);
    if (err.message && err.message.includes('NO_OUTPUT_SELECTED')) {
      showOutputSelectorModal();
    }
  }
}

async function toggleShuffle() {
  try {
    var nextVal = !ServerPlayback.shuffle;
    var resp = await MichiAPI.playbackControl({ command: 'shuffle', value: nextVal });
    ServerPlayback.shuffle = !!resp.shuffle;
    updatePlaybackControlsUI();
    showToast('Shuffle ' + (ServerPlayback.shuffle ? 'ON' : 'OFF'));
  } catch (e) {
    showToast('Failed to toggle shuffle: ' + e.message, true);
  }
}

async function toggleRepeat() {
  try {
    var nextMode = 'off';
    if (ServerPlayback.repeat === 'off') nextMode = 'all';
    else if (ServerPlayback.repeat === 'all') nextMode = 'one';
    else nextMode = 'off';

    var resp = await MichiAPI.playbackControl({ command: 'repeat', value: nextMode });
    ServerPlayback.repeat = resp.repeat || nextMode;
    updatePlaybackControlsUI();
    showToast('Repeat mode: ' + ServerPlayback.repeat.toUpperCase());
  } catch (e) {
    showToast('Failed to toggle repeat: ' + e.message, true);
  }
}

function updatePlaybackControlsUI() {
  var shuffleBtn = $('#btn-shuffle');
  if (shuffleBtn) {
    shuffleBtn.style.color = ServerPlayback.shuffle ? 'var(--primary)' : 'var(--text-3)';
  }
  var repeatBtn = $('#btn-repeat');
  if (repeatBtn) {
    repeatBtn.style.color = (ServerPlayback.repeat && ServerPlayback.repeat !== 'off') ? 'var(--primary)' : 'var(--text-3)';
  }
  updateOutputRoutingBadge();
}

async function addToQueue(idx) {
  const tracks = State.tracks;
  if (!tracks || idx < 0 || idx >= tracks.length) return;
  const t = tracks[idx];
  try {
    await MichiAPI.addQueueItems([t.id]);
    await loadCanonicalQueue();
    showToast('Added to Up Next');
  } catch (e) {
    showToast('Failed to add to queue: ' + e.message, true);
  }
}

async function loadCanonicalQueue() {
  if (AuthSession.state !== 'authenticated') return;
  try {
    const raw = await MichiAPI.queue();
    const items = raw.items || [];
    State.queue = items;
    renderQueue(items, raw.current_index || 0);
  } catch (e) {}
}

function renderQueue(items, currentIndex) {
  const container = $('#queue-content');
  if (!container) return;
  if (!items || items.length === 0) {
    container.innerHTML = '<p class="queue-empty" data-i18n="queue_empty">Queue empty</p>';
    return;
  }

  container.innerHTML = items.map((item, idx) => {
    const isCurrent = idx === currentIndex;
    return '<div class="queue-item ' + (isCurrent ? 'active' : '') + '" style="display:flex;align-items:center;justify-content:space-between;padding:6px 0;border-bottom:1px solid var(--border-subtle);font-size:.78rem">' +
      '<div style="flex:1;overflow:hidden;text-overflow:ellipsis;white-space:nowrap">' +
      (isCurrent ? '<strong style="color:var(--primary)">▶ ' : '') +
      esc(item.title || item.track_id) +
      (isCurrent ? '</strong>' : '') +
      '</div>' +
      '<button class="btn btn-sm btn-ghost" onclick="jumpToQueueItem(' + idx + ')" style="padding:2px 6px">Jump</button>' +
      '</div>';
  }).join('');
}

async function jumpToQueueItem(pos) {
  try {
    await MichiAPI.jumpQueue(pos);
    await loadCanonicalQueue();
    await loadCanonicalPlaybackState();
  } catch (e) {
    showToast(e.message, true);
  }
}

async function loadCanonicalPlaybackState() {
  if (AuthSession.state !== 'authenticated') return;
  try {
    const st = await MichiAPI.playbackState();
    ServerPlayback.state = st.state;
    ServerPlayback.playing = st.playing;
    ServerPlayback.track_id = st.track_id;
    ServerPlayback.current_track = st.current_track;
    ServerPlayback.position_ms = st.position_ms || 0;
    ServerPlayback.duration_ms = st.duration_ms || 0;
    ServerPlayback.volume = st.volume || 80;
    ServerPlayback.shuffle = !!st.shuffle;
    ServerPlayback.repeat = st.repeat || 'off';

    if (st.current_track) {
      State.currentTrack = st.current_track;
      updateNowPlaying(st.current_track);
      updateMiniPlayer(st.current_track);
    }
    updatePlaybackControlsUI();
    updatePlayButtons();
    updatePlaybackProgress();
  } catch (e) {}
}

function onTrackEnd() {
  if (State.currentPodcastEpisode && State.currentPodcastEpisode.id) {
    var epId = State.currentPodcastEpisode.id;
    var audio = getAudio();
    var posMs = audio ? Math.floor(audio.currentTime * 1000) : 0;
    MichiAPI.updateEpisode(epId, posMs, true).catch(function() {});
    State.currentPodcastEpisode = null;
  }
  showToast(t('toast.track_ended'));
}

var _podcastProgressDebounce = null;
function updatePlaybackProgress() {
  if (ServerPlayback.outputTarget === 'server') {
    if (!ServerPlayback.duration_ms) return;
    const pct = (ServerPlayback.position_ms / ServerPlayback.duration_ms) * 100;
    const fill1 = $('#np-progress-fill');
    const fill2 = $('#mini-progress-fill');
    if (fill1) fill1.style.width = pct + '%';
    if (fill2) fill2.style.width = pct + '%';

    const cur = $('#np-current');
    if (cur) cur.textContent = fmtDur(ServerPlayback.position_ms);
    return;
  }

  const audio = getAudio();
  if (!audio || !audio.duration) return;

  const pct = (audio.currentTime / audio.duration) * 100;
  const fill1 = $('#np-progress-fill');
  const fill2 = $('#mini-progress-fill');
  if (fill1) fill1.style.width = pct + '%';
  if (fill2) fill2.style.width = pct + '%';

  const cur = $('#np-current');
  if (cur) cur.textContent = fmtDur(audio.currentTime * 1000);

  if (State.currentPodcastEpisode && State.currentPodcastEpisode.id) {
    if (!_podcastProgressDebounce) {
      _podcastProgressDebounce = setTimeout(function() {
        _podcastProgressDebounce = null;
        if (State.currentPodcastEpisode && State.currentPodcastEpisode.id) {
          MichiAPI.updateEpisode(
            State.currentPodcastEpisode.id,
            Math.floor(audio.currentTime * 1000),
            false
          ).catch(function() {});
        }
      }, 5000);
    }
  }
}

function updatePlayButtons() {
  const isPlaying = ServerPlayback.outputTarget === 'server'
    ? ServerPlayback.playing
    : !getAudio().paused;

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
      ? '<img src="/api/v1/artwork/' + t.artwork_id + '" alt="">'
      : '🎵';
  }
  const title = $('#np-title');
  const artist = $('#np-artist');
  if (title) title.textContent = t.title || 'Unknown';
  if (artist) artist.textContent = (t.artist || 'Unknown') + (t.album ? ' — ' + t.album : '');
  const fmt = $('#np-format-badge');
  if (fmt) {
    if (t.format) {
      fmt.style.display = 'inline-block';
      fmt.className = 'badge format';
      fmt.setAttribute('data-format', (t.format || '').toLowerCase());
      fmt.textContent = t.format;
    } else {
      fmt.style.display = 'none';
    }
  }
  const src = $('#np-source-badge');
  if (src) {
    src.style.display = 'inline-block';
    src.className = 'badge stable';
    src.textContent = t.album ? 'Album' : 'Track';
  }
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
      ? '<img src="/api/v1/artwork/' + t.artwork_id + '" alt="">'
      : '🎵';
  }
  const title = $('#minibar-title');
  const artist = $('#minibar-artist');
  if (title) title.textContent = t.title || 'Unknown';
  if (artist) artist.textContent = t.artist || '—';
}

// ── Michi Link & Ecosystem ──────────────────────────────────────
async function testMichiLink() {
  const btn = $('#page-michilink button[onclick="testMichiLink()"]');
  if (btn) { btn.disabled = true; btn.textContent = 'Testing...'; }
  try {
    const [infoRes, capsRes] = await Promise.allSettled([
      MichiAPI.serverInfo(),
      MichiAPI.capabilities(),
    ]);

    const infoOk = infoRes.status === 'fulfilled' && infoRes.value && !infoRes.value.error;
    const capsOk = capsRes.status === 'fulfilled' && capsRes.value && !capsRes.value.error;

    if (infoOk && capsOk) {
      showToast('Connection verified: CONNECTED / HEALTHY');
      await Promise.allSettled([loadStatus(), loadServerInfo(), loadEcosystemDevices()]);
    } else {
      showToast('Connection degraded', true);
    }
  } catch (err) {
    showToast('Connection test failed: ' + (err.message || 'Error'), true);
  } finally {
    if (btn) { btn.disabled = false; btn.textContent = 'Test Connection'; }
  }
}

async function loadEcosystemDevices() {
  if (AuthSession.state !== 'authenticated') return;
  try {
    var raw = await MichiAPI.linkDevices();
    var devices = raw.devices || [];
    var container = $('#ecosystem-devices');
    if (!container) return;
    if (devices.length === 0) {
      container.innerHTML = '<p style="color:var(--text-dim);font-size:.78rem;padding:8px 0">' + t('empty.no_devices') + '.</p>';
      return;
    }
    var typeIcons = { mobile: '📱', desktop: '💻', player: '🎵', receiver: '📡', server: '🖥️', default: '📱' };
    container.innerHTML = devices.map(function (d) {
      var icon = typeIcons[d.device_type] || typeIcons.default;
      var devId = d.device_id || '';
      var status = d.online
        ? '<span class="badge stable" style="font-size:.6rem">ONLINE</span>'
        : '<span class="badge disabled" style="font-size:.6rem">OFFLINE</span>';
      return '<div class="chain-item" style="cursor:default;padding:10px 14px">' +
        '<div style="font-size:1.1rem">' + icon + '</div>' +
        '<div class="info"><div class="name">' + esc(d.alias || 'Unknown') + ' ' + status + '</div>' +
        '<div class="meta">' + esc(d.device_type) + (d.device_model ? ' · ' + esc(d.device_model) : '') + '</div></div>' +
        '<button class="btn btn-sm btn-ghost" onclick="revokeDevice(\'' + esc(devId) + '\')">Revoke</button>' +
        '</div>';
    }).join('');
  } catch (e) { console.warn('ecosystem:', e.message); }
}

async function revokeDevice(deviceId) {
  if (!deviceId || deviceId === 'undefined') {
    showToast('Invalid device ID', true);
    return;
  }
  showModal('Revoke Device', 'Revoke access for this device? It will need to be re-paired.', 'Revoke', async function () {
    try {
      await MichiAPI.revokeLinkDevice(deviceId);
      showToast('Device revoked');
      loadEcosystemDevices();
    } catch (e) {
      showToast('Failed to revoke device: ' + e.message, true);
    }
  });
}

// ── QR Pairing ───────────────────────────────────────────────────
var _qrCode = null;
var _qrExpiresAt = null;
var _qrTimer = null;
var _qrState = 'IDLE';

async function generateQR() {
  _qrState = 'GENERATING';
  try {
    var originUrl = window.location.origin;
    var resp = await MichiAPI.qrPair(originUrl);
    _qrCode = resp.qr_code;
    _qrExpiresAt = new Date(resp.expires_at);
    _qrState = 'WAITING_FOR_SCAN';
    renderQR(resp);
    pollQRStatus();
  } catch (e) {
    _qrState = 'ERROR';
    showToast(e.message, true);
  }
}

function renderQR(resp) {
  var container = $('#qr-code-container');
  var empty = $('#qr-empty');
  var svgEl = $('#qr-svg');
  var connected = $('#qr-connected');
  if (container) container.classList.remove('hidden');
  if (empty) empty.classList.add('hidden');
  if (connected) connected.classList.add('hidden');

  if (svgEl) {
    svgEl.innerHTML = '<img src="' + resp.svg_url + '" alt="QR Code" style="width:100%;height:100%">';
  }
  updateQRCountdown();
}

function updateQRCountdown() {
  var el = $('#qr-countdown');
  if (!el || !_qrExpiresAt) return;
  var now = new Date();
  var diff = Math.max(0, Math.floor((_qrExpiresAt - now) / 1000));
  if (diff <= 0) {
    el.textContent = 'Expired';
    return;
  }
  var m = Math.floor(diff / 60);
  var s = diff % 60;
  el.textContent = 'Expires in ' + m + 'm ' + s + 's';
}

function pollQRStatus() {
  if (_qrTimer) clearInterval(_qrTimer);
  _qrTimer = setInterval(async function () {
    if (!_qrCode || _qrState !== 'WAITING_FOR_SCAN') { clearInterval(_qrTimer); return; }
    updateQRCountdown();
    if (_qrExpiresAt && new Date() > _qrExpiresAt) {
      clearInterval(_qrTimer);
      _qrState = 'EXPIRED';
      var el = $('#qr-countdown');
      if (el) el.textContent = 'Expired';
      showToast(t('error.qr_expired'), true);
      return;
    }

    try {
      var st = await MichiAPI.qrStatus(_qrCode);
      if (st.status === 'claimed' || st.claimed === true) {
        clearInterval(_qrTimer);
        _qrState = 'CLAIMED';
        var connected = $('#qr-connected');
        var svgEl = $('#qr-svg');
        var countdown = $('#qr-countdown');
        if (svgEl) svgEl.innerHTML = '';
        if (countdown) countdown.textContent = 'Paired successfully';
        if (connected) connected.classList.remove('hidden');
        showToast('Device paired successfully');
        loadEcosystemDevices();
      }
    } catch (e) {}
  }, 1500);
}

function resetQR() {
  _qrCode = null;
  _qrExpiresAt = null;
  _qrState = 'IDLE';
  if (_qrTimer) { clearInterval(_qrTimer); _qrTimer = null; }
  var container = $('#qr-code-container');
  var empty = $('#qr-empty');
  var connected = $('#qr-connected');
  if (container) container.classList.add('hidden');
  if (empty) empty.classList.remove('hidden');
  if (connected) connected.classList.add('hidden');
}

function updateServerUrlDisplay() {
  const inp = $('#server-url-input');
  if (!inp) return;
  inp.value = window.location.origin;
}

function copyServerUrl() {
  const inp = $('#server-url-input');
  if (!inp) return;
  if (!inp.value) updateServerUrlDisplay();
  navigator.clipboard.writeText(inp.value).then(() => {
    showToast(t('toast.copied'));
  }).catch(() => {
    inp.select();
    document.execCommand('copy');
    showToast(t('toast.copied'));
  });
}

// ── Playlists ────────────────────────────────────────────────────
async function loadPlaylists() {
  if (AuthSession.state !== 'authenticated') return;
  try {
    const raw = await MichiAPI.playlists();
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
    renderEmpty(container, '📋', t('empty.no_playlists'), '');
    return;
  }
  container.innerHTML = playlists.map(function (p) {
    return '<div class="playlist-item" onclick="openPlaylistTracks(\'' + p.id + '\',\'' + esc(p.name) + '\')">' +
      '<div class="info"><div class="name">' + esc(p.name) + '</div>' +
      (p.description ? '<div class="desc">' + esc(p.description) + '</div>' : '') +
      '</div>' +
      '<div class="meta" style="display:flex;gap:6px;align-items:center">' +
      '<span>' + (p.track_count || 0) + ' tracks</span>' +
      '<button class="btn btn-sm btn-ghost" onclick="event.stopPropagation();exportPlaylistM3U(\'' + p.id + '\')">M3U</button>' +
      '<button class="btn btn-sm btn-ghost" onclick="event.stopPropagation();confirmDeletePlaylist(\'' + p.id + '\',\'' + esc(p.name) + '\')">✕</button>' +
      '</div>' +
      '</div>';
  }).join('');
}

async function openPlaylistTracks(id, name) {
  try {
    const raw = await MichiAPI.playlistTracks(id);
    const tracks = raw.tracks || [];
    State.tracks = tracks;
    renderTracks(tracks, 'library-table');
    showSection('library');
    showToast('Viewing playlist: ' + name + ' (' + tracks.length + ' tracks)');
  } catch (e) {
    showToast('Failed to load playlist: ' + e.message, true);
  }
}

function exportPlaylistM3U(id) {
  window.open('/api/v1/playlists/' + id + '/export/m3u', '_blank');
}

function confirmDeletePlaylist(id, name) {
  showModal('Delete Playlist', 'Are you sure you want to delete playlist "' + name + '"?', 'Delete', async function () {
    try {
      await MichiAPI.deletePlaylist(id);
      showToast('Playlist deleted');
      loadPlaylists();
    } catch (e) {
      showToast('Failed to delete playlist: ' + e.message, true);
    }
  });
}

function renderSmartList(playlists) {
  const container = $('#smart-playlists-list');
  if (!container) return;
  if (!playlists || playlists.length === 0) {
    container.innerHTML = '<p style="color:var(--text-dim);font-size:.82rem;padding:12px 0">' + t('empty.no_playlists') + '</p>';
    return;
  }
  container.innerHTML = playlists.map(function (p) {
    return '<div class="playlist-item" onclick="openPlaylistTracks(\'' + p.id + '\',\'' + esc(p.name) + '\')">' +
      '<div class="info"><div class="name">' + esc(p.name) + '</div>' +
      '<div class="desc">' + esc(p.description || '') + '</div></div>' +
      '<div class="meta">' + (p.track_count || 0) + ' tracks</div>' +
      '</div>';
  }).join('');
}

function switchPlaylistTab(tab) {
  $$('.tab[data-tab]').forEach(function (b) { b.classList.remove('active'); });
  var btn = $('.tab[data-tab="' + tab + '"]');
  if (btn) btn.classList.add('active');
  $$('[id^="playlist-tab-"]').forEach(function (t) { t.classList.add('hidden'); });
  var pane = $('#playlist-tab-' + tab);
  if (pane) pane.classList.remove('hidden');

  if (tab === 'browse') loadPlaylists();
}

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
  if (!name) { showToast(t('error.please_enter_name'), true); return; }

  var params = {};
  if (rule === 'by_genre' || rule === 'by_year') {
    var pval = $('#smart-param')?.value.trim();
    if (!pval) { showToast(t('error.please_enter_value'), true); return; }
    params[rule === 'by_genre' ? 'genre' : 'year'] = rule === 'by_year' ? parseInt(pval) || 2024 : pval;
  }
  params.limit = limit;

  try {
    await MichiAPI.smartPlaylist(name, rule, params);
    showToast(t('toast.created'));
    $('#smart-name').value = '';
    loadPlaylists();
    switchPlaylistTab('browse');
  } catch (e) { showToast(e.message, true); }
}

// ── History ──────────────────────────────────────────────────────
let _historyOffset = 0;
const _historyLimit = 50;

async function loadHistory() {
  if (AuthSession.state !== 'authenticated') return;
  try {
    const [list, stats] = await Promise.all([
      MichiAPI.history({ limit: _historyLimit, offset: _historyOffset }),
      MichiAPI.historyStats(),
    ]);
    renderHistory(list);
    renderHistoryStats(stats);
  } catch (e) { console.warn('history failed:', e.message); }
}

function renderHistoryStats(stats) {
  const el = $('#history-stats');
  if (!el) return;
  el.innerHTML =
    '<div class="history-stat"><div class="stat-value">' + (stats?.total || 0) + '</div><div class="stat-label">Total Plays</div></div>' +
    '<div class="history-stat"><div class="stat-value">' + (stats?.unique_tracks || 0) + '</div><div class="stat-label">Unique Tracks</div></div>' +
    '<div class="history-stat"><div class="stat-value">' + (stats?.today || 0) + '</div><div class="stat-label">Today</div></div>' +
    '<div class="history-stat"><div class="stat-value">' + (stats?.this_week || 0) + '</div><div class="stat-label">This Week</div></div>' +
    '<div class="history-stat"><div class="stat-value">' + (stats?.this_month || 0) + '</div><div class="stat-label">This Month</div></div>';
}

function renderHistory(list) {
  const container = $('#history-table');
  if (!container) return;
  const entries = list?.history || [];
  if (entries.length === 0) {
    renderEmpty(container, '🕐', t('empty.no_history'), '');
    return;
  }
  let html = '<table><thead><tr><th>#</th><th>Title</th><th>Artist</th><th>Album</th><th>Played At</th></tr></thead><tbody>';
  entries.forEach(function (e, i) {
    html += '<tr>' +
      '<td style="color:var(--text-dim)">' + (_historyOffset + i + 1) + '</td>' +
      '<td class="track-title">' + esc(e.title) + '</td>' +
      '<td class="track-artist">' + esc(e.artist || '—') + '</td>' +
      '<td class="track-artist">' + esc(e.album || '—') + '</td>' +
      '<td style="color:var(--text-dim);font-size:.75rem">' + fmtDate(e.played_at) + '</td>' +
      '</tr>';
  });
  html += '</tbody></table>';
  container.innerHTML = html;

  const total = list?.total || 0;
  const pages = Math.ceil(total / _historyLimit);
  const pag = $('#history-pagination');
  if (pag) {
    let ph = '';
    if (_historyOffset > 0) {
      ph += '<button class="btn btn-sm btn-ghost" onclick="historyPage(' + Math.max(0, _historyOffset - _historyLimit) + ')">Prev</button>';
    }
    ph += '<span style="color:var(--text-dim);font-size:.75rem;padding:0 8px">Page ' + (Math.floor(_historyOffset / _historyLimit) + 1) + ' of ' + (pages || 1) + '</span>';
    if (_historyOffset + _historyLimit < total) {
      ph += '<button class="btn btn-sm btn-ghost" onclick="historyPage(' + (_historyOffset + _historyLimit) + ')">Next</button>';
    }
    pag.innerHTML = ph;
  }
}

function historyPage(offset) {
  _historyOffset = offset;
  loadHistory();
}

async function exportHistory() {
  try {
    const data = await MichiAPI.exportHistory();
    const blob = new Blob([JSON.stringify(data, null, 2)], { type: 'application/json' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = 'michi-history-' + new Date().toISOString().slice(0, 10) + '.json';
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
    URL.revokeObjectURL(url);
    showToast(t('toast.saved'));
  } catch (e) { showToast(e.message, true); }
}

async function clearHistory() {
  showModal('Clear History', 'Clear all play history? This cannot be undone.', 'Clear', async function () {
    try {
      await MichiAPI.clearHistory();
      _historyOffset = 0;
      loadHistory();
      showToast(t('toast.deleted'));
    } catch (e) { showToast(e.message, true); }
  });
}

// ── Room Groups ──────────────────────────────────────────────────
async function loadRoomGroups() {
  if (AuthSession.state !== 'authenticated') return;
  try {
    var raw = await MichiAPI.roomGroups();
    var groups = raw.groups || [];
    var container = $('#room-groups-list');
    if (!container) return;
    if (groups.length === 0) {
      container.innerHTML = '<div class="empty-state"><p><strong>' + t('empty.no_groups') + '</strong></p></div>';
      return;
    }
    container.innerHTML = groups.map(function (g) {
      var modeIcons = { party: '🔥', relax: '🌙', custom: '⚙' };
      var icon = modeIcons[g.mode] || '📡';
      var status = g.active
        ? '<span class="badge stable">Active</span>'
        : '<span class="badge disabled">Inactive</span>';
      return '<div class="chain-item" style="cursor:default">' +
        '<div style="font-size:1.2rem">' + icon + '</div>' +
        '<div class="info"><div class="name">' + esc(g.name) + ' ' + status + '</div>' +
        '<div class="meta">' + g.receiver_ids.length + ' receivers · Mode: ' + g.mode + '</div></div>' +
        '<div style="display:flex;gap:4px">' +
        (g.active
          ? '<button class="btn btn-sm btn-ghost" onclick="deactivateRoomGroup(\'' + g.id + '\')">Stop</button>'
          : '<button class="btn btn-sm btn-primary" onclick="activateRoomGroup(\'' + g.id + '\')">Play</button>') +
        '<button class="btn btn-sm btn-ghost" onclick="deleteRoomGroup(\'' + g.id + '\')">✕</button>' +
        '</div></div>';
    }).join('');
  } catch (e) { console.warn('room groups:', e.message); }
}

async function createRoomGroup() {
  var name = $('#rg-name')?.value.trim();
  var mode = $('#rg-mode')?.value;
  var recvs = $('#rg-receivers')?.value.trim();
  if (!name) { showToast(t('error.please_enter_name'), true); return; }
  var ids = recvs ? recvs.split(',').map(function (s) { return s.trim(); }).filter(Boolean) : [];
  try {
    await MichiAPI.createRoomGroup({ name, mode, receiver_ids: ids });
    $('#rg-name').value = '';
    $('#rg-receivers').value = '';
    loadRoomGroups();
    showToast(t('toast.created'));
  } catch (e) { showToast(e.message, true); }
}

async function activateRoomGroup(id) {
  try {
    var resp = await MichiAPI.activateRoomGroup(id);
    loadRoomGroups();
    if (resp.status === 'already_active') {
      showToast('Room group is already active');
    } else if (resp.status === 'active') {
      showToast('Room group active (' + resp.successful_receivers + '/' + resp.total_receivers + ' receivers)');
    } else if (resp.status === 'partial') {
      showToast('Room group partially active (' + resp.successful_receivers + '/' + resp.total_receivers + ' receivers)', true);
    } else {
      showToast('Failed to activate receivers in room group', true);
    }
  } catch (e) {
    showToast('Failed to activate room group: ' + e.message, true);
    loadRoomGroups();
  }
}

async function deactivateRoomGroup(id) {
  try {
    await MichiAPI.deactivateRoomGroup(id);
    loadRoomGroups();
    showToast(t('toast.deactivated'));
  } catch (e) { showToast(e.message, true); }
}

async function deleteRoomGroup(id) {
  try {
    await MichiAPI.deleteRoomGroup(id);
    loadRoomGroups();
    showToast(t('toast.deleted'));
  } catch (e) { showToast(e.message, true); }
}

// ── Broadcast & Sources ──────────────────────────────────────────
async function loadSources() {
  if (AuthSession.state !== 'authenticated') return;
  try {
    var raw = await MichiAPI.sources();
    var sources = raw.sources || [];
    var container = $('#sources-list');
    var count = $('#sources-count');
    if (count) count.textContent = sources.length + ' source(s)';
    if (!container) return;
    if (sources.length === 0) {
      container.innerHTML = '<div class="empty-state"><p><strong>' + t('empty.no_sources') + '</strong></p></div>';
      return;
    }
    container.innerHTML = sources.map(function (s) {
      var typeIcons = { radio: '📻', podcast: '🎙️', directfile: '🎵', unknown: '❓' };
      var icon = typeIcons[s.stream_type] || '📡';
      var statusDot = s.enabled ? '<span class="feature-dot on"></span>' : '<span class="feature-dot off"></span>';
      var playUrl = (s.stream_type === 'podcast')
        ? '<button class="btn btn-sm btn-ghost" onclick="showEpisodes(\'' + s.id + '\',\'' + esc(s.name || s.url) + '\')">Episodes</button>'
        : '<button class="btn btn-sm btn-ghost" onclick="playSource(\'' + s.id + '\')">▶ Play</button>';
      return '<div class="chain-item" style="cursor:default">' +
        '<div style="font-size:1.3rem">' + icon + '</div>' +
        '<div class="info"><div class="name">' + statusDot + ' ' + esc(s.name || s.url) + '</div>' +
        '<div class="meta">' + s.stream_type + (s.codec ? ' · ' + s.codec : '') + (s.genre ? ' · ' + s.genre : '') + '</div></div>' +
        '<div style="display:flex;gap:4px">' +
        playUrl +
        '<button class="btn btn-sm btn-ghost" onclick="deleteSource(\'' + s.id + '\')">✕</button>' +
        '</div></div>';
    }).join('');
  } catch (e) { console.warn('sources:', e.message); }
}

async function addSource() {
  var url = $('#source-url-input')?.value.trim();
  if (!url) { showToast(t('error.please_enter_url'), true); return; }
  try {
    var resp = await MichiAPI.addSource(url);
    $('#source-url-input').value = '';
    var el = $('#source-add-result');
    var msg = '✓ Added: ' + esc(resp.source?.name || resp.source?.stream_type || 'source');
    if (resp.feed_status === 'fetch_failed') {
      msg += ' (Feed fetch failed or unreachable)';
    } else if (resp.episodes_imported !== undefined) {
      msg += ' (' + resp.episodes_imported + ' episodes imported' + (resp.episodes_failed > 0 ? ', ' + resp.episodes_failed + ' failed' : '') + ')';
    }
    if (el) el.innerHTML = '<span style="color:var(--green)">' + msg + '</span>';
    loadSources();
    showToast(t('toast.created'));
  } catch (e) { showToast(e.message, true); }
}

async function deleteSource(id) {
  try {
    await MichiAPI.deleteSource(id);
    loadSources();
    showToast(t('toast.deleted'));
  } catch (e) { showToast(e.message, true); }
}
function playSource(id) {
  var audio = getAudio();
  audio.src = '/api/v1/stream/proxy/' + id;
  audio.play().then(function() {
    State.currentPodcastEpisode = null;
    State.currentTrack = { title: 'Radio Stream (This Browser)', artist: 'Broadcast', id: id, duration_ms: 0 };
    updateNowPlaying(State.currentTrack);
    updateMiniPlayer(State.currentTrack);
    updatePlayButtons();
  }).catch(function (err) {
    showToast(t('error.could_not_play', {msg: err.message}), true);
  });
}

async function showEpisodes(sourceId, sourceName) {
  var view = $('#episode-view');
  var nameEl = $('#episode-source-name');
  if (view) view.classList.remove('hidden');
  if (nameEl) nameEl.textContent = 'Episodes: ' + sourceName;
  try {
    var raw = await MichiAPI.sourceEpisodes(sourceId);
    var eps = raw.episodes || [];
    var container = $('#episodes-list');
    if (!container) return;
    if (eps.length === 0) {
      container.innerHTML = '<p style="color:var(--text-dim);font-size:.82rem;padding:12px 0">' + t('empty.no_episodes') + '</p>';
      return;
    }
    container.innerHTML = eps.map(function (ep) {
      var dur = ep.duration_secs ? Math.floor(ep.duration_secs / 60) + 'm' : '--';
      var pos = ep.position_ms > 0 ? ' · ' + Math.floor(ep.position_ms / 1000) + 's' : '';
      return '<div class="chain-item" style="cursor:default">' +
        '<div style="font-size:1rem">🎙️</div>' +
        '<div class="info"><div class="name">' + esc(ep.title) + '</div>' +
        '<div class="meta">' + dur + pos + (ep.played ? ' · Played' : '') + '</div></div>' +
        '<button class="btn btn-sm btn-ghost" title="Play in Browser" onclick="playEpisode(\'' + ep.id + '\', ' + (ep.position_ms || 0) + ')">▶ Browser</button>' +
        '</div>';
    }).join('');
  } catch (e) { showToast(e.message, true); }
}

var _episodeProgressTimeout = null;
function playEpisode(episodeId, resumePositionMs) {
  var audio = getAudio();
  audio.src = '/api/v1/stream/proxy/episode/' + episodeId;
  return audio.play().then(function() {
    State.currentPodcastEpisode = { id: episodeId };
    State.currentTrack = { title: 'Podcast Episode (This Browser)', artist: 'Podcast', id: episodeId, duration_ms: 0 };
    if (resumePositionMs && resumePositionMs > 0) {
      audio.currentTime = resumePositionMs / 1000;
    }
    updateNowPlaying(State.currentTrack);
    updateMiniPlayer(State.currentTrack);
    updatePlayButtons();
  }).catch(function (err) {
    showToast(t('error.could_not_play', {msg: err.message}), true);
    throw err;
  });
}

// ── Sync & Handoff Handlers ─────────────────────────────────────
async function uploadFile() {
  var fileInput = $('#settings-file-input');
  var progressWrap = $('#upload-progress-wrap');
  var progressFill = $('#upload-progress-fill');
  var progressText = $('#upload-progress-text');

  if (!fileInput || !fileInput.files || fileInput.files.length === 0) {
    showToast('Please select a file to upload', true);
    return;
  }
  var file = fileInput.files[0];
  if (progressWrap) progressWrap.classList.remove('hidden');
  if (progressFill) progressFill.style.width = '0%';
  if (progressText) progressText.textContent = 'Preparing upload for: ' + file.name;

  try {
    // 1. Calculate file SHA-256
    var buffer = await file.arrayBuffer();
    var hashBuffer = await crypto.subtle.digest('SHA-256', buffer);
    var hashArray = Array.from(new Uint8Array(hashBuffer));
    var expectedHash = hashArray.map(function(b) { return b.toString(16).padStart(2, '0'); }).join('');

    if (progressText) progressText.textContent = 'Initializing upload session (' + file.size + ' bytes)...';

    var initResp = await MichiAPI.syncUploadInit({
      filename: file.name,
      original_path: file.name,
      file_size: file.size,
      expected_hash: expectedHash,
      uploaded_by: 'web-ui',
    });

    if (initResp.status === 'exists') {
      if (progressFill) progressFill.style.width = '100%';
      if (progressText) progressText.textContent = '✓ File already exists on server: ' + file.name;
      showToast('File already synchronized: ' + file.name);
      fileInput.value = '';
      return;
    }

    var fileId = initResp.file_id;
    var chunkSize = 512 * 1024; // 512KB
    var totalChunks = Math.ceil(file.size / chunkSize) || 1;

    for (var chunkIndex = 0; chunkIndex < totalChunks; chunkIndex++) {
      var start = chunkIndex * chunkSize;
      var end = Math.min(file.size, start + chunkSize);
      var sliceBlob = file.slice(start, end);
      var sliceBuf = await sliceBlob.arrayBuffer();
      var sliceBytes = new Uint8Array(sliceBuf);

      // Compute slice hash
      var chunkHashBuf = await crypto.subtle.digest('SHA-256', sliceBuf);
      var chunkHashArr = Array.from(new Uint8Array(chunkHashBuf));
      var chunkHash = chunkHashArr.map(function(b) { return b.toString(16).padStart(2, '0'); }).join('');

      // Base64 encode chunk
      var binary = '';
      for (var i = 0; i < sliceBytes.byteLength; i++) {
        binary += String.fromCharCode(sliceBytes[i]);
      }
      var chunkBase64 = btoa(binary);

      await MichiAPI.syncUploadChunk(fileId, {
        file_id: fileId,
        chunk_index: chunkIndex,
        total_chunks: totalChunks,
        data_base64: chunkBase64,
        chunk_hash: chunkHash,
      });

      var pct = Math.round(((chunkIndex + 1) / totalChunks) * 100);
      if (progressFill) progressFill.style.width = pct + '%';
      if (progressText) progressText.textContent = 'Uploading chunk ' + (chunkIndex + 1) + '/' + totalChunks + ' (' + pct + '%)...';
    }

    if (progressText) progressText.textContent = 'Finalizing upload...';
    var finResp = await MichiAPI.syncUploadFinalize(fileId, file.size, expectedHash);

    if (finResp && (finResp.status === 'completed' || finResp.status === 'finalizing' || finResp.status === 'initialized')) {
      if (progressFill) progressFill.style.width = '100%';
      if (progressText) progressText.textContent = '✓ Upload completed: ' + file.name;
      showToast('File uploaded successfully: ' + file.name);
      fileInput.value = '';
    } else {
      throw new Error('Upload finalization reported uncompleted status: ' + (finResp ? finResp.status : 'unknown'));
    }
  } catch (e) {
    if (progressText) progressText.textContent = '✗ Upload failed: ' + e.message;
    showToast('Upload failed: ' + e.message, true);
  }
}

async function syncPlaylist() {
  var nameInput = $('#sync-playlist-name');
  var tracksInput = $('#sync-playlist-tracks');
  var resEl = $('#sync-playlist-result');

  var name = nameInput ? nameInput.value.trim() : '';
  var tracksText = tracksInput ? tracksInput.value.trim() : '';

  if (!name) {
    showToast('Please enter a playlist name', true);
    return;
  }

  var trackIds = tracksText
    ? tracksText.split('\n').map(function(s) { return s.trim(); }).filter(function(s) { return s.length > 0; })
    : [];

  if (resEl) resEl.innerHTML = '<span style="color:var(--text-3)">Syncing playlist ' + esc(name) + ' (' + trackIds.length + ' tracks)...</span>';

  try {
    var resp = await MichiAPI.createPlaylist(name, 'Synced playlist');
    var plId = resp.id || resp.playlist?.id;
    var added = 0;
    var failures = [];

    for (var i = 0; i < trackIds.length; i++) {
      try {
        await MichiAPI.addPlaylistTrack(plId, trackIds[i]);
        added++;
      } catch (err) {
        failures.push({ track_id: trackIds[i], error: err.message });
      }
    }

    if (failures.length === 0) {
      if (resEl) resEl.innerHTML = '<span style="color:var(--green)">✓ Playlist synced completely: ' + esc(name) + ' (' + added + ' tracks added)</span>';
      showToast('Playlist synced: ' + name);
    } else if (added > 0) {
      var failList = failures.map(function(f) { return '<li>' + esc(f.track_id) + ': ' + esc(f.error) + '</li>'; }).join('');
      if (resEl) resEl.innerHTML = '<div style="color:var(--amber)"><strong>⚠ Playlist synchronized partially</strong>: ' + added + ' added, ' + failures.length + ' failed.<ul style="margin:4px 0 0 16px;font-size:0.85em;">' + failList + '</ul></div>';
      showToast('Playlist synchronized partially (' + failures.length + ' failed)', true);
    } else {
      var failListAll = failures.map(function(f) { return '<li>' + esc(f.track_id) + ': ' + esc(f.error) + '</li>'; }).join('');
      if (resEl) resEl.innerHTML = '<div style="color:var(--error)"><strong>✗ Playlist sync failed completely</strong> (' + failures.length + ' tracks rejected):<ul style="margin:4px 0 0 16px;font-size:0.85em;">' + failListAll + '</ul></div>';
      showToast('Playlist sync failed completely', true);
    }

    if (nameInput && failures.length === 0) nameInput.value = '';
    if (tracksInput && failures.length === 0) tracksInput.value = '';
    loadPlaylists();
  } catch (e) {
    if (resEl) resEl.innerHTML = '<span style="color:var(--error)">✗ Sync failed: ' + esc(e.message) + '</span>';
    showToast('Playlist sync failed: ' + e.message, true);
  }
}

async function transferHandoff() {
  var trackInput = $('#handoff-track-id');
  var posInput = $('#handoff-position');
  var playingInput = $('#handoff-playing');
  var resEl = $('#handoff-result');
  var curStateEl = $('#handoff-current-state');

  var trackId = trackInput ? trackInput.value.trim() : '';
  var posMs = posInput ? parseInt(posInput.value, 10) || 0 : 0;
  var playing = playingInput ? playingInput.checked : false;

  if (!trackId) {
    showToast('Please enter a track ID for handoff', true);
    return;
  }

  if (resEl) resEl.innerHTML = '<span style="color:var(--text-3)">Transferring playback state to server...</span>';

  try {
    await MichiAPI.handoff({
      track_id: trackId,
      position_ms: posMs,
      playing: playing,
    });

    var readback = await MichiAPI.playbackState();
    if (curStateEl) {
      curStateEl.textContent = JSON.stringify(readback, null, 2);
    }
    if (resEl) {
      resEl.innerHTML = '<span style="color:var(--green)">✓ Playback state transferred successfully</span>';
    }
    showToast('Playback handoff state transferred');
  } catch (e) {
    if (resEl) resEl.innerHTML = '<span style="color:var(--error)">✗ Handoff failed: ' + esc(e.message) + '</span>';
    showToast('Handoff failed: ' + e.message, true);
  }
}

async function discoverDevices() {
  var resEl = $('#discover-result');
  if (resEl) resEl.innerHTML = '<span style="color:var(--text-3)">Scanning local network for Michi receivers...</span>';
  try {
    var res = await MichiAPI.discoverDevices();
    var devs = res.devices || [];
    if (resEl) {
      if (devs.length === 0) {
        resEl.innerHTML = '<span style="color:var(--text-3)">No new devices discovered.</span>';
      } else {
        resEl.innerHTML = devs.map(function(d) {
          return '<div style="font-size:0.8rem;padding:4px 0">✓ Found: <strong>' + esc(d.name || d.device_name || d.device_type || 'Device') + '</strong> (' + esc(d.address || d.ip || 'local') + ')</div>';
        }).join('');
      }
    }
    showToast('Discovery complete');
  } catch (e) {
    if (resEl) resEl.innerHTML = '<span style="color:var(--error)">✗ Discovery failed: ' + esc(e.message) + '</span>';
    showToast('Discovery failed: ' + e.message, true);
  }
}

// ── Settings ─────────────────────────────────────────────────────
function switchSettingsTab(tab) {
  $$('.tab[data-stab]').forEach(function (b) {
    b.classList.remove('active');
    b.setAttribute('aria-selected', 'false');
    b.setAttribute('tabindex', '-1');
  });
  var btn = $('.tab[data-stab="' + tab + '"]');
  if (btn) {
    btn.classList.add('active');
    btn.setAttribute('aria-selected', 'true');
    btn.setAttribute('tabindex', '0');
  }
  $$('[id^="stab-"]').forEach(function (t) {
    t.classList.add('hidden');
    t.setAttribute('aria-hidden', 'true');
  });
  var pane = $('#stab-' + tab);
  if (pane) {
    pane.classList.remove('hidden');
    pane.setAttribute('aria-hidden', 'false');
  }

  if (tab === 'diagnostics') loadDiagnostics();
  if (tab === 'jobs') loadJobs();
  if (tab === 'integrations') loadIntegrations();
}

async function loadSettings() {
  if (AuthSession.state !== 'authenticated') return;
  try {
    var s = await MichiAPI.settings();
    if (!$('#settings-port')) return;
    $('#settings-port').textContent = s.port;
    $('#settings-version').textContent = s.version || State.serverInfo?.version || '?';
    $('#settings-ffmpeg').innerHTML = s.ffmpeg_available ? '<span class="badge stable">Available</span>' : '<span class="badge disabled">Not found</span>';
    $('#settings-ffmpeg-avail').innerHTML = s.ffmpeg_available ? '<span class="badge stable">Available</span>' : '<span class="badge disabled">Not found</span>';
    $('#settings-resource-profile').value = s.resource_profile;
    $('#settings-stream-profile').value = s.stream_profile;
    $('#settings-format-policy').value = s.format_policy;
    $('#settings-music-paths').textContent = (s.music_paths || []).join('\n') || 'No paths configured';
    $('#settings-sync-name').textContent = s.sync_name || '--';
    $('#settings-cors').textContent = s.cors_origin || 'Restrictive (default)';
    $('#settings-sync-peers').textContent = (s.sync_peers || []).join(', ') || 'None';
    $('#settings-auth').innerHTML = s.auth_enabled ? '<span class="badge stable">Enabled</span>' : '<span class="badge disabled">Disabled</span>';
    $('#settings-dev-mode').innerHTML = s.dev_mode ? '<span class="badge stable">On</span>' : '<span class="badge disabled">Off</span>';
    $('#settings-scrobble').innerHTML = s.scrobble_enabled ? '<span class="badge stable">Enabled</span>' : '<span class="badge disabled">Disabled</span>';

    var scanWorkers = s.effective_scan_workers !== undefined ? s.effective_scan_workers : (s.resource_profile === 'eco' ? 1 : (s.resource_profile === 'performance' ? 4 : 2));
    var maxTc = s.effective_transcode_workers !== undefined ? s.effective_transcode_workers : (s.resource_profile === 'eco' ? 0 : (s.resource_profile === 'performance' ? 4 : 2));
    var dbPool = s.effective_db_pool !== undefined ? s.effective_db_pool : (s.resource_profile === 'eco' ? 4 : (s.resource_profile === 'performance' ? 16 : 8));
    if ($('#settings-scan-concurrency')) $('#settings-scan-concurrency').textContent = scanWorkers + ' worker(s)';
    if ($('#settings-max-transcodes')) $('#settings-max-transcodes').textContent = maxTc + ' simultaneous';
    if ($('#settings-db-pool')) $('#settings-db-pool').textContent = dbPool + ' connections';

    // Show environment overrides indicators
    var envs = s.env_overrides || [];
    var src = s.effective_sources || {};
    ['resource_profile', 'stream_profile', 'format_policy'].forEach(function(field) {
      var selectEl = $('#settings-' + field.replace('_', '-'));
      if (selectEl && src[field] === 'environment') {
        selectEl.title = 'Overridden by MICHI_' + field.toUpperCase() + ' environment variable';
      }
    });

    if (s.restart_required || localStorage.getItem('michi_restart_required') === 'true') {
      var pendingList = s.pending_restart_fields && s.pending_restart_fields.length > 0
        ? ' (' + s.pending_restart_fields.join(', ') + ')'
        : '';
      renderRestartBanner(pendingList);
    }
  } catch (e) { console.warn('settings:', e.message); }
}

function renderRestartBanner(fieldsStr) {
  var banner = $('#settings-restart-banner');
  if (!banner) {
    var target = $('#page-settings .hero') || $('#page-settings');
    if (target) {
      var div = document.createElement('div');
      div.id = 'settings-restart-banner';
      div.className = 'panel';
      div.style.backgroundColor = 'rgba(234, 179, 8, 0.15)';
      div.style.borderColor = 'rgba(234, 179, 8, 0.4)';
      div.style.color = '#fef08a';
      div.style.padding = '12px 16px';
      div.style.margin = '12px 0';
      div.style.borderRadius = '8px';
      div.style.fontWeight = '500';
      div.innerHTML = '⚠ <strong>Restart Required:</strong> Settings have been saved to disk, but require a server restart to take effect' + (fieldsStr || '') + '.';
      target.appendChild(div);
    }
  }
}

async function saveSetting(key, value) {
  var body = {};
  body[key] = value;
  try {
    var res = await MichiAPI.updateSettings(body);
    if (res && res.restart_required) {
      localStorage.setItem('michi_restart_required', 'true');
      var pStr = res.pending_restart_fields && res.pending_restart_fields.length > 0
        ? ' (' + res.pending_restart_fields.join(', ') + ')'
        : '';
      renderRestartBanner(pStr);
      showToast('Settings saved. ⚠ Server restart required.');
    } else {
      showToast(t('toast.updated'));
    }
    loadSettings();
  } catch (e) { showToast(e.message, true); }
}

// ── Webhooks ─────────────────────────────────────────────────────
async function setWebhook() {
  var url = $('#webhook-url')?.value.trim();
  if (!url) { showToast(t('error.please_enter_url'), true); return; }
  try {
    await MichiAPI.setWebhook(url);
    var el = $('#webhook-status');
    if (el) el.innerHTML = '<span style="color:var(--online)">✓ Webhook set</span>';
    showToast(t('toast.webhook_configured'));
  } catch (e) { showToast(e.message, true); }
}

async function testWebhook() {
  var el = $('#webhook-status');
  if (el) el.innerHTML = '<span style="color:var(--text-dim)">Testing webhook...</span>';
  try {
    var resp = await MichiAPI.testWebhook();
    if (resp && (resp.status === 'success' || (resp.status_code >= 200 && resp.status_code < 300))) {
      var code = resp.status_code || 200;
      var elapsed = resp.elapsed_ms !== undefined ? ' (' + resp.elapsed_ms + 'ms)' : '';
      var msg = '✓ HTTP ' + code + elapsed;
      if (el) el.innerHTML = '<span style="color:var(--online)">' + msg + '</span>';
      showToast('Webhook test passed: HTTP ' + code);
    } else {
      if (el) el.innerHTML = '<span style="color:var(--error)">✗ Webhook returned non-success response</span>';
      showToast('Webhook test returned unexpected response', true);
    }
  } catch (e) {
    if (el) el.innerHTML = '<span style="color:var(--error)">✗ ' + esc(e.message) + '</span>';
    showToast('Webhook test failed: ' + e.message, true);
  }
}

async function deleteWebhook() {
  try {
    await MichiAPI.deleteWebhook();
    var el = $('#webhook-status');
    if (el) el.innerHTML = '<span style="color:var(--text-dim)">Webhook cleared</span>';
    $('#webhook-url').value = '';
    showToast(t('toast.webhook_removed'));
  } catch (e) { showToast(e.message, true); }
}

// ── Backup, Diagnostics, Jobs, Integrations ───────────────────────
async function createSnapshot() {
  var el = $('#snapshot-result');
  if (el) el.innerHTML = '<span style="color:var(--text-dim)">Creating library statistics snapshot...</span>';
  try {
    var resp = await MichiAPI.backupSnapshot();
    if (el) {
      var s = resp.snapshot || {};
      var st = s.stats || {};
      el.innerHTML = '<span style="color:var(--online)">✓ Snapshot recorded: ' +
        (st.tracks || 0) + ' tracks, ' +
        (st.albums || 0) + ' albums, ' +
        (st.artists || 0) + ' artists</span>';
    }
    showToast(t('toast.snapshot_created'));
  } catch (e) {
    if (el) el.innerHTML = '<span style="color:var(--error)">Failed: ' + esc(e.message) + '</span>';
    showToast(e.message, true);
  }
}

function downloadBackup() {
  window.open('/api/v1/backup/download', '_blank');
}

async function restoreBackup() {
  var fileInput = $('#backup-restore-file');
  var resEl = $('#backup-restore-result');

  if (!fileInput || !fileInput.files || fileInput.files.length === 0) {
    showToast('Please select a backup JSON file to restore', true);
    return;
  }
  var file = fileInput.files[0];
  if (resEl) resEl.innerHTML = '<span style="color:var(--text-dim)">Reading backup file ' + esc(file.name) + '...</span>';

  try {
    var text = await file.text();
    var payload = JSON.parse(text);

    var tracksCount = (payload.tracks || []).length;
    var playlistsCount = (payload.playlists || []).length;
    var starredCount = (payload.starred_tracks || []).length;
    var historyCount = (payload.play_history || []).length;

    var confirmed = window.confirm(
      'Restore Database Backup Preflight:\n\n' +
      '• Playlists: ' + playlistsCount + '\n' +
      '• Starred Favorites: ' + starredCount + '\n' +
      '• Play History: ' + historyCount + '\n' +
      '• Track References: ' + tracksCount + '\n\n' +
      'WARNING: Restoring will overwrite existing playlists and metadata.\n' +
      'Do you wish to proceed?'
    );

    if (!confirmed) {
      if (resEl) resEl.innerHTML = '<span style="color:var(--text-dim)">Restore cancelled by user</span>';
      return;
    }

    if (resEl) resEl.innerHTML = '<span style="color:var(--text-dim)">Applying restore payload...</span>';
    var resp = await MichiAPI.restoreBackup(payload);

    if (resEl) {
      resEl.innerHTML = '<div style="color:var(--online)"><strong>✓ Restore completed successfully!</strong><br>' +
        'Restored: ' + (resp.playlists || 0) + ' playlists, ' +
        (resp.starred || 0) + ' favorites, ' +
        (resp.history || 0) + ' history entries.</div>';
    }
    showToast('Backup restored successfully');
    fileInput.value = '';
    loadPlaylists();
  } catch (e) {
    if (resEl) resEl.innerHTML = '<span style="color:var(--error)">✗ Restore failed: ' + esc(e.message) + '</span>';
    showToast('Restore failed: ' + e.message, true);
  }
}

async function loadDiagnostics() {
  var statusEl = $('#diag-status');
  var ffmpegEl = $('#diag-ffmpeg');
  var transEl = $('#diag-transcodes');
  var poolEl = $('#diag-db-pool');
  var capsEl = $('#diag-caps-list');

  try {
    var health = await MichiAPI.health();
    var caps = await MichiAPI.serverCapabilities();
    var settings = await MichiAPI.settings();

    if (statusEl) {
      statusEl.textContent = health.status === 'ok' ? 'Healthy' : health.status;
      statusEl.className = health.status === 'ok' ? 'badge stable' : 'badge disabled';
    }
    if (ffmpegEl) ffmpegEl.textContent = settings.ffmpeg_available ? 'Available (Transcoding active)' : 'Unavailable (Direct play only)';
    if (transEl) transEl.textContent = settings.effective_transcode_workers + ' worker slots';
    if (poolEl) poolEl.textContent = settings.effective_db_pool + ' connections';

    if (capsEl && caps.capabilities) {
      var lines = Object.keys(caps.capabilities).map(function(k) {
        var v = caps.capabilities[k];
        return '<div>' + esc(k) + ': <strong>' + (v ? 'Enabled' : 'Disabled') + '</strong></div>';
      }).join('');
      capsEl.innerHTML = lines;
    }
  } catch (e) {
    console.warn('diagnostics failed:', e.message);
  }
}

async function loadJobs() {
  var maxEl = $('#jobs-max-concurrent');
  var listEl = $('#jobs-list');

  try {
    var settings = await MichiAPI.settings();
    var jobsData = await MichiAPI.jobs();

    if (maxEl) maxEl.textContent = (settings.job_max_concurrent || 2) + ' concurrent workers';
    if (listEl) {
      var jobs = jobsData.jobs || [];
      if (jobs.length === 0) {
        listEl.innerHTML = '<div class="empty-state" style="padding:16px"><p>No active or recent background jobs</p></div>';
      } else {
        listEl.innerHTML = jobs.map(function(j) {
          var statusBadge = j.status === 'running'
            ? '<span class="badge stable">Running</span>'
            : (j.status === 'succeeded' ? '<span class="badge format">Completed</span>' : '<span class="badge disabled">' + esc(j.status) + '</span>');
          return '<div class="panel-row" style="border-bottom:1px solid var(--border);padding:8px 0">' +
            '<span class="panel-label" style="flex:2">' + esc(j.kind || j.type || 'Job') + ' (' + esc(j.id ? j.id.slice(0, 8) : '') + ')</span>' +
            '<span style="flex:1">' + statusBadge + '</span>' +
            '<span class="panel-mono" style="font-size:0.75rem">' + esc(j.created_at || '') + '</span>' +
            '</div>';
        }).join('');
      }
    }
  } catch (e) {
    console.warn('jobs failed:', e.message);
  }
}

async function loadIntegrations() {
  var peersEl = $('#integ-sync-peers');
  var delayEl = $('#integ-reconnect-max');

  try {
    var settings = await MichiAPI.settings();
    if (peersEl) peersEl.textContent = (settings.sync_peers || []).join(', ') || 'No mesh peers configured';
    if (delayEl) delayEl.textContent = (settings.reconnect_delay_max || 300) + ' seconds backoff cap';
  } catch (e) {
    console.warn('integrations failed:', e.message);
  }
}

async function verifyIntegrity() {
  var el = $('#integrity-result');
  if (el) el.innerHTML = '<span style="color:var(--text-dim)">Checking file availability...</span>';
  try {
    var resp = await MichiAPI.backupVerify();
    if (el) {
      var ok = resp.status === 'ok' || resp.missing === 0;
      el.innerHTML = '<span style="color:' + (ok ? 'var(--online)' : 'var(--error)') + '">' +
        (ok ? '✓ Files verified' : '⚠ Missing files') + ' — ' +
        resp.available + ' available, ' + resp.missing + ' missing (Total: ' + resp.total + ')</span>';
    }
  } catch (e) {
    if (el) el.innerHTML = '<span style="color:var(--error)">Check failed: ' + esc(e.message) + '</span>';
    showToast(e.message, true);
  }
}

// ── Chains ───────────────────────────────────────────────────────
var _currentChainId = null;

async function loadChains() {
  if (AuthSession.state !== 'authenticated') return;
  var container = $('#chains-list');
  if (!container) return;
  if (!hasServerFeature('receivers')) {
    container.innerHTML = '<div class="empty-state"><div class="icon">📡</div><p><strong>Receivers & Chains are disabled in Micro Server profile</strong></p><p style="font-size:.78rem;margin-top:4px">This deployment runs standalone audio output. Multi-room receiver playback is unsupported.</p></div>';
    return;
  }
  try {
    var raw = await MichiAPI.chains();
    var chains = raw.chains || [];
    if (chains.length === 0) {
      container.innerHTML = '<div class="empty-state"><p><strong>' + t('empty.no_chains') + '</strong></p></div>';
      return;
    }
    container.innerHTML = chains.map(function (c) {
      var playing = c.playing ? '▶' : '⏹';
      var trackInfo = c.track_id ? c.track_id.slice(0, 8) + '..' : 'no track';
      return '<div class="chain-item" onclick="openChain(\'' + c.id + '\')">' +
        '<div class="info"><div class="name">' + esc(c.name) + '</div>' +
        '<div class="meta">' + trackInfo + ' — ' + c.position_ms + 'ms</div></div>' +
        '<div class="status">' + playing + '</div></div>';
    }).join('');
  } catch (e) { console.warn('chains failed:', e.message); }
}

function showCreateChain() {
  var el = $('#chain-create-form');
  if (el) el.classList.remove('hidden');
}

function hideCreateChain() {
  var el = $('#chain-create-form');
  if (el) el.classList.add('hidden');
}

async function createChain() {
  var name = $('#new-chain-name')?.value.trim();
  if (!name) { showToast(t('error.please_enter_name'), true); return; }
  try {
    await MichiAPI.createChain(name);
    $('#new-chain-name').value = '';
    hideCreateChain();
    loadChains();
    showToast(t('toast.created'));
  } catch (e) { showToast(e.message, true); }
}

async function openChain(id) {
  _currentChainId = id;
  try {
    var raw = await MichiAPI.chain(id);
    var chain = raw.chain || {};
    var links = raw.links || [];

    $('#chain-detail').classList.remove('hidden');
    $('#chain-detail-name').textContent = chain.name;
    $('#chain-track-id').value = chain.track_id || '';
    $('#chain-master-vol').value = 80;
    $('#chain-master-vol-label').textContent = '80';

    renderChainLinks(links);

    var recvResp = await MichiAPI.receivers();
    var recvs = recvResp.receivers || [];
    var sel = $('#chain-add-receiver');
    if (sel) {
      sel.innerHTML = '<option value="">-- select --</option>' +
        recvs.map(function (r) {
          return '<option value="' + esc(r.id) + '">' + esc(r.name || r.id) + '</option>';
        }).join('');
    }
  } catch (e) { showToast(e.message, true); }
}

function closeChainDetail() {
  _currentChainId = null;
  $('#chain-detail').classList.add('hidden');
  loadChains();
}

function renderChainLinks(links) {
  var container = $('#chain-links');
  if (!container) return;
  if (links.length === 0) {
    container.innerHTML = '<p style="color:var(--text-dim);font-size:.82rem;padding:12px 0">' + t('empty.no_links') + '</p>';
    return;
  }
  container.innerHTML = links.map(function (l, i) {
    var arrow = i > 0 ? '<div class="chain-arrow">↓</div>' : '';
    return arrow +
      '<div class="chain-link-card" data-link-id="' + l.id + '">' +
      '<div class="link-info">' +
      '<div class="name">' + esc(l.receiver_name || l.receiver_id) + '</div>' +
      '<div class="detail">Delay: ' + l.delay_ms + 'ms</div></div>' +
      '<div class="link-volume">' +
      '<input type="range" min="0" max="100" value="' + l.volume + '" ' +
      'onchange="saveLinkVolume(\'' + l.id + '\', \'' + l.chain_id + '\', this.value)">' +
      '<span>' + l.volume + '</span></div>' +
      '<button class="link-remove" onclick="removeLink(\'' + l.id + '\', \'' + l.chain_id + '\')">✕</button>' +
      '</div>';
  }).join('');
}

async function addLink() {
  if (!_currentChainId) return;
  var sel = $('#chain-add-receiver');
  var recvId = sel?.value;
  if (!recvId) { showToast(t('error.please_select_receiver'), true); return; }
  try {
    await MichiAPI.addChainLink(_currentChainId, { receiver_id: recvId });
    openChain(_currentChainId);
    showToast(t('toast.created'));
  } catch (e) { showToast(e.message, true); }
}

async function removeLink(linkId, chainId) {
  try {
    await MichiAPI.removeChainLink(chainId, linkId);
    openChain(_currentChainId);
    showToast(t('toast.deleted'));
  } catch (e) { showToast(e.message, true); }
}

async function saveLinkVolume(linkId, chainId, val) {
  try {
    await MichiAPI.updateChainLink(chainId, linkId, { volume: parseInt(val) });
  } catch (e) { showToast(e.message, true); }
}

async function setChainTrack() {
  if (!_currentChainId) return;
  var trackId = $('#chain-track-id')?.value.trim();
  try {
    await MichiAPI.request('/api/v1/chains/' + _currentChainId, {
      method: 'PUT',
      body: { track_id: trackId },
    });
    showToast(t('toast.updated'));
  } catch (e) { showToast(e.message, true); }
}

async function playChain() {
  if (!_currentChainId) return;
  try {
    var resp = await MichiAPI.playChain(_currentChainId);
    if (resp && resp.active_links > 0) {
      ServerPlayback.remoteTargetName = 'Chain: ' + ($('#chain-detail-name')?.textContent || _currentChainId);
      updateOutputRoutingBadge();
      showToast('Chain playing on ' + resp.active_links + ' active receiver(s)');
    } else {
      showToast('Chain started without active receiver links', true);
    }
    loadChains();
  } catch (e) {
    showToast(t('error.could_not_play', {msg: e.message}), true);
    loadChains();
  }
}

async function stopChain() {
  if (!_currentChainId) return;
  try {
    await MichiAPI.stopChain(_currentChainId);
    ServerPlayback.remoteTargetName = null;
    updateOutputRoutingBadge();
    showToast(t('toast.deactivated'));
    loadChains();
  } catch (e) { showToast(e.message, true); }
}

var _chainVolTimeout = null;
function setChainVolume(val) {
  var label = $('#chain-master-vol-label');
  if (label) label.textContent = val;
  if (!_currentChainId) return;

  if (_chainVolTimeout) clearTimeout(_chainVolTimeout);
  _chainVolTimeout = setTimeout(async function () {
    try {
      await MichiAPI.setChainVolume(_currentChainId, parseInt(val));
    } catch (e) {
      showToast('Failed to set chain volume: ' + e.message, true);
    }
  }, 200);
}

// ── Keyboard shortcuts ─────────────────────────────────────────
document.addEventListener('keydown', function (e) {
  if (e.ctrlKey && e.key === 'k') {
    e.preventDefault();
    var si = $('#search-input');
    if (si) { si.focus(); si.select(); }
  }
  if (e.key === ' ' && !e.target.closest('input, textarea, select, button, a, [role="button"], [contenteditable="true"]')) {
    e.preventDefault();
    playPause();
  }
});
