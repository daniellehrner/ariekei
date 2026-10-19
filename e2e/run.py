# Copyright contributors to Ariekei.
# SPDX-License-Identifier: Apache-2.0

"""Runs the e2e scenarios against a test build of the app, each in a fresh work directory.

  cd src-tauri && cargo build --release --features e2e   (after `npm run build`)
  python3 e2e/run.py src-tauri/target/release/ariekei [scenario ...]

Scenarios: review, windows, lifecycle, task, agent (default: all). GitHub is e2e/fake_gh.py and the
agent e2e/fake_claude.py, so nothing leaves the machine. On Linux the app runs on a private
Xvfb display and D-Bus session (Xvfb and dbus-daemon), so its windows and notifications never
reach your desktop; E2E_HEADED=1 shows them on your display instead. Work directories go to
$E2E_WORK (default: a new temporary directory) and are kept after a failure; the app's log is
in <work>/<scenario>/logs.
"""

import os
import shutil
import subprocess
import sys
import tempfile
import time

HERE = os.path.dirname(os.path.abspath(__file__))
# Git settings for the scenarios' git calls and the app's alike, the same on every OS (Windows
# runners convert line endings on checkout by default).
GIT_SETTINGS = {"core.autocrlf": "false", "init.defaultBranch": "main", "commit.gpgsign": "false"}
GIT = {"GIT_CONFIG_COUNT": str(len(GIT_SETTINGS))}
for i, (key, value) in enumerate(GIT_SETTINGS.items()):
    GIT[f"GIT_CONFIG_KEY_{i}"] = key
    GIT[f"GIT_CONFIG_VALUE_{i}"] = value
SCENARIOS = {
    # name: extra environment
    "review": {},
    "windows": {"ARIEKEI_POLL_SECS": "3"},
    "lifecycle": {"ARIEKEI_POLL_SECS": "3"},
    "task": {"ARIEKEI_POLL_SECS": "3"},
    "agent": {"ARIEKEI_NO_POLL": "1", "ARIEKEI_IDLE_SECS": "5", "ARIEKEI_QUIET_SECS": "5"},
}


def start(args):
    """Starts a daemon that writes its address (display number, bus address) to a pipe."""
    r, w = os.pipe()
    proc = subprocess.Popen([a.replace("{fd}", str(w)) for a in args], pass_fds=(w,),
                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    os.close(w)
    out = b""
    while not out.endswith(b"\n"):
        chunk = os.read(r, 256)
        if not chunk:
            break
        out += chunk
    os.close(r)
    return proc, out.decode().strip()


def isolated():
    """Linux: a private Xvfb display and D-Bus session for the app (and their environment).
    Only DISPLAY would not do: GTK prefers Wayland while WAYLAND_DISPLAY is set."""
    if not sys.platform.startswith("linux") or os.environ.get("E2E_HEADED") == "1":
        return [], {}
    if not shutil.which("Xvfb"):
        sys.exit("The e2e tests need Xvfb (e.g. apt install xvfb), or E2E_HEADED=1 to use your display.")
    xvfb, display = start(["Xvfb", "-displayfd", "{fd}", "-screen", "0", "1400x900x24", "-nolisten", "tcp"])
    procs, env = [xvfb], {"DISPLAY": f":{display}", "GDK_BACKEND": "x11"}
    if shutil.which("dbus-daemon"):
        bus, address = start(["dbus-daemon", "--session", "--nofork", "--print-address={fd}"])
        procs.append(bus)
        env["DBUS_SESSION_BUS_ADDRESS"] = address
    return procs, env


def main():
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    app = os.path.abspath(sys.argv[1])
    names = sys.argv[2:] or list(SCENARIOS)
    unknown = [n for n in names if n not in SCENARIOS]
    if unknown:
        sys.exit(f"unknown scenario(s): {', '.join(unknown)}; known: {', '.join(SCENARIOS)}")
    daemons, display = isolated()
    try:
        run(app, names, display)
    finally:
        for d in daemons:
            d.terminate()


def run(app, names, display):
    root = os.environ.get("E2E_WORK") or tempfile.mkdtemp(prefix="ariekei-e2e-")
    results = []
    for name in names:
        work = os.path.join(root, name)
        if os.path.exists(work):
            shutil.rmtree(work)
        os.makedirs(work)
        data = os.path.join(work, "data")
        env = dict(
            os.environ,
            FAKE_GH_DIR=os.path.join(work, "gh"),
            ARIEKEI_GH=os.path.join(HERE, "fake_gh.py"),
            ARIEKEI_CLAUDE=os.path.join(HERE, "fake_claude.py"),
            FAKE_CLAUDE_DIR=os.path.join(work, "claude"),
            ARIEKEI_DATA_DIR=data,
            E2E_LOG_DIR=os.path.join(work, "logs"),
            E2E_SHOTS="0",
            PYTHONIOENCODING="utf-8",
            **GIT,
            **display,
            **SCENARIOS[name],
        )
        if display:
            env.pop("WAYLAND_DISPLAY", None)
        args = [sys.executable, "-I", os.path.join(HERE, f"{name}_scenario.py"), app, work]
        if name == "agent":
            args.append(data)
        print(f"\n=== {name}", flush=True)
        t0 = time.monotonic()
        code = subprocess.run(args, env=env).returncode
        results.append((name, code, time.monotonic() - t0))
        if code == 0:
            shutil.rmtree(work, ignore_errors=True)

    print()
    for name, code, secs in results:
        print(f"{'ok  ' if code == 0 else 'FAIL'} {name} ({secs:.0f}s)")
    failed = [name for name, code, _ in results if code]
    if failed:
        print(f"work directories kept in {root}")
        sys.exit(1)
    shutil.rmtree(root, ignore_errors=True)


if __name__ == "__main__":
    main()
