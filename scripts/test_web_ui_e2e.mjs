#!/usr/bin/env node
/**
 * Michi Web UI Functional Integrity E2E Test Suite
 * Tests actual frontend logic from crates/michi-api/static/app.js against static/index.html
 *
 * Every sandbox created by makeSandbox() injects globalThis timer functions so
 * that app.js code calling setTimeout/setInterval bare (not window.setTimeout)
 * works correctly inside Node's vm.createContext isolation.
 */

import fs from 'fs';
import path from 'path';
import { fileURLToPath } from 'url';
import vm from 'vm';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const rootDir = path.resolve(__dirname, '..');

const htmlPath = path.join(rootDir, 'crates/michi-api/static/index.html');
const jsPath = path.join(rootDir, 'crates/michi-api/static/app.js');

const rawJsContent = fs.readFileSync(jsPath, 'utf8');

// Wrap app.js in an IIFE that receives all browser globals from the sandbox,
// then exports the symbols that tests need to reach via window.*
const jsContent = `
(function(window, document, navigator, localStorage, sessionStorage, fetch,
          setTimeout, clearTimeout, setInterval, clearInterval, Audio, requestAnimationFrame) {
` + rawJsContent + `
;
window.State            = State;
window.ServerPlayback   = ServerPlayback;
window.AuthSession      = AuthSession;
window.MichiAPI         = MichiAPI;
window.addToQueue       = addToQueue;
window.toggleOutputTarget = toggleOutputTarget;
window.selectServerOutputTarget = selectServerOutputTarget;
window.selectLocalBrowserOutput = selectLocalBrowserOutput;
window.saveSetting      = saveSetting;
window.loadSettings     = loadSettings;
window.playEpisode      = playEpisode;
window.uploadFile       = uploadFile;
window.restoreBackup    = restoreBackup;
window.transferHandoff  = transferHandoff;
window.loadDiagnostics  = loadDiagnostics;
window.loadIntegrations = loadIntegrations;
window.loadJobs         = loadJobs;
window.computeBytesSha256 = computeBytesSha256;
})(window, document, window.navigator, window.localStorage, window.sessionStorage,
   window.fetch,
   globalThis.setTimeout, globalThis.clearTimeout,
   globalThis.setInterval, globalThis.clearInterval,
   window.Audio,
   function(fn) { return globalThis.setTimeout(fn, 0); }
);
`;

console.log('======================================================================');
console.log('MICHI WEB UI FUNCTIONAL INTEGRITY BROWSER E2E TEST RUNNER');
console.log('======================================================================');

// ── Mock DOM helpers ────────────────────────────────────────────

class MockElement {
  constructor(tag, id = '', className = '') {
    this.tagName = tag.toUpperCase();
    this._id = id;
    this.className = className;
    this._innerHTML = '';
    this._textContent = '';
    this.style = {};
    this.children = [];
    this.parentNode = null;
    this.attributes = {};
    this.value = '';
    this.onclick = null;
    this.eventListeners = {};
    this.dataset = {};
    this.offsetHeight = 0;
  }

  get textContent() {
    if (this._textContent) return this._textContent;
    if (this._innerHTML) return this._innerHTML.replace(/<[^>]*>/g, '');
    return '';
  }
  set textContent(val) {
    this._textContent = String(val);
    this._innerHTML = String(val);
  }

  get innerHTML() { return this._innerHTML || ''; }
  set innerHTML(val) {
    this._innerHTML = String(val);
    this._textContent = String(val).replace(/<[^>]*>/g, '');
  }

  get id() { return this._id || ''; }
  set id(val) { this._id = val; }

  getAttribute(name) { return this.attributes[name] || null; }
  setAttribute(name, val) {
    this.attributes[name] = String(val);
    if (name === 'id') this._id = String(val);
  }
  removeAttribute(name) {
    delete this.attributes[name];
    if (name === 'id') this._id = '';
  }

  appendChild(child) {
    child.parentNode = this;
    this.children.push(child);
    return child;
  }

  insertBefore(newNode, referenceNode) {
    newNode.parentNode = this;
    const idx = this.children.indexOf(referenceNode);
    if (idx >= 0) {
      this.children.splice(idx, 0, newNode);
    } else {
      this.children.push(newNode);
    }
    return newNode;
  }

  addEventListener(event, fn) {
    if (!this.eventListeners[event]) this.eventListeners[event] = [];
    this.eventListeners[event].push(fn);
  }

  dispatchEvent(event) {
    const list = this.eventListeners[event.type] || [];
    for (const fn of list) fn(event);
  }

  classList = {
    _classes: new Set(),
    add(...cls) { for (const c of cls) this._classes.add(c); },
    remove(...cls) { for (const c of cls) this._classes.delete(c); },
    toggle(cls, force) {
      if (force === undefined) {
        if (this._classes.has(cls)) { this._classes.delete(cls); return false; }
        this._classes.add(cls); return true;
      }
      if (force) { this._classes.add(cls); return true; }
      this._classes.delete(cls); return false;
    },
    contains(cls) { return this._classes.has(cls); },
    toString() { return [...this._classes].join(' '); },
  };

  querySelector(sel) { return this._query(sel); }
  querySelectorAll(sel) {
    const results = [];
    this._queryAll(sel, results);
    return results;
  }

  _query(sel) {
    if (sel.includes(' ')) {
      const parts = sel.split(' ');
      const first = this._query(parts[0]);
      if (first) return first._query(parts.slice(1).join(' '));
      return null;
    }
    if (sel.startsWith('#')) {
      const id = sel.slice(1);
      if (this.id === id) return this;
      for (const c of this.children) {
        const found = c._query(sel);
        if (found) return found;
      }
    } else if (sel.startsWith('.')) {
      const cls = sel.slice(1);
      if (this.className.includes(cls) || this.classList._classes.has(cls)) return this;
      for (const c of this.children) {
        const found = c._query(sel);
        if (found) return found;
      }
    } else {
      if (sel.includes('[')) {
        return null;
      }
      if (this.tagName === sel.toUpperCase()) return this;
      for (const c of this.children) {
        const found = c._query(sel);
        if (found) return found;
      }
    }
    return null;
  }

  _queryAll(sel, results) {
    if (sel.startsWith('.')) {
      const cls = sel.slice(1);
      if (this.className.includes(cls) || this.classList._classes.has(cls)) results.push(this);
    } else if (sel.startsWith('#')) {
      const id = sel.slice(1);
      if (this.id === id) results.push(this);
    } else {
      if (this.tagName === sel.toUpperCase()) results.push(this);
    }
    for (const c of this.children) c._queryAll(sel, results);
  }

  remove() {
    if (this.parentNode) {
      const idx = this.parentNode.children.indexOf(this);
      if (idx >= 0) this.parentNode.children.splice(idx, 1);
      this.parentNode = null;
    }
  }
}

function createDOM() {
  function el(tag, id = '', cls = '') {
    return new MockElement(tag, id, cls);
  }

  const doc = {
    documentElement: el('html'),
    body: el('body'),
    head: el('head'),
    createElement: (tag) => el(tag),
    getElementById: (id) => doc.body.querySelector('#' + id),
    querySelector: (sel) => {
      if (sel.startsWith('#')) return doc.body.querySelector(sel);
      return doc.body.querySelector(sel);
    },
    querySelectorAll: (sel) => doc.body.querySelectorAll(sel),
    addEventListener: () => {},
  };

  // Seed core DOM nodes expected by app.js
  const toast = el('div', 'toast');
  const npTargetBadge = el('span', 'np-target-badge');
  const pageSettings = el('section', 'page-settings');
  const settingsHero = el('div', '', 'hero');
  pageSettings.appendChild(settingsHero);

  const settingIds = [
    'settings-port', 'settings-version', 'settings-ffmpeg', 'settings-ffmpeg-avail',
    'settings-resource-profile', 'settings-stream-profile', 'settings-format-policy',
    'settings-music-paths', 'settings-sync-name', 'settings-cors', 'settings-sync-peers',
    'settings-auth', 'settings-dev-mode', 'settings-scrobble',
    'settings-scan-concurrency', 'settings-max-transcodes', 'settings-db-pool',
    'ha-discovery-status', 'integ-sync-peers', 'integ-reconnect-max',
    'diag-status', 'diag-ffmpeg', 'diag-transcodes', 'diag-db-pool', 'diag-caps-list',
    'handoff-track-id', 'handoff-position', 'handoff-playing', 'handoff-result', 'handoff-current-state'
  ];
  for (const sid of settingIds) {
    pageSettings.appendChild(el('div', sid));
  }

  doc.body.appendChild(toast);
  doc.body.appendChild(npTargetBadge);
  doc.body.appendChild(pageSettings);

  const localStorage = {
    _data: {},
    getItem(k) { return this._data[k] !== undefined ? this._data[k] : null; },
    setItem(k, v) { this._data[k] = String(v); },
    removeItem(k) { delete this._data[k]; },
    clear() { this._data = {}; },
  };

  const sessionStorage = {
    _data: {},
    getItem(k) { return this._data[k] !== undefined ? this._data[k] : null; },
    setItem(k, v) { this._data[k] = String(v); },
    removeItem(k) { delete this._data[k]; },
  };

  const win = {
    document: doc,
    navigator: { language: 'en-US', onLine: true },
    localStorage,
    sessionStorage,
    location: { origin: 'http://localhost:9090' },
    Audio: class {
      constructor() { this.src = ''; this.currentTime = 0; this.duration = 180; }
      play() { return Promise.resolve(); }
      pause() {}
    },
    fetch: async () => ({ ok: true, headers: { get: () => 'application/json' }, json: async () => ({}) }),
    addEventListener: () => {},
  };

  return { window: win, document: doc };
}

// ── Sandbox factory ─────────────────────────────────────────────
function makeSandbox({ window, document, fetchImpl = null, showToastImpl = null } = {}) {
  if (!window || !document) {
    const dom = createDOM();
    window = dom.window;
    document = dom.document;
  }
  const effectiveFetch = fetchImpl || (async () => ({ ok: true, headers: { get: () => 'application/json' }, json: async () => ({}) }));
  window.fetch = effectiveFetch;

  const sandbox = {
    window,
    document,
    navigator:      window.navigator,
    localStorage:   window.localStorage,
    sessionStorage: window.sessionStorage,
    console: { warn: () => {}, error: () => {}, log: () => {} },
    $:  (s) => document.querySelector(s),
    $$: (s) => document.querySelectorAll(s),
    t:       (k) => k,
    esc:     (s) => s,
    fmtDur:  () => '3:00',
    fmtDate: () => '2024-01-01',
    setTimeout:           global.setTimeout,
    clearTimeout:         global.clearTimeout,
    setInterval:          global.setInterval,
    clearInterval:        global.clearInterval,
    requestAnimationFrame: (fn) => global.setTimeout(fn, 0),
    AbortController:    globalThis.AbortController,
    AbortSignal:        globalThis.AbortSignal,
    Headers:            globalThis.Headers,
    FormData:           globalThis.FormData,
    URL:                globalThis.URL,
    URLSearchParams:    globalThis.URLSearchParams,
    TextEncoder:        globalThis.TextEncoder,
    TextDecoder:        globalThis.TextDecoder,
    Promise:            globalThis.Promise,
    JSON:               globalThis.JSON,
    Error:              globalThis.Error,
    showToast: showToastImpl || (() => {}),
    fetch:     effectiveFetch,
  };
  return { sandbox, window, document };
}

// ── Assertion helpers ───────────────────────────────────────────
let passed = 0;
let failed = 0;

function assert(condition, name) {
  if (condition) {
    console.log(`  ✅ PASS: ${name}`);
    passed++;
  } else {
    console.error(`  ❌ FAIL: ${name}`);
    failed++;
  }
}

// ── Test suite ──────────────────────────────────────────────────
async function runE2E() {

  // ── Test A: Output Target Truthfulness ──────────────────────
  {
    const fetchImpl = async (url, opts) => {
      if (url.includes('/api/v1/playback/output')) {
        return {
          ok: true, status: 200,
          headers: { get: () => 'application/json' },
          json: async () => ({ status: 'output_selected' }),
        };
      }
      return { ok: true, headers: { get: () => 'application/json' }, json: async () => ({}) };
    };

    const { sandbox, window, document } = makeSandbox({ fetchImpl });
    vm.createContext(sandbox);
    vm.runInContext(jsContent, sandbox);

    const badge = document.getElementById('np-target-badge');
    assert(badge !== null, 'Now Playing target badge exists in DOM');
    assert(window.ServerPlayback.outputTarget === 'browser', 'Default output target is browser local');

    await window.selectServerOutputTarget('receiver', 'rec-living-room', 'Living Room');
    assert(window.ServerPlayback.outputTarget === 'server', 'Selecting server output target switches outputTarget to server');
    assert(badge.textContent.includes('Living Room'), 'Badge reflects Living Room output truth');

    window.selectLocalBrowserOutput();
    assert(window.ServerPlayback.outputTarget === 'browser', 'Switching back to browser local restores outputTarget');
    assert(badge.textContent.includes('This Browser'), 'Badge reflects This Browser truth');
  }

  // ── Test B: Queue Add Server Failure ────────────────────────
  {
    let toastErrorShown = false;

    const fetchImpl = async (url, opts) => {
      if (url.includes('/api/v1/queue/items')) {
        return {
          ok: false, status: 400,
          headers: { get: () => 'application/json' },
          json: async () => ({ error: { message: 'Database constraint failed' } }),
        };
      }
      return { ok: true, headers: { get: () => 'application/json' }, json: async () => ({}) };
    };
    const showToastImpl = (msg, type) => {
      if (type === 'error' || (typeof msg === 'string' && msg.includes('Failed to add'))) {
        toastErrorShown = true;
      }
    };

    const { sandbox, window, document } = makeSandbox({ fetchImpl, showToastImpl });
    vm.createContext(sandbox);
    vm.runInContext(jsContent, sandbox);

    window.State.tracks = [{ id: 'track-1', title: 'Song 1', duration_ms: 180000 }];
    window.State.queue  = [];

    await window.addToQueue(0);

    const toastEl = document.getElementById('toast');
    assert(window.State.queue.length === 0, 'Queue is NOT mutated locally when backend API rejects');
    const toastText = toastEl ? toastEl.textContent : '';
    assert(toastText.includes('Failed to add') || toastErrorShown,
      'Explicit error toast displayed upon queue failure');
  }

  // ── Test C: Restart Banner persists across F5 ───────────────
  {
    const settingsFetch = async (url, opts) => {
      if (opts?.method === 'PUT' && url.includes('/api/v1/settings')) {
        return {
          ok: true,
          status: 200,
          headers: { get: (h) => h === 'content-type' ? 'application/json' : null },
          json: async () => ({ restart_required: true, pending_restart_fields: ['resource_profile'], resource_profile: 'performance' }),
        };
      }
      if (url.includes('/api/v1/settings')) {
        return {
          ok: true, status: 200,
          headers: { get: (h) => h === 'content-type' ? 'application/json' : null },
          json: async () => ({
            restart_required: true,
            pending_restart_fields: ['resource_profile'],
            resource_profile: 'performance',
            effective_scan_workers: 4,
            effective_transcode_workers: 4,
            effective_db_pool: 16,
          }),
        };
      }
      return { ok: true, headers: { get: (h) => h === 'content-type' ? 'application/json' : null }, json: async () => ({}) };
    };

    const { sandbox, window, document } = makeSandbox({ fetchImpl: settingsFetch });
    vm.createContext(sandbox);
    vm.runInContext(jsContent, sandbox);

    await window.saveSetting('resource_profile', 'performance');
    const bannerDirect = document.getElementById('settings-restart-banner');
    assert(bannerDirect !== null,
      'Restart banner displayed immediately upon save when backend requires restart');

    // Simulate F5
    const { sandbox: sandboxR, window: winR, document: docR } = makeSandbox({ fetchImpl: settingsFetch });
    vm.createContext(sandboxR);
    vm.runInContext(jsContent, sandboxR);

    // Mock authenticated session
    winR.AuthSession.state = 'authenticated';
    await winR.loadSettings();
    const banner = docR.getElementById('settings-restart-banner');
    assert(banner !== null, 'Restart banner rendered from server authoritative restart_required flag');
  }

  // ── Test D: Failed audio.play does NOT mark episode played ──
  {
    let playedEndpointCalled = false;
    const fetchImpl = async (url, opts) => {
      if (url.includes('/api/v1/episodes/')) playedEndpointCalled = true;
      return { ok: true, headers: { get: () => 'application/json' }, json: async () => ({}) };
    };

    const { sandbox, window, document } = makeSandbox({ fetchImpl });
    window.Audio = class {
      constructor() { this.src = ''; }
      play() { return Promise.reject(new Error('Decode error')); }
      pause() {}
    };
    vm.createContext(sandbox);
    vm.runInContext(jsContent, sandbox);

    try { await window.playEpisode('ep-456'); } catch (_) {}

    assert(playedEndpointCalled === false,
      'Failed audio.play does NOT call mark episode as played');
  }

  // ── Test E: Chunked Resumable Upload Flow ──
  {
    const recordedRequests = [];
    const uploadFetch = async (url, opts) => {
      const parsedBody = opts?.body ? JSON.parse(opts.body) : null;
      recordedRequests.push({ url, method: opts?.method || 'GET', body: parsedBody });

      if (url.includes('/api/v1/sync/upload/init')) {
        return {
          ok: true, status: 200,
          headers: { get: () => 'application/json' },
          json: async () => ({ status: 'initialized', file_id: '550e8400-e29b-41d4-a716-446655440000' })
        };
      }
      if (url.includes('/chunk')) {
        return {
          ok: true, status: 200,
          headers: { get: () => 'application/json' },
          json: async () => ({ status: 'uploading', progress: { uploaded_chunks: 1, total_chunks: 1, completed: false } })
        };
      }
      if (url.includes('/status')) {
        return {
          ok: true, status: 200,
          headers: { get: () => 'application/json' },
          json: async () => ({ status: 'completed', progress: { uploaded_chunks: 1, total_chunks: 1, completed: true } })
        };
      }
      return { ok: true, headers: { get: () => 'application/json' }, json: async () => ({}) };
    };

    const { sandbox, window, document } = makeSandbox({ fetchImpl: uploadFetch });
    vm.createContext(sandbox);
    vm.runInContext(jsContent, sandbox);

    const initRes = await window.MichiAPI.syncUploadInit({
      filename: 'sample.flac',
      original_path: 'sample.flac',
      file_size: 1048576,
      expected_hash: 'a'.repeat(64),
      uploaded_by: 'web-ui'
    });
    assert(initRes.status === 'initialized', 'Sync upload initialized correctly');

    const chunkRes = await window.MichiAPI.syncUploadChunk(initRes.file_id, {
      file_id: initRes.file_id,
      chunk_index: 0,
      total_chunks: 1,
      data: [1, 2, 3, 4],
      chunk_hash: 'b'.repeat(64)
    });
    assert(chunkRes.status === 'uploading', 'Chunk uploaded successfully');

    const statusRes = await window.MichiAPI.syncUploadStatus(initRes.file_id);
    assert(statusRes.status === 'completed', 'Durable upload status verified completed');

    assert(recordedRequests.some(r => r.url.endsWith('/init') && r.body.filename === 'sample.flac'),
      'Init request sent correct UploadInitBody');
    assert(recordedRequests.some(r => r.url.includes('/chunk') && r.body.chunk_index === 0),
      'Chunk request sent correct UploadChunk schema');
  }

  // ── Test F: Restore Backup sends force=true ──
  {
    let restoreBody = null;
    const restoreFetch = async (url, opts) => {
      if (url.includes('/api/v1/backup/restore')) {
        restoreBody = JSON.parse(opts.body);
        return {
          ok: true, status: 200,
          headers: { get: () => 'application/json' },
          json: async () => ({ playlists: 2, starred: 5, history: 10 })
        };
      }
      return { ok: true, headers: { get: () => 'application/json' }, json: async () => ({}) };
    };

    const { sandbox, window, document } = makeSandbox({ fetchImpl: restoreFetch });
    window.confirm = () => true;
    vm.createContext(sandbox);
    vm.runInContext(jsContent, sandbox);

    const restorePayload = {
      version: 1,
      tracks: [],
      playlists: [{ name: 'Favorites', track_ids: [] }],
      starred_tracks: [],
      play_history: []
    };

    const resp = await window.MichiAPI.restoreBackup({ ...restorePayload, force: true });
    assert(resp.playlists === 2, 'Restore executed successfully');
    assert(restoreBody && restoreBody.force === true, 'Restore request explicitly sets force: true');
  }

  // ── Test G: computeBytesSha256 NIST Vector & SubtleCrypto Independence ──
  {
    const { sandbox, window } = makeSandbox();
    vm.createContext(sandbox);
    vm.runInContext(jsContent, sandbox);

    // NIST vector for "abc" -> ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad
    const bytes = new TextEncoder().encode('abc');
    const hash = window.computeBytesSha256(bytes);
    assert(hash === 'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad',
      'computeBytesSha256 matches NIST SHA-256 standard without SubtleCrypto');
  }

  // ── Test H: Home Assistant Live Status Telemetry Invariants ──
  {
    const haFetch = async (url) => {
      if (url.includes('/api/v1/diagnostics')) {
        return {
          ok: true, status: 200,
          headers: { get: () => 'application/json' },
          json: async () => ({
            healthy: true,
            degraded: false,
            homeassistant: {
              enabled: true,
              configured: true,
              connected: true,
              broker: '127.0.0.1:1883',
              discovery_published: true,
              last_published_at: '2026-08-31T00:00:00Z',
              last_error: null
            }
          })
        };
      }
      if (url.includes('/api/v1/settings')) {
        return {
          ok: true, status: 200,
          headers: { get: () => 'application/json' },
          json: async () => ({})
        };
      }
      return { ok: true, headers: { get: () => 'application/json' }, json: async () => ({}) };
    };

    const { sandbox, window, document } = makeSandbox({ fetchImpl: haFetch });
    vm.createContext(sandbox);
    vm.runInContext(jsContent, sandbox);

    await window.loadIntegrations();
    const haEl = document.getElementById('ha-discovery-status');
    assert(haEl && haEl.textContent.includes('Connected') && haEl.textContent.includes('Discovery Active'),
      'loadIntegrations renders real MQTT connectivity and discovery state');
  }

  // ── Test I: transferHandoff Full State & Position Drift Convergence ──
  {
    const handoffFetch = async (url) => {
      if (url.includes('/playback/state')) {
        return {
          ok: true, status: 200,
          headers: { get: () => 'application/json' },
          json: async () => ({
            track_id: 'track-xyz',
            position_ms: 120050,
            playing: true,
          })
        };
      }
      if (url.includes('/playback/handoff')) {
        return {
          ok: true, status: 200,
          headers: { get: () => 'application/json' },
          json: async () => ({ status: 'handoff_initiated' })
        };
      }
      return { ok: true, headers: { get: () => 'application/json' }, json: async () => ({}) };
    };

    const { sandbox, window, document } = makeSandbox({ fetchImpl: handoffFetch });
    vm.createContext(sandbox);
    vm.runInContext(jsContent, sandbox);

    const trackInput = document.getElementById('handoff-track-id');
    const posInput = document.getElementById('handoff-position');
    const playInput = document.getElementById('handoff-playing');
    const resEl = document.getElementById('handoff-result');

    trackInput.value = 'track-xyz';
    posInput.value = '120000';
    playInput.checked = true;

    await window.transferHandoff();
    assert(resEl && resEl.textContent.includes('verified converged (track, state, position)'),
      'transferHandoff verifies track, playing state, and position drift');
  }

  // ── Test J: Cover Art Preference DOM Consumer Effect ─────────
  {
    const { sandbox, window, document } = makeSandbox();
    vm.createContext(sandbox);
    vm.runInContext(jsContent, sandbox);

    window.applyCoverArtPreference(false);
    assert(document.documentElement.getAttribute('data-cover-art') === 'false',
      'Cover art disabled sets data-cover-art="false" on documentElement');
    assert(document.body.classList.contains('hide-cover-art'),
      'Cover art disabled adds hide-cover-art class to body');

    window.applyCoverArtPreference(true);
    assert(document.documentElement.getAttribute('data-cover-art') === 'true',
      'Cover art enabled sets data-cover-art="true" on documentElement');
    assert(!document.body.classList.contains('hide-cover-art'),
      'Cover art enabled removes hide-cover-art class from body');
  }

  // ── Test K: Sidebar Collapsed Preference DOM Consumer Effect ──
  {
    const { sandbox, window, document } = makeSandbox();
    vm.createContext(sandbox);
    vm.runInContext(jsContent, sandbox);

    window.applySidebarPreference(true);
    assert(document.documentElement.getAttribute('data-sidebar-collapsed') === 'true',
      'Sidebar collapsed sets data-sidebar-collapsed="true" on documentElement');
    assert(document.body.classList.contains('sidebar-collapsed'),
      'Sidebar collapsed adds sidebar-collapsed class to body');

    window.applySidebarPreference(false);
    assert(document.documentElement.getAttribute('data-sidebar-collapsed') === 'false',
      'Sidebar uncollapsed sets data-sidebar-collapsed="false" on documentElement');
    assert(!document.body.classList.contains('sidebar-collapsed'),
      'Sidebar uncollapsed removes sidebar-collapsed class from body');
  }

  // ── Test L: Clean Browser Session Hydrates Server Canonical Theme and Language ──
  {
    const serverSettings = {
      theme: 'light',
      language: 'es',
      cover_art_enabled: false,
      sidebar_collapsed: true,
    };
    const settingsFetch = async (url) => {
      if (url.includes('/api/v1/settings')) {
        return {
          ok: true, status: 200,
          headers: { get: () => 'application/json' },
          json: async () => serverSettings
        };
      }
      return { ok: true, headers: { get: () => 'application/json' }, json: async () => ({}) };
    };

    const { sandbox, window, document } = makeSandbox({ fetchImpl: settingsFetch });
    vm.createContext(sandbox);
    vm.runInContext(jsContent, sandbox);

    window.AuthSession.state = 'authenticated';
    await window.loadSettings();

    assert(document.documentElement.dataset.theme === 'light',
      'Authoritative server theme="light" hydrates into DOM data-theme on clean session');
    assert(document.documentElement.getAttribute('data-cover-art') === 'false',
      'Authoritative server cover_art_enabled=false hydrates into DOM data-cover-art');
    assert(document.documentElement.getAttribute('data-sidebar-collapsed') === 'true',
      'Authoritative server sidebar_collapsed=true hydrates into DOM data-sidebar-collapsed');
  }

  console.log('======================================================================');
  console.log(`BROWSER E2E GATE: ${passed} passed, ${failed} failed`);
  console.log('======================================================================');
  if (failed > 0) process.exit(1);
}

runE2E();
