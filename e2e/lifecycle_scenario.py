"""E2E: a work item's life, from an issue to cleanup, against a fake GitHub.

GitHub is e2e/fake_gh.py (ARIEKEI_GH); the "GitHub" repo is a local git repo reached
through insteadOf. Covers: an assigned issue in the Inbox, Start working (branch
issue-<N>-<slug> from main, push set up), Done refused while a commit is unpushed and while
a file is uncommitted, git push → draft PR opened ("Closes #N") and the item becomes the PR,
the PR merged → automatic cleanup (worktree, branch, agent, item); and an adopted worktree
that is kept when its PR is merged. Commits are made with git directly (no agent turn).

Run it with `python3 e2e/run.py <test-app> lifecycle` (see run.py), which sets the environment.
"""

import json
import os
import shutil
import subprocess
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from check import check, contains, equal  # noqa: E402
from ui import open_inbox, select_row  # noqa: E402
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
    open_inbox(s)
    return s.js("return [...document.querySelectorAll('.inbox .row')].map(r => r.dataset.number)")


def header_button(s, text):
    s.click(s.wait(".item header .actions button", text=text))


def press_done(s):
    header_button(s, "Done")
    header_button(s, "Clean up")


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
    print("[inbox]", s.js("return [...document.querySelectorAll('.inbox .section-head')].map(h => h.textContent.trim().replace(/\\s+/g, ' '))"))
    equal("Inbox sections", s.js("return [...document.querySelectorAll('.inbox .section-head')].map(h => h.textContent.trim().replace(/\\s+/g, ' '))"), ["Mine 1", "Issues 1", "Opened 0"])

    # --- the issue ---
    select_row(s, 5)
    s.wait(".item header .title", timeout=30, text="Crash on start")
    s.wait(".overview .meta", timeout=30)
    print("[issue] overview:", s.js("return document.querySelector('.overview .meta').innerText.replace(/\\n+/g, ' ')"),
          "| tabs:", s.js("return [...document.querySelectorAll('.item [role=tab]')].map(b => b.textContent.trim())"))
    header_button(s, "Start working")
    s.wait(".terminal", timeout=120)
    wt = os.path.join(work, "proj_issue5")
    branch = git(wt, "rev-parse", "--abbrev-ref", "HEAD")
    print("[start] worktree:", os.path.isdir(wt), "| branch:", branch, "| from main:", git(wt, "rev-parse", "HEAD") == git(clone, "rev-parse", "origin/main"),
          "| push to:", git(clone, "config", f"branch.{branch}.remote"))
    check("Start working makes a worktree on a branch from main", os.path.isdir(wt) and branch == "issue-5-crash-on-start" and git(wt, "rev-parse", "HEAD") == git(clone, "rev-parse", "origin/main"), branch)

    # The fix (made directly with git), not pushed yet: Done must refuse.
    open(os.path.join(wt, "a.txt"), "a").write("fixed\n")
    commit(wt, "Fix the crash on start")
    open(os.path.join(wt, "notes.txt"), "w").write("todo\n")
    press_done(s)
    s.wait("[data-blocked]", timeout=30)
    print("[done] refused:", s.js("return [...document.querySelectorAll('[data-blocked] li')].map(l => l.textContent)"),
          "| worktree still there:", os.path.isdir(wt))
    blocked = s.js("return [...document.querySelectorAll('[data-blocked] li')].map(l => l.textContent)")
    check("Done refuses while work would be lost", len(blocked) == 2 and os.path.isdir(wt), blocked)
    os.remove(os.path.join(wt, "notes.txt"))

    s.js("document.querySelector('[data-tab=review]').click()")
    s.wait(".review .marker", timeout=30)
    print("[review]", " ".join(s.js("return document.querySelector('.review .marker').textContent").split()),
          "| submit to GitHub offered:", s.js("return !!document.querySelector('.review [data-open-submit]')"))

    # Push: the next poll opens a draft PR and the item becomes that PR.
    git(wt, "push", "-q")
    deadline = time.monotonic() + 60
    while time.monotonic() < deadline and "100" not in (rows(s) or []):
        time.sleep(0.5)
    created = [w for w in writes() if w.get("post", "").endswith("/pulls")]
    print("[push] PR request:", json.dumps(created[-1]["body"]) if created else None)
    check("the first push opens a draft PR", created and created[-1]["body"]["draft"] and created[-1]["body"]["body"] == "Closes #5.", created)
    print("[push] inbox now:", rows(s), "| sections:", s.js("return [...document.querySelectorAll('.inbox .section-head')].map(h => h.textContent.trim().replace(/\\s+/g, ' '))"))
    s.wait(".item header .meta", timeout=20, text="from issue #5")
    print("[push] header:", s.js("return document.querySelector('.item header').innerText.replace(/\\n+/g, ' | ')"))
    contains("the item became the PR", s.js("return document.querySelector('.item header .title').textContent"), "#100")

    # Merged on GitHub: the next poll cleans up on its own.
    set_state(100, "MERGED")
    wait_gone(s, 100)
    time.sleep(1)
    branch_left = subprocess.run(["git", "-C", clone, "show-ref", "--verify", "--quiet", f"refs/heads/{branch}"]).returncode == 0
    print("[merged] worktree removed:", not os.path.exists(wt), "| branch deleted:", not branch_left,
          "| agent processes left:", subprocess.run(["pgrep", "-f", "proj_issue5"], capture_output=True, text=True).stdout.split() if shutil.which("pgrep") else "n/a")
    check("merging cleans up the worktree and the branch", not os.path.exists(wt) and not branch_left)

    # --- an adopted worktree: kept when its PR is merged ---
    select_row(s, 42)
    s.wait(".item header .title", timeout=30, text="Feature x")
    header_button(s, "Start working")
    s.wait("[data-notice]", timeout=60)
    print("[adopt]", s.js("return document.querySelector('.item [data-notice]').textContent"))
    contains("an existing worktree is adopted", s.js("return document.querySelector('.item [data-notice]').textContent"), "existing worktree")
    set_state(42, "MERGED")
    wait_gone(s, 42)
    print("[merged] adopted worktree kept:", os.path.isdir(mine), "| its branch kept:",
          subprocess.run(["git", "-C", clone, "show-ref", "--verify", "--quiet", "refs/heads/pr-42"]).returncode == 0)
    check("an adopted worktree is kept", os.path.isdir(mine))
finally:
    s.quit()
