"""Serve Trunk's dist and smoke-test the game in headless Chromium."""

import os
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from threading import Thread

from playwright.sync_api import sync_playwright


def check_game(browser, url):
    page = browser.new_page(viewport={"width": 720, "height": 480}, device_scale_factor=1)
    errors = []
    page.on("pageerror", lambda error: errors.append(str(error)))
    page.on("console", lambda message: errors.append(message.text) if message.type == "error" else None)

    # Winit's web set_title writes the canvas's alt attribute. The game updates its FPS
    # title from the redraw handler every half second, so repeated writes prove progress.
    page.add_init_script("""
        window.fpsUpdates = 0;
        window.smokeError = false;
        window.addEventListener("error", () => { window.smokeError = true; });
        window.addEventListener("unhandledrejection", () => { window.smokeError = true; });
        new MutationObserver(records => {
            for (const {target} of records) {
                if (target.matches("canvas[alt*=' FPS |']")) window.fpsUpdates++;
            }
        }).observe(document, {subtree: true, attributes: true, attributeFilter: ["alt"]});
    """)
    try:
        response = page.goto(f"{url}/?map=highland&seed=7", wait_until="domcontentloaded")
        assert response and response.ok, "Could not load the game page"
        page.wait_for_function("window.smokeError || window.fpsUpdates >= 2", timeout=45_000)
        assert not errors, "\n".join(errors)
        print("Game rendered repeatedly without input", flush=True)

        page.set_viewport_size({"width": 800, "height": 520})
        page.wait_for_function("""
            () => {
                const canvas = document.querySelector("canvas");
                return window.smokeError || (canvas?.width === 800 && canvas.height === 520);
            }
        """, timeout=20_000)
        updates = page.evaluate("window.fpsUpdates")
        page.wait_for_function("count => window.smokeError || window.fpsUpdates > count",
                               arg=updates, timeout=20_000)
        assert not errors, "\n".join(errors)
        print("Game continued rendering after resize", flush=True)
    except Exception:
        if errors:
            print("Browser errors:\n" + "\n".join(errors), flush=True)
        raise
    finally:
        page.close()


def main():
    dist = Path("dist").resolve()
    assert (dist / "index.html").is_file(), "Run trunk build before the browser smoke test"
    handler = partial(SimpleHTTPRequestHandler, directory=dist)
    with ThreadingHTTPServer(("127.0.0.1", 0), handler) as server:
        thread = Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            with sync_playwright() as playwright:
                browser = playwright.chromium.launch(
                    executable_path=os.environ.get("CHROMIUM_PATH"),
                    args=["--use-angle=swiftshader", "--enable-unsafe-swiftshader"],
                )
                check_game(browser, f"http://127.0.0.1:{server.server_port}")
                browser.close()
        finally:
            server.shutdown()
            thread.join()


if __name__ == "__main__":
    main()
