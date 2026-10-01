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
const enDict = JSON.parse(fs.readFileSync(path.join(rootDir, 'crates/michi-api/static/i18n/en.json'), 'utf8'));

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
window.toggleShuffle    = toggleShuffle;
window.toggleRepeat     = toggleRepeat;
window.discoverDevices  = discoverDevices;
window.ReceiverPairingState  = ReceiverPairingState;
window.openReceiverPairModal = openReceiverPairModal;
window.closeReceiverPairModal = closeReceiverPairModal;
window.proceedToPairingPin   = proceedToPairingPin;
window.submitReceiverPairPin = submitReceiverPairPin;
window.formatPairTimer       = formatPairTimer;
window.handleSearch     = handleSearch;
window.toggleStar       = toggleStar;
window.reevaluateCurrentSectionAccess = reevaluateCurrentSectionAccess;
window.canPerformProtectedAction = canPerformProtectedAction;
window.getCanonicalServerVersion = getCanonicalServerVersion;
window.checkForUpdates  = checkForUpdates;
window.showSection      = showSection;
_i18n = Object.assign({}, window._initialI18n || {});
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
    this._innerHTML = '';
    this._textContent = '';
    this.style = {};
    this.children = [];
    this.parentNode = null;
    this.ownerDocument = null;
    this.attributes = {};
    this.value = '';
    this.onclick = null;
    this.eventListeners = {};
    this.dataset = {};
    this.offsetHeight = 0;
    this.disabled = false;
    this.classList = {
      _classes: new Set(className.split(/\s+/).filter(Boolean)),
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
    this.className = className;
  }

  get className() { return this._className || ''; }
  set className(val) {
    this._className = String(val);
    if (this.classList) {
      this.classList._classes = new Set(String(val).split(/\s+/).filter(Boolean));
    }
  }

  get textContent() {
    if (this.children.length > 0) {
      return this.children.map(c => c.textContent).join('');
    }
    if (this._textContent) return this._textContent;
    if (this._innerHTML) return this._innerHTML.replace(/<[^>]*>/g, '');
    return '';
  }
  set textContent(val) {
    this._textContent = String(val);
    this._innerHTML = String(val);
    this.children = [];
  }

  get innerHTML() { return this._innerHTML || ''; }
  set innerHTML(val) {
    this._innerHTML = String(val);
    this._textContent = String(val).replace(/<[^>]*>/g, '');
    this.children = [];
    parseHTMLInto(this, String(val), this.ownerDocument);
  }

  get id() { return this._id || ''; }
  set id(val) { this._id = val; }

  getAttribute(name) {
    if (name === 'id') return this.id || null;
    if (name === 'class') return this.className || null;
    return this.attributes[name] !== undefined ? this.attributes[name] : null;
  }
  setAttribute(name, val) {
    this.attributes[name] = String(val);
    if (name === 'id') this._id = String(val);
    if (name === 'class') {
      this.className = String(val);
      this.classList._classes = new Set(String(val).split(/\s+/).filter(Boolean));
    }
    if (name.startsWith('data-')) {
      const camel = name.slice(5).replace(/-([a-z])/g, (_, l) => l.toUpperCase());
      this.dataset[camel] = String(val);
    }
    if (name === 'disabled') this.disabled = true;
  }
  removeAttribute(name) {
    delete this.attributes[name];
    if (name === 'id') this._id = '';
    if (name === 'disabled') this.disabled = false;
  }

  appendChild(child) {
    child.parentNode = this;
    child.ownerDocument = this.ownerDocument;
    this.children.push(child);
    return child;
  }

  prepend(child) {
    child.parentNode = this;
    child.ownerDocument = this.ownerDocument;
    this.children.unshift(child);
    return child;
  }

  insertBefore(newNode, referenceNode) {
    newNode.parentNode = this;
    newNode.ownerDocument = this.ownerDocument;
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

  focus() {
    if (this.ownerDocument) {
      this.ownerDocument.activeElement = this;
    }
  }

  closest(sel) {
    let curr = this;
    while (curr) {
      if (curr._matches && curr._matches(sel)) return curr;
      curr = curr.parentNode;
    }
    return null;
  }

  _matches(sel) {
    if (!sel) return false;
    let remaining = sel;
    const tagMatch = remaining.match(/^([a-zA-Z0-9]+)/);
    if (tagMatch) {
      if (this.tagName !== tagMatch[1].toUpperCase()) return false;
      remaining = remaining.slice(tagMatch[0].length);
    }
    while (remaining.length > 0) {
      if (remaining.startsWith('#')) {
        const m = remaining.match(/^#([a-zA-Z0-9_-]+)/);
        if (!m) return false;
        if (this.id !== m[1]) return false;
        remaining = remaining.slice(m[0].length);
      } else if (remaining.startsWith('.')) {
        const m = remaining.match(/^\.([a-zA-Z0-9_-]+)/);
        if (!m) return false;
        if (!this.classList.contains(m[1]) && !this.className.split(/\s+/).includes(m[1])) return false;
        remaining = remaining.slice(m[0].length);
      } else if (remaining.startsWith('[')) {
        const m = remaining.match(/^\[([a-zA-Z0-9_-]+)(?:=("[^"]*"|'[^']*'|[^\]]+))?\]/);
        if (!m) return false;
        const attrName = m[1];
        const rawExpected = m[2];
        const val = this.getAttribute(attrName);
        if (rawExpected === undefined) {
          if (val === null && !this[attrName]) return false;
        } else {
          const expected = (rawExpected.startsWith('"') && rawExpected.endsWith('"')) ||
                           (rawExpected.startsWith("'") && rawExpected.endsWith("'"))
                           ? rawExpected.slice(1, -1) : rawExpected;
          if (String(val) !== expected) return false;
        }
        remaining = remaining.slice(m[0].length);
      } else {
        return false;
      }
    }
    return true;
  }

  querySelector(sel) { return this._query(sel); }
  querySelectorAll(sel) {
    const results = [];
    this._queryAll(sel, results);
    return results;
  }

  _query(sel) {
    if (sel.includes(' ')) {
      const parts = sel.trim().split(/\s+/);
      const first = this._query(parts[0]);
      if (first) return first._query(parts.slice(1).join(' '));
      return null;
    }
    for (const c of this.children) {
      if (c._matches(sel)) return c;
      const found = c._query(sel);
      if (found) return found;
    }
    return null;
  }

  _queryAll(sel, results) {
    for (const c of this.children) {
      if (c._matches(sel)) results.push(c);
      c._queryAll(sel, results);
    }
  }

  remove() {
    if (this.parentNode) {
      const idx = this.parentNode.children.indexOf(this);
      if (idx >= 0) this.parentNode.children.splice(idx, 1);
      this.parentNode = null;
    }
  }
}

function parseHTMLInto(parent, html, doc) {
  const tagRegex = /<\/?([a-zA-Z0-9-]+)((?:\s+[^=>\/\s]+(?:=(?:"[^"]*"|'[^']*'|[^>\s]+))?)*)\s*(\/?)>|([^<]+)/g;
  let match;
  let current = parent;
  const stack = [parent];
  const VOID_TAGS = new Set(['IMG', 'INPUT', 'BR', 'HR', 'META', 'LINK']);

  while ((match = tagRegex.exec(html)) !== null) {
    const [full, tagName, attrStr, selfClose, textContent] = match;
    if (textContent) {
      if (textContent.trim()) {
        current._textContent = (current._textContent || '') + textContent;
      }
      continue;
    }
    if (full.startsWith('</')) {
      if (stack.length > 1) {
        const closeTag = tagName.toUpperCase();
        let idx = stack.length - 1;
        while (idx > 0 && stack[idx].tagName !== closeTag) {
          idx--;
        }
        if (idx > 0) {
          stack.splice(idx);
          current = stack[stack.length - 1];
        } else {
          stack.pop();
          current = stack[stack.length - 1];
        }
      }
      continue;
    }
    const tag = tagName.toUpperCase();
    const el = new MockElement(tag);
    el.parentNode = current;
    el.ownerDocument = doc || parent.ownerDocument;

    if (attrStr) {
      const attrRegex = /([a-zA-Z0-9_-]+)(?:=(?:"([^"]*)"|'([^']*)'|([^\s>]+)))?/g;
      let am;
      while ((am = attrRegex.exec(attrStr)) !== null) {
        const name = am[1];
        const val = am[2] !== undefined ? am[2] : (am[3] !== undefined ? am[3] : (am[4] !== undefined ? am[4] : ''));
        el.attributes[name] = val;
        if (name === 'id') el.id = val;
        if (name === 'class') {
          el.className = val;
          val.split(/\s+/).filter(Boolean).forEach(c => el.classList.add(c));
        }
        if (name === 'disabled') el.disabled = true;
        if (name.startsWith('data-')) {
          const camel = name.slice(5).replace(/-([a-z])/g, (_, l) => l.toUpperCase());
          el.dataset[camel] = val;
        }
      }
    }
    current.children.push(el);
    if (!selfClose && !VOID_TAGS.has(tag)) {
      stack.push(el);
      current = el;
    }
  }
}

function createDOM() {
  function el(tag, id = '', cls = '') {
    const node = new MockElement(tag, id, cls);
    node.ownerDocument = doc;
    return node;
  }

  const doc = {
    documentElement: null,
    body: null,
    head: null,
    activeElement: null,
    createElement: (tag) => el(tag),
    getElementById: (id) => doc.body.querySelector('#' + id),
    querySelector: (sel) => {
      if (doc.body && doc.body._matches(sel)) return doc.body;
      return doc.body ? doc.body.querySelector(sel) : null;
    },
    querySelectorAll: (sel) => doc.body ? doc.body.querySelectorAll(sel) : [],
    addEventListener: () => {},
  };
  doc.documentElement = el('html');
  doc.body = el('body');
  doc.head = el('head');

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
    'settings-watcher-status',
    'ha-discovery-status', 'integ-sync-peers', 'integ-reconnect-max',
    'diag-status', 'diag-ffmpeg', 'diag-transcodes', 'diag-db-pool', 'diag-caps-list',
    'jobs-max-concurrent', 'jobs-list',
    'update-status-badge', 'update-current-version',
    'handoff-track-id', 'handoff-position', 'handoff-playing', 'handoff-result', 'handoff-current-state',
    'discover-result'
  ];
  for (const sid of settingIds) {
    pageSettings.appendChild(el('div', sid));
  }

  doc.body.appendChild(toast);
  doc.body.appendChild(npTargetBadge);

  const searchInput = el('input', 'search-input');
  const authOverlay = el('div', 'auth-overlay');
  authOverlay.classList.add('hidden');
  doc.body.appendChild(searchInput);
  doc.body.appendChild(authOverlay);

  const stabDevices = el('div', 'stab-devices');
  stabDevices.innerHTML = '<span id="discovery-status-badge" class="discovery-status-badge discovery-status-badge--unavailable"><span class="discovery-status-dot"></span><span id="discovery-status-text">Discovery unavailable</span></span>';
  doc.body.appendChild(stabDevices);

  const pairModal = el('div', 'receiver-pair-modal');
  pairModal.classList.add('hidden');
  const pairModalTitle = el('div', 'pair-modal-title');
  const stepButton = el('div', 'pair-step-button');
  const stepButtonDesc = el('p', 'pair-step-button-desc');
  stepButtonDesc.textContent = 'Press and hold the button on your Michi Music Stream for 5 seconds until the status indicator starts flashing.';
  const btnPairReady = el('button', 'btn-pair-ready');
  btnPairReady.textContent = 'Continue';
  stepButton.appendChild(stepButtonDesc);
  stepButton.appendChild(btnPairReady);

  const stepPin = el('div', 'pair-step-pin');
  stepPin.classList.add('hidden');
  const pinInput = el('input', 'pair-pin-input');
  const pinTimerVal = el('span', 'pair-timer-val');
  const pinError = el('div', 'pair-pin-error');
  const btnPairConfirm = el('button', 'btn-pair-confirm');
  stepPin.appendChild(pinInput);
  stepPin.appendChild(pinTimerVal);
  stepPin.appendChild(pinError);
  stepPin.appendChild(btnPairConfirm);

  const stepSuccess = el('div', 'pair-step-success');
  stepSuccess.classList.add('hidden');
  const successTitle = el('p', 'pair-success-title');
  const successMsg = el('p', 'pair-success-message');
  const btnPairDone = el('button', 'btn-pair-done');
  btnPairDone.textContent = 'Done';
  stepSuccess.appendChild(successTitle);
  stepSuccess.appendChild(successMsg);
  stepSuccess.appendChild(btnPairDone);

  pairModal.appendChild(pairModalTitle);
  pairModal.appendChild(stepButton);
  pairModal.appendChild(stepPin);
  pairModal.appendChild(stepSuccess);

  doc.body.appendChild(pairModal);
  doc.body.appendChild(el('span', 'devices-last-updated'));

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
  window._initialI18n = enDict;

  const sandbox = {
    window,
    document,
    navigator:      window.navigator,
    localStorage:   window.localStorage,
    sessionStorage: window.sessionStorage,
    console: { warn: () => {}, error: () => {}, log: () => {} },
    $:  (s) => document.querySelector(s),
    $$: (s) => document.querySelectorAll(s),
    t:       (k, vars) => {
      let str = enDict[k] || k;
      if (vars) {
        for (const [vKey, val] of Object.entries(vars)) {
          str = str.replace(new RegExp('\\{' + vKey + '\\}', 'g'), val);
        }
      }
      return str;
    },
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

    window.AuthSession.state = 'authenticated';
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

    window.AuthSession.state = 'authenticated';
    window.ServerPlayback.outputTarget = 'server';
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

    window.AuthSession.state = 'authenticated';
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

    window.AuthSession.state = 'authenticated';
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

  // ── Regression Test 1: Podcast updateEpisode must target canonical route ──
  {
    let requestedUrl = null;
    const fetchImpl = async (url, opts) => {
      requestedUrl = url;
      return { ok: true, status: 200, headers: { get: () => 'application/json' }, json: async () => ({ status: 'ok' }) };
    };
    const { sandbox, window } = makeSandbox({ fetchImpl });
    vm.createContext(sandbox);
    vm.runInContext(jsContent, sandbox);

    await window.MichiAPI.updateEpisode('ep-123', 5000, false);
    assert(requestedUrl && requestedUrl.includes('/api/v1/sources/episodes/ep-123'),
      `REGRESSION: updateEpisode called '${requestedUrl}', expected canonical '/api/v1/sources/episodes/ep-123'`);
  }

  // ── Regression Test 2: Receiver discovery must parse 'receivers' key ──
  {
    const fetchImpl = async (url) => {
      if (url.includes('/api/v1/devices/discover')) {
        return {
          ok: true, status: 200,
          headers: { get: () => 'application/json' },
          json: async () => ({ receivers: [{ id: 'rx-1', name: 'Living Room Receiver' }] })
        };
      }
      return { ok: true, headers: { get: () => 'application/json' }, json: async () => ({}) };
    };
    const { sandbox, window, document } = makeSandbox({ fetchImpl });
    vm.createContext(sandbox);
    vm.runInContext(jsContent, sandbox);

    window.AuthSession.state = 'authenticated';
    await window.discoverDevices();
    const resultText = document.querySelector('#discover-result')?.textContent || '';
    assert(resultText.includes('Living Room Receiver'),
      `REGRESSION: discoverDevices rendered '${resultText}', expected 'Living Room Receiver' from receivers key`);
  }

  // ── Regression Test 3: Browser output authority must not mutate server queue ──
  {
    let serverQueueCalled = false;
    const fetchImpl = async (url) => {
      if (url.includes('/api/v1/queue')) {
        serverQueueCalled = true;
      }
      return { ok: true, headers: { get: () => 'application/json' }, json: async () => ({}) };
    };
    const { sandbox, window } = makeSandbox({ fetchImpl });
    vm.createContext(sandbox);
    vm.runInContext(jsContent, sandbox);

    window.AuthSession.state = 'authenticated';
    window.ServerPlayback.outputTarget = 'browser';
    window.State.tracks = [{ id: 'track-1', title: 'Browser Local Track' }];

    await window.addToQueue(0);
    assert(!serverQueueCalled,
      'REGRESSION: addToQueue in browser output mode called server /api/v1/queue, violating authority separation');
  }

  // ── Auth Perimeter Test 1: Search blocked when anonymous ──
  {
    let searchCalled = false;
    const fetchImpl = async (url) => {
      if (url.includes('/api/v1/search')) searchCalled = true;
      return { ok: true, headers: { get: () => 'application/json' }, json: async () => ({ tracks: [] }) };
    };
    const { sandbox, window, document } = makeSandbox({ fetchImpl });
    vm.createContext(sandbox);
    vm.runInContext(jsContent, sandbox);

    window.AuthSession.state = 'anonymous';
    const searchInput = document.querySelector('#search-input');
    if (searchInput) searchInput.value = 'Radiohead';

    await window.handleSearch();
    assert(!searchCalled, 'PERIMETER: Anonymous search must not call /api/v1/search');
    const toastEl = document.getElementById('toast');
    assert(toastEl && toastEl.textContent.includes('Sign in required'), 'PERIMETER: Anonymous search displays sign-in required toast');
    const authOverlay = document.querySelector('#auth-overlay');
    assert(authOverlay && !authOverlay.classList.contains('hidden'), 'PERIMETER: Anonymous search opens auth modal');
  }

  // ── Auth Perimeter Test 2: Search blocked when auth disabled ──
  {
    let searchCalled = false;
    const fetchImpl = async (url) => {
      if (url.includes('/api/v1/search')) searchCalled = true;
      return { ok: true, headers: { get: () => 'application/json' }, json: async () => ({ tracks: [] }) };
    };
    const { sandbox, window, document } = makeSandbox({ fetchImpl });
    vm.createContext(sandbox);
    vm.runInContext(jsContent, sandbox);

    window.AuthSession.state = 'disabled';
    const searchInput = document.querySelector('#search-input');
    if (searchInput) searchInput.value = 'Radiohead';

    await window.handleSearch();
    assert(!searchCalled, 'PERIMETER: Disabled auth search must not call /api/v1/search');
    const toastEl = document.getElementById('toast');
    assert(toastEl && toastEl.textContent.includes('Administrative actions are unavailable'),
      'PERIMETER: Disabled auth search displays disabled notice toast');
    const authOverlay = document.querySelector('#auth-overlay');
    assert(authOverlay && authOverlay.classList.contains('hidden'),
      'PERIMETER: Disabled auth search does NOT open auth modal');
  }

  // ── Auth Perimeter Test 3: Star track blocked when anonymous ──
  {
    let starCalled = false;
    const fetchImpl = async (url) => {
      if (url.includes('/star')) starCalled = true;
      return { ok: true, headers: { get: () => 'application/json' }, json: async () => ({ status: 'ok' }) };
    };
    const { sandbox, window } = makeSandbox({ fetchImpl });
    vm.createContext(sandbox);
    vm.runInContext(jsContent, sandbox);

    window.AuthSession.state = 'anonymous';
    window.State.tracks = [{ id: 't-1', title: 'Song 1', starred: false }];

    await window.toggleStar(0);
    assert(!starCalled, 'PERIMETER: Anonymous star track must not call /star');
    assert(window.State.tracks[0].starred === false, 'PERIMETER: Track star state unchanged');
  }

  // ── Truthful UX Test 4: Section gating renders disabled notice without sign-in button ──
  {
    const { sandbox, window, document } = makeSandbox();
    vm.createContext(sandbox);
    vm.runInContext(jsContent, sandbox);

    window.AuthSession.state = 'disabled';
    const settingsPage = document.querySelector('#page-settings');
    assert(settingsPage !== null, 'Settings page exists in DOM');

    window.showSection('settings');

    const gate = settingsPage.querySelector('.auth-required-gate');
    assert(gate !== null, 'Auth gate element injected into settings page');
    assert(gate.textContent.includes('Administrative access unavailable'),
      'TRUTHFUL UX: Disabled gate renders "Administrative access unavailable" title');
    assert(!gate.innerHTML.includes('<button'),
      'TRUTHFUL UX: Disabled gate contains NO sign-in button');
  }

  // ── Truthful UX Test 5: Section gating renders sign-in button when anonymous ──
  {
    const { sandbox, window, document } = makeSandbox();
    vm.createContext(sandbox);
    vm.runInContext(jsContent, sandbox);

    window.AuthSession.state = 'anonymous';
    const settingsPage = document.querySelector('#page-settings');
    window.showSection('settings');

    const gate = settingsPage.querySelector('.auth-required-gate');
    assert(gate !== null, 'Auth gate element injected into settings page');
    assert(gate.textContent.includes('Sign in required'),
      'TRUTHFUL UX: Anonymous gate renders "Sign in required"');
    assert(gate.innerHTML.includes('<button') && gate.innerHTML.includes('Sign in to Michi'),
      'TRUTHFUL UX: Anonymous gate contains "Sign in to Michi" button');
  }

  // ── Truthful UX Test 6: Removal of magic defaults across diagnostics, jobs, integrations ──
  {
    const fetchImpl = async (url) => {
      if (url.includes('/api/v1/diagnostics')) {
        return {
          ok: true, status: 200,
          headers: { get: () => 'application/json' },
          json: async () => ({ healthy: true, degraded: false })
        };
      }
      if (url.includes('/api/v1/settings')) {
        return {
          ok: true, status: 200,
          headers: { get: () => 'application/json' },
          json: async () => ({}) // Empty settings object without workers/pool/jobs/reconnect
        };
      }
      if (url.includes('/api/v1/jobs')) {
        return {
          ok: true, status: 200,
          headers: { get: () => 'application/json' },
          json: async () => ({ jobs: [] })
        };
      }
      return { ok: true, headers: { get: () => 'application/json' }, json: async () => ({}) };
    };

    const { sandbox, window, document } = makeSandbox({ fetchImpl });
    vm.createContext(sandbox);
    vm.runInContext(jsContent, sandbox);

    window.AuthSession.state = 'authenticated';
    await window.loadDiagnostics();
    const transEl = document.querySelector('#diag-transcodes');
    assert(transEl && transEl.textContent === 'Capacity: Unavailable',
      'TRUTHFUL UX: Missing effective_transcode_workers renders Capacity: Unavailable (not 0)');

    const poolEl = document.querySelector('#diag-db-pool');
    assert(poolEl && poolEl.textContent.includes('Unavailable'),
      'TRUTHFUL UX: Missing effective_db_pool renders Unavailable (not 8)');

    await window.loadJobs();
    const maxEl = document.querySelector('#jobs-max-concurrent');
    assert(maxEl && maxEl.textContent === 'Unavailable',
      'TRUTHFUL UX: Missing job_max_concurrent renders Unavailable (not 2)');

    await window.loadIntegrations();
    const delayEl = document.querySelector('#integ-reconnect-max');
    assert(delayEl && delayEl.textContent === 'Unavailable',
      'TRUTHFUL UX: Missing reconnect_delay_max renders Unavailable (not 300)');
  }

  // ── Truthful UX Test 7: Canonical Version authority and Watcher truth in loadSettings ──
  {
    const fetchImpl = async (url) => {
      if (url.includes('/api/v1/server/info')) {
        return {
          ok: true, status: 200,
          headers: { get: () => 'application/json' },
          json: async () => ({ version: '1.0.0-rc.2' })
        };
      }
      if (url.includes('/api/v1/modules')) {
        return {
          ok: true, status: 200,
          headers: { get: () => 'application/json' },
          json: async () => ({ modules: [] }) // scan module missing
        };
      }
      if (url.includes('/api/v1/settings')) {
        return {
          ok: true, status: 200,
          headers: { get: () => 'application/json' },
          json: async () => ({ port: 9090 })
        };
      }
      return { ok: true, headers: { get: () => 'application/json' }, json: async () => ({}) };
    };

    const { sandbox, window, document } = makeSandbox({ fetchImpl });
    vm.createContext(sandbox);
    vm.runInContext(jsContent, sandbox);

    window.AuthSession.state = 'authenticated';
    await window.loadSettings();

    const verEl = document.querySelector('#settings-version');
    assert(verEl && verEl.textContent === '1.0.0-rc.2',
      'TRUTHFUL UX: Canonical server version authority hydrates into #settings-version');

    const watcherEl = document.querySelector('#settings-watcher-status');
    assert(watcherEl && watcherEl.textContent.includes('Status Unavailable'),
      'TRUTHFUL UX: Missing scan module displays Status Unavailable');
  }

  // ── Truthful UX Test 8: Updater check displays neutral update.checking string ──
  {
    let resolveCheck;
    const fetchImpl = async (url) => {
      if (url.includes('/api/v1/update/check')) {
        await new Promise((r) => { resolveCheck = r; });
        return {
          ok: true, status: 200,
          headers: { get: () => 'application/json' },
          json: async () => ({ status: 'up_to_date' })
        };
      }
      return { ok: true, headers: { get: () => 'application/json' }, json: async () => ({}) };
    };
    const { sandbox, window, document } = makeSandbox({ fetchImpl });
    vm.createContext(sandbox);
    vm.runInContext(jsContent, sandbox);

    const checkPromise = window.checkForUpdates();
    const badgeEl = document.querySelector('#update-status-badge');
    assert(badgeEl && badgeEl.textContent === 'Checking for updates...',
      'TRUTHFUL UX: Update check badge uses neutral string, never "Checking upstream..."');
    if (resolveCheck) resolveCheck();
    await checkPromise;
  }

  // ── I18n Completeness Test 9: Parity between en.json and es.json ──
  {
    const enPath = path.join(rootDir, 'crates/michi-api/static/i18n/en.json');
    const esPath = path.join(rootDir, 'crates/michi-api/static/i18n/es.json');
    const enKeys = Object.keys(JSON.parse(fs.readFileSync(enPath, 'utf8'))).sort();
    const esKeys = Object.keys(JSON.parse(fs.readFileSync(esPath, 'utf8'))).sort();

    const missingInEs = enKeys.filter(k => !esKeys.includes(k));
    const missingInEn = esKeys.filter(k => !enKeys.includes(k));

    assert(missingInEs.length === 0, `I18N PARITY: Keys present in en.json missing in es.json: ${missingInEs.join(', ')}`);
    assert(missingInEn.length === 0, `I18N PARITY: Keys present in es.json missing in en.json: ${missingInEn.join(', ')}`);

    const requiredKeys = [
      'auth.disabled_title',
      'auth.disabled_explanation',
      'auth.signin_required_title',
      'auth.signin_required_desc',
      'auth.signin_action',
      'update.checking'
    ];
    for (const k of requiredKeys) {
      assert(enKeys.includes(k), `I18N PARITY: Required key '${k}' must exist in en.json`);
      assert(esKeys.includes(k), `I18N PARITY: Required key '${k}' must exist in es.json`);
    }
  }

  // ── Test K: Output Selector Zero-Request Perimeter ────────────
  {
    let outputCalls = [];
    const trackingFetch = async (url, opts) => {
      if (url.includes('/receivers') || url.includes('/rooms') || url.includes('/chains') || url.includes('/playback/output')) {
        outputCalls.push(url);
      }
      return { ok: true, status: 200, headers: { get: () => 'application/json' }, json: async () => ({}) };
    };

    const { sandbox, window, document } = makeSandbox({ fetchImpl: trackingFetch });
    vm.createContext(sandbox);
    vm.runInContext(jsContent, sandbox);

    // Initial state is anonymous
    window.AuthSession.state = 'anonymous';
    await window.showOutputSelectorModal();

    assert(outputCalls.length === 0, 'PERIMETER: Anonymous showOutputSelectorModal must not issue protected network requests');
    const authOverlay = document.getElementById('auth-overlay');
    assert(authOverlay && !authOverlay.classList.contains('hidden'), 'PERIMETER: Anonymous showOutputSelectorModal opens auth modal');
  }

  // ── 22 Explicit Devices & Pairing UX Integrity Tests ──────────
  
  // 1. Initial non-silent discoverDevices shows "Refreshing Michi devices..." before resolution
  {
    let resolveDiscover;
    const fetchImpl = async (url) => {
      if (url.includes('/api/v1/devices/discover')) {
        await new Promise((r) => { resolveDiscover = r; });
        return { ok: true, status: 200, headers: { get: () => 'application/json' }, json: async () => ({ receivers: [] }) };
      }
      return { ok: true, headers: { get: () => 'application/json' }, json: async () => ({}) };
    };
    const { sandbox, window, document } = makeSandbox({ fetchImpl });
    vm.createContext(sandbox);
    vm.runInContext(jsContent, sandbox);
    window.AuthSession.state = 'authenticated';

    const p = window.discoverDevices(false);
    const text = document.querySelector('#discover-result')?.textContent || '';
    assert(text.includes('Refreshing Michi devices...'), 'UX TEST 1: Initial load shows "Refreshing Michi devices..."');
    if (resolveDiscover) resolveDiscover();
    await p;
  }

  // 2. Initial non-silent discoverDevices shows skeleton cards before resolution
  {
    let resolveDiscover;
    const fetchImpl = async (url) => {
      if (url.includes('/api/v1/devices/discover')) {
        await new Promise((r) => { resolveDiscover = r; });
        return { ok: true, status: 200, headers: { get: () => 'application/json' }, json: async () => ({ receivers: [] }) };
      }
      return { ok: true, headers: { get: () => 'application/json' }, json: async () => ({}) };
    };
    const { sandbox, window, document } = makeSandbox({ fetchImpl });
    vm.createContext(sandbox);
    vm.runInContext(jsContent, sandbox);
    window.AuthSession.state = 'authenticated';

    const p = window.discoverDevices(false);
    const html = document.querySelector('#discover-result')?.innerHTML || '';
    assert(html.includes('device-card--skeleton'), 'UX TEST 2: Initial load shows skeleton cards');
    if (resolveDiscover) resolveDiscover();
    await p;
  }

  // 3. Empty receivers array renders empty state message
  {
    const fetchImpl = async (url) => {
      if (url.includes('/api/v1/devices/discover')) {
        return { ok: true, status: 200, headers: { get: () => 'application/json' }, json: async () => ({ receivers: [] }) };
      }
      return { ok: true, headers: { get: () => 'application/json' }, json: async () => ({}) };
    };
    const { sandbox, window, document } = makeSandbox({ fetchImpl });
    vm.createContext(sandbox);
    vm.runInContext(jsContent, sandbox);
    window.AuthSession.state = 'authenticated';

    await window.discoverDevices();
    const text = document.querySelector('#discover-result')?.textContent || '';
    const html = document.querySelector('#discover-result')?.innerHTML || '';
    assert(text.includes('No Michi Stream devices found'), 'UX TEST 3: Empty receivers renders empty state');
    assert(html.includes('Refresh Devices') && html.includes('device-empty-icon'), 'UX TEST 3: Empty state contains stream icon and refresh button');
  }

  // 4. Unpaired device with pairable: true renders enabled Pair button
  {
    const fetchImpl = async (url) => {
      if (url.includes('/api/v1/devices/discover')) {
        return { ok: true, status: 200, headers: { get: () => 'application/json' }, json: async () => ({
          receivers: [{ receiver_id: 'rx-pairable', name: 'Pairable Stream', paired: false, pairable: true }]
        }) };
      }
      return { ok: true, headers: { get: () => 'application/json' }, json: async () => ({}) };
    };
    const { sandbox, window, document } = makeSandbox({ fetchImpl });
    vm.createContext(sandbox);
    vm.runInContext(jsContent, sandbox);
    window.AuthSession.state = 'authenticated';

    await window.discoverDevices();
    const pairBtn = document.querySelector('.action-pair[data-receiver-id="rx-pairable"]');
    assert(pairBtn && !pairBtn.disabled, 'UX TEST 4: pairable=true renders enabled Pair button');
  }

  // 5. Unpaired device with pairable: false renders disabled Pair button
  {
    const fetchImpl = async (url) => {
      if (url.includes('/api/v1/devices/discover')) {
        return { ok: true, status: 200, headers: { get: () => 'application/json' }, json: async () => ({
          receivers: [{ receiver_id: 'rx-unpairable', name: 'Unpairable Stream', paired: false, pairable: false }]
        }) };
      }
      return { ok: true, headers: { get: () => 'application/json' }, json: async () => ({}) };
    };
    const { sandbox, window, document } = makeSandbox({ fetchImpl });
    vm.createContext(sandbox);
    vm.runInContext(jsContent, sandbox);
    window.AuthSession.state = 'authenticated';

    await window.discoverDevices();
    const pairBtn = document.querySelector('.action-pair[data-receiver-id="rx-unpairable"]');
    assert(pairBtn && pairBtn.disabled, 'UX TEST 5: pairable=false renders disabled Pair button');
  }

  // 6. Unpaired device with pairable omitted defaults to disabled (strict fail-closed)
  {
    const fetchImpl = async (url) => {
      if (url.includes('/api/v1/devices/discover')) {
        return { ok: true, status: 200, headers: { get: () => 'application/json' }, json: async () => ({
          receivers: [{ receiver_id: 'rx-omit-pairable', name: 'Omitted Pairable', paired: false }]
        }) };
      }
      return { ok: true, headers: { get: () => 'application/json' }, json: async () => ({}) };
    };
    const { sandbox, window, document } = makeSandbox({ fetchImpl });
    vm.createContext(sandbox);
    vm.runInContext(jsContent, sandbox);
    window.AuthSession.state = 'authenticated';

    await window.discoverDevices();
    const pairBtn = document.querySelector('.action-pair[data-receiver-id="rx-omit-pairable"]');
    assert(pairBtn && pairBtn.disabled, 'UX TEST 6: omitted pairable defaults to disabled (fail-closed)');
  }

  // 7. Paired + verified_online + qualified renders enabled Use as Output and Forget
  {
    const fetchImpl = async (url) => {
      if (url.includes('/api/v1/devices/discover')) {
        return { ok: true, status: 200, headers: { get: () => 'application/json' }, json: async () => ({
          receivers: [{ receiver_id: 'rx-ready', name: 'Living Room', paired: true, presence: 'verified_online', qualification: 'qualified' }]
        }) };
      }
      return { ok: true, headers: { get: () => 'application/json' }, json: async () => ({}) };
    };
    const { sandbox, window, document } = makeSandbox({ fetchImpl });
    vm.createContext(sandbox);
    vm.runInContext(jsContent, sandbox);
    window.AuthSession.state = 'authenticated';

    await window.discoverDevices();
    const outputBtn = document.querySelector('.action-use-output[data-receiver-id="rx-ready"]');
    const unpairBtn = document.querySelector('.action-unpair[data-receiver-id="rx-ready"]');
    assert(outputBtn && !outputBtn.disabled, 'UX TEST 7: verified_online + qualified enables Use as Output');
    assert(unpairBtn && !unpairBtn.disabled, 'UX TEST 7: paired receiver enables Forget');
  }

  // 8. Paired + provisional_mdns disables Use as Output and enables Forget
  {
    const fetchImpl = async (url) => {
      if (url.includes('/api/v1/devices/discover')) {
        return { ok: true, status: 200, headers: { get: () => 'application/json' }, json: async () => ({
          receivers: [{ receiver_id: 'rx-prov', name: 'Den Stream', paired: true, presence: 'provisional_mdns', qualification: 'qualified' }]
        }) };
      }
      return { ok: true, headers: { get: () => 'application/json' }, json: async () => ({}) };
    };
    const { sandbox, window, document } = makeSandbox({ fetchImpl });
    vm.createContext(sandbox);
    vm.runInContext(jsContent, sandbox);
    window.AuthSession.state = 'authenticated';

    await window.discoverDevices();
    const outputBtn = document.querySelector('.action-use-output[data-receiver-id="rx-prov"]');
    const unpairBtn = document.querySelector('.action-unpair[data-receiver-id="rx-prov"]');
    assert(outputBtn && outputBtn.disabled, 'UX TEST 8: provisional_mdns disables Use as Output');
    assert(unpairBtn && !unpairBtn.disabled, 'UX TEST 8: provisional_mdns enables Forget');
  }

  // 9. Paired + offline disables Use as Output and enables Forget
  {
    const fetchImpl = async (url) => {
      if (url.includes('/api/v1/devices/discover')) {
        return { ok: true, status: 200, headers: { get: () => 'application/json' }, json: async () => ({
          receivers: [{ receiver_id: 'rx-off', name: 'Patio Stream', paired: true, presence: 'offline', qualification: 'qualified' }]
        }) };
      }
      return { ok: true, headers: { get: () => 'application/json' }, json: async () => ({}) };
    };
    const { sandbox, window, document } = makeSandbox({ fetchImpl });
    vm.createContext(sandbox);
    vm.runInContext(jsContent, sandbox);
    window.AuthSession.state = 'authenticated';

    await window.discoverDevices();
    const outputBtn = document.querySelector('.action-use-output[data-receiver-id="rx-off"]');
    const unpairBtn = document.querySelector('.action-unpair[data-receiver-id="rx-off"]');
    assert(outputBtn && outputBtn.disabled, 'UX TEST 9: offline disables Use as Output');
    assert(unpairBtn && !unpairBtn.disabled, 'UX TEST 9: offline enables Forget');
  }

  // 10. Paired + unqualified disables Use as Output
  {
    const fetchImpl = async (url) => {
      if (url.includes('/api/v1/devices/discover')) {
        return { ok: true, status: 200, headers: { get: () => 'application/json' }, json: async () => ({
          receivers: [{ receiver_id: 'rx-unqual', name: 'Kitchen Stream', paired: true, presence: 'verified_online', qualification: 'unqualified' }]
        }) };
      }
      return { ok: true, headers: { get: () => 'application/json' }, json: async () => ({}) };
    };
    const { sandbox, window, document } = makeSandbox({ fetchImpl });
    vm.createContext(sandbox);
    vm.runInContext(jsContent, sandbox);
    window.AuthSession.state = 'authenticated';

    await window.discoverDevices();
    const outputBtn = document.querySelector('.action-use-output[data-receiver-id="rx-unqual"]');
    assert(outputBtn && outputBtn.disabled, 'UX TEST 10: unqualified disables Use as Output even if online');
  }

  // 11. Device with qualification: 'identity_mismatch' renders Identity Conflict badge
  {
    const fetchImpl = async (url) => {
      if (url.includes('/api/v1/devices/discover')) {
        return { ok: true, status: 200, headers: { get: () => 'application/json' }, json: async () => ({
          receivers: [{ receiver_id: 'rx-conflict', name: 'Suspect Stream', paired: false, qualification: 'identity_mismatch' }]
        }) };
      }
      return { ok: true, headers: { get: () => 'application/json' }, json: async () => ({}) };
    };
    const { sandbox, window, document } = makeSandbox({ fetchImpl });
    vm.createContext(sandbox);
    vm.runInContext(jsContent, sandbox);
    window.AuthSession.state = 'authenticated';

    await window.discoverDevices();
    const html = document.querySelector('#discover-result')?.innerHTML || '';
    assert(html.includes('Identity Conflict'), 'UX TEST 11: identity_mismatch renders Identity Conflict badge');
  }

  // 12. Device with qualification: 'identity_mismatch' renders warning explanation
  {
    const fetchImpl = async (url) => {
      if (url.includes('/api/v1/devices/discover')) {
        return { ok: true, status: 200, headers: { get: () => 'application/json' }, json: async () => ({
          receivers: [{ receiver_id: 'rx-conflict', name: 'Suspect Stream', paired: false, qualification: 'identity_mismatch' }]
        }) };
      }
      return { ok: true, headers: { get: () => 'application/json' }, json: async () => ({}) };
    };
    const { sandbox, window, document } = makeSandbox({ fetchImpl });
    vm.createContext(sandbox);
    vm.runInContext(jsContent, sandbox);
    window.AuthSession.state = 'authenticated';

    await window.discoverDevices();
    const html = document.querySelector('#discover-result')?.innerHTML || '';
    assert(html.includes('device-card__warning-text') && html.includes('Device identity could not be verified. Pairing and playback are disabled.'), 'UX TEST 12: renders warning explanation message');
  }

  // 13. Paired device with identity_mismatch disables Use as Output and enables Forget
  {
    const fetchImpl = async (url) => {
      if (url.includes('/api/v1/devices/discover')) {
        return { ok: true, status: 200, headers: { get: () => 'application/json' }, json: async () => ({
          receivers: [{ receiver_id: 'rx-p-conflict', name: 'Tampered Paired', paired: true, presence: 'verified_online', qualification: 'identity_mismatch' }]
        }) };
      }
      return { ok: true, headers: { get: () => 'application/json' }, json: async () => ({}) };
    };
    const { sandbox, window, document } = makeSandbox({ fetchImpl });
    vm.createContext(sandbox);
    vm.runInContext(jsContent, sandbox);
    window.AuthSession.state = 'authenticated';

    await window.discoverDevices();
    const outputBtn = document.querySelector('.action-use-output[data-receiver-id="rx-p-conflict"]');
    const unpairBtn = document.querySelector('.action-unpair[data-receiver-id="rx-p-conflict"]');
    assert(outputBtn && outputBtn.disabled, 'UX TEST 13: paired identity_mismatch disables Use as Output');
    assert(unpairBtn && !unpairBtn.disabled, 'UX TEST 13: paired identity_mismatch allows Forget');
  }

  // 14. Unpaired device with identity_mismatch disables Pair button
  {
    const fetchImpl = async (url) => {
      if (url.includes('/api/v1/devices/discover')) {
        return { ok: true, status: 200, headers: { get: () => 'application/json' }, json: async () => ({
          receivers: [{ receiver_id: 'rx-u-conflict', name: 'Tampered Unpaired', paired: false, pairable: true, qualification: 'identity_mismatch' }]
        }) };
      }
      return { ok: true, headers: { get: () => 'application/json' }, json: async () => ({}) };
    };
    const { sandbox, window, document } = makeSandbox({ fetchImpl });
    vm.createContext(sandbox);
    vm.runInContext(jsContent, sandbox);
    window.AuthSession.state = 'authenticated';

    await window.discoverDevices();
    const pairBtn = document.querySelector('.action-pair[data-receiver-id="rx-u-conflict"]');
    assert(pairBtn && pairBtn.disabled, 'UX TEST 14: unpaired identity_mismatch disables Pair');
  }

  // 15. discoverDevices updates #devices-last-updated with timestamp
  {
    const fetchImpl = async (url) => {
      if (url.includes('/api/v1/devices/discover')) {
        return { ok: true, status: 200, headers: { get: () => 'application/json' }, json: async () => ({ receivers: [] }) };
      }
      return { ok: true, headers: { get: () => 'application/json' }, json: async () => ({}) };
    };
    const { sandbox, window, document } = makeSandbox({ fetchImpl });
    vm.createContext(sandbox);
    vm.runInContext(jsContent, sandbox);
    window.AuthSession.state = 'authenticated';

    await window.discoverDevices();
    const tsText = document.querySelector('#devices-last-updated')?.textContent || '';
    assert(/^Updated at \d\d:\d\d:\d\d$/.test(tsText), `UX TEST 15: timestamp updated (${tsText})`);
  }

  // 16. discoverDevices polling with identical data preserves card fingerprint without re-mutating innerHTML
  {
    const fetchImpl = async (url) => {
      if (url.includes('/api/v1/devices/discover')) {
        return { ok: true, status: 200, headers: { get: () => 'application/json' }, json: async () => ({
          receivers: [{ receiver_id: 'rx-stable', name: 'Stable Stream', paired: false, pairable: true }]
        }) };
      }
      return { ok: true, headers: { get: () => 'application/json' }, json: async () => ({}) };
    };
    const { sandbox, window, document } = makeSandbox({ fetchImpl });
    vm.createContext(sandbox);
    vm.runInContext(jsContent, sandbox);
    window.AuthSession.state = 'authenticated';

    await window.discoverDevices();
    const fp1 = document.querySelector('#discover-result')?.dataset.cardsFingerprint;
    const cardEl = document.querySelector('#device-card-rx-stable');
    cardEl.marker = 'custom-property';

    // Poll with same data
    await window.discoverDevices(true);
    const fp2 = document.querySelector('#discover-result')?.dataset.cardsFingerprint;
    const cardElAfter = document.querySelector('#device-card-rx-stable');
    assert(fp1 === fp2, 'UX TEST 16: Fingerprint unchanged across identical poll');
    assert(cardElAfter && cardElAfter.marker === 'custom-property', 'UX TEST 16: DOM element preserved across reconciliation');
  }

  // 17. discoverDevices preserves focus on active action button across reconciliation
  {
    const fetchImpl = async (url) => {
      if (url.includes('/api/v1/devices/discover')) {
        return { ok: true, status: 200, headers: { get: () => 'application/json' }, json: async () => ({
          receivers: [{ receiver_id: 'rx-foc', name: 'Focus Stream', paired: false, pairable: true }]
        }) };
      }
      return { ok: true, headers: { get: () => 'application/json' }, json: async () => ({}) };
    };
    const { sandbox, window, document } = makeSandbox({ fetchImpl });
    vm.createContext(sandbox);
    vm.runInContext(jsContent, sandbox);
    window.AuthSession.state = 'authenticated';

    await window.discoverDevices();
    const btn = document.querySelector('.action-pair[data-receiver-id="rx-foc"]');
    document.activeElement = btn;

    // Mutate data slightly to force reconciliation
    window.MichiAPI.discoverDevices = async () => ({
      receivers: [{ receiver_id: 'rx-foc', name: 'Focus Stream Updated', paired: false, pairable: true }]
    });

    await window.discoverDevices(true);
    const newBtn = document.querySelector('.action-pair[data-receiver-id="rx-foc"]');
    assert(document.activeElement === newBtn, 'UX TEST 17: Focus preserved on active action button after reconciliation');
  }

  // 18. openReceiverPairModal reveals modal with Step 1 and hides Step 2 and Step 3
  {
    const { sandbox, window, document } = makeSandbox();
    vm.createContext(sandbox);
    vm.runInContext(jsContent, sandbox);

    window.openReceiverPairModal('rx-modal-test', 'Living Room');
    const modal = document.querySelector('#receiver-pair-modal');
    const step1 = document.querySelector('#pair-step-button');
    const step2 = document.querySelector('#pair-step-pin');
    const step3 = document.querySelector('#pair-step-success');

    assert(modal && !modal.classList.contains('hidden'), 'UX TEST 18: Modal is visible');
    assert(step1 && !step1.classList.contains('hidden'), 'UX TEST 18: Step 1 button prompt visible');
    assert(step2 && step2.classList.contains('hidden'), 'UX TEST 18: Step 2 PIN entry hidden');
    assert(step3 && step3.classList.contains('hidden'), 'UX TEST 18: Step 3 success hidden');
  }

  // 19. proceedToPairingPin renders countdown timer in mm:ss format
  {
    const fetchImpl = async (url) => {
      if (url.includes('/pair/start')) {
        const exp = new Date(Date.now() + 60000).toISOString();
        return { ok: true, status: 200, headers: { get: () => 'application/json' }, json: async () => ({
          pairing_id: 'pair-sess-1', expires_at: exp
        }) };
      }
      return { ok: true, headers: { get: () => 'application/json' }, json: async () => ({}) };
    };
    const { sandbox, window, document } = makeSandbox({ fetchImpl });
    vm.createContext(sandbox);
    vm.runInContext(jsContent, sandbox);

    window.openReceiverPairModal('rx-timer-test', 'Test Device');
    await window.proceedToPairingPin();

    const timerVal = document.querySelector('#pair-timer-val')?.textContent || '';
    assert(/^\d\d:\d\d$/.test(timerVal), `UX TEST 19: Timer rendered in mm:ss format (${timerVal})`);
    window.closeReceiverPairModal();
  }

  // 20. submitReceiverPairPin rejects invalid PIN without calling API
  {
    let confirmCalled = false;
    const fetchImpl = async (url) => {
      if (url.includes('/pair/confirm')) {
        confirmCalled = true;
        return { ok: true, status: 200, headers: { get: () => 'application/json' }, json: async () => ({ status: 'paired' }) };
      }
      return { ok: true, headers: { get: () => 'application/json' }, json: async () => ({}) };
    };
    const { sandbox, window, document } = makeSandbox({ fetchImpl });
    vm.createContext(sandbox);
    vm.runInContext(jsContent, sandbox);

    window.ReceiverPairingState.pairingId = 'pair-test-id';
    const pinInput = document.querySelector('#pair-pin-input');
    pinInput.value = '123'; // invalid length

    await window.submitReceiverPairPin();
    assert(!confirmCalled, 'UX TEST 20: Short PIN rejected without API request');
    const pinErr = document.querySelector('#pair-pin-error')?.textContent || '';
    assert(pinErr.includes('Enter the six-digit code'), 'UX TEST 20: Displays validation error for invalid PIN');
  }

  // 21. submitReceiverPairPin with presence='verified_online' displays truthful verified success
  {
    const fetchImpl = async (url) => {
      if (url.includes('/pair/confirm')) {
        return { ok: true, status: 200, headers: { get: () => 'application/json' }, json: async () => ({
          status: 'paired', presence: 'verified_online'
        }) };
      }
      return { ok: true, headers: { get: () => 'application/json' }, json: async () => ({}) };
    };
    const { sandbox, window, document } = makeSandbox({ fetchImpl });
    vm.createContext(sandbox);
    vm.runInContext(jsContent, sandbox);

    window.ReceiverPairingState.pairingId = 'pair-verified-id';
    const pinInput = document.querySelector('#pair-pin-input');
    pinInput.value = '654321';

    await window.submitReceiverPairPin();
    const title = document.querySelector('#pair-success-title')?.textContent || '';
    const msg = document.querySelector('#pair-success-message')?.textContent || '';
    assert(title.includes('Michi Music Stream paired'), `UX TEST 21: Step 3 shows Michi Music Stream paired (${title})`);
    assert(msg.includes('Identity verified. The receiver is ready for playback.'), `UX TEST 21: Step 3 shows verified online message (${msg})`);
  }

  // 22. submitReceiverPairPin with presence='provisional_mdns' displays truthful provisional success
  {
    const fetchImpl = async (url) => {
      if (url.includes('/pair/confirm')) {
        return { ok: true, status: 200, headers: { get: () => 'application/json' }, json: async () => ({
          status: 'paired', presence: 'provisional_mdns'
        }) };
      }
      return { ok: true, headers: { get: () => 'application/json' }, json: async () => ({}) };
    };
    const { sandbox, window, document } = makeSandbox({ fetchImpl });
    vm.createContext(sandbox);
    vm.runInContext(jsContent, sandbox);

    window.ReceiverPairingState.pairingId = 'pair-prov-id';
    const pinInput = document.querySelector('#pair-pin-input');
    pinInput.value = '112233';

    await window.submitReceiverPairPin();
    const title = document.querySelector('#pair-success-title')?.textContent || '';
    const msg = document.querySelector('#pair-success-message')?.textContent || '';
    assert(title.includes('Michi Music Stream paired'), `UX TEST 22: Step 3 shows Michi Music Stream paired (${title})`);
    assert(msg.includes('Pairing completed. Waiting for signed Michi Link presence.'), `UX TEST 22: Step 3 shows truthful provisional message (${msg})`);
  }

  // 23. Discovered · mDNS badge and provisional explanatory text rendered for provisional_mdns
  {
    const fetchImpl = async (url) => {
      if (url.includes('/api/v1/devices/discover')) {
        return { ok: true, status: 200, headers: { get: () => 'application/json' }, json: async () => ({
          receivers: [{ receiver_id: 'rx-prov-test', name: 'Provisional Stream', presence: 'provisional_mdns', paired: false, pairable: true }]
        }) };
      }
      return { ok: true, headers: { get: () => 'application/json' }, json: async () => ({}) };
    };
    const { sandbox, window, document } = makeSandbox({ fetchImpl });
    vm.createContext(sandbox);
    vm.runInContext(jsContent, sandbox);
    window.AuthSession.state = 'authenticated';

    await window.discoverDevices();
    const html = document.querySelector('#discover-result')?.innerHTML || '';
    assert(html.includes('Discovered · mDNS'), 'UX TEST 23: Discovered · mDNS badge rendered for provisional_mdns');
    assert(html.includes('device-card__provisional-text') && html.includes('Identity endpoint verified. Waiting for signed Michi Link presence.'), 'UX TEST 23: provisional message rendered');
  }

  // 24. Verified Online badge rendered for verified_online
  {
    const fetchImpl = async (url) => {
      if (url.includes('/api/v1/devices/discover')) {
        return { ok: true, status: 200, headers: { get: () => 'application/json' }, json: async () => ({
          receivers: [{ receiver_id: 'rx-ver-test', name: 'Verified Stream', presence: 'verified_online', paired: true, qualification: 'qualified' }]
        }) };
      }
      return { ok: true, headers: { get: () => 'application/json' }, json: async () => ({}) };
    };
    const { sandbox, window, document } = makeSandbox({ fetchImpl });
    vm.createContext(sandbox);
    vm.runInContext(jsContent, sandbox);
    window.AuthSession.state = 'authenticated';

    await window.discoverDevices();
    const html = document.querySelector('#discover-result')?.innerHTML || '';
    assert(html.includes('Verified Online'), 'UX TEST 24: Verified Online badge rendered for verified_online');
  }

  // 25. Offline badge and relative last seen rendered for offline
  {
    const fiveMinAgo = new Date(Date.now() - 5 * 60 * 1000).toISOString();
    const fetchImpl = async (url) => {
      if (url.includes('/api/v1/devices/discover')) {
        return { ok: true, status: 200, headers: { get: () => 'application/json' }, json: async () => ({
          receivers: [{ receiver_id: 'rx-off-test', name: 'Offline Stream', presence: 'offline', last_seen: fiveMinAgo }]
        }) };
      }
      return { ok: true, headers: { get: () => 'application/json' }, json: async () => ({}) };
    };
    const { sandbox, window, document } = makeSandbox({ fetchImpl });
    vm.createContext(sandbox);
    vm.runInContext(jsContent, sandbox);
    window.AuthSession.state = 'authenticated';

    await window.discoverDevices();
    const html = document.querySelector('#discover-result')?.innerHTML || '';
    assert(html.includes('Offline'), 'UX TEST 25: Offline badge rendered for offline');
    assert(html.includes('Last seen 5 min ago'), 'UX TEST 25: relative last seen rendered');
  }

  // 26. Discovery indicator: dynamic discovery status badge based on res.discovery
  {
    const rawHtml = fs.readFileSync(htmlPath, 'utf8');
    assert(rawHtml.includes('discovery-status-badge'), 'UX TEST 26: discovery-status-badge present in index.html');
    assert(rawHtml.includes('Discovery unavailable'), 'UX TEST 26: Discovery unavailable indicator initial in index.html');

    const fetchImpl = async (url) => {
      if (url.includes('/api/v1/devices/discover')) {
        return {
          ok: true,
          status: 200,
          headers: { get: () => 'application/json' },
          json: async () => ({
            receivers: [],
            discovery: { active: true, degraded: false, whisker_listening: true, interfaces_joined: 2 }
          })
        };
      }
      return { ok: true, headers: { get: () => 'application/json' }, json: async () => ({}) };
    };
    const { sandbox, window, document } = makeSandbox({ fetchImpl });
    vm.createContext(sandbox);
    vm.runInContext(jsContent, sandbox);
    window.AuthSession.state = 'authenticated';

    await window.discoverDevices();
    const badge = document.querySelector('#discovery-status-badge');
    const badgeText = document.querySelector('#discovery-status-text')?.textContent || '';
    assert(badge && badge.classList.contains('discovery-status-badge--active'), 'UX TEST 26: Active discovery sets active badge class');
    assert(badgeText === 'Discovery active', `UX TEST 26: Active discovery sets text 'Discovery active' (${badgeText})`);

    // Test degraded
    window.MichiAPI.discoverDevices = async () => ({
      receivers: [],
      discovery: { active: false, degraded: true, whisker_listening: false, interfaces_joined: 0 }
    });
    await window.discoverDevices(true);
    assert(badge.classList.contains('discovery-status-badge--degraded'), 'UX TEST 26: Degraded discovery sets degraded badge class');
    assert(badge.textContent.includes('Discovery degraded'), 'UX TEST 26: Degraded discovery sets text Discovery degraded');

    // Test error / unavailable
    window.MichiAPI.discoverDevices = async () => { throw new Error('Network error'); };
    await window.discoverDevices(true);
    assert(badge.classList.contains('discovery-status-badge--unavailable'), 'UX TEST 26: Error sets unavailable badge class');
    assert(badge.textContent.includes('Discovery unavailable'), 'UX TEST 26: Error sets text Discovery unavailable');
  }

  // 27. Step 1: Prepare Michi Music Stream wording and button Continue
  {
    const { sandbox, window, document } = makeSandbox();
    vm.createContext(sandbox);
    vm.runInContext(jsContent, sandbox);

    window.openReceiverPairModal('rx-test', 'Living Room Stream');
    const title = document.querySelector('#pair-modal-title')?.textContent || '';
    const desc = document.querySelector('#pair-step-button-desc')?.textContent || '';
    const readyBtn = document.querySelector('#btn-pair-ready')?.textContent || '';
    assert(title.includes('Prepare Living Room Stream'), `UX TEST 27: Title has Prepare (${title})`);
    assert(desc.includes('Press and hold the button on your Michi Music Stream for 5 seconds'), `UX TEST 27: Step 1 description matches canonical wording (${desc})`);
    assert(readyBtn.includes('Continue'), `UX TEST 27: Button is Continue (${readyBtn})`);
  }

  // 28. Step 2: Enter pairing code wording + PIN validation accepts leading zeros like 000123
  {
    let sentPin = null;
    const fetchImpl = async (url, opts) => {
      if (url.includes('/pair/start')) {
        return { ok: true, status: 200, headers: { get: () => 'application/json' }, json: async () => ({
          pairing_id: 'sess-zeros-1', expires_at: new Date(Date.now() + 60000).toISOString()
        }) };
      }
      if (url.includes('/pair/confirm')) {
        const body = JSON.parse(opts.body);
        sentPin = body.pin;
        return { ok: true, status: 200, headers: { get: () => 'application/json' }, json: async () => ({
          status: 'paired', presence: 'verified_online'
        }) };
      }
      return { ok: true, headers: { get: () => 'application/json' }, json: async () => ({}) };
    };
    const { sandbox, window, document } = makeSandbox({ fetchImpl });
    vm.createContext(sandbox);
    vm.runInContext(jsContent, sandbox);

    window.ReceiverPairingState.receiverId = 'rx-zeros';
    await window.proceedToPairingPin();

    const title = document.querySelector('#pair-modal-title')?.textContent || '';
    assert(title.includes('Enter pairing code'), `UX TEST 28: Title changed to Enter pairing code (${title})`);

    const pinInput = document.querySelector('#pair-pin-input');
    pinInput.value = '000123'; // Leading zeros
    await window.submitReceiverPairPin();

    assert(sentPin === '000123', `UX TEST 28: PIN with leading zeros accepted and sent (${sentPin})`);
  }

  console.log('======================================================================');
  console.log(`BROWSER E2E GATE: ${passed} passed, ${failed} failed`);
  console.log('======================================================================');
  if (failed > 0) process.exit(1);
}

runE2E();
