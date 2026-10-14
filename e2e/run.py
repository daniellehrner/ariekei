"""Runs the e2e scenarios against a test build of the app, each in a fresh work directory.

  cd src-tauri && cargo build --release --features e2e   (after `npm run build`)
  python3 e2e/run.py src-tauri/target/release/ariekei [scenario ...]

Scenarios: review, windows, lifecycle, m1 (default: all). GitHub is e2e/fake_gh.py and the
agent e2e/fake_claude.py, so nothing leaves the machine. On Linux without a display, run it
under `xvfb-run -a`. Work directories go to $E2E_WORK (default: a new temporary directory)
and are kept after a failure; the app's log is in <work>/<scenario>/logs.
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
    "m1": {"ARIEKEI_NO_POLL": "1", "ARIEKEI_IDLE_SECS": "5"},
}


def main():
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    app = os.path.abspath(sys.argv[1])
    names = sys.argv[2:] or list(SCENARIOS)
    unknown = [n for n in names if n not in SCENARIOS]
    if unknown:
        sys.exit(f"unknown scenario(s): {', '.join(unknown)}; known: {', '.join(SCENARIOS)}")
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
            **SCENARIOS[name],
        )
        args = [sys.executable, "-I", os.path.join(HERE, f"{name}_scenario.py"), app, work]
        if name == "m1":
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
