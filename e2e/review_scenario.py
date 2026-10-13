"""E2E: reviewing PRs (your own and other people's) against a fake GitHub.

GitHub is replaced by e2e/fake_gh.py (ARIEKEI_GH), which serves the fixture's PRs and
records every write instead of sending it. Covers: Inbox sections (Mine / Review
requested), search and opening another author's PR, Overview (sanitized description),
Comments (resolve, reply, new comment), reviewing without a worktree ("All changes vs
main" from refs/pull/N/head), draft comments, Submit to GitHub (line comment inside the
PR diff, the other one folded into the body), and "All changes" after Mark reviewed.
No agent is started.

  FAKE_GH_DIR=<work>/gh ARIEKEI_GH=$PWD/e2e/fake_gh.py ARIEKEI_DATA_DIR=<work>/data \\
    xvfb-run -a -s "-screen 0 1400x900x24" tauri-driver --port 4444 &
  python3 -I e2e/review_scenario.py <app-binary> <work>
"""

import json
import os
import subprocess
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from wd import Session  # noqa: E402

app, work = sys.argv[1:3]
gh_dir, shots = os.path.join(work, "gh"), os.path.join(work, "shots")
os.makedirs(gh_dir, exist_ok=True)
os.makedirs(shots, exist_ok=True)
GITHUB = "git@github.com:owner/proj.git"


def git(cwd, *args):
    return subprocess.run(["git", "-C", cwd, *args], check=True, capture_output=True, text=True).stdout.strip()


def write(path, text):
    with open(path, "w") as f:
        f.write(text)


def commit(cwd, msg):
    git(cwd, "add", "-A")
    git(cwd, "-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false", "commit", "-qm", msg)


def branch(upstream, name, number, edit):
    git(upstream, "checkout", "-q", "-b", name, "main")
    edit()
    commit(upstream, f"work on {name}")
    git(upstream, "update-ref", f"refs/pull/{number}/head", name)
    sha = git(upstream, "rev-parse", name)
    git(upstream, "checkout", "-q", "main")
    return sha


# --- fixture: a "GitHub" repo with three PR branches, and a clone of it ---------------
upstream, clone = os.path.join(work, "github-proj"), os.path.join(work, "proj")
subprocess.run(["git", "init", "-q", "-b", "main", upstream], check=True)
lines = [f"line{i}" for i in range(1, 13)]
write(os.path.join(upstream, "a.txt"), "\n".join(lines) + "\n")
commit(upstream, "init")
sha42 = branch(upstream, "feature/x", 42, lambda: write(os.path.join(upstream, "a.txt"), "\n".join(lines[:1] + ["line2 mine"] + lines[2:]) + "\n"))
sha43 = branch(upstream, "feature/y", 43, lambda: (
    write(os.path.join(upstream, "a.txt"), "\n".join(lines[:9] + ["line10 changed"] + lines[10:]) + "\n"),
    write(os.path.join(upstream, "b.txt"), "new file\n"),
    write(os.path.join(upstream, "c.rs"), "fn main() {\n    let answer = 42; // highlighted\n}\n"),
))
sha44 = branch(upstream, "feature/z", 44, lambda: write(os.path.join(upstream, "c.txt"), "z\n"))
subprocess.run(["git", "clone", "-q", upstream, clone], check=True)
git(clone, "config", f"url.{upstream}.insteadOf", GITHUB)
git(clone, "remote", "set-url", "origin", GITHUB)

json.dump({"viewer": "me", "prs": [
    {"number": 42, "title": "My change", "author": "me", "head_ref": "feature/x", "head_sha": sha42, "base_ref": "main"},
    {"number": 43, "title": "Add y to the parser", "author": "alice", "head_ref": "feature/y", "head_sha": sha43, "base_ref": "main",
     "body_html": "<p>Adds <strong>y</strong>. See <a href=\"https://example.com/spec\">the spec</a>.</p>"
                  "<script>window.__pwned = 1</script><img src=\"x\" onerror=\"window.__pwned = 2\">",
     "comments": [{"author": "carol", "html": "<p>Nice idea.</p>"}],
     "threads": [{"id": "T1", "path": "a.txt", "line": 10, "comment_id": 101, "author": "bob", "html": "<p>Why this change?</p>",
                  "diff_hunk": "@@ -9,2 +9,2 @@\n line9\n-line10\n+line10 changed"}]},
    {"number": 44, "title": "Add z", "author": "dave", "head_ref": "feature/z", "head_sha": sha44, "base_ref": "main", "requested": True},
]}, open(os.path.join(gh_dir, "data.json"), "w"))
writes_file = os.path.join(gh_dir, "writes.jsonl")


def writes():
    if not os.path.exists(writes_file):
        return []
    return [json.loads(line) for line in open(writes_file)]


# --- helpers ------------------------------------------------------------------------
def select_row(s, number):
    s.wait(f".inbox .row[data-number='{number}']", timeout=60)
    s.js("document.querySelector(`.inbox .row[data-number='${arguments[0]}']`).click()", number)


def tab(s, name):
    s.click(s.wait("nav button", text=name))


def select_lines(s, first, last):
    """Selects lines in the right (new) side of the merge view, as a user would."""
    return s.js(
        """const el = document.querySelectorAll('.cm-mergeViewEditor .cm-content')[1];
        const view = el && (el.cmView?.rootView?.view ?? el.cmTile?.root?.view ?? el.cmView?.view);
        if (!view) return 'no view: ' + Object.keys(el || {}).join(',');
        const doc = view.state.doc;
        view.dispatch({ selection: { anchor: doc.line(arguments[0]).from, head: doc.line(arguments[1]).to } });
        return 'ok';""",
        first,
        last,
    )


def dom_click(s, css):
    s.wait(css)
    s.js("document.querySelector(arguments[0]).click()", css)


def add_comment(s, first, last, text):
    print("   select:", select_lines(s, first, last))
    box = s.wait(".toolbar input")
    s.type(box, text)
    s.click(s.wait(".toolbar button", text="Add comment"))


s = Session(app)
try:
    s.type(s.wait(".connect form input"), clone)
    s.click(s.wait(".connect form button"))
    s.wait(".inbox .row[data-number='44']", timeout=60)
    sections = s.js("return [...document.querySelectorAll('.inbox h3')].map(h => h.textContent.trim())")
    print("[inbox] sections:", sections)

    # Find another author's PR and open it.
    s.type(s.wait(".search input"), "parser")
    s.wait(".hits [data-hit='43']", timeout=20)
    print("[search] hits:", s.js("return [...document.querySelectorAll('.hits .row')].map(r => r.innerText.replace(/\\n+/g, ' | '))"))
    s.click(s.wait(".hits [data-hit='43']"))
    s.wait(".overview h2", timeout=30, text="Add y to the parser")
    print("[overview]", s.js("return document.querySelector('.overview h2').textContent.trim()"), "|",
          s.js("return document.querySelector('.overview .meta').innerText.replace(/\\n+/g, ' ')"))
    print("[overview] description:", s.js("return document.querySelector('.overview .body').innerText.trim()"),
          "| scripts in it:", s.js("return document.querySelectorAll('.overview .body script, .overview .body [onerror]').length"),
          "| __pwned:", s.js("return window.__pwned ?? null"))
    s.screenshot(os.path.join(shots, "r1-overview.png"))
    print("[inbox] after opening:", s.js("return [...document.querySelectorAll('.inbox h3')].map(h => h.textContent.trim())"))

    tab(s, "Comments")
    s.wait(".thread", timeout=20)
    print("[comments] threads:", s.js("return document.querySelectorAll('.thread').length"), "| conversation:",
          s.js("return [...document.querySelectorAll('.entry')].map(e => e.innerText.replace(/\\n+/g, ' '))"))
    s.click(s.wait(".thread button", text="Resolve"))
    s.click(s.wait(".thread .link", text="Reply"))
    s.type(s.wait(".thread textarea"), "Because y needs it.")
    s.click(s.wait(".thread button", text="Reply on GitHub"))
    s.type(s.wait(".comments > .compose textarea"), "Overall looks good.")
    s.click(s.wait(".comments > .compose button", text="Comment on GitHub"))
    time.sleep(1.5)
    s.screenshot(os.path.join(shots, "r2-comments.png"))
    for w in writes():
        print("   wrote:", json.dumps(w)[:160])

    tab(s, "Review")
    s.wait(".review .marker", timeout=30)
    print("[review]", " ".join(s.js("return document.querySelector('.review .marker').textContent").split()),
          "| scopes:", s.js("return [...document.querySelectorAll('.scopes li')].map(l => l.textContent.trim())"),
          "| files:", s.js("return [...document.querySelectorAll('.files li button')].map(b => b.textContent.trim())"))
    print("[review] worktrees in the clone:", len(git(clone, "worktree", "list").splitlines()))
    # Any language is highlighted, not just Java: a Rust file gets syntax-styled tokens.
    s.click(s.wait(".files button", text="c.rs"))
    s.wait_js("return document.querySelectorAll('.cm-mergeViewEditor .cm-line span[class]').length > 0", timeout=20)
    print("[review] c.rs styled tokens:", s.js("return document.querySelectorAll('.cm-mergeViewEditor .cm-line span[class]').length"))
    s.click(s.wait(".files button", text="a.txt"))
    s.wait(".cm-mergeViewEditor", timeout=20)
    time.sleep(0.5)
    add_comment(s, 10, 10, "Name this better.")
    add_comment(s, 1, 1, "Unrelated: the header is stale.")
    print("[draft]", s.js("return [...document.querySelectorAll('.review .comments li')].map(l => l.innerText.replace('×','').trim())"))
    # WebDriver's synthesized clicks miss elements this close to the window's bottom edge
    # under Xvfb (the element at the point is the button itself), so click in the DOM.
    dom_click(s, ".review .comments .actions button.primary")
    s.wait(".submit", timeout=10)
    s.js("document.querySelector(\".submit input[value='REQUEST_CHANGES']\").click()")
    s.type(s.wait(".submit textarea"), "A couple of things.")
    s.wait_js("return !!document.querySelector('.submit .hint')", timeout=10)
    print("[submit] preview:", s.js("return document.querySelector('.submit .hint').textContent.trim()"))
    s.screenshot(os.path.join(shots, "r3-submit.png"))
    dom_click(s, ".submit .actions button.primary")
    s.wait(".review .notice", timeout=15)
    print("[submit]", s.js("return document.querySelector('.review .notice').textContent.trim()"))
    review_post = [w for w in writes() if w.get("post", "").endswith("/pulls/43/reviews")]
    print("[submit] request:", json.dumps(review_post[-1]["body"]) if review_post else None, "| expected commit:", sha43)

    # Own PR, also without a worktree: after Mark reviewed, "All changes" is still there.
    select_row(s, 42)
    s.wait(".overview h2", timeout=30)
    tab(s, "Review")
    s.wait(".review .marker", timeout=30)
    s.click(s.wait("button.primary", text="Mark reviewed"))
    s.wait(".review .notice", timeout=15)
    time.sleep(0.5)
    print("[own] after Mark reviewed, scopes:", s.js("return [...document.querySelectorAll('.scopes li')].map(l => l.textContent.trim())"))
    s.click(s.wait("[data-scope='all']"))
    time.sleep(1)
    print("[own] All changes vs main files:", s.js("return [...document.querySelectorAll('.files li button')].map(b => b.textContent.trim())"),
          "| own PR offers approve:", s.js("return !!document.querySelector(\".submit input[value='APPROVE']\")"))
    s.screenshot(os.path.join(shots, "r4-own.png"))
finally:
    s.quit()
