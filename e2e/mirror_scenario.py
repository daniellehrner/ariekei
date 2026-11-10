# Copyright contributors to Ariekei.
# SPDX-License-Identifier: Apache-2.0

"""E2E: a private mirror, another GitHub repo in the same clone (e.g. a private copy for
security fixes), against a fake GitHub.

GitHub is e2e/fake_gh.py (ARIEKEI_GH) and the agent e2e/fake_claude.py (ARIEKEI_CLAUDE); the
"GitHub" repos are local git repos reached through insteadOf. Covers: adding owner/proj-sec as
a private mirror from the repo menu (a public one is refused; its remote and the pre-push guard
are added), its assigned issue in the Inbox tagged, a task made in it from New task (its
branch pushes to the mirror, not to the fork remote.pushDefault names; pushing it elsewhere is
refused), its agent opening an issue (gh goes to the mirror: GH_REPO) and being refused one on
the public repo, a task of the repo moved to the mirror, and removing the mirror.

Run it with `python3 e2e/run.py <test-app> mirror` (see run.py), which sets the environment.
"""

import json
import os
import subprocess
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from check import check, contains, equal, eventually  # noqa: E402
from ui import open_inbox, wait_rows  # noqa: E402
from wd import Session  # noqa: E402

app, work = sys.argv[1:3]
data_dir = os.path.join(work, "data")
gh_dir = os.path.join(work, "gh")
os.makedirs(gh_dir, exist_ok=True)
ENTER = "\r"


def git(cwd, *args, check=True):
    return subprocess.run(["git", "-C", cwd, *args], check=check, capture_output=True, text=True)


def out(cwd, *args):
    return git(cwd, *args).stdout.strip()


def commit(cwd, msg):
    git(cwd, "add", "-A")
    git(cwd, "-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false", "commit", "-qm", msg)


upstream, clone = os.path.join(work, "github-proj"), os.path.join(work, "proj")
subprocess.run(["git", "init", "-q", "-b", "main", upstream], check=True)
open(os.path.join(upstream, "a.txt"), "w").write("one\n")
commit(upstream, "init")
git(upstream, "config", "receive.denyCurrentBranch", "ignore")
subprocess.run(["git", "clone", "-q", "-c", "core.autocrlf=false", upstream, clone], check=True)
git(clone, "config", f"url.{upstream}.insteadOf", "git@github.com:owner/proj.git")
git(clone, "remote", "set-url", "origin", "git@github.com:owner/proj.git")
# "GitHub's" private copy, owner/proj-sec, and the user's public fork me/proj, where the clone
# pushes by default (remote.pushDefault), as many clones are set up.
bare = {}
for slug in ("owner/proj-sec", "me/proj"):
    bare[slug] = os.path.join(work, "github-" + slug.replace("/", "-") + ".git")
    subprocess.run(["git", "clone", "-q", "--bare", upstream, bare[slug]], check=True)
    git(clone, "config", f"url.{bare[slug]}.insteadOf", f"git@github.com:{slug}.git")
git(clone, "remote", "add", "me", "git@github.com:me/proj.git")
git(clone, "config", "remote.pushDefault", "me")

data_file = os.path.join(gh_dir, "data.json")
json.dump({"viewer": "me", "upstream": upstream, "private": ["proj-sec"], "prs": [], "issues": [
    {"number": 7, "title": "Crash on a long name", "author": "alice", "assigned": True},
    {"number": 7, "title": "Leak in the parser", "author": "alice", "assigned": True, "repo": "proj-sec"},
]}, open(data_file, "w"))


def on_github(slug, branch):
    return git(bare[slug], "rev-parse", "--verify", "--quiet", f"refs/heads/{branch}", check=False).returncode == 0


def rows(s):
    """The Inbox rows: (number, title, mirror tag)."""
    open_inbox(s)
    return s.js("return [...document.querySelectorAll('.inbox .row')].map(r => "
                "[Number(r.dataset.number), r.title, r.querySelector('[data-mirror-tag]')?.dataset.mirrorTag ?? null])")


def agent(s, number):
    open_inbox(s)
    return s.js("return document.querySelector(`.inbox .row[data-number='${arguments[0]}'] [data-agent]`)?.dataset.agent ?? null", number)


def keys(s, text):
    s.wait(".terminal:not(.hidden) .xterm")
    s.js("Object.values(window.__ariekeiTerminals).find(t => t.element?.isConnected && t.element.offsetParent)?.input(arguments[0])", text)


def ask(s, number, text):
    """Sends the shown item's agent a prompt and waits for its turn to end."""
    keys(s, text)
    keys(s, ENTER)
    eventually(f"the agent finishes {text!r}", lambda: agent(s, number), lambda a: a in ("finished", "error"), timeout=60)


def menu(s):
    s.click(s.wait("[data-repo-menu]"))
    return s.wait(".menu[role=menu]")


def add_mirror(s, name):
    menu(s)
    s.click(s.wait("[data-add-mirror]"))
    field = s.wait("[data-mirror-name]")
    s.js("const f = document.querySelector('[data-mirror-name]'); f.value = ''; f.dispatchEvent(new Event('input'))")
    s.type(field, name)
    s.click(s.wait("[data-mirror-form] button[type=submit]"))


s = Session(app)
try:
    s.type(s.wait(".connect form input"), clone)
    s.click(s.wait(".connect form button"))
    wait_rows(s)
    eventually("the repo's issue is listed", lambda: rows(s), lambda r: [7, "Crash on a long name", None] in r, timeout=30)

    # --- adding the mirror: not a public repo ---
    add_mirror(s, "owner/proj")
    refused = s.text(s.wait(".menu .error", timeout=30))
    print("[add] owner/proj:", refused)
    contains("the repo itself is refused", refused, "is this repository itself")
    s.js("document.querySelector('[data-mirror-form] button[type=button]').click()")
    s.js("document.querySelector('[data-repo-menu]').click()")
    add_mirror(s, "https://github.com/owner/proj-sec")
    s.wait("[data-mirror]", timeout=30)
    listed = s.js("return [...document.querySelectorAll('.menu [data-mirror]')].map(m => m.innerText.replace(/\\s+/g, ' ').trim())")
    print("[add] the menu lists:", listed)
    contains("the menu lists the mirror", " ".join(listed), "owner/proj-sec private mirror")
    s.js("document.querySelector('[data-repo-menu]').click()")
    equal("its remote is added", out(clone, "config", "--get", "remote.proj-sec.url"), "git@github.com:owner/proj-sec.git")
    hook = os.path.join(clone, ".git", "hooks", "pre-push")
    check("the pre-push guard is installed", os.path.exists(hook) and "ariekei-mirror-guard" in open(hook).read())

    # --- its issue is listed with the repo's, tagged ---
    s.click(s.wait("[data-refresh]"))
    found = eventually("the mirror's issue is listed, tagged", lambda: rows(s),
                       lambda r: [7, "Leak in the parser", "proj-sec"] in r, timeout=30)
    print("[inbox]", found)
    check("the repo's #7 stays untagged", [7, "Crash on a long name", None] in found, found)

    # --- a task of the mirror: its work goes only there ---
    s.click(s.wait("[data-new-task]"))
    mirror_id = s.js("return document.querySelector('[data-task-repo] button:not([aria-checked=true])').dataset.value")
    s.click(s.wait(f"[data-task-repo] [data-value='{mirror_id}']"))
    s.type(s.wait("[data-task-form] input:not([data-task-branch])"), "Seal the leak")
    s.click(s.wait("[data-task-form] button[type=submit]"))
    s.wait(".terminal", timeout=120)
    meta = s.text(s.wait(".item [data-mirror-meta]", timeout=30))
    wt = os.path.join(work, "proj_seal-the-leak")
    branch = out(wt, "rev-parse", "--abbrev-ref", "HEAD")
    print("[task] meta:", meta, "| branch:", branch, "| pushRemote:", out(clone, "config", "--get", f"branch.{branch}.pushRemote"))
    contains("the task says it is the mirror's", meta, "private mirror proj-sec")
    equal("its branch pushes to the mirror", out(clone, "config", "--get", f"branch.{branch}.pushRemote"), "proj-sec")
    open(os.path.join(wt, "fix.txt"), "w").write("sealed\n")
    commit(wt, "Seal the leak")
    git(wt, "push", "-q")
    check("a plain git push lands on the mirror, not the fork", on_github("owner/proj-sec", branch) and not on_github("me/proj", branch))
    elsewhere = git(wt, "push", "me", branch, check=False)
    print("[task] push to the fork:", elsewhere.stderr.strip().splitlines()[:1])
    check("pushing it to the fork is refused", elsewhere.returncode != 0 and not on_github("me/proj", branch), elsewhere.stderr)
    contains("by the guard", elsewhere.stderr, "belongs to the private owner/proj-sec")
    context = os.path.join(data_dir, "context", "owner-proj-sec-task1.md")
    contains("its agent is told to keep it there", open(context, encoding="utf-8").read(), "## Private repository")

    # Its agent's gh goes to the mirror; one aimed at the public repo is refused.
    task_number = -1
    eventually("the task's agent is ready", lambda: agent(s, task_number), lambda a: a in ("ready", "finished"), timeout=60)
    ask(s, task_number, "Open an issue titled 'Parser leak details'")
    data = eventually("the issue is opened on the mirror", lambda: json.load(open(data_file))["issues"],
                      lambda issues: any(i["title"] == "Parser leak details" for i in issues), timeout=30)
    opened = next(i for i in data if i["title"] == "Parser leak details")
    print("[agent] opened:", opened)
    equal("on the mirror (GH_REPO)", opened.get("repo"), "proj-sec")
    eventually("it is listed, tagged", lambda: rows(s), lambda r: [opened["number"], "Parser leak details", "proj-sec"] in r, timeout=30)
    keys(s, "Open an issue titled 'Leak, publicly' on owner/proj")
    keys(s, ENTER)
    told = eventually("the agent is refused an issue on the public repo", lambda: s.js(
        "return Object.values(window.__ariekeiTerminals).map(t => { const b = t.buffer.active; "
        "return [...Array(b.length).keys()].map(i => b.getLine(i)?.translateToString(true)).join('\\n') }).join('\\n')"),
        lambda t: "Refused by Ariekei" in t, timeout=30)
    print("[agent] refused:", [l.strip() for l in told.splitlines() if "Refused by Ariekei" in l][:1])
    eventually("its turn ends", lambda: agent(s, task_number), lambda a: a in ("finished", "error"), timeout=30)
    check("and none is opened", not any(i["title"] == "Leak, publicly" for i in json.load(open(data_file))["issues"]))

    # --- a task of the repo, moved to the mirror ---
    s.click(s.wait("[data-new-task]"))
    s.click(s.wait("[data-task-repo] [aria-checked=false]"))  # back to the repo itself
    s.type(s.wait("[data-task-form] input:not([data-task-branch])"), "Look into it")
    s.click(s.wait("[data-task-form] button[type=submit]"))
    s.wait(".item [data-move-task]", timeout=120)
    check("a task of the repo is not tagged", not s.find(".item [data-mirror-meta]"))
    s.click(s.wait(".item [data-move-task]"))
    s.click(s.wait("[data-confirm-move] button", text="Move"))
    s.wait(".item [data-mirror-meta]", timeout=30)
    moved = out(os.path.join(work, "proj_look-into-it"), "rev-parse", "--abbrev-ref", "HEAD")
    print("[move] pushRemote:", out(clone, "config", "--get", f"branch.{moved}.pushRemote"))
    equal("the moved task's branch pushes to the mirror", out(clone, "config", "--get", f"branch.{moved}.pushRemote"), "proj-sec")
    eventually("it is listed tagged", lambda: rows(s), lambda r: [-2, "Look into it", "proj-sec"] in r, timeout=30)

    # --- removing the mirror: its items go, the repo's stay ---
    menu(s)
    s.js("document.querySelector('[data-remove-mirror]').click()")
    s.click(s.wait(".menu .removing button", text="Remove"))
    s.wait_js("return !document.querySelector('.menu [data-mirror]')", timeout=30)
    s.js("document.querySelector('[data-repo-menu]').click()")
    left = eventually("only the repo's items are left", lambda: rows(s), lambda r: all(tag is None for _, _, tag in r), timeout=30)
    print("[remove] inbox:", left)
    check("the repo's issue stays", [7, "Crash on a long name", None] in left, left)
finally:
    s.quit()
