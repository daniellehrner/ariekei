# Copyright contributors to Ariekei.
# SPDX-License-Identifier: Apache-2.0

"""Drives the Ariekei app for the e2e scenarios (stdlib only).

By default the app is a test build (`npx tauri build --no-bundle --features e2e`, see e2e/run.py):
`Session(app)` starts it with the e2e bridge on a free localhost port and runs JavaScript in
its windows through it. This works on Linux, macOS and Windows. The environment of the
calling process (fake gh/claude, data dir, …) is the app's environment.

With E2E_WEBDRIVER=http://127.0.0.1:4444 the session goes through tauri-driver instead
(W3C WebDriver; Linux and Windows only), which can also take screenshots (e2e/gallery.py).

Both offer the same calls: js, find, wait, wait_js, click, type, text, screenshot, quit, and
s(method, path) for the WebDriver paths the scenarios use (/window/handles, /window, /title,
/window/rect).
"""

import base64
import json
import os
import socket
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request

ELEMENT = "element-6066-11e4-a52e-4f735466cecf"


class WebDriverError(Exception):
    pass


def Session(application):
    base = os.environ.get("E2E_WEBDRIVER")
    return WebDriverSession(application, base) if base else BridgeSession(application)


def _request(base, method, path, body=None, timeout=120):
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(base + path, data=data, method=method, headers={"Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            return json.load(r)
    except urllib.error.HTTPError as e:
        raise WebDriverError(f"{method} {path}: {e.read().decode(errors='replace')[:600]}") from None


class _Common:
    def wait(self, css, timeout=60, text=None):
        """Waits for an element matching css (and containing text, if given)."""
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            try:
                for el in self.find(css):
                    if text is None or text in self.text(el):
                        return el
            except WebDriverError:
                pass  # e.g. a window whose page is still loading
            time.sleep(0.25)
        raise TimeoutError(f"no {css!r}" + (f" containing {text!r}" if text else "") + f" within {timeout}s" + self._page())

    def wait_js(self, script, timeout=60):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            try:
                v = self.js(script)
                if v:
                    return v
            except WebDriverError:
                pass
            time.sleep(0.25)
        raise TimeoutError(f"condition never true: {script}" + self._page())

    def _page(self):
        """What the window shows, for a failure message."""
        try:
            text = self.js("return document.body.innerText || document.body.textContent")
        except Exception as e:
            return f"\n(page text unavailable: {e})"
        return "\n--- the page shows:\n" + "\n".join(l for l in text.splitlines() if l.strip())[:3000]


class BridgeSession(_Common):
    """The app started here, driven through its e2e bridge."""

    def __init__(self, application):
        with socket.socket() as probe:
            probe.bind(("127.0.0.1", 0))
            port = probe.getsockname()[1]
        self.base = f"http://127.0.0.1:{port}"
        env = dict(os.environ, ARIEKEI_E2E_PORT=str(port), ARIEKEI_PYTHON=sys.executable)
        log_dir = os.environ.get("E2E_LOG_DIR") or tempfile.gettempdir()
        os.makedirs(log_dir, exist_ok=True)
        self.log_path = os.path.join(log_dir, f"ariekei-{time.strftime('%H%M%S')}-{port}.log")
        self.log = open(self.log_path, "wb")
        t0 = time.monotonic()
        self.proc = subprocess.Popen([application], env=env, stdout=self.log, stderr=subprocess.STDOUT)
        self.window = "main"
        deadline = time.monotonic() + 90
        while True:
            if self.proc.poll() is not None:
                raise WebDriverError(f"the app exited with {self.proc.returncode}; log: {self.log_path}")
            try:
                if _request(self.base, "GET", "/status", timeout=2) == "ok" and self.js(
                    "return !!document.getElementById('app')?.firstChild", timeout=3
                ):
                    break
            except (OSError, WebDriverError):
                pass
            if time.monotonic() > deadline:
                raise TimeoutError(f"the app did not come up; log: {self.log_path}")
            time.sleep(0.25)
        self.started_in = time.monotonic() - t0
        size = self.js("return [innerWidth, innerHeight]")
        print(f"   (app up in {self.started_in:.1f}s, window {size[0]}x{size[1]})", flush=True)

    def _eval(self, window, script, args, timeout):
        body = {"window": window, "script": script, "args": list(args), "timeout_ms": int(timeout * 1000)}
        return _request(self.base, "POST", "/eval", body, timeout=timeout + 10)

    def js(self, script, *args, timeout=30):
        return self._eval(self.window, script, args, timeout)

    def clipboard(self):
        """The text on the system clipboard, as the app reads it."""
        return _request(self.base, "GET", "/clipboard")

    def s(self, method, path, body=None):
        if (method, path) == ("GET", "/window/handles"):
            return _request(self.base, "GET", "/windows")
        if (method, path) == ("GET", "/window"):
            return self.window
        if (method, path) == ("POST", "/window"):
            self.window = body["handle"]
            return None
        if (method, path) == ("GET", "/title"):
            return self.js("return document.title")
        if (method, path) == ("POST", "/window/rect"):
            return _request(self.base, "POST", "/resize", {"window": self.window, **body})
        raise WebDriverError(f"{method} {path} is not supported by the e2e bridge")

    # Elements are kept in the page (window.__e2e) and referred to as "<window>:<index>".
    def find(self, css):
        found = self.js(
            "const reg = (window.__e2e ??= { next: 0, els: new Map() });"
            "return [...document.querySelectorAll(arguments[0])].map(e => {"
            "  const id = reg.next++; reg.els.set(id, e); reg.els.delete(id - 5000); return id; });",
            css,
        )
        return [f"{self.window}:{i}" for i in found]

    def _on(self, el, body, *args):
        window, index = el.rsplit(":", 1)
        script = (
            "const el = window.__e2e?.els.get(arguments[0]);"
            "if (!el) throw new Error('stale element');"
            "const args = [...arguments].slice(1);" + body
        )
        return self._eval(window, script, [int(index), *args], 30)

    def click(self, el):
        self._on(el, "el.scrollIntoView({ block: 'nearest' }); el.focus?.(); el.click();")

    def type(self, el, text):
        """Appends text to an input or textarea the way typing would (an input event per call)."""
        self._on(
            el,
            "el.focus();"
            "const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;"
            "Object.getOwnPropertyDescriptor(proto, 'value').set.call(el, el.value + args[0]);"
            "el.dispatchEvent(new Event('input', { bubbles: true }));",
            text,
        )

    def text(self, el):
        return self._on(el, "return el.innerText;")

    def screenshot(self, path):
        """Not available through the bridge (scenarios check the DOM); see E2E_WEBDRIVER."""
        return False

    def quit(self):
        try:
            _request(self.base, "POST", "/quit", timeout=10)
            self.proc.wait(timeout=30)
        except Exception:
            self.proc.kill()
            self.proc.wait()
        self.log.close()


class WebDriverSession(_Common):
    """The app started by tauri-driver (W3C WebDriver)."""

    def __init__(self, application, base):
        self.base = base
        caps = {"capabilities": {"alwaysMatch": {"browserName": "wry", "tauri:options": {"application": application}}}}
        t0 = time.monotonic()
        self.id = _request(self.base, "POST", "/session", caps)["value"]["sessionId"]
        self.started_in = time.monotonic() - t0

    def _call(self, method, path, body=None, timeout=120):
        return _request(self.base, method, path, body, timeout).get("value")

    def s(self, method, path, body=None):
        return self._call(method, f"/session/{self.id}{path}", body)

    def js(self, script, *args, timeout=None):
        return self.s("POST", "/execute/sync", {"script": script, "args": list(args)})

    def find(self, css):
        found = self.s("POST", "/elements", {"using": "css selector", "value": css})
        return [f[ELEMENT] for f in found]

    def click(self, el):
        self.s("POST", f"/element/{el}/click", {})

    def type(self, el, text):
        self.s("POST", f"/element/{el}/value", {"text": text})

    def text(self, el):
        return self.s("GET", f"/element/{el}/text")

    def screenshot(self, path):
        """Slow under Xvfb; E2E_SHOTS=0 skips them. Scenarios check the DOM, not pictures."""
        if os.environ.get("E2E_SHOTS") == "0":
            return False
        try:
            data = base64.b64decode(self._call("GET", f"/session/{self.id}/screenshot", timeout=300))
        except (TimeoutError, OSError, WebDriverError) as e:
            # A missing picture must not fail a scenario; the DOM checks are what count.
            print(f"   (screenshot {os.path.basename(path)} skipped: {e})")
            return False
        with open(path, "wb") as f:
            f.write(data)
        return True

    def quit(self):
        try:
            self._call("DELETE", f"/session/{self.id}")
        except Exception:
            pass
