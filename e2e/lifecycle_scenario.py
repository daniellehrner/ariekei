"""E2E: a work item's life, from an issue to cleanup, against a fake GitHub.

GitHub is e2e/fake_gh.py (ARIEKEI_GH); the "GitHub" repo is a local git repo reached
through insteadOf. Covers: an assigned issue in the Inbox, Start working (branch
issue-<N>-<slug> from main, push set up), Done refused while a commit is unpushed and while
a file is uncommitted, git push → draft PR opened ("Closes #N") and the item becomes the PR,
the PR merged → automatic cleanup (worktree, branch, agent, item); and an adopted worktree
that is kept when its PR is merged. Also: the rate limit used up (the Inbox says so; a
refresh recovers), a tracked issue deleted on GitHub (polling goes on), CI going from
running to failing without the PR changing, automatic cleanup kept back while a merged PR's
worktree has uncommitted work, and an issue whose branch already has a PR (no draft opened).
Commits are made with git directly (no agent turn).

Run it with `python3 e2e/run.py <test-app> lifecycle` (see run.py), which sets the environment.
"""

import json
import os
import shutil
import subprocess
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from check import check, contains, equal, eventually  # noqa: E402
from ui import open_inbox, select_row  # noqa: E402
from wd import Session  # noqa: E402

app, work = sys.argv[1:3]
gh_dir = os.path.join(work, "gh")
os.makedirs(gh_dir, exist_ok=True)
GITHUB = "git@github.com:owner/proj.git"


def git(cwd, *args):
    return subprocess.run(["git", "-C", cwd, *args], check=True, capture_output=True, text=True).stdout.strip()


def commit(cwd, msg, who="t"):
    """A commit by `who`, whose name the fake GitHub takes as their login."""
    git(cwd, "add", "-A")
    git(cwd, "-c", f"user.name={who}", "-c", f"user.email={who}@t", "-c", "commit.gpgsign=false", "commit", "-qm", msg)


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
    "issues": [{"number": 5, "title": "Crash on start", "author": "carol", "body": "It crashes when the config is empty.", "assigned": True},
               {"number": 6, "title": "Old idea", "author": "carol", "assigned": True},
               {"number": 7, "title": "Typo in docs", "author": "carol", "assigned": True}],
}, open(data_file, "w"))


def set_state(number, state):
    set_pr(number, state=state)


def set_pr(number, **fields):
    data = json.load(open(data_file))
    for p in data["prs"]:
        if p["number"] == number:
            p.update(fields)
    json.dump(data, open(data_file, "w"))


def attention(s, number):
    open_inbox(s)
    return s.js("return document.querySelector(`.inbox .row[data-number='${arguments[0]}'] .attention`)?.textContent ?? ''", number)


def refresh(s):
    s.js("document.querySelector(\".inbox header button[aria-label='Refresh from GitHub']\").click()")


def poll_error(s):
    return s.js("return document.querySelector('.poll-error')?.textContent ?? ''")


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
    equal("Inbox sections", s.js("return [...document.querySelectorAll('.inbox .section-head')].map(h => h.textContent.trim().replace(/\\s+/g, ' '))"), ["Assigned 3", "My PRs 1", "Open 0"])

    # --- GitHub's rate limit used up: the Inbox says so; a refresh after the reset recovers ---
    limit_flag = os.path.join(gh_dir, "rate-limited")
    open(limit_flag, "w").close()
    refresh(s)
    message = eventually("the rate limit message", lambda: poll_error(s), lambda v: "rate limit" in v)
    print("[rate limit]", " ".join(message.split()))
    contains("the rate limit message says when polling resumes", message, "polls again in 2 min")
    os.remove(limit_flag)
    refresh(s)
    eventually("the message goes once GitHub answers again", lambda: poll_error(s), lambda v: v == "")

    # --- a tracked issue disappears from GitHub (deleted, transferred): polling goes on ---
    select_row(s, 6)
    s.wait(".item header .title", timeout=30, text="Old idea")
    data = json.load(open(data_file))
    data["issues"] = [i for i in data["issues"] if i["number"] != 6]
    json.dump(data, open(data_file, "w"))
    set_pr(42, title="Feature x, renamed")
    eventually("other items stay current", lambda: s.js("return document.querySelector(\".inbox .row[data-number='42']\")?.title ?? ''"),
               lambda v: v == "Feature x, renamed", timeout=30)
    print("[deleted issue] poll error:", repr(poll_error(s)), "| rows:", rows(s))
    check("a deleted issue does not break polling", poll_error(s) == "" and "6" not in rows(s), poll_error(s))

    # --- CI: a push starts it (running), then it fails; the PR itself does not change ---
    git(upstream, "checkout", "-q", "feature/x")
    open(os.path.join(upstream, "x.txt"), "a").write("more\n")
    commit(upstream, "more x")
    git(upstream, "checkout", "-q", "main")
    set_pr(42, ci="PENDING")
    eventually("CI shows as running", lambda: s.js("return !!document.querySelector(\".inbox .row[data-number='42'] .ci.warn\")"), lambda v: v, timeout=30)
    set_pr(42, ci="FAILURE", failing="unit-tests")
    eventually("CI failing shows without the PR changing", lambda: attention(s, 42), lambda v: v == "CI failing", timeout=30)
    print("[ci] #42:", attention(s, 42))
    set_pr(42, ci="SUCCESS", failing=None)

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

    # The fix (made directly with git, as the user "me"), not pushed yet: Done must refuse.
    open(os.path.join(wt, "a.txt"), "a").write("fixed\n")
    commit(wt, "Fix the crash on start", who="me")
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

    # Another item is shown while the PR is merged.
    select_row(s, 42)
    s.wait(".item header .title", timeout=30, text="#42")

    # Merged on GitHub while the worktree has uncommitted work: kept, and the Inbox says why.
    open(os.path.join(wt, "scratch.txt"), "w").write("not committed\n")
    set_state(100, "MERGED")
    eventually("a merged PR with uncommitted work is kept", lambda: attention(s, 100), lambda v: v == "Cleanup blocked", timeout=30)
    print("[merged, blocked]", attention(s, 100), "| worktree kept:", os.path.isdir(wt))
    check("cleanup never loses uncommitted work", os.path.isdir(wt) and os.path.exists(os.path.join(wt, "scratch.txt")))
    # Once the work is gone, the next poll cleans up on its own.
    os.remove(os.path.join(wt, "scratch.txt"))
    wait_gone(s, 100)
    time.sleep(1)
    branch_left = subprocess.run(["git", "-C", clone, "show-ref", "--verify", "--quiet", f"refs/heads/{branch}"]).returncode == 0
    print("[merged] worktree removed:", not os.path.exists(wt), "| branch deleted:", not branch_left,
          "| agent processes left:", subprocess.run(["pgrep", "-f", "proj_issue5"], capture_output=True, text=True).stdout.split() if shutil.which("pgrep") else "n/a")
    check("merging cleans up the worktree and the branch", not os.path.exists(wt) and not branch_left)

    # --- an issue whose branch already has a PR (opened with gh pr create): no draft ---
    select_row(s, 7)
    s.wait(".item header .title", timeout=30, text="Typo in docs")
    header_button(s, "Start working")
    s.wait(".terminal", timeout=120)
    wt7 = os.path.join(work, "proj_issue7")
    branch7 = git(wt7, "rev-parse", "--abbrev-ref", "HEAD")
    data = json.load(open(data_file))
    data["prs"].append({"number": 130, "title": "Fix the typo", "author": "me", "head_ref": branch7, "base_ref": "main", "state": "OPEN"})
    json.dump(data, open(data_file, "w"))
    open(os.path.join(wt7, "a.txt"), "a").write("typo fixed\n")
    commit(wt7, "Fix the typo", who="me")
    posts_before = len([w for w in writes() if w.get("post", "").endswith("/pulls")])
    git(wt7, "push", "-q")
    s.wait(".item header .meta", timeout=30, text="from issue #7")
    listed = rows(s)
    posts = len([w for w in writes() if w.get("post", "").endswith("/pulls")])
    print("[existing PR] header:", s.js("return document.querySelector('.item header .title').textContent"), "| rows:", listed, "| new PR requests:", posts - posts_before)
    contains("the issue became the existing PR", s.js("return document.querySelector('.item header .title').textContent"), "#130")
    check("no draft PR is opened when the branch has one", posts == posts_before)
    equal("the PR is listed once", listed.count("130"), 1)

    # --- an adopted worktree: kept when its PR is merged ---
    select_row(s, 42)
    s.wait(".item header .title", timeout=30, text="Feature x")
    header_button(s, "Start working")
    s.wait(".item [data-notice]", timeout=60)
    print("[adopt]", s.js("return document.querySelector('.item [data-notice]').textContent"))
    contains("an existing worktree is adopted", s.js("return document.querySelector('.item [data-notice]').textContent"), "existing worktree")
    set_state(42, "MERGED")
    wait_gone(s, 42)
    print("[merged] adopted worktree kept:", os.path.isdir(mine), "| its branch kept:",
          subprocess.run(["git", "-C", clone, "show-ref", "--verify", "--quiet", "refs/heads/pr-42"]).returncode == 0)
    check("an adopted worktree is kept", os.path.isdir(mine))
finally:
    s.quit()
