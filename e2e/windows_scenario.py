# Copyright contributors to Ariekei.
# SPDX-License-Identifier: Apache-2.0

"""E2E: two repos, switching between them, and a window per repo, against a fake GitHub.

Covers: connecting a second repo from the repo menu, the poller keeping both Inboxes
current, a second Ariekei on the same data showing the first one's window instead of running
too, switching back (the window title follows), opening a repo in a new window, a
repo shown in another window being focused instead of opened twice, the selected item
remembered per repo, the open windows coming back after a restart, closing one of them,
an empty window (New window) picking a connected repo, and removing a repo from Ariekei.

Run it with `python3 e2e/run.py <test-app> windows` (see run.py), which sets the environment.
"""

import json
import os
import sqlite3
import subprocess
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from check import check, equal, eventually  # noqa: E402
from ui import open_inbox  # noqa: E402
from wd import Session  # noqa: E402

app, work = sys.argv[1:3]
gh_dir = os.path.join(work, "gh")
data = os.environ.get("ARIEKEI_DATA_DIR", os.path.join(work, "data"))
os.makedirs(gh_dir, exist_ok=True)


def git(cwd, *args):
    return subprocess.run(["git", "-C", cwd, *args], check=True, capture_output=True, text=True).stdout.strip()


def fixture(name):
    """A "GitHub" repo owner/<name> (a local repo reached through insteadOf) and a clone."""
    upstream, clone = os.path.join(work, f"github-{name}"), os.path.join(work, name)
    subprocess.run(["git", "init", "-q", "-b", "main", upstream], check=True)
    open(os.path.join(upstream, "a.txt"), "w").write(f"{name}\n")
    git(upstream, "add", "-A")
    git(upstream, "-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false", "commit", "-qm", "init")
    subprocess.run(["git", "clone", "-q", "-c", "core.autocrlf=false", upstream, clone], check=True)
    remote = f"git@github.com:owner/{name}.git"
    git(clone, "config", f"url.{upstream}.insteadOf", remote)
    git(clone, "remote", "set-url", "origin", remote)
    return upstream, clone


up_proj, proj = fixture("proj")
_, other = fixture("other")
json.dump({
    "viewer": "me", "upstream": up_proj,
    "prs": [
        {"number": 42, "title": "Feature in proj", "author": "me", "head_ref": "main", "base_ref": "main", "state": "OPEN"},
        {"number": 43, "title": "Second in proj", "author": "me", "head_ref": "main", "base_ref": "main", "state": "OPEN"},
        {"number": 7, "repo": "other", "title": "Feature in other", "author": "me", "head_ref": "main", "base_ref": "main", "state": "OPEN"},
    ],
}, open(os.path.join(gh_dir, "data.json"), "w"))


def header(s):
    return s.js("return document.querySelector('.inbox [data-repo-menu] .name')?.textContent")


def rows(s):
    open_inbox(s)
    return s.js("return [...document.querySelectorAll('.inbox .row')].map(r => r.dataset.number)")


def wait_repo(s, name, row):
    s.wait_js(f"return document.querySelector('.inbox [data-repo-menu] .name')?.textContent === 'owner/{name}'", timeout=30)
    s.wait(f".inbox .row[data-number='{row}']", timeout=60)


def menu(s):
    s.click(s.wait(".inbox [data-repo-menu]"))
    s.wait(".menu [data-repo-id]")
    return s.js("return [...document.querySelectorAll('.menu .entry')].map(e => e.innerText.replace(/\\s+/g, ' ').trim())")


def repo_ids():
    db = sqlite3.connect(os.path.join(data, "ariekei.db"))
    ids = dict(db.execute("SELECT name, id FROM repos").fetchall())
    windows = db.execute("SELECT value FROM settings WHERE key = 'open_windows'").fetchone()
    db.close()
    return ids, json.loads(windows[0]) if windows else None


def handles(s, n):
    """Waits for n windows and returns their handles."""
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline:
        found = s.s("GET", "/window/handles")
        if len(found) == n:
            return found
        time.sleep(0.25)
    raise TimeoutError(f"expected {n} windows, have {s.s('GET', '/window/handles')}")


def to_window(s, handle):
    s.s("POST", "/window", {"handle": handle})


# --- run 1 ------------------------------------------------------------------------
s = Session(app)
try:
    s.type(s.wait(".connect form input"), proj)
    s.click(s.wait(".connect form button"))
    wait_repo(s, "proj", 42)
    print("[connect] proj:", header(s), rows(s), "| title:", s.s("GET", "/title"))
    equal("proj's PRs", rows(s), ["43", "42"])
    equal("window title", s.s("GET", "/title"), "owner/proj — Ariekei")

    # Ariekei started again on the same data: it shows this one's window instead and exits.
    again = subprocess.run([app], capture_output=True, text=True, timeout=60)
    print("[second instance] exit:", again.returncode)
    equal("a second Ariekei exits", again.returncode, 0)
    if getattr(s, "log_path", None):
        eventually("this one was asked to show its window", lambda: open(s.log_path, errors="replace").read(),
                   lambda log: "another Ariekei was started" in log, timeout=10)
    equal("this one still works", rows(s), ["43", "42"])

    # A second repo, connected from the repo menu, replaces the first in this window.
    print("[menu]", menu(s))
    s.click(s.wait(".menu [data-connect-another]"))
    s.wait(".connect [data-cancel-connect]")
    print("[connect screen] recent:", s.js("return [...document.querySelectorAll('.connect [data-recent]')].length"))
    s.type(s.wait(".connect form input"), other)
    s.click(s.wait(".connect form button[type=submit]"))
    wait_repo(s, "other", 7)
    print("[connect] other:", header(s), rows(s), "| title:", s.s("GET", "/title"))
    equal("connecting another repo shows it here", (header(s), rows(s)), ("owner/other", ["7"]))

    # Switch back from the menu; the selection is per repo.
    ids, _ = repo_ids()
    print("[menu]", menu(s))
    s.click(s.wait(f".menu [data-repo-id='{ids['proj']}']"))
    wait_repo(s, "proj", 42)
    s.js("document.querySelector(\".inbox .row[data-number='43']\").click()")
    s.wait(".item header .title", timeout=30, text="Second in proj")
    print("[switch] back to:", header(s), rows(s), "| title:", s.s("GET", "/title"))
    equal("switching back", header(s), "owner/proj")

    # The other repo in a new window.
    main = s.s("GET", "/window")
    menu(s)
    s.click(s.wait(f".menu [data-new-window='{ids['other']}']"))
    second = next(h for h in handles(s, 2) if h != main)
    to_window(s, second)
    wait_repo(s, "other", 7)
    print("[new window] shows:", header(s), rows(s), "| title:", s.s("GET", "/title"))
    equal("the new window shows other", (header(s), rows(s)), ("owner/other", ["7"]))

    # Both Inboxes stay current: a new PR in each repo shows up in its own window.
    d = json.load(open(os.path.join(gh_dir, "data.json")))
    d["prs"] += [
        {"number": 8, "repo": "other", "title": "Later in other", "author": "me", "head_ref": "main", "base_ref": "main", "state": "OPEN"},
        {"number": 44, "title": "Later in proj", "author": "me", "head_ref": "main", "base_ref": "main", "state": "OPEN"},
    ]
    json.dump(d, open(os.path.join(gh_dir, "data.json"), "w"))
    s.wait(".inbox .row[data-number='8']", timeout=30)
    print("[poll] other window:", rows(s))
    equal("polling keeps other current", rows(s), ["8", "7"])
    to_window(s, main)
    s.wait(".inbox .row[data-number='44']", timeout=30)
    print("[poll] main window:", rows(s))
    equal("polling keeps proj current", rows(s), ["44", "43", "42"])

    # The main window's menu: other is open elsewhere, so picking it focuses that window.
    menu_entries = menu(s)
    print("[menu] from main:", menu_entries, "| new-window button for other:",
          s.js(f"return !!document.querySelector(\".menu [data-new-window='{ids['other']}']\")"))
    check("a repo open elsewhere is marked so", any("open in another window" in e for e in menu_entries), menu_entries)
    s.click(s.wait(f".menu [data-repo-id='{ids['other']}']"))
    time.sleep(1)
    print("[focus other] main still shows:", header(s), "| windows:", len(handles(s, 2)))
    equal("picking it focuses its window instead", (header(s), len(handles(s, 2))), ("owner/proj", 2))
    print("[db] open windows:", repo_ids()[1], "ids:", ids)
    equal("open windows recorded", repo_ids()[1], [ids["proj"], ids["other"]])
finally:
    s.quit()

# --- run 2: the windows come back ----------------------------------------------------
s = Session(app)
try:
    hs = handles(s, 2)
    shown = {}
    for h in hs:
        to_window(s, h)
        s.wait(".inbox [data-repo-menu]", timeout=30)
        shown[header(s)] = h
    print("[restart] windows:", sorted(shown))
    equal("both windows come back", sorted(shown), ["owner/other", "owner/proj"])
    to_window(s, shown["owner/proj"])
    s.wait(".item header .title", timeout=30, text="Second in proj")
    print("[restart] proj selection kept:", s.js("return document.querySelector('.item header .title').textContent"))

    # Closing the other window forgets it; the remaining one stays recorded. (Closed the
    # way the window manager does; WebDriver's own close only closes the page.)
    to_window(s, shown["owner/other"])
    # Deferred, so the script returns before its window goes away.
    s.js("const label = window.__TAURI_INTERNALS__.metadata.currentWindow.label;"
         "setTimeout(() => window.__TAURI_INTERNALS__.invoke('plugin:window|close', { label }), 100)")
    to_window(s, shown["owner/proj"])
    handles(s, 1)
    time.sleep(0.5)
    print("[close] open windows:", repo_ids()[1])
    equal("closing a window forgets it", repo_ids()[1], [ids["proj"]])

    # New window: an empty one with the Connect screen, where a connected repo is picked.
    menu(s)
    s.click(s.wait(".menu [data-empty-window]"))
    new = next(h for h in handles(s, 2) if h != shown["owner/proj"])
    to_window(s, new)
    s.wait(".connect [data-recent]", timeout=30)
    print("[empty window] recent:", s.js("return [...document.querySelectorAll('.connect [data-recent]')].map(b => b.innerText.replace(/\\s+/g, ' ').trim())"),
          "| cancel offered:", s.js("return !!document.querySelector('.connect [data-cancel-connect]')"))
    ids, _ = repo_ids()
    s.click(s.wait(f".connect [data-recent='{ids['other']}']"))
    wait_repo(s, "other", 7)
    print("[empty window] now shows:", header(s), "| title:", s.s("GET", "/title"), "| open windows:", repo_ids()[1])
    equal("an empty window picks a connected repo", (header(s), repo_ids()[1]), ("owner/other", [ids["proj"], ids["other"]]))

    # Removing a repo from Ariekei: not one open in another window; this window's own goes
    # back to the Connect screen, and the clone stays.
    menu(s)
    offered = s.js("return [...document.querySelectorAll('.menu [data-remove-repo]')].map(b => Number(b.dataset.removeRepo))")
    print("[remove] offered for:", offered)
    equal("Remove is offered for this window's repo, not for one open elsewhere", offered, [ids["other"]])
    s.js(f"document.querySelector(\".menu [data-remove-repo='{ids['other']}']\").click()")
    print("[remove] asks:", s.text(s.wait(f".menu [data-removing='{ids['other']}']")).replace("\n", " "))
    s.click(s.wait("[data-confirm-remove] button", text="Remove"))
    s.wait(".connect [data-recent]", timeout=30)
    recent = s.js("return [...document.querySelectorAll('.connect [data-recent]')].map(b => Number(b.dataset.recent))")
    print("[remove] connect screen, recent:", recent, "| title:", s.s("GET", "/title"), "| repos:", repo_ids()[0],
          "| clone kept:", os.path.isdir(other))
    equal("the repo is gone, its clone is not", (recent, sorted(repo_ids()[0]), os.path.isdir(other)), ([ids["proj"]], ["proj"], True))
    equal("the window is empty again", (s.s("GET", "/title"), repo_ids()[1]), ("Ariekei", [ids["proj"]]))
finally:
    s.quit()
