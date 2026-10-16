"""E2E for the agent in the terminal and the review loop, on a local fixture (no real GitHub
repo is touched).

The fixture is a clone whose `origin` looks like git@github.com:owner/proj.git (mapped to
a local "GitHub" repo with insteadOf), plus two work items inserted into Ariekei's
database as if the poller had found them. Covers: Inbox, Start working (worktree on the
PR branch), the agent in the terminal (slash-command autocomplete, a committing turn),
Review since the PR base, a second agent commit reviewed on its own (its commit scope),
Inbox attention, idle shutdown (not while a background job runs), and resume after an app
restart.

The agent is e2e/fake_claude.py (ARIEKEI_CLAUDE): a small terminal agent that commits when
asked and reports through the hooks like Claude Code. With ARIEKEI_CLAUDE unset the user's
real `claude` runs instead (two short turns; answer its trust dialog). Run it with
`python3 e2e/run.py <test-app> agent`, which sets the environment (ARIEKEI_NO_POLL=1,
ARIEKEI_IDLE_SECS=5, fake gh).
"""

import json
import os
import sqlite3
import subprocess
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from check import equal, eventually  # noqa: E402
from ui import open_inbox, select_row, wait_rows  # noqa: E402
from wd import Session  # noqa: E402

app, work, data = sys.argv[1:4]
shots = os.path.join(work, "shots")
os.makedirs(shots, exist_ok=True)

ENTER, DOWN, BACKSPACE = "\r", "\x1b[B", "\x7f"  # as the terminal sends them
SCREEN = """
const t = (window.__ariekeiTerminals || {})[arguments[0]];
if (!t) return '';
const b = t.buffer.active, lines = [];
for (let i = 0; i < b.length; i++) lines.push(b.getLine(i).translateToString(true));
return lines.join('\\n');
"""
GITHUB = "git@github.com:owner/proj.git"


def git(cwd, *args):
    return subprocess.run(["git", "-C", cwd, *args], check=True, capture_output=True, text=True).stdout.strip()


def commit(cwd, msg):
    git(cwd, "add", "-A")
    git(cwd, "-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false", "commit", "-qm", msg)


def fixture():
    upstream, clone = os.path.join(work, "github-proj"), os.path.join(work, "proj")
    subprocess.run(["git", "init", "-q", "-b", "main", upstream], check=True)
    with open(os.path.join(upstream, "a.txt"), "w") as f:
        f.write("one\n")
    commit(upstream, "init")
    git(upstream, "checkout", "-q", "-b", "feature/x")
    with open(os.path.join(upstream, "b.txt"), "w") as f:
        f.write("feature\n")
    commit(upstream, "feature work")
    git(upstream, "checkout", "-q", "main")
    subprocess.run(["git", "clone", "-q", upstream, clone], check=True)
    git(clone, "config", f"url.{upstream}.insteadOf", GITHUB)
    git(clone, "remote", "set-url", "origin", GITHUB)
    return clone


def pr(number, title):
    return {
        "number": number, "title": title, "url": f"https://github.com/owner/proj/pull/{number}",
        "isDraft": False, "updatedAt": "2026-10-13T10:00:00Z", "headRef": "feature/x", "headSha": "",
        "headOwner": "owner", "headRepo": "proj", "headSshUrl": GITHUB, "headHttpsUrl": "",
        "baseRef": "main", "reviewDecision": "REVIEW_REQUIRED", "ci": "SUCCESS", "failingChecks": [],
        "unresolvedThreads": 0, "latestActivity": None, "reviews": [],
    }


def screen(s, key):
    return s.js(SCREEN, key) or ""


def wait_screen(s, key, text, timeout=60):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if text in screen(s, key):
            return
        time.sleep(0.3)
    tail = "\n".join(line for line in screen(s, key).splitlines() if line.strip())[-1500:]
    raise TimeoutError(f"{text!r} never appeared in terminal {key}; screen ends with:\n{tail}")


def row_info(s, number):
    open_inbox(s)
    return s.js(
        "const r = document.querySelector(`.inbox .row[data-number='${arguments[0]}']`);"
        "if (!r) return null;"
        "const agent = r.querySelector('[data-agent]')?.dataset.agent ?? null;"
        "return {agent, attention: r.querySelector('.attention')?.textContent ?? null};",
        number,
    )


def wait_agent(s, number, wanted, timeout=180):
    deadline, last = time.monotonic() + timeout, None
    while time.monotonic() < deadline:
        last = (row_info(s, number) or {}).get("agent")
        if last in wanted:
            return last
        time.sleep(0.3)
    raise TimeoutError(f"#{number}: agent {last!r}, wanted {wanted}")


def select(s, number):
    select_row(s, number)


def keys(s, text):
    """Types into the visible terminal, as the keyboard would."""
    s.wait(".terminal:not(.hidden) .xterm")
    s.js("Object.values(window.__ariekeiTerminals).find(t => t.element?.isConnected && t.element.offsetParent)?.input(arguments[0])", text)


def prompt(s, number, text):
    """Sends a prompt and waits for the turn to end, accepting permission prompts."""
    keys(s, text)
    keys(s, ENTER)
    wait_agent(s, number, {"working", "waitingForPermission"}, timeout=30)
    while True:
        st = wait_agent(s, number, {"waitingForPermission", "finished", "error"})
        if st != "waitingForPermission":
            return st
        keys(s, ENTER)  # accept the highlighted "Yes"
        wait_agent(s, number, {"working", "finished", "error"}, timeout=30)


def tab(s, name):
    s.js("[...document.querySelectorAll('[role=tab]')].find(t => t.textContent.trim().startsWith(arguments[0]))?.click()", name)


def review_marker(s):
    s.wait(".review .marker")
    return " ".join(s.js("return document.querySelector('.review .marker').textContent").split())


def invoke(s, cmd, args):
    return s.js("return window.__TAURI_INTERNALS__.invoke(arguments[0], arguments[1])", cmd, args)


clone = fixture()
worktree = os.path.join(work, "proj_pr42")
# The fake gh only has to know who the viewer is; the PRs are inserted below.
if os.environ.get("FAKE_GH_DIR"):
    os.makedirs(os.environ["FAKE_GH_DIR"], exist_ok=True)
    with open(os.path.join(os.environ["FAKE_GH_DIR"], "data.json"), "w") as f:
        json.dump({"viewer": "me", "prs": []}, f)

# --- run 1: connect the fixture clone -------------------------------------------
s = Session(app)
try:
    s.type(s.wait(".connect form input"), clone)
    s.click(s.wait(".connect form button"))
    s.wait(".inbox [data-repo-menu]", timeout=30)
    print("[connect]", s.js("return document.querySelector('.inbox [data-repo-menu] .name').textContent"))
finally:
    s.quit()

# The poller is off for the fake repo: add the PRs it would have found.
db = sqlite3.connect(os.path.join(data, "ariekei.db"))
repo_id = db.execute("SELECT id FROM repos").fetchone()[0]
for number, title in [(42, "Add the feature"), (43, "Another PR")]:
    db.execute("INSERT INTO work_items (repo_id, number, snapshot, open, last_seen) VALUES (?, ?, ?, 1, '')",
               (repo_id, number, json.dumps(pr(number, title))))
db.commit()
db.close()

# --- run 2: work on PR #42 -----------------------------------------------------------
s = Session(app)
try:
    wait_rows(s)
    print("[inbox]", s.js("return [...document.querySelectorAll('.inbox .row .title')].map(t => t.textContent)"))
    equal("the inserted PRs", sorted(s.js("return [...document.querySelectorAll('.inbox .row')].map(r => r.dataset.number)")), ["42", "43"])
    select(s, 42)
    s.click(s.wait("[data-start]"))
    s.wait(".terminal", timeout=120)
    print("[start]", s.js("return document.querySelector('[data-notice]')?.textContent"), "| branch:", git(worktree, "rev-parse", "--abbrev-ref", "HEAD"),
          "| upstream:", git(worktree, "rev-parse", "--abbrev-ref", "@{upstream}"))
    equal("the worktree is on the PR branch", git(worktree, "rev-parse", "--abbrev-ref", "HEAD"), "feature/x")
    item_id = int(s.wait_js("return Object.keys(window.__ariekeiTerminals || {})[0] || null", timeout=30))
    key = str(item_id)
    wait_agent(s, 42, {"ready"}, timeout=40)
    # A repository Claude Code has not seen asks for trust first (default: "No, exit").
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline and "trust this folder" not in screen(s, key) and "? for shortcuts" not in screen(s, key):
        time.sleep(0.3)
    if "trust this folder" in screen(s, key):
        keys(s, DOWN + ENTER)
        print("[agent] answered the trust dialog for the new repository")
    wait_screen(s, key, "❯")
    time.sleep(1)
    keys(s, "/code-rev")
    wait_screen(s, key, "/code-review", timeout=15)
    print("[agent] TUI up, /code-review autocompletes")
    keys(s, BACKSPACE * len("/code-rev"))
    time.sleep(1.5)  # let the slash-command popup close before typing the prompt

    st = prompt(s, 42, "Append a line with the word two to a.txt, then commit with the message 'add two'. Do nothing else.")
    print("[turn 1]", st, "| last commit:", git(worktree, "log", "-1", "--format=%s"))
    equal("turn 1 commits", (st, git(worktree, "log", "-1", "--format=%s")), ("finished", "add two"))
    print("[inbox] #42 attention:", row_info(s, 42)["attention"])
    equal("after the turn: nothing in the Inbox wants attention", row_info(s, 42)["attention"], None)
    s.screenshot(os.path.join(shots, "agent-terminal.png"))

    tab(s, "Review")
    s.wait(".files [data-file]", timeout=30)
    print("[review]", review_marker(s), "| files:", s.js("return [...document.querySelectorAll('.files [data-file]')].map(b => b.dataset.file)"))
    equal("files vs main", s.js("return [...document.querySelectorAll('.files [data-file]')].map(b => b.dataset.file)"), ["a.txt", "b.txt"])
    s.screenshot(os.path.join(shots, "agent-review-base.png"))
    tab(s, "Terminal")
    st = prompt(s, 42, "Append a line with the word three to a.txt, then commit with the message 'add three'. Do nothing else.")
    print("[turn 2]", st, "| last commit:", git(worktree, "log", "-1", "--format=%s"))
    equal("turn 2 commits", (st, git(worktree, "log", "-1", "--format=%s")), ("finished", "add three"))
    tab(s, "Review")
    s.click(s.wait(".review [data-reload]"))
    eventually("the review has the new commit", lambda: invoke(s, "review_state", {"id": item_id})["commits"][0]["subject"], lambda v: v == "add three")
    print("[review]", review_marker(s), "| scopes:", s.js("return [...document.querySelectorAll('.scopes button, .scope-folded .active-scope, .scope-folded button')].map(l => l.textContent.trim())"))
    newest = invoke(s, "review_state", {"id": item_id})["commits"][0]
    diff = invoke(s, "review_file", {"id": item_id, "from": newest["sha"] + "^", "to": newest["sha"], "path": "a.txt", "ignoreWs": False})
    print("[review] a.txt in", newest["sha"][:8], newest["subject"], ": hunks", diff["hunks"], "| new side ends with:", diff["new"].splitlines()[-2:])
    equal("the newest commit has only turn 2's line", diff["new"].splitlines()[-1], "three")
    equal("in one hunk", len(diff["hunks"]), 1)
    s.screenshot(os.path.join(shots, "agent-review-commit.png"))

    # Leave #42 (its terminal detaches); with ARIEKEI_IDLE_SECS=5 the idle agent closes, but
    # not while a background job it started still runs (closing would end the job).
    tab(s, "Terminal")
    st = prompt(s, 42, "Run a 25 second job in the background")
    equal("the background job's turn finishes", st, "finished")
    select(s, 43)
    s.wait("[data-start]", timeout=10)
    t0 = time.monotonic()
    time.sleep(15)
    equal("an idle agent with a background job stays open", (row_info(s, 42) or {}).get("agent"), "finished")
    wait_agent(s, 42, {"none"}, timeout=90)
    print(f"[idle] #42's agent closed {time.monotonic() - t0:.0f}s after it was left idle")
    select(s, 42)
    wait_agent(s, 42, {"ready"}, timeout=40)
    try:
        wait_screen(s, key, "add three", timeout=30)
    except TimeoutError:
        # Seen on Windows only: see the app log for the agent's start, output and exit.
        print("[idle] resume failed; #42's agent is", (row_info(s, 42) or {}).get("agent"))
        raise
    print("[idle] reopening #42 resumed the conversation")
finally:
    s.quit()
time.sleep(2)

# --- run 3: restart -----------------------------------------------------------------
s = Session(app)
try:
    wait_rows(s)
    select(s, 42)
    wait_agent(s, 42, {"ready"}, timeout=40)
    # The resumed TUI shows the latest turns of the conversation.
    wait_screen(s, str(item_id), "add three", timeout=30)
    tab(s, "Review")
    print("[restart] conversation resumed;", review_marker(s))
    s.screenshot(os.path.join(shots, "agent-restart.png"))
finally:
    s.quit()
