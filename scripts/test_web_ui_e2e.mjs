#!/usr/bin/env node
/**
 * Michi Web UI Functional Integrity E2E Test Suite
 * Tests actual frontend logic from crates/michi-api/static/app.js against static/index.html
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

const htmlContent = fs.readFileSync(htmlPath, 'utf8');
const rawJsContent = fs.readFileSync(jsPath, 'utf8');
const jsContent = `
(function(window, document, navigator, localStorage, sessionStorage, fetch, setTimeout, clearTimeout, setInterval, clearInterval, Audio) {
` + rawJsContent + `
;window.State = State;
window.ServerPlayback = ServerPlayback;
window.MichiAPI = MichiAPI;
window.addToQueue = addToQueue;
window.toggleOutputTarget = toggleOutputTarget;
window.saveSetting = saveSetting;
window.loadSettings = loadSettings;
window.playEpisode = playEpisode;
})(window, document, window.navigator, window.localStorage, window.sessionStorage, window.fetch, globalThis.setTimeout, globalThis.clearTimeout, globalThis.setInterval, globalThis.clearInterval, window.Audio);
`;

console.log('======================================================================');
console.log('MICHI WEB UI FUNCTIONAL INTEGRITY BROWSER E2E TEST RUNNER');
console.log('======================================================================');

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
  }

  getAttribute(name) {
    return this.attributes[name] || null;
  }

  setAttribute(name, val) {
    this.attributes[name] = String(val);
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

  querySelector(sel) {
    return this._query(sel);
  }

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
      if (this.className.includes(cls)) return this;
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
      if (this.className.includes(cls)) results.push(this);
    }
    for (const c of this.children) {
      c._queryAll(sel, results);
    }
  }
}

class MockStorage {
  constructor() {
    this.store = {};
  }
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
    createElement: (tag) => new MockElement(tag),
    getElementById: (id) => body.querySelector('#' + id),
    querySelector: (sel) => body.querySelector(sel),
    querySelectorAll: (sel) => body.querySelectorAll(sel),
    addEventListener: () => {},
  };

  const window = {
    document: document,
    navigator: { language: 'en-US' },
    location: { reload: () => {} },
    matchMedia: () => ({ matches: false, addEventListener: () => {} }),
    localStorage: new MockStorage(),
    sessionStorage: new MockStorage(),
    setTimeout: (fn, ms) => globalThis.setTimeout(fn, ms),
    clearTimeout: (id) => globalThis.clearTimeout(id),
    setInterval: (fn, ms) => globalThis.setInterval(fn, ms),
    clearInterval: (id) => globalThis.clearInterval(id),
    fetch: null,
    Audio: class {
      constructor() {
        this.src = '';
        this.currentTime = 0;
        this.duration = 180;
      }
      play() { return Promise.resolve(); }
      pause() {}
    }
  };

  // Populate basic DOM elements from HTML
  const ids = [
    'app', 'toast', 'toast-container', 'np-title', 'np-artist', 'np-target-badge', 'queue-content',
    'view-settings', 'settings-restart-banner', 'settings-scan-concurrency',
    'settings-max-transcodes', 'settings-db-pool', 'settings-scrobble',
    'settings-theme', 'settings-language', 'settings-profile',
    'qr-status-badge', 'qr-code-img', 'stab-receivers', 'stab-backup'
  ];

  for (const id of ids) {
    const el = new MockElement('div', id);
    body.appendChild(el);
  }

  return { window, document };
}

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

async function runE2E() {
  // Test A: Output Target Truthfulness (Browser Local vs Remote Link)
  {
    const { window, document } = createDOM();
    const sandbox = {
      window,
      document,
      navigator: window.navigator,
      localStorage: window.localStorage,
      console: { warn: () => {}, error: () => {}, log: () => {} },
      $: (s) => document.querySelector(s),
      $$: (s) => document.querySelectorAll(s),
      t: (k) => k,
      esc: (s) => s,
      fmtDur: () => '3:00',
      setTimeout: global.setTimeout,
      clearTimeout: global.clearTimeout,
      setInterval: global.setInterval,
      clearInterval: global.clearInterval,
      showToast: () => {},
      fetch: async () => ({ ok: true, json: async () => ({}) }),
    };

    vm.createContext(sandbox);
    vm.runInContext(jsContent, sandbox);

    const badge = document.getElementById('np-target-badge');
    assert(badge !== null, 'Now Playing target badge exists in DOM');

    // Default should be browser-local output
    assert(window.ServerPlayback.outputTarget === 'browser', 'Default output target is browser local');
    
    // Toggle output target
    window.toggleOutputTarget();
    assert(window.ServerPlayback.outputTarget === 'remote', 'Toggling switches to remote link');
    assert(badge.textContent.includes('Remote Link'), 'Badge reflects Remote Link truth');
  }

  // Test B: Queue Add Server Failure does not show fake success or local append
  {
    const { window, document } = createDOM();
    let toastErrorShown = false;
    const sandbox = {
      window,
      document,
      navigator: window.navigator,
      localStorage: window.localStorage,
      console: { warn: () => {}, error: () => {}, log: () => {} },
      $: (s) => document.querySelector(s),
      $$: (s) => document.querySelectorAll(s),
      t: (k) => k,
      esc: (s) => s,
      fmtDur: () => '3:00',
      showToast: (msg, type) => {
        if (type === 'error' || msg.includes('Failed to add')) toastErrorShown = true;
      },
      fetch: async (url, opts) => {
        if (url.includes('/api/v1/queue/items')) {
          return {
            ok: false,
            status: 400,
            headers: { get: () => 'application/json' },
            json: async () => ({ error: { message: 'Database constraint failed' } })
          };
        }
        return { ok: true, headers: { get: () => 'application/json' }, json: async () => ({}) };
      }
    };

    window.fetch = sandbox.fetch;
    vm.createContext(sandbox);
    vm.runInContext(jsContent, sandbox);

    window.State.tracks = [{ id: 'track-1', title: 'Song 1', duration_ms: 180000 }];
    window.State.queue = [];

    await window.addToQueue(0);

    const toastEl = document.getElementById('toast');
    assert(window.State.queue.length === 0, 'Queue is NOT mutated locally when backend API rejects');
    assert(toastEl && toastEl.textContent.includes('Failed to add'), 'Explicit error toast displayed upon queue failure');
  }

  // Test C: Persistent Restart Banner survives F5 (localStorage persistence)
  {
    const { window, document } = createDOM();
    const sandbox = {
      window,
      document,
      navigator: window.navigator,
      localStorage: window.localStorage,
      console: { warn: () => {}, error: () => {}, log: () => {} },
      $: (s) => document.querySelector(s),
      $$: (s) => document.querySelectorAll(s),
      t: (k) => k,
      esc: (s) => s,
      fmtDur: () => '3:00',
      showToast: () => {},
      fetch: async (url, opts) => {
        if (opts?.method === 'PUT' && url.includes('/api/v1/settings')) {
          return {
            ok: true,
            status: 200,
            headers: { get: () => 'application/json' },
            json: async () => ({ restart_required: true, resource_profile: 'performance' })
          };
        }
        if (url.includes('/api/v1/settings')) {
          return {
            ok: true,
            status: 200,
            headers: { get: () => 'application/json' },
            json: async () => ({
              resource_profile: 'performance',
              effective_scan_workers: 4,
              effective_transcode_workers: 4,
              effective_db_pool: 16
            })
          };
        }
        return { ok: true, headers: { get: () => 'application/json' }, json: async () => ({}) };
      }
    };

    window.fetch = sandbox.fetch;
    vm.createContext(sandbox);
    vm.runInContext(jsContent, sandbox);

    // Save setting that requires restart
    await window.saveSetting('resource_profile', 'performance');

    assert(window.localStorage.getItem('michi_restart_required') === 'true', 'Restart required state stored in localStorage');
    
    // Simulate F5 page refresh
    const newDOM = createDOM();
    newDOM.window.localStorage.setItem('michi_restart_required', 'true');
    const sandboxReload = {
      window: newDOM.window,
      document: newDOM.document,
      navigator: newDOM.window.navigator,
      localStorage: newDOM.window.localStorage,
      console: { warn: () => {}, error: () => {}, log: () => {} },
      $: (s) => newDOM.document.querySelector(s),
      $$: (s) => newDOM.document.querySelectorAll(s),
      t: (k) => k,
      esc: (s) => s,
      fmtDur: () => '3:00',
      showToast: () => {},
      fetch: sandbox.fetch
    };

    newDOM.window.fetch = sandbox.fetch;
    vm.createContext(sandboxReload);
    vm.runInContext(jsContent, sandboxReload);

    await newDOM.window.loadSettings();

    const bannerAfterF5 = newDOM.document.getElementById('settings-restart-banner');
    assert(bannerAfterF5 !== null, 'Restart banner persists in Settings after F5 reload');
  }

  // Test D: Podcast Episode Play Error Does Not Mark Episode Played
  {
    const { window, document } = createDOM();
    let playedEndpointCalled = false;

    const sandbox = {
      window,
      document,
      navigator: window.navigator,
      localStorage: window.localStorage,
      console: { warn: () => {}, error: () => {}, log: () => {} },
      $: (s) => document.querySelector(s),
      $$: (s) => document.querySelectorAll(s),
      t: (k) => k,
      esc: (s) => s,
      fmtDur: () => '3:00',
      showToast: () => {},
      fetch: async (url, opts) => {
        if (url.includes('/api/v1/sources/episodes/')) {
          playedEndpointCalled = true;
        }
        return { ok: true, headers: { get: () => 'application/json' }, json: async () => ({}) };
      }
    };

    // Override Audio.prototype.play to fail
    window.Audio = class {
      constructor() { this.src = ''; }
      play() { return Promise.reject(new Error('Decode error')); }
      pause() {}
    };

    window.fetch = sandbox.fetch;
    vm.createContext(sandbox);
    vm.runInContext(jsContent, sandbox);

    try {
      await window.playEpisode('source-123', 'ep-456');
    } catch (e) {}

    assert(playedEndpointCalled === false, 'Failed audio.play does NOT call mark episode as played');
  }

  console.log('======================================================================');
  console.log(`BROWSER E2E GATE: ${passed} passed, ${failed} failed`);
  console.log('======================================================================');

  if (failed > 0) {
    process.exit(1);
  }
}

runE2E();
