"""E2E: reviewing PRs (your own and other people's) against a fake GitHub.

GitHub is replaced by e2e/fake_gh.py (ARIEKEI_GH), which serves the fixture's PRs and
records every write instead of sending it. Covers: Inbox sections (Assigned, Approved,
Review requested, My PRs, Reviewed, Open), browsing open PRs and issues (Ctrl+K) to add them
to Open, starring and removing, opening another author's PR, Overview (sanitized description),
Comments (resolve, reply, new comment), reviewing without a worktree ("All changes vs
main" from refs/pull/N/head), draft comments, Submit to GitHub (line comment inside the
PR diff, the other one folded into the body), and "All changes" after Mark reviewed.
No agent is started. The review guide comes from e2e/fake_claude.py (ARIEKEI_CLAUDE).

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
    write(os.path.join(upstream, "c.rs"), "fn main() {\n    let answer = 42; // highlighted\n}\n"),
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


NEW_SIDE = """const el = document.querySelectorAll('.cm-mergeViewEditor .cm-content')[1];
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
    equal("Inbox sections", sections(s), ["Assigned 1", "Approved 1", "Review requested 1", "My PRs 1", "Reviewed 1", "Open 0"])
    equal("each PR is in its section", {k: section_rows(s, k) for k in ["assigned", "approved", "requested", "mine", "reviewed"]},
          {"assigned": [45], "approved": [46], "requested": [44], "mine": [42], "reviewed": [47]})

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
    # Enter adds the hit; Enter again opens it (and closes the browser).
    press_on(s, "[data-browse-search]", "Enter")
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
    # Not while typing: ? in the search field is just a character.
    press_on(s, "[data-browse-field]", "?")
    time.sleep(0.3)
    check("? in a text field types, it does not open the shortcuts", not s.find("[data-shortcuts]"))

    tab(s, "Comments")
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
    s.click(s.wait("[data-send-comment]"))
    wrote("issues/43/comments", 0)
    s.click(s.wait(".thread button", text="Resolve"))
    wrote("resolveReviewThread", 0)
    s.screenshot(os.path.join(shots, "r2-comments.png"))
    for w in writes():
        print("   wrote:", json.dumps(w)[:160])

    tab(s, "Review")
    s.wait(".review .marker", timeout=30)
    s.wait(".files [data-file]", timeout=30)
    print("[review]", " ".join(s.js("return document.querySelector('.review .marker').textContent").split()),
          "| scopes:", s.js("return [...document.querySelectorAll('.scopes button, .scope-folded .active-scope, .scope-folded button')].map(l => l.textContent.trim())"),
          "| files:", s.js("return [...document.querySelectorAll('.files [data-file]')].map(b => b.dataset.file)"))
    print("[review] worktrees in the clone:", len(git(clone, "worktree", "list").splitlines()))
    equal("reviewing without a worktree adds none", len(git(clone, "worktree", "list").splitlines()), 1)
    equal("files of #43 in review order", s.js("return [...document.querySelectorAll('.files [data-file]')].map(b => b.dataset.file)"), ["a.txt", "b.txt", "c.rs", "w.txt", "tests/parser_test.rs"])
    # Any language is highlighted, not just Java: a Rust file gets syntax-styled tokens.
    side_by_side(s)
    s.click(s.wait(".files [data-file='c.rs']"))
    s.wait_js("return document.querySelectorAll('.cm-mergeViewEditor .cm-line span[class]').length > 0", timeout=20)
    print("[review] c.rs styled tokens:", s.js("return document.querySelectorAll('.cm-mergeViewEditor .cm-line span[class]').length"))
    check("a Rust file is highlighted", s.js("return document.querySelectorAll('.cm-mergeViewEditor .cm-line span[class]').length") > 0)
    s.click(s.wait(".files [data-file='a.txt']"))
    wait_new_side(s, 10)
    # A selection shows on changed lines too: it is drawn above their background.
    select_lines(s, 10, 10)
    shown = s.js(NEW_SIDE + """
        const n = view.domAtPos(view.state.doc.line(10).from + 1).node;
        const el10 = (n.nodeType === 1 ? n : n.parentElement).closest('.cm-line'), line = el10.getBoundingClientRect();
        const layer = view.dom.querySelector('.cm-selectionLayer');
        const over = [...layer.querySelectorAll('.cm-selectionBackground')].some((b) => {
          const r = b.getBoundingClientRect(); return r.top < line.bottom && r.bottom > line.top && r.width > 0; });
        return [Number(getComputedStyle(layer).zIndex), over, el10.className];""")
    print("[selection] layer z-index, drawn over line 10, its classes:", shown)
    check("a selection on a changed line is drawn above it", shown[0] > 0 and shown[1] and "cm-changedLine" in shown[2], shown)
    add_comment(s, 10, 10, "Name this better.")
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
    print("[layout] scope list collapsed by default:", s.js("return document.querySelectorAll('.scopes button, .scope-folded .active-scope, .scope-folded button').length"), "entries:",
          s.js("return [...document.querySelectorAll('.scopes button, .scope-folded .active-scope, .scope-folded button')].map(l => l.textContent.trim())"))
    s.click(s.wait("[data-toggle='commits']"))
    print("[layout] expanded:", s.js("return [...document.querySelectorAll('.scopes button, .scope-folded .active-scope, .scope-folded button')].map(l => l.textContent.trim())"))
    s.click(s.wait("[data-toggle='commits']"))
    s.click(s.wait(".inbox header button[aria-label^='Collapse Inbox']"))
    s.wait(".inbox.collapsed", timeout=5)
    s.click(s.wait("[data-toggle='side']"))
    time.sleep(0.5)
    after = (width(".item"), width(".review .main"))
    print(f"[layout] work item width {before[0]} -> {after[0]} px, diff area {before[1]} -> {after[1]} px",
          "| attention dots:", s.js("return document.querySelectorAll('.inbox.collapsed .pip').length"))
    check("collapsing the Inbox and the side panel widens the diff", after[1] > before[1] + 300, (before, after))
    first = s.js("return document.querySelector('.toolbar .current-file')?.textContent.trim()")
    s.js("document.body.dispatchEvent(new KeyboardEvent('keydown', { key: ']', bubbles: true }))")
    time.sleep(0.5)
    print("[layout] file before/after ']':", first, "->", s.js("return document.querySelector('.toolbar .current-file')?.textContent.trim()"))
    contains("']' goes to the next file", s.js("return document.querySelector('.toolbar .current-file')?.textContent.trim()"), "b.txt")
    s.screenshot(os.path.join(shots, "r5-wide.png"))
    # Ctrl+B brings the Inbox back (the state is saved for the next launch).
    s.js("document.body.dispatchEvent(new KeyboardEvent('keydown', { key: 'b', ctrlKey: true, bubbles: true }))")
    s.wait(".inbox:not(.collapsed)", timeout=5)
    s.click(s.wait("[data-toggle='side']"))
    print("[layout] Ctrl+B restored the Inbox; side panel shown again:", bool(s.find(".review .side")))
    check("the side panel is back", s.find(".review .side"))

    # --- review order, reviewed marks, whitespace, unified, keys (PR #43) ---
    s.click(s.wait("[data-toggle='side']")) if not s.find(".review .side") else None
    groups = s.js("return [...document.querySelectorAll('.side .group')].map(g => [g.dataset.group, g.querySelectorAll('li.file').length])")
    print("[order] groups:", groups)
    equal("review order groups", [g[0] for g in groups], ["source", "tests", "generated"])
    print("[order] files in order:", s.js("return [...document.querySelectorAll('.side .files [data-file]')].map(b => b.dataset.file)"))
    print("[marks] progress before:", s.js("return document.querySelector('[data-progress]').textContent.trim()"))
    s.click(s.wait("[data-file='a.txt']"))
    time.sleep(0.5)
    dom_click(s, "[data-reviewed-next]")
    eventually("'Reviewed, next' marks the file", lambda: s.js("return document.querySelector('[data-progress]').textContent"), lambda v: "1/6" in v)
    print("[marks] after 'reviewed, next':", s.js("return document.querySelector('[data-progress]').textContent.trim()"),
          "| now on:", s.js("return document.querySelector('.side .files button.active')?.dataset.file"))
    contains("'Reviewed, next' marks the file", s.js("return document.querySelector('[data-progress]').textContent"), "1/6")
    equal("and moves on", s.js("return document.querySelector('.side .files button.active')?.dataset.file"), "b.txt")
    s.js("document.body.dispatchEvent(new KeyboardEvent('keydown', { key: 'r', bubbles: true }))")
    eventually("'r' marks the file", lambda: s.js("return document.querySelector('[data-progress]').textContent"), lambda v: "2/6" in v)
    print("[keys] after 'r':", s.js("return document.querySelector('[data-progress]').textContent.trim()"),
          "| now on:", s.js("return document.querySelector('.side .files button.active')?.dataset.file"))
    contains("'r' marks the file", s.js("return document.querySelector('[data-progress]').textContent"), "2/6")
    active = lambda: s.js("return document.querySelector('.side .files button.active')?.dataset.file")
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

    # The author pushes again: the reviewed file a.txt changes, c.rs does not.
    git(upstream, "checkout", "-q", "feature/y")
    write(os.path.join(upstream, "a.txt"), "\n".join(lines[:9] + ["line10 changed again"] + lines[10:]) + "\n")
    commit(upstream, "address review")
    git(upstream, "update-ref", "refs/pull/43/head", "feature/y")
    git(upstream, "checkout", "-q", "main")
    reviewed_before = s.js("return [...document.querySelectorAll('[data-reviewed]')].filter(c => c.getAttribute('aria-checked') === 'true').map(c => c.dataset.reviewed)")

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
    s.wait("[data-progress]", timeout=30)
    s.wait_js("return document.querySelector('.review .marker').textContent.includes('2 commits')", timeout=30)
    reviewed_after = eventually("after the push, only the unchanged file stays reviewed", lambda: s.js("return [...document.querySelectorAll('[data-reviewed]')].filter(c => c.getAttribute('aria-checked') === 'true').map(c => c.dataset.reviewed)"), lambda v: v == ["b.txt"])
    print("[marks] reviewed before the push:", reviewed_before, "| after:", reviewed_after)
    equal("a push unmarks the changed file only", (sorted(reviewed_before), reviewed_after), (["a.txt", "b.txt"], ["b.txt"]))

    t0 = time.monotonic()
    s.click(s.wait("[data-prepare-guide]"))
    s.wait("[data-guide] .focus", timeout=600)
    print(f"[guide] ready after {time.monotonic() - t0:.0f}s; focus:", s.js("return document.querySelector('.guide .focus')?.textContent"))
    print("[guide] groups:", s.js("return [...document.querySelectorAll('.side .group')].map(g => g.querySelector('.group-title').textContent.trim().replace(/\\s+/g, ' '))"))
    print("[guide] claims:", s.js("return [...document.querySelectorAll('.claims li')].map(l => l.textContent.trim())"))
    check("the guide has a focus", s.js("return document.querySelector('.guide .focus')?.textContent"))
    equal("guide claims", s.js("return document.querySelectorAll('.claims li').length"), 2)
    s.screenshot(os.path.join(shots, "r6-guide.png"))

    # The guide line above the diff: the shown file's group and claims, also with the side
    # panel hidden; expanded, it adds the focus for the whole change.
    def guide_line():
        return " ".join(s.js("return document.querySelector('[data-file-guide] .expand')?.innerText ?? ''").split())
    first = s.js("return document.querySelector('.side .files [data-file]').dataset.file")
    s.js("document.querySelector(`.side [data-file='${arguments[0]}']`).click()", first)
    s.wait_js("return document.querySelector('[data-file-guide]')")
    print(f"[guide line] {first}:", guide_line())
    contains("the guide line names the file's group", guide_line(), "Parser")
    s.js("document.querySelector(\"[data-toggle='side']\").click()")
    s.wait_js("return !document.querySelector('.side')")
    s.js("document.querySelector('.toolbar button[aria-label^=\"Next file\"]').click()")
    time.sleep(0.5)
    print("[guide line] side hidden, next file", s.js("return document.querySelector('.toolbar .current-file').textContent.trim()").split()[0] + ":", guide_line())
    s.js("document.querySelector('[data-file-guide] .expand').click()")
    print("[guide line] expanded:", s.js("return document.querySelector('[data-file-guide] .focus-all')?.textContent"))
    contains("expanded, it shows the focus", s.js("return document.querySelector('[data-file-guide] .focus-all')?.textContent"), "Focus for the whole change")
    s.js("document.querySelector('[data-file-guide] .expand').click()")
    s.js("document.querySelector(\"[data-toggle='side']\").click()")
    s.wait(".side")

    # --- own PR, rebased since the review (PR #42) ---
    select_row(s, 42)
    s.wait(".item header .title", timeout=30, text="#42")
    tab(s, "Review")
    s.wait(".review .marker", timeout=30)
    s.click(s.wait("[data-mark-reviewed]"))
    if s.find(".review [data-guard]"):
        print("[own] Mark reviewed asks first:", s.js("return document.querySelector('.review [data-guard] .text').textContent"))
        s.click(s.wait(".review [data-guard] button", text="Mark anyway"))
    s.wait(".review [data-notice]", timeout=15)
    # main moves on (also in d.txt, far from the PR's line); the branch is rebased onto it
    # and gets one more commit, in a.txt only.
    write(os.path.join(upstream, "base.txt"), "from main\n")
    write(os.path.join(upstream, "d.txt"), "\n".join(d_lines[:5] + ["d6 from main"]) + "\n")
    commit(upstream, "main moves on")
    git(upstream, "checkout", "-q", "feature/x")
    git(upstream, "-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false", "rebase", "-q", "main")
    write(os.path.join(upstream, "a.txt"), "\n".join(lines[:1] + ["line2 mine, improved"] + lines[2:]) + "\n")
    commit(upstream, "after the review")
    git(upstream, "update-ref", "refs/pull/42/head", "feature/x")
    git(upstream, "checkout", "-q", "main")
    select_row(s, 43)
    s.wait(".item header .title", timeout=30, text="#43")
    select_row(s, 42)
    s.wait(".item header .title", timeout=30, text="#42")
    tab(s, "Review")
    s.wait_js("return document.querySelector('.review .marker')?.textContent.includes('new since')", timeout=30)
    s.click(s.wait("[data-scope='review']")) if s.find("[data-scope='review']") else None
    eventually("since your review: the rebase note", lambda: s.js("return document.querySelector('[data-rebase-note]')?.textContent ?? ''"), lambda v: "Rebased" in v)
    print("[rebase]", " ".join(s.js("return document.querySelector('.review .marker').textContent").split()),
          "| note:", s.js("return document.querySelector('[data-rebase-note]')?.textContent ?? null"))
    print("[rebase] since your review, files:", s.js("return [...document.querySelectorAll('.side .files [data-file]')].map(b => b.dataset.file)"))
    contains("the rebase note", s.js("return document.querySelector('[data-rebase-note]')?.textContent"), "Rebased since your review")
    eventually("since the review, only the PR's own change", lambda: s.js("return [...document.querySelectorAll('.side .files [data-file]')].map(b => b.dataset.file)"), lambda v: v == ["a.txt"])
    s.click(s.wait("[data-scope='all']"))
    eventually("all changes vs main", lambda: s.js("return document.querySelector('.scope-folded .active-scope, .scopes button.active')?.textContent ?? ''"), lambda v: "All" in v)
    print("[rebase] all changes vs main, files:", s.js("return [...document.querySelectorAll('.side .files [data-file]')].map(b => b.dataset.file)"),
          "| own PR offers approve:", s.js("return !!document.querySelector(\"[data-event='APPROVE']:not(:disabled)\")"))
    check("no approving your own PR", not s.js("return !!document.querySelector(\"[data-event='APPROVE']:not(:disabled)\")"))
    # The reviewed marks survive the rebase where only the base changed the file: d.txt's
    # content is new (main's d6), but not the PR's change in it; a.txt changed after the review.
    marks = eventually("reviewed marks after the rebase", lambda: s.js(
        "return Object.fromEntries([...document.querySelectorAll('[data-reviewed]')].map(c => [c.dataset.reviewed, c.getAttribute('aria-checked') === 'true']))"),
        lambda v: v and set(v) == {"a.txt", "d.txt"})
    print("[rebase] reviewed marks:", marks)
    equal("after a rebase only the file changed since the review is unmarked", marks, {"a.txt": False, "d.txt": True})
finally:
    s.quit()
