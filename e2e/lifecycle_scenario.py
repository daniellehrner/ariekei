"""E2E: a work item's life, from an issue to cleanup, against a fake GitHub.

GitHub is e2e/fake_gh.py (ARIEKEI_GH); the "GitHub" repo is a local git repo reached
through insteadOf. Covers: an assigned issue in the Inbox, Start working (branch
issue-<N>-<slug> from main, push set up), Done refused while a commit is unpushed and while
a file is uncommitted, git push → draft PR opened ("Closes #N") and the item becomes the PR,
the PR merged → automatic cleanup (worktree, branch, agent, item); and an adopted worktree
that is kept when its PR is merged. Commits are made with git directly (no agent turn).

  FAKE_GH_DIR=<work>/gh ARIEKEI_GH=$PWD/e2e/fake_gh.py ARIEKEI_DATA_DIR=<work>/data \\
  ARIEKEI_POLL_SECS=3 xvfb-run -a -s "-screen 0 1400x900x24" tauri-driver --port 4444 &
  python3 -I e2e/lifecycle_scenario.py <app-binary> <work>
"""

import json
import os
import subprocess
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from wd import Session  # noqa: E402

app, work = sys.argv[1:3]
gh_dir = os.path.join(work, "gh")
os.makedirs(gh_dir, exist_ok=True)
GITHUB = "git@github.com:owner/proj.git"


def git(cwd, *args):
    return subprocess.run(["git", "-C", cwd, *args], check=True, capture_output=True, text=True).stdout.strip()


def commit(cwd, msg):
    git(cwd, "add", "-A")
    git(cwd, "-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false", "commit", "-qm", msg)


upstream, clone = os.path.join(work, "github-proj"), os.path.join(work, "proj")
subprocess.run(["git", "init", "-q", "-b", "main", upstream], check=True)
open(os.path.join(upstream, "a.txt"), "w").write("one\n")
commit(upstream, "init")
git(upstream, "checkout", "-q", "-b", "feature/x")
open(os.path.join(upstream, "x.txt"), "w").write("x\n")
commit(upstream, "feature x")
git(upstream, "checkout", "-q", "main")
# Let pushes to the checked-out "GitHub" repo's main not be refused (we push other branches).
git(upstream, "config", "receive.denyCurrentBranch", "ignore")
subprocess.run(["git", "clone", "-q", "-c", "core.autocrlf=false", upstream, clone], check=True)
git(clone, "config", f"url.{upstream}.insteadOf", GITHUB)
git(clone, "remote", "set-url", "origin", GITHUB)
# The user's own worktree for PR #42 (to be adopted, and kept at cleanup).
mine = os.path.join(work, "my-pr42")
git(clone, "worktree", "add", "-q", "-b", "pr-42", mine, "origin/feature/x")

data_file = os.path.join(gh_dir, "data.json")
json.dump({
    "viewer": "me", "upstream": upstream,
    "prs": [{"number": 42, "title": "Feature x", "author": "me", "head_ref": "feature/x", "base_ref": "main", "state": "OPEN"}],
    "issues": [{"number": 5, "title": "Crash on start", "author": "carol", "body": "It crashes when the config is empty.", "assigned": True}],
}, open(data_file, "w"))


def set_state(number, state):
    data = json.load(open(data_file))
    for p in data["prs"]:
        if p["number"] == number:
            p["state"] = state
    json.dump(data, open(data_file, "w"))


def writes():
    path = os.path.join(gh_dir, "writes.jsonl")
    return [json.loads(line) for line in open(path)] if os.path.exists(path) else []


def rows(s):
    return s.js("return [...document.querySelectorAll('.inbox .row')].map(r => r.dataset.number)")


def select_row(s, number):
    s.wait(f".inbox .row[data-number='{number}']", timeout=60)
    s.js("document.querySelector(`.inbox .row[data-number='${arguments[0]}']`).click()", number)


def header_button(s, text):
    s.click(s.wait(".item header .title button", text=text))


def press_done(s):
    header_button(s, "Done")
    header_button(s, "Yes, clean up")


def wait_gone(s, number, timeout=60):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if str(number) not in (rows(s) or []):
            return
        time.sleep(0.5)
    raise TimeoutError(f"#{number} is still in the Inbox")


s = Session(app)
try:
    s.type(s.wait(".connect form input"), clone)
    s.click(s.wait(".connect form button"))
    s.wait(".inbox .row[data-number='5']", timeout=60)
    print("[inbox]", s.js("return [...document.querySelectorAll('.inbox h3')].map(h => h.textContent.trim())"))

    # --- the issue ---
    select_row(s, 5)
    s.wait(".overview h2", timeout=30, text="Crash on start")
    print("[issue] overview:", s.js("return document.querySelector('.overview .meta').innerText.replace(/\\n+/g, ' ')"),
          "| tabs:", s.js("return [...document.querySelectorAll('.item nav button')].map(b => b.textContent.trim())"))
    header_button(s, "Start working")
    s.wait(".terminal", timeout=120)
    wt = os.path.join(work, "proj_issue5")
    branch = git(wt, "rev-parse", "--abbrev-ref", "HEAD")
    print("[start] worktree:", os.path.isdir(wt), "| branch:", branch, "| from main:", git(wt, "rev-parse", "HEAD") == git(clone, "rev-parse", "origin/main"),
          "| push to:", git(clone, "config", f"branch.{branch}.remote"))

    # The fix (made directly with git), not pushed yet: Done must refuse.
    open(os.path.join(wt, "a.txt"), "a").write("fixed\n")
    commit(wt, "Fix the crash on start")
    open(os.path.join(wt, "notes.txt"), "w").write("todo\n")
    press_done(s)
    s.wait(".blocked", timeout=30)
    print("[done] refused:", s.js("return [...document.querySelectorAll('.blocked li')].map(l => l.textContent)"),
          "| worktree still there:", os.path.isdir(wt))
    os.remove(os.path.join(wt, "notes.txt"))

    s.click(s.wait("nav button", text="Review"))
    s.wait(".review .marker", timeout=30)
    print("[review]", " ".join(s.js("return document.querySelector('.review .marker').textContent").split()),
          "| submit to GitHub offered:", s.js("return !!document.querySelector('.review .comments.quiet button')"))

    # Push: the next poll opens a draft PR and the item becomes that PR.
    git(wt, "push", "-q")
    deadline = time.monotonic() + 60
    while time.monotonic() < deadline and "100" not in (rows(s) or []):
        time.sleep(0.5)
    created = [w for w in writes() if w.get("post", "").endswith("/pulls")]
    print("[push] PR request:", json.dumps(created[-1]["body"]) if created else None)
    print("[push] inbox now:", rows(s), "| sections:", s.js("return [...document.querySelectorAll('.inbox h3')].map(h => h.textContent.trim())"))
    s.wait(".item header .meta", timeout=20, text="from issue #5")
    print("[push] header:", s.js("return document.querySelector('.item header').innerText.replace(/\\n+/g, ' | ')"))

    # Merged on GitHub: the next poll cleans up on its own.
    set_state(100, "MERGED")
    wait_gone(s, 100)
    time.sleep(1)
    branch_left = subprocess.run(["git", "-C", clone, "show-ref", "--verify", "--quiet", f"refs/heads/{branch}"]).returncode == 0
    print("[merged] worktree removed:", not os.path.exists(wt), "| branch deleted:", not branch_left,
          "| agent processes left:", subprocess.run(["pgrep", "-f", "proj_issue5"], capture_output=True, text=True).stdout.split())

    # --- an adopted worktree: kept when its PR is merged ---
    select_row(s, 42)
    s.wait(".overview h2", timeout=30, text="Feature x")
    header_button(s, "Start working")
    s.wait(".notice", timeout=60)
    print("[adopt]", s.js("return document.querySelector('.item .notice').textContent"))
    set_state(42, "MERGED")
    wait_gone(s, 42)
    print("[merged] adopted worktree kept:", os.path.isdir(mine), "| its branch kept:",
          subprocess.run(["git", "-C", clone, "show-ref", "--verify", "--quiet", "refs/heads/pr-42"]).returncode == 0)
finally:
    s.quit()
