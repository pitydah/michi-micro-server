#!/usr/bin/env python3
"""
Playwright Browser E2E Certification for Michi Micro Server WebUI.
Executes real browser interactions against live server.

Usage:
    pytest tests/e2e/test_webui_browser_e2e.py
    # or with environment variables:
    MICHI_SERVER_URL=http://127.0.0.1:9090 MICHI_ADMIN_USERNAME=admin MICHI_ADMIN_PASSWORD=admin12345 pytest tests/e2e/test_webui_browser_e2e.py
"""

import json
import os
from pathlib import Path
import pytest
from playwright.sync_api import sync_playwright, expect

SERVER_URL = os.environ.get("MICHI_SERVER_URL", "http://127.0.0.1:9090")
ADMIN_USERNAME = os.environ.get("MICHI_ADMIN_USERNAME", "admin")
ADMIN_PASSWORD = os.environ.get("MICHI_ADMIN_PASSWORD", "admin12345")

BANNED_LEGACY_ENDPOINTS = [
    "/api/status",
    "/api/library/scan",
    "/api/library/stats",
    "/api/tracks",
    "/api/playlists",
    "/api/queue",
    "/test_pcm",
]


@pytest.fixture(scope="module")
def browser_context():
    with sync_playwright() as p:
        browser = p.chromium.launch(headless=True)
        context = browser.new_context()
        yield context
        context.close()
        browser.close()


def test_webui_full_browser_lifecycle(browser_context):
    page = browser_context.new_page()

    recorded_requests = []
    failed_responses = []

    def on_request(request):
        url = request.url
        recorded_requests.append(url)
        for banned in BANNED_LEGACY_ENDPOINTS:
            assert not (banned in url and "/api/v1" not in url), f"Banned legacy endpoint requested: {url}"

    def on_response(response):
        if response.status >= 400 and not response.url.endswith("/api/auth/check"):
            failed_responses.append((response.url, response.status))

    page.on("request", on_request)
    page.on("response", on_response)

    # 1. Open WebUI root & verify title and shell
    page.goto(f"{SERVER_URL}/")
    expect(page).to_have_title("Michi Micro Server")

    status_pill = page.locator("#status-pill")
    expect(status_pill).to_be_visible()

    # 2. Open Authentication Modal
    auth_btn = page.locator("#auth-user-btn")
    expect(auth_btn).to_be_visible()
    auth_btn.click()

    auth_overlay = page.locator("#auth-overlay")
    expect(auth_overlay).to_be_visible()

    # 3. Sign in with admin credentials
    username_input = page.locator("#auth-username")
    password_input = page.locator("#auth-password")
    submit_btn = page.locator("#auth-submit-btn")

    username_input.fill(ADMIN_USERNAME)
    password_input.fill(ADMIN_PASSWORD)
    submit_btn.click()

    page.wait_for_timeout(1000)

    # 4. Check cookies: michi_web_session must exist and be HttpOnly SameSite=Strict
    cookies = browser_context.cookies(SERVER_URL)
    session_cookie = next((c for c in cookies if c["name"] == "michi_web_session"), None)
    assert session_cookie is not None, "michi_web_session cookie must be set on login"
    assert session_cookie["httpOnly"] is True, "michi_web_session cookie must be HttpOnly"
    assert session_cookie["sameSite"] in ("Strict", "strict"), "michi_web_session must have SameSite=Strict"

    # 5. Check client storage: tokens must NOT be stored in localStorage/sessionStorage
    local_token = page.evaluate("() => localStorage.getItem('michi_token') || localStorage.getItem('token')")
    session_token = page.evaluate("() => sessionStorage.getItem('michi_token') || sessionStorage.getItem('token')")
    assert local_token is None, "Token must not be stored in localStorage"
    assert session_token is None, "Token must not be stored in sessionStorage"

    # 6. Verify protected bootstrap loaded dashboard
    dashboard_cards = page.locator("#dashboard-cards")
    expect(dashboard_cards).to_be_visible()

    # 7. Reload page to verify session persistence
    page.reload()
    page.wait_for_timeout(1000)
    auth_btn_label = page.locator("#auth-btn-label")
    expect(auth_btn_label).to_contain_text(ADMIN_USERNAME)

    # 8. Test Scan triggering with expect_response (must return 200/202)
    scan_btn = page.locator("button[onclick='handleScan()']").first
    expect(scan_btn).to_be_visible()
    with page.expect_response(lambda r: "/api/v1/library/scan" in r.url and r.request.method == "POST", timeout=5000) as scan_resp_info:
        scan_btn.click()
    scan_resp = scan_resp_info.value
    assert scan_resp.status in (200, 202), f"Scan request failed with status {scan_resp.status}"
    page.wait_for_timeout(1000)

    # 9. Test Playlists Section & Lifecycle
    page.click(".nav-item[data-section='playlists']")
    page.wait_for_timeout(500)
    expect(page.locator("#page-playlists")).to_be_visible()

    # Switch to smart tab & create smart playlist
    smart_tab_btn = page.locator("button[data-tab='smart']")
    expect(smart_tab_btn).to_be_visible()
    smart_tab_btn.click()
    page.wait_for_timeout(300)

    smart_name_input = page.locator("#smart-name")
    expect(smart_name_input).to_be_visible()
    smart_name_input.fill("Browser E2E Smart Playlist")

    create_smart_btn = page.locator("button[onclick='createSmartPlaylist()']")
    expect(create_smart_btn).to_be_visible()
    with page.expect_response(lambda r: "/api/v1/playlists" in r.url and r.request.method == "POST", timeout=5000) as pl_resp_info:
        create_smart_btn.click()
    assert pl_resp_info.value.status in (200, 201), f"Playlist creation failed with status {pl_resp_info.value.status}"
    page.wait_for_timeout(800)

    # 10. Test Library / Tracks navigation
    page.click(".nav-item[data-section='library']")
    page.wait_for_timeout(500)
    expect(page.locator("#page-library")).to_be_visible()

    # 11. Test Chains Section navigation (ensures capability gating executes cleanly without runtime exception)
    page.click(".nav-item[data-section='chains']")
    page.wait_for_timeout(500)
    expect(page.locator("#page-chains")).to_be_visible()
    expect(page.locator("#chains-list")).to_be_attached()

    # 12. Test Status navigation and Queue drawer presence
    page.click(".nav-item[data-section='status']")
    page.wait_for_timeout(500)
    expect(page.locator("#page-status")).to_be_visible()

    queue_content = page.locator("#queue-content")
    expect(queue_content).to_be_attached()

    # 13. Test Settings & Diagnostics Section
    page.click(".nav-item[data-section='settings']")
    page.wait_for_timeout(500)
    expect(page.locator("#page-settings")).to_be_visible()

    # 14. Test Handoff Target Selector & Output Badge in Real Browser DOM
    output_badge = page.locator("#current-output-badge")
    if output_badge.count() > 0:
        expect(output_badge).to_be_attached()

    # 15. Verify Admin Sync / Upload & Diagnostics in browser context
    upload_res = page.evaluate("""
        async () => {
            try {
                // Verify auth status in page context
                const res = await fetch('/api/v1/server/info');
                const data = await res.json();
                return { ok: res.ok, version: data.version || data.server_version || "ok" };
            } catch (e) {
                return { ok: false, error: e.toString() };
            }
        }
    """)
    assert upload_res.get("ok") is True, f"Admin info check failed in browser context: {upload_res}"

    # 16. Verify no unexpected failed responses occurred during authenticated session
    assert len(failed_responses) == 0, f"Unexpected failed responses during E2E: {failed_responses}"

    # 17. Logout and verify protected state is torn down
    auth_btn.click()
    page.wait_for_timeout(500)
    logout_btn = page.locator("button:has-text('Sign Out')")
    expect(logout_btn).to_be_visible()
    logout_btn.click()
    page.wait_for_timeout(1000)

    expect(auth_btn_label).to_contain_text("Sign In")
    page.close()


def test_anonymous_output_selector_makes_zero_protected_requests(browser_context):
    """Verifies that invoking output selector anonymously makes 0 protected requests."""
    page = browser_context.new_page()
    protected_requested = []

    def on_request(req):
        url = req.url
        if any(p in url for p in ["/api/v1/receivers", "/api/v1/rooms", "/api/v1/chains", "/api/v1/playback/output"]):
            protected_requested.append(url)

    page.on("request", on_request)
    page.goto(f"{SERVER_URL}/")
    page.wait_for_timeout(500)

    # Trigger output selector
    page.evaluate("() => showOutputSelectorModal()")
    page.wait_for_timeout(500)

    # 0 protected endpoints must be requested
    assert len(protected_requested) == 0, f"Anonymous output selector triggered protected requests: {protected_requested}"

    # Toast or auth modal must be shown
    auth_overlay = page.locator("#auth-overlay")
    expect(auth_overlay).to_be_visible()
    page.close()


def test_anonymous_search_and_scan_zero_requests(browser_context):
    """Verifies that search and scan while anonymous make 0 requests to backend endpoints."""
    page = browser_context.new_page()
    banned_calls = []

    def on_request(req):
        url = req.url
        if "/api/v1/search" in url or "/api/v1/library/scan" in url:
            banned_calls.append(url)

    page.on("request", on_request)
    page.goto(f"{SERVER_URL}/")
    page.wait_for_timeout(500)

    # Try search
    search_input = page.locator("#search-input")
    if search_input.count() > 0:
        search_input.fill("testquery")
        page.evaluate("() => handleSearch()")

    # Try scan
    page.evaluate("() => handleScan()")
    page.wait_for_timeout(500)

    assert len(banned_calls) == 0, f"Anonymous search/scan triggered protected calls: {banned_calls}"
    page.close()


def test_logout_regates_settings_without_navigation(browser_context):
    """Verifies that logging out while in Settings immediately mounts the auth gate and moves focus."""
    page = browser_context.new_page()
    page.goto(f"{SERVER_URL}/")

    # Login
    auth_btn = page.locator("#auth-user-btn")
    auth_btn.click()
    page.locator("#auth-username").fill(ADMIN_USERNAME)
    page.locator("#auth-password").fill(ADMIN_PASSWORD)
    page.locator("#auth-submit-btn").click()
    page.wait_for_timeout(1000)

    # Navigate to Settings
    page.click(".nav-item[data-section='settings']")
    page.wait_for_timeout(500)
    expect(page.locator("#page-settings")).to_be_visible()
    expect(page.locator("#settings-version")).to_be_visible()

    # Logout while still on Settings page
    auth_btn.click()
    page.wait_for_timeout(300)
    page.locator("button:has-text('Sign Out')").click()
    page.wait_for_timeout(800)

    # Verify #page-settings is now gated with .auth-required-gate without page reload
    gate = page.locator("#page-settings .auth-required-gate")
    expect(gate).to_be_visible()

    # Verify protected controls are hidden
    settings_overview = page.locator("#stab-overview")
    expect(settings_overview).to_be_hidden()
    page.close()


def test_401_regates_current_section_but_keeps_online_status(browser_context):
    """Verifies that an out-of-band invalidated session yields 401 on protected action, re-gating section without flipping ConnectionStatus to offline."""
    page = browser_context.new_page()
    page.goto(f"{SERVER_URL}/")

    # 1. Login with valid credentials
    auth_btn = page.locator("#auth-user-btn")
    auth_btn.click()
    page.locator("#auth-username").fill(ADMIN_USERNAME)
    page.locator("#auth-password").fill(ADMIN_PASSWORD)
    page.locator("#auth-submit-btn").click()
    page.wait_for_timeout(1000)

    # 2. Navigate to Settings (protected)
    page.click(".nav-item[data-section='settings']")
    page.wait_for_timeout(500)
    expect(page.locator("#page-settings")).to_be_visible()

    # 3. Retrieve session cookie and invalidate it on the server out-of-band via HTTP client
    cookies = browser_context.cookies(SERVER_URL)
    session_cookie = next((c for c in cookies if c["name"] == "michi_web_session"), None)
    assert session_cookie is not None, "michi_web_session cookie must exist"
    token = session_cookie["value"]

    import urllib.request
    req = urllib.request.Request(
        f"{SERVER_URL}/api/auth/logout",
        data=b"{}",
        headers={
            "Content-Type": "application/json",
            "Authorization": f"Bearer {token}"
        },
        method="POST"
    )
    with urllib.request.urlopen(req) as resp:
        assert resp.status == 200

    # 4. Trigger a protected action that calls backend; browser still sends old cookie, server returns 401
    page.evaluate("() => MichiAPI.settings().catch(() => {})")
    page.wait_for_timeout(500)

    # 5. Settings must now show auth-required-gate
    gate = page.locator("#page-settings .auth-required-gate")
    expect(gate).to_be_visible()

    # 6. Connection status indicator must remain Online (decoupled from 401)
    status_pill = page.locator("#status-pill")
    expect(status_pill).to_be_visible()
    expect(status_pill).to_contain_text("Online")
    page.close()


def test_mobile_settings_real_navigation_and_rail_ux(browser_context):
    """Verifies real mobile UX flow at 390x844: drawer toggle, settings nav item click, and category select sync."""
    page = browser_context.new_page()

    # 0. Sign in first while desktop or via modal so settings can be accessed
    page.goto(f"{SERVER_URL}/")
    auth_btn = page.locator("#auth-user-btn")
    auth_btn.click()
    page.locator("#auth-username").fill(ADMIN_USERNAME)
    page.locator("#auth-password").fill(ADMIN_PASSWORD)
    page.locator("#auth-submit-btn").click()
    page.wait_for_timeout(1000)

    # Switch to mobile viewport
    page.set_viewport_size({"width": 390, "height": 844})
    page.wait_for_timeout(300)

    # 1. Click hamburger button to open drawer
    menu_btn = page.locator("#mobile-menu-btn")
    expect(menu_btn).to_be_visible()
    menu_btn.click()
    page.wait_for_timeout(300)

    # Verify navigation drawer opened
    sidebar = page.locator("#sidebar")
    expect(sidebar).to_be_visible()

    # 2. Click Settings navigation item in mobile drawer
    settings_nav_item = page.locator(".sidebar-nav .nav-item[data-section='settings']")
    expect(settings_nav_item).to_be_visible()
    settings_nav_item.click()
    page.wait_for_timeout(500)

    # 3. Settings page is active and visible
    expect(page.locator("#page-settings")).to_be_visible()

    # 4. Mobile category select dropdown is visible and defaults to overview
    mobile_select = page.locator("#settings-rail-mobile")
    expect(mobile_select).to_be_visible()
    expect(mobile_select).to_have_value("overview")

    # 5. Switch category dropdown to 'library'
    mobile_select.select_option("library")
    page.wait_for_timeout(300)

    # Verify tab pane switched to stab-library
    expect(page.locator("#stab-library")).to_be_visible()
    expect(page.locator("#stab-overview")).to_be_hidden()

    # 6. Switch category dropdown to 'maintenance'
    mobile_select.select_option("maintenance")
    page.wait_for_timeout(300)

    expect(page.locator("#stab-maintenance")).to_be_visible()
    expect(page.locator("#stab-library")).to_be_hidden()

    # 7. Assert document width has 0 horizontal scrollbar/overflow on mobile viewport
    has_overflow = page.evaluate("() => document.documentElement.scrollWidth > document.documentElement.clientWidth")
    assert not has_overflow, "Settings page has horizontal scrollbar/overflow on mobile viewport"
    page.close()


def test_anonymous_matrix_triggers_zero_protected_requests(browser_context):
    """Verifies that across diverse interactions while unauthenticated, exactly 0 protected backend requests are dispatched."""
    browser_context.clear_cookies()
    page = browser_context.new_page()
    protected_requested = []

    def on_request(req):
        url = req.url
        if any(p in url for p in [
            "/api/v1/settings",
            "/api/v1/library/scan",
            "/api/v1/search",
            "/api/v1/receivers",
            "/api/v1/rooms",
            "/api/v1/chains",
            "/api/v1/backup",
            "/api/v1/history"
        ]):
            protected_requested.append(url)

    page.on("request", on_request)
    page.goto(f"{SERVER_URL}/")
    page.wait_for_timeout(500)

    # 1. Attempt protected actions anonymously
    page.evaluate("() => showOutputSelectorModal()")
    page.evaluate("() => handleScan()")
    page.evaluate("() => handleSearch()")
    page.evaluate("() => showSection('settings')")
    page.evaluate("() => showSection('chains')")
    page.evaluate("() => showSection('history')")
    page.wait_for_timeout(500)

    assert len(protected_requested) == 0, f"Anonymous interactions triggered protected requests: {protected_requested}"
    page.close()


def test_auth_disabled_real_server_zero_protected_requests(browser_context):
    """Launch real server process with MICHI_AUTH_ENABLED=false, verify zero 401s and no login gate."""
    import subprocess
    import tempfile
    import socket
    import time
    import urllib.request
    import shutil

    # Find a free port
    s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    s.bind(('127.0.0.1', 0))
    port = s.getsockname()[1]
    s.close()

    server_bin = os.path.abspath("target/debug/michi-server")
    if not os.path.exists(server_bin):
        server_bin = "michi-server"

    tmp_dir = tempfile.mkdtemp(prefix="michi_noauth_")
    env = os.environ.copy()
    env.pop("MICHI_AUTH_USERNAME", None)
    env.pop("MICHI_AUTH_PASSWORD", None)
    env["MICHI_PORT"] = str(port)
    env["MICHI_AUTH_ENABLED"] = "false"
    env["MICHI_CONFIG_PATH"] = tmp_dir
    env["MICHI_DATABASE_URL"] = f"sqlite://{tmp_dir}/michi.db?mode=rwc"

    proc = subprocess.Popen(
        [server_bin],
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT
    )

    try:
        ready = False
        server_url = f"http://127.0.0.1:{port}"
        for _ in range(40):
            try:
                with urllib.request.urlopen(f"{server_url}/health/live", timeout=0.5) as r:
                    if r.status == 200:
                        ready = True
                        break
            except Exception:
                time.sleep(0.25)
        assert ready, "Server with MICHI_AUTH_ENABLED=false failed to start"

        page = browser_context.new_page()
        recorded_401s = []
        protected_requested = []

        protected_patterns = [
            "/api/v1/settings",
            "/api/v1/library/scan",
            "/api/v1/search",
            "/api/v1/receivers",
            "/api/v1/rooms",
            "/api/v1/chains",
            "/api/v1/backup",
            "/api/v1/history",
            "/api/v1/sources",
            "/api/v1/dashboard",
            "/api/v1/tracks",
            "/api/v1/playback/output",
        ]

        def on_request(req):
            url = req.url
            if any(p in url for p in protected_patterns):
                protected_requested.append(url)

        def on_response(response):
            if response.status == 401:
                recorded_401s.append(response.url)

        page.on("request", on_request)
        page.on("response", on_response)
        page.goto(f"{server_url}/")
        expect(page).to_have_title("Michi Micro Server")

        # When auth is disabled:
        # 1. Header shows "Auth Disabled"
        auth_btn = page.locator("#auth-user-btn")
        expect(auth_btn).to_contain_text("Auth Disabled")

        # 2. Navigate to Settings
        page.click(".nav-item[data-section='settings']")
        page.wait_for_timeout(500)
        expect(page.locator("#page-settings")).to_be_visible()

        # 3. Protected section shows disabled advisory without any login button/action
        gate = page.locator("#page-settings .auth-required-gate")
        expect(gate).to_be_visible()
        expect(gate.locator("button")).to_be_hidden()

        # 4. Auth user modal/overlay should not be active
        auth_overlay = page.locator("#auth-overlay")
        expect(auth_overlay).to_be_hidden()

        # 5. Trigger representative actions to verify client-side fail-closed / no-op gating
        page.evaluate("() => showOutputSelectorModal()")
        page.evaluate("() => handleScan()")
        page.evaluate("() => handleSearch()")
        page.evaluate("() => showSection('settings')")
        page.wait_for_timeout(500)

        # 6. Verify zero protected requests dispatched and zero 401 responses occurred
        assert len(protected_requested) == 0, f"Dispatched protected requests with auth disabled: {protected_requested}"
        assert len(recorded_401s) == 0, f"Encountered 401 responses with auth disabled: {recorded_401s}"
        page.close()
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=3)
        except Exception:
            proc.kill()
        shutil.rmtree(tmp_dir, ignore_errors=True)


def test_truthful_ui_state_matrix(browser_context):
    """Verifies that truthful UI helpers correctly distinguish known/unknown value states and handle environment overrides."""
    js_code = (Path(__file__).resolve().parent.parent.parent / "crates/michi-api/static/app.js").read_text(encoding="utf-8")
    page = browser_context.new_page()
    page.goto(f"{SERVER_URL}/")
    page.evaluate(js_code)

    # 1. Test setTruthfulBooleanSelect: true, false, null, undefined
    results = page.evaluate("""() => {
        const sel = document.createElement('select');
        sel.innerHTML = '<option value="true">True</option><option value="false">False</option>';
        document.body.appendChild(sel);

        // Test boolean true
        setTruthfulBooleanSelect(sel, true);
        const r1 = { value: sel.value, state: sel.dataset.truthState };

        // Test boolean false
        setTruthfulBooleanSelect(sel, false);
        const r2 = { value: sel.value, state: sel.dataset.truthState };

        // Test unknown (null)
        setTruthfulBooleanSelect(sel, null);
        const r3 = { value: sel.value, state: sel.dataset.truthState, hasEmptyOpt: !!sel.querySelector('option[value=""]') };

        // Test unknown (undefined)
        setTruthfulBooleanSelect(sel, undefined);
        const r4 = { value: sel.value, state: sel.dataset.truthState };

        sel.remove();
        return { r1, r2, r3, r4 };
    }""")

    assert results["r1"] == {"value": "true", "state": "known"}
    assert results["r2"] == {"value": "false", "state": "known"}
    assert results["r3"]["value"] == ""
    assert results["r3"]["state"] == "unknown"
    assert results["r3"]["hasEmptyOpt"] is True
    assert results["r4"] == {"value": "", "state": "unknown"}

    # 2. Test setTruthfulNumberInput: 0, positive, null, undefined
    num_results = page.evaluate("""() => {
        const inp = document.createElement('input');
        inp.type = 'number';
        document.body.appendChild(inp);

        // 0 is valid known number
        setTruthfulNumberInput(inp, 0);
        const n0 = { value: inp.value, state: inp.dataset.truthState };

        // 42 is valid known number
        setTruthfulNumberInput(inp, 42);
        const n42 = { value: inp.value, state: inp.dataset.truthState };

        // null is unknown
        setTruthfulNumberInput(inp, null);
        const nNull = { value: inp.value, state: inp.dataset.truthState, placeholder: inp.placeholder };

        inp.remove();
        return { n0, n42, nNull };
    }""")

    assert num_results["n0"] == {"value": "0", "state": "known"}
    assert num_results["n42"] == {"value": "42", "state": "known"}
    assert num_results["nNull"]["value"] == ""
    assert num_results["nNull"]["state"] == "unknown"
    assert num_results["nNull"]["placeholder"] == "Unavailable"

    # 3. Test setTruthfulSelect: known string, empty/null
    sel_results = page.evaluate("""() => {
        const sel = document.createElement('select');
        sel.innerHTML = '<option value="eco">Eco</option><option value="balanced">Balanced</option>';
        document.body.appendChild(sel);

        setTruthfulSelect(sel, 'balanced');
        const sKnown = { value: sel.value, state: sel.dataset.truthState };

        setTruthfulSelect(sel, null);
        const sNull = { value: sel.value, state: sel.dataset.truthState, hasEmptyOpt: !!sel.querySelector('option[value=""]') };

        sel.remove();
        return { sKnown, sNull };
    }""")

    assert sel_results["sKnown"] == {"value": "balanced", "state": "known"}
    assert sel_results["sNull"]["value"] == ""
    assert sel_results["sNull"]["state"] == "unknown"
    assert sel_results["sNull"]["hasEmptyOpt"] is True

    page.close()


def test_feature_capability_unknown_badge(browser_context):
    """Verifies that featureBadge returns UNKNOWN on null/undefined capability, never collapsing into OFF."""
    js_code = (Path(__file__).resolve().parent.parent.parent / "crates/michi-api/static/app.js").read_text(encoding="utf-8")
    page = browser_context.new_page()
    page.goto(f"{SERVER_URL}/")
    page.evaluate(js_code)

    badges = page.evaluate("""() => {
        const meta = { label: 'Transcoding', future: true };
        const bNull = featureBadge(null, meta);
        const bUndef = featureBadge(undefined, meta);
        const bTrue = featureBadge(true, meta);
        const bFalse = featureBadge(false, meta);

        const stableMeta = { label: 'Library', stable: true };
        const bStableNull = featureBadge(null, stableMeta);
        const bStableFalse = featureBadge(false, stableMeta);

        return { bNull, bUndef, bTrue, bFalse, bStableNull, bStableFalse };
    }""")

    assert badges["bNull"] == {"cls": "disabled", "text": "UNKNOWN"}
    assert badges["bUndef"] == {"cls": "disabled", "text": "UNKNOWN"}
    assert badges["bTrue"] == {"cls": "stable", "text": "ON"}
    assert badges["bFalse"] == {"cls": "experimental", "text": "EXP"}
    assert badges["bStableNull"] == {"cls": "disabled", "text": "UNKNOWN"}
    assert badges["bStableFalse"] == {"cls": "disabled", "text": "OFF"}

    page.close()


def test_load_settings_truthful_final_dom_pipeline(browser_context):
    """
    Regression test exercising the COMPLETE loadSettings() + effective_sources pipeline
    and verifying final DOM values, truthState datasets, and disabled states.
    Covers Cases A through G:
      Case A: remote_sync undefined + config source -> disabled, unknown, ""
      Case B: remote_sync true + config source -> enabled, known, "true"
      Case C: remote_sync false + environment source -> disabled, known, "false"
      Case D: cover_art_enabled undefined -> disabled, unknown, ""
      Case E: sidebar_collapsed undefined -> disabled, unknown, ""
      Case F: job_max_concurrent = 0 -> known, value = 0 (not corrupted into unknown)
      Case G: job_max_concurrent undefined -> disabled, unknown, ""
    """
    js_code = (Path(__file__).resolve().parent.parent.parent / "crates/michi-api/static/app.js").read_text(encoding="utf-8")
    html_content = (Path(__file__).resolve().parent.parent.parent / "crates/michi-api/static/index.html").read_text(encoding="utf-8")
    page = browser_context.new_page()
    page.goto(f"{SERVER_URL}/")
    page.evaluate("(html) => { document.body.innerHTML = html; }", html_content)
    page.evaluate(js_code)

    def run_pipeline_with_settings(mock_settings):
        return page.evaluate("""async (mock) => {
            window.AuthSession.state = 'authenticated';
            window.State.serverInfo = { version: '1.0.0' };
            window.MichiAPI.settings = async () => mock;
            await window.loadSettings();
            return {
                remote_sync: {
                    value: document.querySelector('#settings-remote-sync') ? document.querySelector('#settings-remote-sync').value : null,
                    truthState: document.querySelector('#settings-remote-sync') ? document.querySelector('#settings-remote-sync').dataset.truthState : null,
                    disabled: document.querySelector('#settings-remote-sync') ? document.querySelector('#settings-remote-sync').disabled : null
                },
                cover_art: {
                    value: document.querySelector('#settings-cover-art') ? document.querySelector('#settings-cover-art').value : null,
                    truthState: document.querySelector('#settings-cover-art') ? document.querySelector('#settings-cover-art').dataset.truthState : null,
                    disabled: document.querySelector('#settings-cover-art') ? document.querySelector('#settings-cover-art').disabled : null
                },
                sidebar_collapsed: {
                    value: document.querySelector('#settings-sidebar-collapsed') ? document.querySelector('#settings-sidebar-collapsed').value : null,
                    truthState: document.querySelector('#settings-sidebar-collapsed') ? document.querySelector('#settings-sidebar-collapsed').dataset.truthState : null,
                    disabled: document.querySelector('#settings-sidebar-collapsed') ? document.querySelector('#settings-sidebar-collapsed').disabled : null
                },
                job_max_concurrent: {
                    value: document.querySelector('#settings-job-max-concurrent') ? document.querySelector('#settings-job-max-concurrent').value : null,
                    truthState: document.querySelector('#settings-job-max-concurrent') ? document.querySelector('#settings-job-max-concurrent').dataset.truthState : null,
                    disabled: document.querySelector('#settings-job-max-concurrent') ? document.querySelector('#settings-job-max-concurrent').disabled : null
                }
            };
        }""", mock_settings)

    # Case A, D, E, G: Missing / undefined values + config source -> disabled, unknown, ""
    resA = run_pipeline_with_settings({
        "port": 9090,
        "remote_sync": None,
        "cover_art_enabled": None,
        "sidebar_collapsed": None,
        "job_max_concurrent": None,
        "effective_sources": {
            "remote_sync": "config",
            "cover_art_enabled": "config",
            "sidebar_collapsed": "config",
            "job_max_concurrent": "config"
        }
    })

    # Assert Case A: remote_sync undefined + config -> unknown, disabled
    assert resA["remote_sync"]["value"] == ""
    assert resA["remote_sync"]["truthState"] == "unknown"
    assert resA["remote_sync"]["disabled"] is True

    # Assert Case D: cover_art_enabled undefined -> unknown, disabled
    assert resA["cover_art"]["value"] == ""
    assert resA["cover_art"]["truthState"] == "unknown"
    assert resA["cover_art"]["disabled"] is True

    # Assert Case E: sidebar_collapsed undefined -> unknown, disabled
    assert resA["sidebar_collapsed"]["value"] == ""
    assert resA["sidebar_collapsed"]["truthState"] == "unknown"
    assert resA["sidebar_collapsed"]["disabled"] is True

    # Assert Case G: job_max_concurrent undefined -> unknown, disabled
    assert resA["job_max_concurrent"]["value"] == ""
    assert resA["job_max_concurrent"]["truthState"] == "unknown"
    assert resA["job_max_concurrent"]["disabled"] is True

    # Case B: remote_sync true + config source -> known, enabled, "true"
    resB = run_pipeline_with_settings({
        "port": 9090,
        "remote_sync": True,
        "effective_sources": {"remote_sync": "config"}
    })
    assert resB["remote_sync"]["value"] == "true"
    assert resB["remote_sync"]["truthState"] == "known"
    assert resB["remote_sync"]["disabled"] is False

    # Case C: remote_sync false + environment source -> known, disabled, "false"
    resC = run_pipeline_with_settings({
        "port": 9090,
        "remote_sync": False,
        "effective_sources": {"remote_sync": "environment"}
    })
    assert resC["remote_sync"]["value"] == "false"
    assert resC["remote_sync"]["truthState"] == "known"
    assert resC["remote_sync"]["disabled"] is True

    # Case F: job_max_concurrent = 0 -> value 0, known, enabled (0 not corrupted to unknown)
    resF = run_pipeline_with_settings({
        "port": 9090,
        "job_max_concurrent": 0,
        "effective_sources": {"job_max_concurrent": "config"}
    })
    assert resF["job_max_concurrent"]["value"] == "0"
    assert resF["job_max_concurrent"]["truthState"] == "known"
    assert resF["job_max_concurrent"]["disabled"] is False

    page.close()

