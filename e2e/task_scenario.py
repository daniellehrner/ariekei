"""E2E: a task, work that starts with no PR or issue, against a fake GitHub.

GitHub is e2e/fake_gh.py (ARIEKEI_GH) and the agent e2e/fake_claude.py (ARIEKEI_CLAUDE); the
"GitHub" repo is a local git repo reached through insteadOf. Covers: New task (Ctrl+N;
worktree on task-<n>-<slug> from main, the notes in the agent's context, the agent starts), the agent
opening a PR and an issue (`gh pr create` / `gh issue create`) which are linked to the task
and listed under it rather than in their sections, a linked PR pointing back to the task and
sending its review threads to the task's agent, asking the agent about code from the review
(right-click a line or an old-side selection; refused while it waits for permission), Ctrl+B
and ? in the terminal staying with the agent, Discard refused while the agent waits for
permission, a closed issue staying under its task, Discard (shows the work it loses, then
removes the worktree, branch and task; the PR goes back to Mine), and a second task (the +
button) whose merged PR stays under it ("All closed") until Done removes it.

Run it with `python3 e2e/run.py <test-app> task` (see run.py), which sets the environment.
"""

import json
import os
import subprocess
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from check import check, contains, equal, eventually  # noqa: E402
from ui import open_inbox, side_by_side, wait_rows  # noqa: E402
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


def set_pr(number, **fields):
    data = json.load(open(data_file))
    for p in data["prs"]:
        if p["number"] == number:
            p.update(fields)
    json.dump(data, open(data_file, "w"))


def transcripts():
    """Everything the fake agents were told (their transcripts)."""
    folder = os.path.join(work, "claude")
    return "".join(open(os.path.join(folder, f), encoding="utf-8").read() for f in os.listdir(folder) if f.endswith(".txt"))


def tab(s, name):
    s.js("[...document.querySelectorAll('.item [role=tab]')].find(t => t.textContent.trim() === arguments[0]).click()", name)


def editor(side):
    """JS for the CodeMirror view of one side of the side-by-side diff."""
    i = 0 if side == "old" else 1
    return (f"(() => {{ const el = document.querySelectorAll('.cm-mergeViewEditor .cm-content')[{i}];"
            " return el && (el.cmView?.rootView?.view ?? el.cmTile?.root?.view ?? el.cmView?.view); })()")


def right_click(s, side, line, selected=None):
    """Right-clicks line `line` of a side of the diff, after selecting lines `selected` (a, b)."""
    s.js(f"""const view = {editor(side)}, doc = view.state.doc, sel = arguments[1];
      if (sel) view.dispatch({{ selection: {{ anchor: doc.line(sel[0]).from, head: doc.line(sel[1]).to }} }});
      const c = view.coordsAtPos(doc.line(arguments[0]).from), x = c.left + 2, y = (c.top + c.bottom) / 2;
      document.elementFromPoint(x, y).dispatchEvent(new MouseEvent('contextmenu', {{ bubbles: true, cancelable: true, clientX: x, clientY: y }}));""",
         line, selected)


def ask_about(s, side, line, prompt, selected=None, enter=False):
    """Right-click → Ask agent… → the prompt, sent with Send (or Enter)."""
    right_click(s, side, line, selected)
    s.click(s.wait("[data-code-menu] [data-ask-agent]", timeout=10))

    s.type(s.wait("[data-ask] textarea", timeout=10), prompt)
    if enter:
        s.js("document.querySelector('[data-ask] textarea').dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }))")
    else:
        s.click(s.wait("[data-ask-send]"))


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
    # A review thread on the PR goes to the task's agent (the PR has no agent of its own).
    set_pr(100, threads=[{"id": "T1", "path": "a.txt", "line": 2, "comment_id": 7, "author": "bob",
                          "body": "Handle a peer that reconnects mid-reset.", "html": "<p>Handle a peer that reconnects mid-reset.</p>"}])
    s.js("[...document.querySelectorAll('.item .tabs button')].find(b => b.textContent.includes('Send PR feedback')).click()")
    s.wait(".item [data-notice]", timeout=30, text="Sent the PR's review threads")
    told = eventually("the task's agent gets the PR feedback", transcripts, lambda t: "Handle a peer that reconnects mid-reset." in t, timeout=30)
    print("[feedback] the task's agent was told:", [l for l in told.splitlines() if "PR #100" in l][-1][:160])
    contains("the feedback names the PR", told, "review feedback and CI state of PR #100")
    eventually("the agent's turn ends", lambda: agent(s, -1), lambda a: a == "finished", timeout=30)
    s.click(s.wait(".item [data-from-task]"))
    s.wait(".item header .title", timeout=10, text="Peers drop after restart")

    # --- Ask agent from the review: right-click a line, or a selection on the old side ---
    s.click(s.wait(".item [data-tab='review']"))
    side_by_side(s)
    s.wait_js(f"return !!{editor('new')}?.state.doc.toString().includes('reset peers once')", timeout=30)
    ask_about(s, "new", 2, "Why only once here?")
    note = s.text(s.wait(".review [data-notice]", timeout=30, text="and your prompt"))
    told = eventually("the agent gets the code and the prompt", transcripts, lambda t: "Why only once here?" in t, timeout=30)
    asked = [line for line in told.splitlines() if "Why only once here?" in line][-1]
    print("[ask] note:", note.strip(), "| the agent was told:", asked[:200])
    contains("the note says what was sent", note, "Sent a.txt:2 and your prompt to the agent.")
    check("the agent is told the file, line, version and code",
          "About a.txt line 2 (" in asked and "reset peers once" in asked and asked.rstrip().endswith("Why only once here?"), asked)
    check("the note offers the terminal", bool(s.find(".review [data-open-terminal]")))
    eventually("the agent's turn ends", lambda: agent(s, -1), lambda a: a == "finished", timeout=30)
    ask_about(s, "old", 1, "Where did this line come from?", selected=(1, 1), enter=True)
    told = eventually("the agent gets the old side's code", transcripts, lambda t: "Where did this line come from?" in t, timeout=30)
    asked = [line for line in told.splitlines() if "Where did this line come from?" in line][-1]
    print("[ask] old side:", asked[:200])
    check("the old side is named as the version before the change",
          "About a.txt line 1 (old version, before the change, commit " in asked and "```txt one ```" in asked, asked)
    eventually("the agent's turn ends", lambda: agent(s, -1), lambda a: a == "finished", timeout=30)

    # In the agent's terminal, the app's shortcuts are Claude Code's keys (Ctrl+B is its
    # "background" key): neither the Inbox nor the shortcuts overlay react.
    tab(s, "Terminal")
    for key, ctrl in (("b", True), ("?", False)):
        s.js("const t = document.querySelector('.terminal:not(.hidden) .xterm textarea'); t.focus();"
             "t.dispatchEvent(new KeyboardEvent('keydown', { key: arguments[0], ctrlKey: arguments[1], bubbles: true }))", key, ctrl)
    time.sleep(0.3)
    print("[terminal keys] inbox collapsed:", bool(s.find(".inbox.collapsed")), "| shortcuts shown:", bool(s.find("[data-shortcuts]")))
    check("Ctrl+B and ? in the terminal stay with the agent", not s.find(".inbox.collapsed") and not s.find("[data-shortcuts]"))

    # Discard is refused while the agent waits for an answer (it is in the middle of a turn).
    keys(s, "ask before running the tests")
    keys(s, ENTER)
    eventually("the agent asks for permission", lambda: agent(s, -1), lambda a: a == "waitingForPermission", timeout=30)
    s.click(s.wait("[data-discard]"))
    s.click(s.wait("[data-confirm-discard] button", text="Discard"))
    refused = s.text(s.wait(".item [data-error]", timeout=30))
    print("[discard] while the agent waits:", refused.strip()[:120], "| worktree kept:", os.path.isdir(wt))
    check("Discard waits for the agent's turn", "middle of a turn" in refused and os.path.isdir(wt), refused)
    s.js("document.querySelector('.item [data-error] button')?.click()")
    # Nor can it be asked about code: the paste would answer its permission prompt.
    s.click(s.wait(".item [data-tab='review']"))
    s.wait_js(f"return !!{editor('new')}", timeout=30)
    right_click(s, "new", 2)
    s.click(s.wait("[data-code-menu] [data-ask-agent]", timeout=10))
    blocked = s.text(s.wait("[data-ask-blocked]", timeout=10))
    print("[ask] while the agent waits:", blocked.strip())
    check("Send waits for the permission answer", s.js("return document.querySelector('[data-ask-send]').disabled"), blocked)
    s.js("document.querySelector('[data-ask] textarea').dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))")
    s.wait_js("return !document.querySelector('[data-ask]')", timeout=10)
    tab(s, "Terminal")
    keys(s, "never mind")
    keys(s, ENTER)
    eventually("the agent's turn ends", lambda: agent(s, -1), lambda a: a == "finished", timeout=30)

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

    # --- a second task (the + button), whose PR is merged, then Done ---
    s.click(s.wait("[data-new-task]"))
    s.type(s.wait("[data-task-form] input"), "Slow start")
    s.click(s.wait("[data-task-form] button[type=submit]"))
    s.wait(".item header .title", timeout=120, text="Slow start")
    wt2 = os.path.join(work, "proj_task1")
    eventually("the second task's worktree", lambda: os.path.isdir(wt2), lambda v: v, timeout=60)
    branch2 = git(wt2, "rev-parse", "--abbrev-ref", "HEAD")
    eventually("its agent is ready", lambda: agent(s, -1), lambda a: a in ("ready", "finished"), timeout=60)
    open(os.path.join(wt2, "start.txt"), "w").write("faster\n")
    commit(wt2, "Start faster")
    ask(s, "Open a pull request titled 'Start faster'")
    eventually("its PR appears under it", lambda: listed(s), lambda rows: [102, True] in rows, timeout=30)
    # Merged: the PR is cleaned up (it has no worktree) but stays under its task, the task
    # keeps its worktree and agent, and says that all it opened is closed.
    set_pr(102, state="MERGED")
    attention = lambda: s.js("return document.querySelector(\".inbox .row[data-number='-1'] .attention\")?.textContent ?? ''")
    eventually("the task says all its PRs and issues are closed", attention, lambda v: v == "All closed", timeout=30)
    print("[merged] inbox:", listed(s), "| task:", attention(), "| worktree kept:", os.path.isdir(wt2),
          "| notice:", s.js("return [...document.querySelectorAll('[data-notice]')].map(n => n.textContent.trim())"))
    check("a merged PR stays under its task, which keeps its worktree", [102, True] in listed(s) and os.path.isdir(wt2), listed(s))
    s.click(s.wait(".item header .actions button", text="Done"))
    s.click(s.wait("[data-confirm-done] button", text="Clean up"))
    eventually("Done removes the task", lambda: [n for n, _ in listed(s)], lambda rows: -1 not in rows and 102 not in rows, timeout=30)
    branch_left = subprocess.run(["git", "-C", clone, "show-ref", "--verify", "--quiet", f"refs/heads/{branch2}"]).returncode == 0
    print("[done] worktree removed:", not os.path.exists(wt2), "| branch deleted:", not branch_left, "| inbox:", sections(s), listed(s))
    check("Done removes the task's worktree and branch", not os.path.exists(wt2) and not branch_left)
    equal("what is left", sections(s), ["Mine 1", "Opened 0"])
finally:
    s.quit()
