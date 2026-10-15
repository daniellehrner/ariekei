"""E2E: a task, work that starts with no PR or issue, against a fake GitHub.

GitHub is e2e/fake_gh.py (ARIEKEI_GH) and the agent e2e/fake_claude.py (ARIEKEI_CLAUDE); the
"GitHub" repo is a local git repo reached through insteadOf. Covers: New task (Ctrl+N;
worktree on task-<n>-<slug> from main, the notes in the agent's context, the agent starts), the agent
opening a PR and an issue (`gh pr create` / `gh issue create`) which are linked to the task
and listed under it rather than in their sections, a linked PR pointing back to the task,
a closed issue staying under its task, and Discard (shows the work it loses, then removes
the worktree, branch and task; the PR goes back to Mine).

Run it with `python3 e2e/run.py <test-app> task` (see run.py), which sets the environment.
"""

import json
import os
import subprocess
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from check import check, contains, equal, eventually  # noqa: E402
from ui import open_inbox, wait_rows  # noqa: E402
from wd import Session  # noqa: E402

app, work = sys.argv[1:3]
data_dir = os.path.join(work, "data")
gh_dir = os.path.join(work, "gh")
os.makedirs(gh_dir, exist_ok=True)
GITHUB = "git@github.com:owner/proj.git"
ENTER = "\r"


def git(cwd, *args):
    return subprocess.run(["git", "-C", cwd, *args], check=True, capture_output=True, text=True).stdout.strip()


def commit(cwd, msg):
    git(cwd, "add", "-A")
    git(cwd, "-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false", "commit", "-qm", msg)


upstream, clone = os.path.join(work, "github-proj"), os.path.join(work, "proj")
subprocess.run(["git", "init", "-q", "-b", "main", upstream], check=True)
open(os.path.join(upstream, "a.txt"), "w").write("one\n")
commit(upstream, "init")
git(upstream, "config", "receive.denyCurrentBranch", "ignore")
subprocess.run(["git", "clone", "-q", "-c", "core.autocrlf=false", upstream, clone], check=True)
git(clone, "config", f"url.{upstream}.insteadOf", GITHUB)
git(clone, "remote", "set-url", "origin", GITHUB)

data_file = os.path.join(gh_dir, "data.json")
json.dump({"viewer": "me", "upstream": upstream, "prs": [], "issues": []}, open(data_file, "w"))


def set_issue_state(number, state):
    data = json.load(open(data_file))
    for i in data["issues"]:
        if i["number"] == number:
            i["state"] = state
    json.dump(data, open(data_file, "w"))


def sections(s):
    open_inbox(s)
    return s.js("return [...document.querySelectorAll('.inbox .section-head')].map(h => h.textContent.trim().replace(/\\s+/g, ' '))")


def listed(s):
    """The Inbox rows as (number, nested under a task)."""
    open_inbox(s)
    return s.js("return [...document.querySelectorAll('.inbox .row')].map(r => [Number(r.dataset.number), !!r.closest('li.nested')])")


def agent(s, number):
    open_inbox(s)
    return s.js("return document.querySelector(`.inbox .row[data-number='${arguments[0]}'] [data-agent]`)?.dataset.agent ?? null", number)


def keys(s, text):
    """Types into the visible terminal, as the keyboard would."""
    s.wait(".terminal:not(.hidden) .xterm")
    s.js("Object.values(window.__ariekeiTerminals).find(t => t.element?.isConnected && t.element.offsetParent)?.input(arguments[0])", text)


def ask(s, text):
    """Sends the task's agent a prompt and waits for its turn to end."""
    keys(s, text)
    keys(s, ENTER)
    eventually(f"the agent finishes {text!r}", lambda: agent(s, -1), lambda a: a in ("finished", "error"), timeout=60)


def select(s, number):
    wait_rows(s)
    s.wait(f".inbox .row[data-number='{number}']", timeout=60)
    s.js("document.querySelector(`.inbox .row[data-number='${arguments[0]}']`).click()", number)


s = Session(app)
try:
    s.type(s.wait(".connect form input"), clone)
    s.click(s.wait(".connect form button"))
    s.wait(".inbox [data-section='mine']", timeout=60)
    equal("Inbox sections before", sections(s), ["Mine 0", "Opened 0"])

    # --- New task ---
    # Ctrl+N, as a user would (+ in the Inbox header does the same).
    s.js("document.body.dispatchEvent(new KeyboardEvent('keydown', { key: 'n', ctrlKey: true, bubbles: true }))")
    s.type(s.wait("[data-task-form] input"), "Peers drop after restart")
    s.type(s.wait("[data-task-form] textarea"), "Look at how the peer table is reset.")
    s.click(s.wait("[data-task-form] button[type=submit]"))
    s.wait(".terminal", timeout=120)
    wt = os.path.join(work, "proj_task1")
    branch = git(wt, "rev-parse", "--abbrev-ref", "HEAD")
    print("[task] header:", s.js("return document.querySelector('.item header').innerText.replace(/\\n+/g, ' | ')"),
          "| branch:", branch, "| tabs:", s.js("return [...document.querySelectorAll('.item [role=tab]')].map(b => b.textContent.trim())"))
    check("New task makes a worktree on a branch from main",
          branch == "task-1-peers-drop-after-restart" and git(wt, "rev-parse", "HEAD") == git(clone, "rev-parse", "origin/main"), branch)
    contains("the task is titled", s.js("return document.querySelector('.item header .title').textContent"), "T1Peers drop after restart")
    equal("a task has no Comments tab", s.js("return [...document.querySelectorAll('.item [role=tab]')].map(b => b.textContent.trim())"),
          ["Overview", "Review", "Terminal"])
    context = os.path.join(data_dir, "context", "owner-proj-task1.md")
    eventually("the agent's context has the notes", lambda: os.path.exists(context) and open(context).read(),
               lambda md: md and "Look at how the peer table is reset." in md and "`gh pr create`" in md)
    eventually("the task's agent is ready", lambda: agent(s, -1), lambda a: a in ("ready", "finished"), timeout=60)
    print("[task] inbox:", sections(s), listed(s))
    equal("the task is listed under Tasks", sections(s), ["Tasks 1", "Mine 0", "Opened 0"])

    # --- the agent opens a PR and an issue: both are linked to the task ---
    open(os.path.join(wt, "a.txt"), "a").write("reset peers once\n")
    commit(wt, "Reset the peer table once")
    ask(s, "Open a pull request titled 'Reset the peer table once'")
    eventually("the PR appears under the task", lambda: listed(s), lambda rows: [100, True] in rows, timeout=30)
    ask(s, "Open an issue titled 'Peer table grows without bound'")
    eventually("the issue appears under the task", lambda: listed(s), lambda rows: [101, True] in rows, timeout=30)
    print("[linked] inbox:", sections(s), listed(s))
    equal("linked items are not in their own sections", sections(s), ["Tasks 1", "Mine 0", "Opened 0"])
    s.js("[...document.querySelectorAll('.item [role=tab]')].find(t => t.textContent.trim() === 'Overview').click()")
    links = eventually("the task's Overview lists them", lambda: s.js("return [...document.querySelectorAll('[data-links] [data-link]')].map(b => b.dataset.link)"),
                       lambda v: v and sorted(v) == ["100", "101"])
    print("[linked] overview:", links)

    select(s, 100)
    s.wait(".item [data-from-task]", timeout=30)
    header = s.js("return document.querySelector('.item header').innerText.replace(/\\n+/g, ' | ')")
    print("[pr] header:", header, "| start offered:", s.js("return !!document.querySelector('[data-start]')"),
          "| PR feedback to the agent:", s.js("return [...document.querySelectorAll('.item .tabs button')].some(b => b.textContent.includes('Send PR feedback'))"))
    contains("the PR points to its task", header, "from task T1 Peers drop after restart")
    check("the PR uses the task's agent (no Start working)", not s.js("return !!document.querySelector('[data-start]')"))
    s.click(s.wait(".item [data-from-task]"))
    s.wait(".item header .title", timeout=10, text="Peers drop after restart")

    # A closed issue stays listed under its task.
    set_issue_state(101, "CLOSED")
    time.sleep(8)  # two polls
    print("[closed] inbox:", listed(s))
    check("a closed issue stays under its task", [101, True] in listed(s), listed(s))

    # --- Discard: shows the work it loses first ---
    open(os.path.join(wt, "notes.txt"), "w").write("draft\n")
    s.click(s.wait("[data-discard]"))
    s.wait("[data-losses]", timeout=30)
    losses = s.js("return [...document.querySelectorAll('[data-losses] li')].map(l => l.textContent)")
    print("[discard] losses:", losses)
    check("Discard names the uncommitted change", any("notes.txt" in l for l in losses), losses)
    s.click(s.wait("[data-confirm-discard] button", text="Discard"))
    eventually("the task is gone", lambda: [n for n, _ in listed(s)], lambda rows: -1 not in rows, timeout=30)
    branch_left = subprocess.run(["git", "-C", clone, "show-ref", "--verify", "--quiet", f"refs/heads/{branch}"]).returncode == 0
    print("[discard] worktree removed:", not os.path.exists(wt), "| branch deleted:", not branch_left, "| inbox:", sections(s), listed(s))
    check("Discard removes the worktree and the branch", not os.path.exists(wt) and not branch_left)
    eventually("the PR is back in Mine", lambda: sections(s), lambda v: "Mine 1" in v and "Tasks 1" not in v)
finally:
    s.quit()
