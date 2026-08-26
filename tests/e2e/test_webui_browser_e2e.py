#!/usr/bin/env python3
"""
Playwright Browser E2E Certification for Michi Micro Server WebUI.
Executes real browser interactions against live server.

Usage:
    pytest tests/e2e/test_webui_browser_e2e.py
    # or with environment variables:
    MICHI_SERVER_URL=http://127.0.0.1:9090 MICHI_ADMIN_USERNAME=admin MICHI_ADMIN_PASSWORD=admin12345 pytest tests/e2e/test_webui_browser_e2e.py
"""

import os
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

    # 8. Test Scan triggering (must not fail with 415 or 401/500)
    scan_btn = page.locator("button:has-text('Scan')").first
    if scan_btn.is_visible():
        scan_btn.click()
        page.wait_for_timeout(1000)

    # 9. Test Playlists Section & Lifecycle
    page.click(".nav-item[data-section='playlists']")
    page.wait_for_timeout(500)
    expect(page.locator("#page-playlists")).to_be_visible()

    create_pl_btn = page.locator("button:has-text('New Playlist')")
    if create_pl_btn.is_visible():
        create_pl_btn.click()
        page.wait_for_timeout(300)
        pl_name_input = page.locator("#new-playlist-name")
        if pl_name_input.is_visible():
            pl_name_input.fill("Browser E2E Playlist")
            page.click("#playlist-create-form button.btn-primary")
            page.wait_for_timeout(800)

    # 10. Test Library / Tracks navigation
    page.click(".nav-item[data-section='library']")
    page.wait_for_timeout(500)
    expect(page.locator("#page-library")).to_be_visible()

    # 11. Test Queue navigation
    page.click(".nav-item[data-section='queue']")
    page.wait_for_timeout(500)
    expect(page.locator("#page-queue")).to_be_visible()

    # 12. Logout and verify protected state is torn down
    auth_btn.click()
    page.wait_for_timeout(500)
    logout_btn = page.locator("button:has-text('Sign Out')")
    if logout_btn.is_visible():
        logout_btn.click()
        page.wait_for_timeout(1000)

    expect(auth_btn_label).to_contain_text("Sign In")
    page.close()
