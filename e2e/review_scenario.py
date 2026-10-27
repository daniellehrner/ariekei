# Copyright contributors to Ariekei.
# SPDX-License-Identifier: Apache-2.0

"""E2E: reviewing PRs (your own and other people's) against a fake GitHub.

GitHub is replaced by e2e/fake_gh.py (ARIEKEI_GH), which serves the fixture's PRs and
records every write instead of sending it. Covers: Inbox sections (Assigned, Approved,
Review requested, My PRs, Reviewed, Open), browsing open PRs and issues (Ctrl+K) to add them
to Open, starring and removing, opening another author's PR, Overview (sanitized description),
Comments (resolve, reply, new comment), reviewing without a worktree ("All changes vs
main" from refs/pull/N/head), draft comments, Submit to GitHub (line comment inside the
PR diff, the other one folded into the body), a push showing up in the review, new comments
marked and their thread first, an item opening on the tab it was left on (also one picked just
before leaving it), a conversation GitHub fails to give (shown as last given, with a bar), and
no approving your own PR.
No agent is started.

Run it with `python3 e2e/run.py <test-app> review` (see run.py), which sets the environment.
"""

import json
import os
import subprocess
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from check import check, contains, equal, eventually  # noqa: E402
from ui import open_inbox, select_row, side_by_side  # noqa: E402
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
# A line wider than the diff pane (the scrollbar below the diff).
lines[11] = "line12 " + "wide " * 80
write(os.path.join(upstream, "a.txt"), "\n".join(lines) + "\n")
write(os.path.join(upstream, "w.txt"), "keep\n")
d_lines = [f"d{i}" for i in range(1, 7)]
write(os.path.join(upstream, "d.txt"), "\n".join(d_lines) + "\n")
commit(upstream, "init")
sha42 = branch(upstream, "feature/x", 42, lambda: (
    write(os.path.join(upstream, "a.txt"), "\n".join(lines[:1] + ["line2 mine"] + lines[2:]) + "\n"),
    write(os.path.join(upstream, "d.txt"), "\n".join(["d1 mine"] + d_lines[1:]) + "\n"),
))
sha43 = branch(upstream, "feature/y", 43, lambda: (
    write(os.path.join(upstream, "a.txt"), "\n".join(lines[:9] + ["line10 changed"] + lines[10:]) + "\n"),
    write(os.path.join(upstream, "b.txt"), "new file\n"),
    write(os.path.join(upstream, "c.rs"), "fn main() {\n    let answer = 42; // highlighted\n    // " + "wide " * 80 + "\n"
          + "    let x = 0;\n" * 120 + "}\n"),
    os.makedirs(os.path.join(upstream, "tests"), exist_ok=True),
    write(os.path.join(upstream, "tests", "parser_test.rs"), "#[test]\nfn parses() {}\n"),
    write(os.path.join(upstream, "Cargo.lock"), "# generated\n"),
    write(os.path.join(upstream, "w.txt"), "keep  \n"),
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
    # One PR for each of the other sections; their branches are not needed.
    {"number": 45, "title": "Handle w", "author": "erin", "head_ref": "feature/w", "head_sha": sha44, "base_ref": "main", "assigned": True},
    {"number": 46, "title": "My approved change", "author": "me", "head_ref": "feature/v", "head_sha": sha42, "base_ref": "main", "decision": "APPROVED"},
    {"number": 47, "title": "Tidy u", "author": "frank", "head_ref": "feature/u", "head_sha": sha44, "base_ref": "main", "reviewed": True},
], "issues": [
    {"number": 8, "title": "Flaky test", "author": "carol"},
]}, open(os.path.join(gh_dir, "data.json"), "w"))
writes_file = os.path.join(gh_dir, "writes.jsonl")


def writes():
    if not os.path.exists(writes_file):
        return []
    return [json.loads(line) for line in open(writes_file)]


# --- helpers ------------------------------------------------------------------------
def sections(s):
    return s.js("return [...document.querySelectorAll('.inbox .section-head')].map(h => h.textContent.trim().replace(/\\s+/g, ' '))")


def section_rows(s, key):
    """The numbers of the rows in Inbox section `key`, in order."""
    return s.js("return [...document.querySelectorAll(`.inbox [data-section='${arguments[0]}'] .row`)].map(r => +r.dataset.number)", key)


def hits(s):
    return s.js("return [...document.querySelectorAll('[data-browse-hit]')].map(h => +h.dataset.browseHit)")


def ctrl(s, key):
    s.js("document.body.dispatchEvent(new KeyboardEvent('keydown', { key: arguments[0], ctrlKey: true, bubbles: true }))", key)


def tab(s, name):
    s.js("[...document.querySelectorAll('[role=tab]')].find(t => t.textContent.trim().startsWith(arguments[0]))?.click()", name)


# The new side's editor: the right one side by side, the only one in a unified diff (an added
# or deleted file shows as one pane).
NEW_SIDE = """const el = [...document.querySelectorAll('.diff .cm-content')].at(-1);
        const view = el && (el.cmView?.rootView?.view ?? el.cmTile?.root?.view ?? el.cmView?.view);"""


def wait_new_side(s, lines):
    """Waits until the new side shows a file of at least `lines` lines (after picking a file,
    the previous one can still be on screen for a moment)."""
    s.wait_js(NEW_SIDE + f"\n        return !!view && view.state.doc.lines >= {int(lines)};", timeout=20)


def select_lines(s, first, last):
    """Selects lines in the right (new) side of the merge view, as a user would."""
    return s.js(
        NEW_SIDE + """
        if (!view) return 'no view: ' + Object.keys(el || {}).join(',');
        const doc = view.state.doc;
        view.dispatch({ selection: { anchor: doc.line(arguments[0]).from, head: doc.line(arguments[1]).to } });
        return 'ok';""",
        first,
        last,
    )


def press_on(s, css, key):
    """Presses `key` with the focus on the element `css`, as the keyboard would."""
    s.js("const el = document.querySelector(arguments[0]); el.focus();"
         "el.dispatchEvent(new KeyboardEvent('keydown', { key: arguments[1], bubbles: true }))", css, key)


def dom_click(s, css):
    s.wait(css)
    s.js("document.querySelector(arguments[0]).click()", css)


def add_comment(s, first, last, text):
    print("   select:", select_lines(s, first, last))
    box = s.wait("[data-composer] input")
    s.type(box, text)
    s.click(s.wait("[data-add-comment]"))


s = Session(app)
try:
    s.type(s.wait(".connect form input"), clone)
    s.click(s.wait(".connect form button"))
    s.wait(".inbox .row[data-number='44']", timeout=60)
    print("[inbox] sections:", sections(s))
    equal("Inbox sections", sections(s), ["Assigned 1", "Approved 1", "Review requested 1", "My PRs 1", "Reviewed 1"])
    equal("each PR is in its section", {k: section_rows(s, k) for k in ["assigned", "approved", "requested", "mine", "reviewed"]},
          {"assigned": [45], "approved": [46], "requested": [44], "mine": [42], "reviewed": [47]})

    # Refresh spins until the poll it asked for is done.
    busy = s.js("document.querySelector('[data-refresh]').click(); await new Promise((r) => setTimeout(r, 0));"
                "return document.querySelector('[data-refresh]').hasAttribute('data-refreshing')")
    check("the refresh button spins while it refreshes", busy)
    eventually("and stops when the poll is done", lambda: bool(s.find("[data-refreshing]")), lambda v: not v, timeout=20)

    # Browse (Ctrl+K): add an open issue, then find another author's PR, add it and open it.
    ctrl(s, "k")
    s.wait("[data-browse]", timeout=10)
    dom_click(s, "[data-browse-kind] [data-value='issue']")
    s.wait("[data-browse-hit='8']", timeout=20)
    s.click(s.wait("[data-browse-hit='8'] [data-browse-add]"))
    s.wait("[data-browse-hit='8'] [data-browse-open]", timeout=20)
    eventually("an added issue is listed under Open", lambda: section_rows(s, "open"), lambda v: v == [8], timeout=10)
    dom_click(s, "[data-browse-kind] [data-value='pr']")
    s.type(s.wait("[data-browse-search]"), "parser")
    eventually("the search finds #43", lambda: hits(s), lambda v: v == [43], timeout=20)
    print("[browse] hits:", s.js("return [...document.querySelectorAll('[data-browse-hit]')].map(r => r.innerText.replace(/\\n+/g, ' | '))"))
    s.screenshot(os.path.join(shots, "r0-browse.png"))
    # Add puts the hit in the Inbox without an agent (Enter would start working on it); Enter
    # then opens it (and closes the browser).
    s.click(s.wait("[data-browse-hit='43'] [data-browse-add]"))
    s.wait("[data-browse-hit='43'] [data-browse-open]", timeout=20)
    press_on(s, "[data-browse-search]", "Enter")
    s.wait(".item header .title", timeout=30, text="Add y to the parser")
    s.wait(".overview .meta", timeout=30)
    print("[overview]", s.js("return document.querySelector('.item header .title').textContent.trim()"), "|",
          s.js("return document.querySelector('.overview .meta').innerText.replace(/\\n+/g, ' ')"))
    print("[overview] description:", s.js("return document.querySelector('.overview .body').innerText.trim()"),
          "| scripts in it:", s.js("return document.querySelectorAll('.overview .body script, .overview .body [onerror]').length"),
          "| __pwned:", s.js("return window.__pwned ?? null"))
    s.screenshot(os.path.join(shots, "r1-overview.png"))
    check("opening an item closes the browser", not s.find("[data-browse]"))
    print("[inbox] after opening:", sections(s))
    check("the description is sanitized", s.js("return document.querySelectorAll('.overview .body script, .overview .body [onerror]').length") == 0 and s.js("return window.__pwned ?? null") is None)
    eventually("added items are listed under Open", lambda: sorted(section_rows(s, "open")), lambda v: v == [8, 43], timeout=10)

    # Starring the open item (in its header) moves it to the top; unstarring it in the Inbox
    # moves it back.
    dom_click(s, "[data-item-star]")
    eventually("a starred item heads the Inbox", lambda: section_rows(s, "starred"), lambda v: v == [43], timeout=10)
    equal("Starred is the first section", sections(s)[0], "Starred 1")
    check("its star is set in the Inbox and the header",
          s.js("return document.querySelector(\".inbox [data-star='43']\").getAttribute('aria-pressed') === 'true' && document.querySelector('[data-item-star]').getAttribute('aria-pressed') === 'true'"))
    s.screenshot(os.path.join(shots, "r0-starred.png"))
    dom_click(s, ".inbox [data-star='43']")
    eventually("unstarred, it is back under Open", lambda: sorted(section_rows(s, "open")), lambda v: v == [8, 43], timeout=10)
    check("no Starred section is left", not s.find(".inbox [data-section='starred']"))
    # An added item can be taken out again; one the user did not add cannot.
    check("only added items can be removed", s.find(".inbox [data-remove='8']") and not s.find(".inbox [data-remove='44']"))
    dom_click(s, ".inbox [data-remove='8']")
    eventually("a removed item leaves the Inbox", lambda: section_rows(s, "open"), lambda v: v == [43], timeout=10)
    ctrl(s, "k")
    dom_click(s, "[data-browse-kind] [data-value='issue']")
    s.wait("[data-browse-hit='8'] [data-browse-add]", timeout=20)  # it offers to add it again
    press_on(s, "[data-browse-search]", "Escape")
    eventually("Esc closes the browser", lambda: bool(s.find("[data-browse]")), lambda v: not v, timeout=10)

    # ? shows the shortcuts anywhere, not only in the review: here with the focus on the
    # Overview tab, as clicking it leaves it. Esc closes them.
    press_on(s, "[data-tab=overview]", "?")
    s.wait("[data-shortcuts]", timeout=10)
    sections = s.js("return [...document.querySelectorAll('[data-shortcuts] h3')].map(h => h.textContent)")
    print("[keys] ? on the Overview tab:", sections, "| Ctrl+N listed:", s.js("return document.querySelector('[data-shortcuts]').textContent.includes('Ctrl+N')"))
    equal("? on the Overview shows all shortcuts", sections, ["Anywhere", "In the review"])
    press_on(s, "[data-tab=overview]", "Escape")
    eventually("Esc closes the shortcuts", lambda: bool(s.find("[data-shortcuts]")), lambda v: not v, timeout=10)
    tab(s, "Comments")
    # Not while typing: ? in a text field is just a character.
    s.wait("label.compose textarea", timeout=10)
    press_on(s, "label.compose textarea", "?")
    time.sleep(0.3)
    check("? in a text field types, it does not open the shortcuts", not s.find("[data-shortcuts]"))
    s.wait(".thread", timeout=20)
    print("[comments] threads:", s.js("return document.querySelectorAll('.thread').length"), "| conversation:",
          s.js("return [...document.querySelectorAll('.timeline li')].map(e => e.innerText.replace(/\\n+/g, ' '))"))
    # Each action waits for its write: the buttons are disabled while one is in flight.
    def wrote(what, before):
        deadline = time.monotonic() + 20
        while time.monotonic() < deadline:
            if len([w for w in writes() if what in json.dumps(w)]) > before:
                return
            time.sleep(0.2)
        raise TimeoutError(f"no {what} write")
    s.click(s.wait(".thread [data-reply]"))
    s.type(s.wait(".thread textarea"), "Because y needs it.")
    s.click(s.wait(".thread button", text="Reply on GitHub"))
    wrote("/replies", 0)
    s.type(s.wait("label.compose textarea"), "Overall looks good.")
    # Enabled once the reply's request has finished, not only once its write arrived.
    s.click(s.wait("[data-send-comment]:not(:disabled)", timeout=20))
    wrote("issues/43/comments", 0)
    s.click(s.wait(".thread button:not(:disabled)", text="Resolve", timeout=20))
    wrote("resolveReviewThread", 0)
    s.screenshot(os.path.join(shots, "r2-comments.png"))
    for w in writes():
        print("   wrote:", json.dumps(w)[:160])

    tab(s, "Review")
    s.wait(".review .marker", timeout=30)
    s.wait(".files [data-file]", timeout=30)
    print("[review]", " ".join(s.js("return document.querySelector('.review .marker').textContent").split()),
          "| scope:", s.js("return document.querySelector('[data-scope-menu]')?.textContent.trim()"),
          "| files:", s.js("return [...document.querySelectorAll('.files [data-file]')].map(b => b.dataset.file)"))
    print("[review] worktrees in the clone:", len(git(clone, "worktree", "list").splitlines()))
    equal("reviewing without a worktree adds none", len(git(clone, "worktree", "list").splitlines()), 1)
    equal("files of #43 in review order", s.js("return [...document.querySelectorAll('.files [data-file]')].map(b => b.dataset.file)"), ["a.txt", "b.txt", "c.rs", "w.txt", "tests/parser_test.rs"])
    # Any language is highlighted, not just Java: a Rust file gets syntax-styled tokens. (An
    # added file shows as one pane.)
    side_by_side(s)
    s.click(s.wait(".files [data-file='c.rs']"))
    s.wait_js("return document.querySelectorAll('.diff .cm-line span[class]').length > 0", timeout=20)
    print("[review] c.rs styled tokens:", s.js("return document.querySelectorAll('.diff .cm-line span[class]').length"))
    check("a Rust file is highlighted", s.js("return document.querySelectorAll('.diff .cm-line span[class]').length") > 0)
    check("an added file shows as one pane, even side by side", not s.find(".diff .cm-mergeView"))
    # Code shows as written: no ligatures (-> or <= drawn as one arrow by fonts like JetBrains Mono).
    lig = s.js("const st = getComputedStyle(document.querySelector('.diff .cm-line')); return [st.fontVariantLigatures, st.fontFeatureSettings]")
    print("[review] code ligatures:", lig)
    check("the diff draws no ligatures", lig[0] == "none" and '"calt" 0' in lig[1], lig)
    # Lines wider than the pane: a scrollbar right below the visible diff (the editor's own
    # sits below its last line), as wide as the code, which it scrolls.
    bar = eventually("a wide file shows the scrollbar below the diff", lambda: s.js("""
        const d = document.querySelector('.diff').getBoundingClientRect(), bars = [...document.querySelectorAll('.hbars .ak-hscroll')];
        const ed = document.querySelector('.diff .cm-editor').getBoundingClientRect();
        const code = document.querySelector('.diff .cm-scroller').getBoundingClientRect();
        return bars.map((b) => { const r = b.getBoundingClientRect(); return [!b.hidden && r.height >= 12,
          Math.abs(r.top - d.bottom) <= 1 && r.bottom <= window.innerHeight, Math.abs(r.left - code.left) <= 1 && Math.abs(r.width - code.width) <= 1,
          ed.bottom > d.bottom + 100]; });"""),
        lambda v: v == [[True, True, True, True]], timeout=10)
    print("[hscroll] c.rs bar shown, below the diff, under the code, editor taller than the diff:", bar)
    scrolled = s.js("""const bar = document.querySelector('.hbars .ak-hscroll'); bar.scrollLeft = 80;
        bar.dispatchEvent(new Event('scroll')); await new Promise((r) => setTimeout(r, 50));
        const diff = document.querySelector('.diff');
        return [document.querySelector('.diff .cm-scroller').scrollLeft, diff.scrollWidth <= diff.clientWidth];""")
    print("[hscroll] code scrollLeft after the bar's, the diff itself not wider:", scrolled)
    check("the scrollbar below the diff scrolls the code, not the diff", scrolled == [80, True], scrolled)
    s.click(s.wait(".files [data-file='b.txt']"))
    wait_new_side(s, 1)
    eventually("a file whose lines fit shows no scrollbar",
               lambda: s.js("const h = document.querySelector('.hbars'); return [[...h.children].map((b) => b.hidden), h.offsetHeight]"), lambda v: v == [[True], 0], timeout=10)
    s.click(s.wait(".files [data-file='a.txt']"))
    wait_new_side(s, 10)
    eventually("side by side, each pane has its scrollbar under it",
               lambda: s.js("""const panes = [...document.querySelectorAll('.diff .cm-scroller')].map((c) => c.getBoundingClientRect());
                   return [...document.querySelectorAll('.hbars .ak-hscroll')].map((b, i) => { const r = b.getBoundingClientRect();
                     return !b.hidden && Math.abs(r.left - panes[i].left) <= 1 && Math.abs(r.width - panes[i].width) <= 1; });"""),
               lambda v: v == [True, True], timeout=10)
    # A selection shows on changed lines too: it is drawn above their background.
    select_lines(s, 10, 10)
    # CodeMirror draws the selection layer in its next measure cycle, so wait for it.
    shown = eventually("a selection on a changed line is drawn above it", lambda: s.js(NEW_SIDE + """
        const n = view.domAtPos(view.state.doc.line(10).from + 1).node;
        const el10 = (n.nodeType === 1 ? n : n.parentElement).closest('.cm-line'), line = el10.getBoundingClientRect();
        const layer = view.dom.querySelector('.cm-selectionLayer');
        const over = [...layer.querySelectorAll('.cm-selectionBackground')].some((b) => {
          const r = b.getBoundingClientRect(); return r.top < line.bottom && r.bottom > line.top && r.width > 0; });
        return [Number(getComputedStyle(layer).zIndex), over, el10.className];"""),
        lambda v: v[0] > 0 and v[1] and "cm-changedLine" in v[2], timeout=10)
    print("[selection] layer z-index, drawn over line 10, its classes:", shown)
    # Esc in the composer drops the selection (and the composer with it).
    select_lines(s, 3, 4)
    s.wait("[data-composer] input", timeout=10)
    press_on(s, "[data-composer] input", "Escape")
    eventually("Esc in the composer drops the selection", lambda: bool(s.find("[data-composer]")), lambda v: not v, timeout=5)
    add_comment(s, 10, 10, "Name this better.")
    eventually("adding closes the composer", lambda: bool(s.find("[data-composer]")), lambda v: not v, timeout=5)
    add_comment(s, 1, 1, "Unrelated: the header is stale.")
    print("[draft]", s.js("return [...document.querySelectorAll('[data-drafts] li')].map(l => l.innerText.replace(/\\s+/g, ' ').trim())"))
    equal("draft comments", s.js("return document.querySelectorAll('[data-drafts] li').length"), 2)
    # WebDriver's synthesized clicks miss elements this close to the window's bottom edge
    # under Xvfb (the element at the point is the button itself), so click in the DOM.
    dom_click(s, "[data-open-submit]")
    s.wait(".submit", timeout=10)
    s.js("document.querySelector(\"[data-event='REQUEST_CHANGES']\").click()")
    s.type(s.wait(".submit textarea"), "A couple of things.")
    s.wait_js("return !!document.querySelector('[data-preview]')", timeout=10)
    print("[submit] preview:", s.js("return document.querySelector('[data-preview]').textContent.trim()"))
    s.screenshot(os.path.join(shots, "r3-submit.png"))
    dom_click(s, "[data-send-review]")
    s.wait(".review [data-notice]", timeout=15)
    print("[submit]", s.js("return document.querySelector('.review [data-notice]').textContent.trim()"))
    review_post = [w for w in writes() if w.get("post", "").endswith("/pulls/43/reviews")]
    print("[submit] request:", json.dumps(review_post[-1]["body"]) if review_post else None, "| expected commit:", sha43)
    check("the review was sent", review_post)
    body = review_post[-1]["body"]
    equal("review event", body["event"], "REQUEST_CHANGES")
    equal("review commit", body["commit_id"], sha43)
    equal("line comments", [(c["path"], c["line"]) for c in body["comments"]], [("a.txt", 10)])
    contains("the comment outside the PR diff goes to the body", body["body"], "a.txt:1")

    # More room for the diff: collapse the Inbox, the commit list and the side panel.
    width = lambda css: s.js("return Math.round(document.querySelector(arguments[0]).getBoundingClientRect().width)", css)
    open_inbox(s)
    before = (width(".item"), width(".review .main"))
    # The scope is a menu in the review bar: all changes, or one commit (with its file count).
    s.click(s.wait("[data-scope-menu]"))
    scopes = s.js("return [...document.querySelectorAll('[data-scope]')].map(l => l.textContent.replace(/\\s+/g, ' ').trim())")
    print("[layout] scope menu:", scopes)
    check("the scope menu lists all changes and each commit", len(scopes) >= 2 and scopes[0].startswith("All changes"), scopes)
    press_on(s, "[role=menu][aria-label=Scope]", "Escape")
    eventually("Esc closes the scope menu", lambda: bool(s.find("[data-scope='all']")), lambda v: not v, timeout=5)
    s.click(s.wait(".inbox header button[aria-label^='Collapse Inbox']"))
    s.wait(".inbox.collapsed", timeout=5)
    s.click(s.wait("[data-toggle='side']"))
    time.sleep(0.5)
    after = (width(".item"), width(".review .main"))
    print(f"[layout] work item width {before[0]} -> {after[0]} px, diff area {before[1]} -> {after[1]} px",
          "| attention dots:", s.js("return document.querySelectorAll('.inbox.collapsed .pip').length"))
    check("collapsing the Inbox and the side panel widens the diff", after[1] > before[1] + 300, (before, after))
    first = s.js("return document.querySelector('.bar .current-file')?.textContent.trim()")
    s.js("document.body.dispatchEvent(new KeyboardEvent('keydown', { key: ']', bubbles: true }))")
    time.sleep(0.5)
    print("[layout] file before/after ']':", first, "->", s.js("return document.querySelector('.bar .current-file')?.textContent.trim()"))
    contains("']' goes to the next file", s.js("return document.querySelector('.bar .current-file')?.textContent.trim()"), "b.txt")
    s.screenshot(os.path.join(shots, "r5-wide.png"))
    # Ctrl+B brings the Inbox back (the state is saved for the next launch).
    s.js("document.body.dispatchEvent(new KeyboardEvent('keydown', { key: 'b', ctrlKey: true, bubbles: true }))")
    s.wait(".inbox:not(.collapsed)", timeout=5)
    s.click(s.wait("[data-toggle='side']"))
    print("[layout] Ctrl+B restored the Inbox; side panel shown again:", bool(s.find(".review .side")))
    check("the side panel is back", s.find(".review .side"))

    # --- review order, whitespace, unified, keys (PR #43) ---
    s.click(s.wait("[data-toggle='side']")) if not s.find(".review .side") else None
    groups = s.js("return [...document.querySelectorAll('.side .group')].map(g => [g.dataset.group, g.querySelectorAll('li.file').length])")
    print("[order] groups:", groups)
    equal("review order groups", [g[0] for g in groups], ["source", "tests", "generated"])
    print("[order] files in order:", s.js("return [...document.querySelectorAll('.side .files [data-file]')].map(b => b.dataset.file)"))
    active = lambda: s.js("return document.querySelector('.side .files button.active')?.dataset.file")
    s.click(s.wait("[data-file='b.txt']"))
    eventually("b.txt shows", active, lambda v: v == "b.txt")
    before = active()
    press_on(s, "[data-tab=review]", "[")
    eventually("'[' goes to the previous file", active, lambda v: v and v != before)
    print("[keys] '[' from", before, "to", active())
    press_on(s, "[data-tab=review]", "w")
    eventually("whitespace-only files are labelled", lambda: s.js("return document.querySelectorAll('.side .file .ws').length"), lambda v: v > 0)
    print("[ws] labelled whitespace-only:", s.js("return [...document.querySelectorAll('.side .file')].filter(l => l.querySelector('.ws')).map(l => l.querySelector('[data-file]').dataset.file)"))
    equal("whitespace-only files", s.js("return [...document.querySelectorAll('.side .file')].filter(l => l.querySelector('.ws')).map(l => l.querySelector('[data-file]').dataset.file)"), ["w.txt"])
    press_on(s, "[data-tab=review]", "w")
    eventually("'w' shows whitespace again", lambda: s.js("return document.querySelectorAll('.side .file .ws').length"), lambda v: v == 0)
    # n / p go from change to change and on into the next / previous file; at the ends of the
    # scope a note says there is no change further on.
    def key(k):
        s.js("document.body.dispatchEvent(new KeyboardEvent('keydown', { key: arguments[0], bubbles: true }))", k)

    def press_until(k, done, limit=30):
        for _ in range(limit):
            key(k)
            time.sleep(0.25)
            if done():
                return True
        return False

    shown = s.js("return [...document.querySelectorAll('.side .files [data-file]')].map(b => b.dataset.file)")
    s.click(s.wait("[data-file='%s']" % shown[0]))
    eventually("the first file shows", active, lambda v: v == shown[0])
    check("n goes on into the next file", press_until("n", lambda: active() != shown[0]))
    equal("n entered the next file", active(), shown[1])
    # Where the cursor is: the new side's text (which file), its line, on a marked change.
    at = lambda: s.js(NEW_SIDE + """return view && [view.state.doc.line(1).text, view.state.doc.lineAt(view.state.selection.main.head).number,
        document.querySelectorAll('.diff .ak-current-change').length > 0];""")
    eventually("n enters it at its first change", at, lambda v: v and v[0] == "new file" and v[1] == 1 and v[2], timeout=10)
    key("p")
    eventually("p goes back into the previous file", active, lambda v: v == shown[0])
    landed = eventually("at its last change", at, lambda v: v and v[0] == "line1" and v[1] == 10 and v[2], timeout=10)
    print(f"[n/p] {shown[0]} -> n -> {shown[1]} line 1 -> p -> {shown[0]} line {landed[1]}")
    check("p at the first change says so", press_until("p", lambda: bool(s.find("[data-edge='start']"))))
    equal("and stays in the first file", active(), shown[0])
    s.click(s.wait("[data-file='%s']" % shown[-1]))
    eventually("the last file shows", active, lambda v: v == shown[-1])
    check("n at the last change says so", press_until("n", lambda: bool(s.find("[data-edge='end']"))))
    print("[n/p] at the end:", s.js("return document.querySelector('[data-edge]').textContent.trim()"))
    eventually("the note goes away", lambda: bool(s.find("[data-edge]")), lambda v: not v, timeout=10)

    s.click(s.wait("[data-file='a.txt']"))
    press_on(s, "[data-tab=review]", "u")
    eventually("the unified diff shows", lambda: s.js("return document.querySelectorAll('.diff .cm-editor').length"), lambda v: v == 1)
    print("[unified] editors:", s.js("return document.querySelectorAll('.diff .cm-editor').length"),
          "| deleted chunks shown inline:", s.js("return document.querySelectorAll('.diff .cm-deletedChunk').length"))
    equal("unified diff: one editor", s.js("return document.querySelectorAll('.diff .cm-editor').length"), 1)
    press_on(s, "[data-tab=review]", "u")
    eventually("'u' goes back to side by side", lambda: s.js("return document.querySelectorAll('.diff .cm-editor').length"), lambda v: v == 2)
    # With the focus where clicking the Review tab leaves it, as a user would press it.
    press_on(s, "[data-tab=review]", "?")
    print("[keys] shortcut overlay in the review:", bool(s.find("[data-shortcuts]")))
    check("'?' shows the shortcuts in the review", s.find("[data-shortcuts]"))
    press_on(s, "[data-tab=review]", "Escape")
    eventually("Esc closes the shortcuts", lambda: bool(s.find("[data-shortcuts]")), lambda v: not v, timeout=10)

    # The author pushes again: a.txt changes.
    git(upstream, "checkout", "-q", "feature/y")
    write(os.path.join(upstream, "a.txt"), "\n".join(lines[:9] + ["line10 changed again"] + lines[10:]) + "\n")
    commit(upstream, "address review")
    git(upstream, "update-ref", "refs/pull/43/head", "feature/y")
    git(upstream, "checkout", "-q", "main")
    data = json.load(open(os.path.join(gh_dir, "data.json")))
    next(pr for pr in data["prs"] if pr["number"] == 43)["head_sha"] = git(upstream, "rev-parse", "feature/y")
    json.dump(data, open(os.path.join(gh_dir, "data.json"), "w"))

    # A notice about another item, as the backend sends one while the window is focused
    # (notify.rs): clicking it opens that item, and it goes away.
    s.js("""const rows = await window.__TAURI_INTERNALS__.invoke('inbox');
      const item = rows.find((r) => r.number === 44).id;
      await window.__TAURI_INTERNALS__.invoke('plugin:event|emit',
        { event: 'notice', payload: { item, kind: 'finished', title: '#44 Other', body: 'The agent finished.' } });""")
    s.wait("[data-notice='finished'] .body", timeout=10)
    s.screenshot(os.path.join(shots, "r5-notice.png"))
    s.click(s.wait("[data-notice='finished'] .body"))
    s.wait(".item header .title", timeout=30, text="#44")
    eventually("an opened notice goes away", lambda: bool(s.find("[data-notice]")), lambda v: not v, timeout=10)
    print("[notice] clicking it opened #44")

    # Items reopen on the tab they were left on, so wait for the header, not a tab.
    select_row(s, 44)
    s.wait(".item header .title", timeout=30, text="#44")
    select_row(s, 43)
    s.wait(".item header .title", timeout=30, text="#43")
    tab(s, "Review")
    # The review opens on what the clone has; once a poll sees the push, it offers it, and the
    # diff stays as it is until the user reloads.
    marker = lambda: s.js("return document.querySelector('.review .marker')?.textContent ?? ''")
    eventually("the review opens on what it fetched before", marker, lambda v: "1 commit" in v, timeout=30)
    s.js("document.querySelector('[data-refresh]').click()")
    try:
        s.wait("[data-newer='head']", timeout=30)
    except TimeoutError:
        print("[newer] not offered; the backend says:", s.js("""const rows = await window.__TAURI_INTERNALS__.invoke('inbox');
          const id = rows.find((r) => r.number === 43).id;
          const fetched = await window.__TAURI_INTERNALS__.invoke('review_fetch', { id }).catch((e) => `error: ${e}`);
          const state = await window.__TAURI_INTERNALS__.invoke('review_state', { id }).catch((e) => `error: ${e}`);
          return [fetched, state.head ?? state];"""), "| expected head:", git(upstream, "rev-parse", "feature/y"))
        raise
    s.screenshot(os.path.join(shots, "r5-newer.png"))
    print("[newer] bar:", s.js("return document.querySelector('[data-newer]').textContent.trim()"), "| marker:", marker().strip())
    check("the diff waits for Reload", "1 commit" in marker())
    # A poll's check of GitHub that overlaps the Reload compares with the review shown before
    # it: its answer must not bring the bar back.
    s.js("document.querySelector('[data-refresh]').click()")
    s.click(s.wait("[data-newer-reload]"))
    eventually("Reload shows the push", marker, lambda v: "2 commits" in v, timeout=30)
    eventually("the poll is done", lambda: s.find("[data-refreshing]"), lambda v: not v, timeout=30)
    time.sleep(1)
    check("and the bar goes", not s.find("[data-newer]"))

    # Carol's comment becomes activity the poll sees; loading the open item's conversation after
    # the poll counts as seeing it. Left on Comments, #43 comes back on Comments.
    def edit_43(change):
        data = json.load(open(os.path.join(gh_dir, "data.json")))
        change(next(pr for pr in data["prs"] if pr["number"] == 43))
        json.dump(data, open(os.path.join(gh_dir, "data.json"), "w"))
    def poll():
        s.js("document.querySelector('[data-refresh]').click()")
        eventually("the poll is done", lambda: bool(s.find("[data-refreshing]")), lambda v: not v, timeout=20)
    edit_43(lambda pr: pr["comments"][0].update(at="2026-10-11T12:00:00Z"))
    tab(s, "Comments")
    poll()
    time.sleep(2)
    select_row(s, 44)
    s.wait(".item header .title", timeout=30, text="#44")
    select_row(s, 43)
    s.wait(".item header .title", timeout=30, text="#43")
    equal("#43 opens on the tab it was left on", s.js("return document.querySelector('.item [aria-selected=true][data-tab]')?.dataset.tab"), "comments")
    check("nothing is new yet", not s.find("[data-new]"))

    # Carol answers in a thread while the item is open: once the poll brings it, Comments shows
    # that thread first, her reply marked new, and the Inbox does not say the item has news.
    edit_43(lambda pr: pr["threads"].append(
        {"id": "T2", "path": "b.txt", "line": 1, "comment_id": 102, "author": "bob", "html": "<p>And this?</p>",
         "replies": [{"comment_id": 103, "author": "carol", "html": "<p>Same question.</p>", "at": "2026-10-12T09:00:00Z"}]}))
    poll()
    first = lambda: s.js("return document.querySelector('.thread')?.dataset.thread ?? ''")
    eventually("a thread with a new reply comes first", first, lambda v: v == "T2", timeout=30)
    equal("only Carol's reply is new", s.js("return document.querySelectorAll('[data-new]').length"), 1)
    check("it is the reply", "Same question." in s.js("return document.querySelector('[data-new]').innerText"))
    check("the heading counts it", "1 with new comments" in s.js("return document.querySelector('.comments h2').textContent"))
    eventually("the open item has no new activity in the Inbox",
               lambda: bool(s.find(".inbox .row[data-number='43'] svg.new")), lambda v: not v, timeout=20)
    s.screenshot(os.path.join(shots, "r5-new-reply.png"))

    # A tab picked right before opening another item is kept too: its save, still waiting,
    # is not dropped, nor written to the other item.
    shown_tab = lambda: s.js("return document.querySelector('.item [aria-selected=true][data-tab]')?.dataset.tab")
    def open_item(number):
        select_row(s, number)
        s.wait(".item header .title", timeout=30, text=f"#{number}")
        s.wait(".item [aria-selected=true][data-tab]", timeout=30)
    tab(s, "Overview")
    open_item(44)
    tab(s, "Comments")
    open_item(43)
    equal("#43 kept the tab picked just before leaving it", shown_tab(), "overview")
    open_item(44)
    equal("and #44 its own", shown_tab(), "comments")

    # GitHub fails to give #43's conversation: the one shown stays, and a bar says it is not
    # current; the next poll that gets it takes the bar away.
    open_item(43)
    tab(s, "Comments")
    s.wait(".comments .thread", timeout=30)
    failing = os.path.join(gh_dir, "fail-conversation-43")
    open(failing, "w").close()
    poll()
    stale = s.text(s.wait(".item [data-stale]", timeout=30))
    print("[stale]", stale.strip())
    check("the conversation stays shown", bool(s.find(".comments .thread")))
    os.remove(failing)
    poll()
    eventually("the bar goes once GitHub answers", lambda: bool(s.find(".item [data-stale]")), lambda v: not v, timeout=30)

    # --- own PR (PR #42): GitHub does not let you approve it ---
    select_row(s, 42)
    s.wait(".item header .title", timeout=30, text="#42")
    tab(s, "Review")
    s.wait(".review .marker", timeout=30)
    eventually("all changes vs main", lambda: s.js("return document.querySelector('[data-scope-menu]')?.textContent ?? ''"), lambda v: "All" in v)
    print("[own] files:", s.js("return [...document.querySelectorAll('.side .files [data-file]')].map(b => b.dataset.file)"),
          "| own PR offers approve:", s.js("return !!document.querySelector(\"[data-event='APPROVE']:not(:disabled)\")"))
    check("no approving your own PR", not s.js("return !!document.querySelector(\"[data-event='APPROVE']:not(:disabled)\")"))
finally:
    s.quit()
