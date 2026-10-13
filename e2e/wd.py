"""Minimal W3C WebDriver client (stdlib only) for driving the Tauri app via tauri-driver."""

import base64
import json
import time
import urllib.error
import urllib.request

ELEMENT = "element-6066-11e4-a52e-4f735466cecf"


class WebDriverError(Exception):
    pass


class Session:
    def __init__(self, application, base="http://127.0.0.1:4444"):
        self.base = base
        caps = {"capabilities": {"alwaysMatch": {"browserName": "wry", "tauri:options": {"application": application}}}}
        t0 = time.monotonic()
        self.id = self._call("POST", "/session", caps)["sessionId"]
        self.started_in = time.monotonic() - t0

    def _call(self, method, path, body=None):
        data = json.dumps(body).encode() if body is not None else None
        req = urllib.request.Request(self.base + path, data=data, method=method, headers={"Content-Type": "application/json"})
        try:
            with urllib.request.urlopen(req, timeout=120) as r:
                return json.load(r).get("value") if path != "/session" else json.load(r)["value"]
        except urllib.error.HTTPError as e:
            raise WebDriverError(f"{method} {path}: {e.read().decode(errors='replace')[:400]}") from None

    def s(self, method, path, body=None):
        return self._call(method, f"/session/{self.id}{path}", body)

    def js(self, script, *args):
        return self.s("POST", "/execute/sync", {"script": script, "args": list(args)})

    def find(self, css):
        found = self.s("POST", "/elements", {"using": "css selector", "value": css})
        return [f[ELEMENT] for f in found]

    def wait(self, css, timeout=60, text=None):
        """Waits for an element matching css (and containing text, if given)."""
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            for el in self.find(css):
                if text is None or text in self.text(el):
                    return el
            time.sleep(0.25)
        raise TimeoutError(f"no {css!r}" + (f" containing {text!r}" if text else "") + f" within {timeout}s")

    def wait_js(self, script, timeout=60):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            v = self.js(script)
            if v:
                return v
            time.sleep(0.25)
        raise TimeoutError(f"condition never true: {script}")

    def click(self, el):
        self.s("POST", f"/element/{el}/click", {})

    def type(self, el, text):
        self.s("POST", f"/element/{el}/value", {"text": text})

    def text(self, el):
        return self.s("GET", f"/element/{el}/text")

    def screenshot(self, path):
        with open(path, "wb") as f:
            f.write(base64.b64decode(self.s("GET", "/screenshot")))

    def quit(self):
        try:
            self._call("DELETE", f"/session/{self.id}")
        except Exception:
            pass
