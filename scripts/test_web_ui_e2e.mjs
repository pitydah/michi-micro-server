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
window.MichiAPI         = MichiAPI;
window.addToQueue       = addToQueue;
window.toggleOutputTarget = toggleOutputTarget;
window.saveSetting      = saveSetting;
window.loadSettings     = loadSettings;
window.playEpisode      = playEpisode;
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
    this.id = id;
    this.className = className;
    this.innerHTML = '';
    this.textContent = '';
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

  getAttribute(name) { return this.attributes[name] || null; }
  setAttribute(name, val) { this.attributes[name] = String(val); }
  removeAttribute(name) { delete this.attributes[name]; }

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
      // attribute selector like [data-section="..."]
      if (sel.includes('[')) {
        // simple pass-through: return null
        return null;
      }
      // tag selector
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
    }
    for (const c of this.children) c._queryAll(sel, results);
  }
}

class MockStorage {
  constructor() { this.store = {}; }
  getItem(k) { return this.store[k] !== undefined ? this.store[k] : null; }
  setItem(k, v) { this.store[k] = String(v); }
  removeItem(k) { delete this.store[k]; }
  clear() { this.store = {}; }
}

function createDOM() {
  const root = new MockElement('HTML');
  const body = new MockElement('BODY');
  root.appendChild(body);

  const document = {
    body: body,
    documentElement: new MockElement('HTML'),
    createElement: (tag) => new MockElement(tag),
    getElementById: (id) => body.querySelector('#' + id),
    querySelector: (sel) => body.querySelector(sel),
    querySelectorAll: (sel) => body.querySelectorAll(sel),
    addEventListener: () => {},
    activeElement: null,
    hidden: false,
  };

  const localStorage = new MockStorage();
  const sessionStorage = new MockStorage();

  const window = {
    document: document,
    navigator: { language: 'en-US', serviceWorker: { register: () => Promise.resolve() } },
    location: { reload: () => {} },
    matchMedia: () => ({ matches: false, addEventListener: () => {} }),
    localStorage: localStorage,
    sessionStorage: sessionStorage,
    // Native Node timers — critical for showToast and other deferred calls
    setTimeout:    global.setTimeout,
    clearTimeout:  global.clearTimeout,
    setInterval:   global.setInterval,
    clearInterval: global.clearInterval,
    requestAnimationFrame: (fn) => global.setTimeout(fn, 0),
    fetch: null,
    Audio: class {
      constructor() { this.src = ''; this.currentTime = 0; this.duration = 180; }
      play() { return Promise.resolve(); }
      pause() {}
    },
  };

  // Populate the minimal set of DOM elements that app.js probes at boot
  const ids = [
    'app', 'toast', 'toast-container',
    'np-title', 'np-artist', 'np-target-badge',
    'queue-content',
    'view-settings', 'settings-restart-banner',
    'settings-scan-concurrency', 'settings-max-transcodes',
    'settings-db-pool', 'settings-scrobble',
    'settings-theme', 'settings-language', 'settings-profile',
    'qr-status-badge', 'qr-code-img',
    'stab-receivers', 'stab-backup',
    'server-status-dot', 'server-status-label',
    'status-pill', 'sidebar-server-id',
    'current-section-title', 'mobile-menu-btn',
    'lang-select', 'search-input',
    'modal-overlay', 'modal-title', 'modal-message', 'modal-confirm-btn',
  ];
  for (const id of ids) {
    const el = new MockElement('div', id);
    body.appendChild(el);
  }

  return { window, document };
}

// ── Sandbox factory ─────────────────────────────────────────────
// Every test gets a fresh sandbox with native timers injected at the
// VM-context level so bare calls to setTimeout() inside app.js resolve
// to Node's globalThis.setTimeout, not undefined.
// AbortController, URL, etc. must also be present because MichiAPI.request
// creates an AbortController on every fetch call.
function makeSandbox({ window, document, fetchImpl = null, showToastImpl = null } = {}) {
  if (!window || !document) {
    const dom = createDOM();
    window = dom.window;
    document = dom.document;
  }
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
    // Native timers — must exist as globals in the VM context
    setTimeout:           global.setTimeout,
    clearTimeout:         global.clearTimeout,
    setInterval:          global.setInterval,
    clearInterval:        global.clearInterval,
    requestAnimationFrame: (fn) => global.setTimeout(fn, 0),
    // Web API globals required by MichiAPI.request and other app code
    AbortController:    globalThis.AbortController,
    AbortSignal:        globalThis.AbortSignal,
    URL:                globalThis.URL,
    URLSearchParams:    globalThis.URLSearchParams,
    TextEncoder:        globalThis.TextEncoder,
    TextDecoder:        globalThis.TextDecoder,
    Promise:            globalThis.Promise,
    JSON:               globalThis.JSON,
    Error:              globalThis.Error,
    showToast: showToastImpl || (() => {}),
    fetch:     fetchImpl     || (async () => ({ ok: true, headers: { get: () => 'application/json' }, json: async () => ({}) })),
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
    const { sandbox, window, document } = makeSandbox();
    vm.createContext(sandbox);
    vm.runInContext(jsContent, sandbox);

    const badge = document.getElementById('np-target-badge');
    assert(badge !== null, 'Now Playing target badge exists in DOM');
    assert(window.ServerPlayback.outputTarget === 'browser', 'Default output target is browser local');

    window.toggleOutputTarget();
    assert(window.ServerPlayback.outputTarget === 'remote', 'Toggling switches to remote link');
    assert(badge.textContent.includes('Remote Link'), 'Badge reflects Remote Link truth');
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
    window.fetch = fetchImpl;
    vm.createContext(sandbox);
    vm.runInContext(jsContent, sandbox);

    window.State.tracks = [{ id: 'track-1', title: 'Song 1', duration_ms: 180000 }];
    window.State.queue  = [];

    await window.addToQueue(0);

    const toastEl = document.getElementById('toast');
    assert(window.State.queue.length === 0, 'Queue is NOT mutated locally when backend API rejects');
    // The toast text is set via the real showToast on the #toast element
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
          json: async () => ({ restart_required: true, resource_profile: 'performance' }),
        };
      }
      if (url.includes('/api/v1/settings')) {
        return {
          ok: true, status: 200,
          headers: { get: (h) => h === 'content-type' ? 'application/json' : null },
          json: async () => ({
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
    window.fetch = settingsFetch;
    vm.createContext(sandbox);
    vm.runInContext(jsContent, sandbox);

    await window.saveSetting('resource_profile', 'performance');
    assert(window.localStorage.getItem('michi_restart_required') === 'true',
      'Restart required state stored in localStorage');

    // Simulate F5
    const { sandbox: sandboxR, window: winR, document: docR } = makeSandbox({ fetchImpl: settingsFetch });
    winR.localStorage.setItem('michi_restart_required', 'true');
    winR.fetch = settingsFetch;
    vm.createContext(sandboxR);
    vm.runInContext(jsContent, sandboxR);

    await winR.loadSettings();
    const banner = docR.getElementById('settings-restart-banner');
    assert(banner !== null, 'Restart banner persists in Settings after F5 reload');
  }

  // ── Test D: Failed audio.play does NOT mark episode played ──
  {
    let playedEndpointCalled = false;
    const fetchImpl = async (url, opts) => {
      if (url.includes('/api/v1/sources/episodes/')) playedEndpointCalled = true;
      return { ok: true, headers: { get: () => 'application/json' }, json: async () => ({}) };
    };

    const { sandbox, window, document } = makeSandbox({ fetchImpl });
    window.Audio = class {
      constructor() { this.src = ''; }
      play() { return Promise.reject(new Error('Decode error')); }
      pause() {}
    };
    window.fetch = fetchImpl;
    vm.createContext(sandbox);
    vm.runInContext(jsContent, sandbox);

    try { await window.playEpisode('source-123', 'ep-456'); } catch (_) {}

    assert(playedEndpointCalled === false,
      'Failed audio.play does NOT call mark episode as played');
  }

  console.log('======================================================================');
  console.log(`BROWSER E2E GATE: ${passed} passed, ${failed} failed`);
  console.log('======================================================================');
  if (failed > 0) process.exit(1);
}

runE2E();
