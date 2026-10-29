# Copyright contributors to Ariekei.
# SPDX-License-Identifier: Apache-2.0

"""Screenshots of every screen, for comparing the UI with the design (not a test).

Builds a fixture with PRs in every attention state, drives the app through the screens and
saves one PNG per screen into <work>/shots/<theme>-<size>/. The agent is e2e/fake_claude.py,
which reports status through the hooks like Claude Code does. Screenshots need WebDriver, so
this runs through tauri-driver (Linux), on Xvfb (without WAYLAND_DISPLAY: GTK would prefer
your Wayland desktop over Xvfb's DISPLAY). The desktop's dark preference reaches the app over
D-Bus; to choose the theme, point D-Bus nowhere and set it in a GTK settings file:

  mkdir -p <work>/xdg/gtk-3.0 && printf '[Settings]\\ngtk-application-prefer-dark-theme=1\\n' \\
    > <work>/xdg/gtk-3.0/settings.ini       # 0 for light
  FAKE_GH_DIR=<work>/gh ARIEKEI_GH=$PWD/e2e/fake_gh.py ARIEKEI_DATA_DIR=<work>/data \\
    ARIEKEI_CLAUDE=$PWD/e2e/fake_claude.py ARIEKEI_POLL_SECS=600 XDG_CONFIG_HOME=<work>/xdg \\
    DBUS_SESSION_BUS_ADDRESS=unix:path=/nonexistent GDK_BACKEND=x11 \\
    env -u WAYLAND_DISPLAY xvfb-run -a -s "-screen 0 1600x1000x24" tauri-driver --port 4444 &
  E2E_WEBDRIVER=http://127.0.0.1:4444 python3 -I e2e/gallery.py <app> <work> [light|dark] [1440x900] [screen,...]

Screens: connect, empty (nothing selected yet), overview, menu, repo-remove (its confirm), comments, collapsed, waiting, terminal, review,
scope-menu, selection, drafts, submit, code-menu, ask-agent, shortcuts, review-hidden, notice-arriving,
notices, inbox (every section, one item starred), browse, new-task (on a fork's branch),
change-end (n past the last change), task (a task with the PR its agent opened), quit (asked
before quitting stops agents' work).
"""

import json
import os
import subprocess
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from wd import ELEMENT, Session  # noqa: E402

app, work = sys.argv[1:3]
theme = sys.argv[3] if len(sys.argv) > 3 else "light"
width, height = map(int, (sys.argv[4] if len(sys.argv) > 4 else "1440x900").split("x"))
wanted = set(sys.argv[5].split(",")) if len(sys.argv) > 5 else None
gh_dir = os.path.join(work, "gh")
shots = os.path.join(work, "shots", f"{theme}-{width}")
os.makedirs(gh_dir, exist_ok=True)
os.makedirs(shots, exist_ok=True)
GITHUB = "git@github.com:owner/proj.git"


def git(cwd, *args):
    return subprocess.run(["git", "-C", cwd, *args], check=True, capture_output=True, text=True).stdout.strip()


def write(path, text):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w") as f:
        f.write(text)


def commit(cwd, msg):
    git(cwd, "add", "-A")
    git(cwd, "-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false", "commit", "-qm", msg)


# --- fixture ----------------------------------------------------------------------------
upstream, clone = os.path.join(work, "github-proj"), os.path.join(work, "proj")
if not os.path.exists(clone):
    subprocess.run(["git", "init", "-q", "-b", "main", upstream], check=True)
    src = "src/main/java/org/example/sync"
    body = "\n".join(
        ["package org.example.sync;", "", "public class SnapSyncState {"]
        + [f"  private long field{i} = {i};" for i in range(1, 30)]
        + ["", "  public boolean isHealing() {", "    return healing;", "  }", "}"]
    )
    write(os.path.join(upstream, src, "SnapSyncState.java"), body + "\n")
    write(os.path.join(upstream, src, "SnapSyncDownloader.java"), "package org.example.sync;\n\nclass SnapSyncDownloader {}\n")
    write(os.path.join(upstream, "build.gradle"), "dependencies {\n  implementation 'a:b:1'\n}\n")
    commit(upstream, "init")
    git(upstream, "checkout", "-q", "-b", "snap-pivot-reorg")
    write(os.path.join(upstream, src, "SnapSyncState.java"), body.replace("    return healing;", "    // A reorged pivot restarts healing.\n    return healing && !pivotReorged;").replace("field7 = 7;", "field7 = 70; // a -> b <= c != d >= e") + "\n")
    write(os.path.join(upstream, src, "PivotReorgDetector.java"), "package org.example.sync;\n\n/** Detects a reorged pivot. */\nclass PivotReorgDetector {\n  boolean reorged(long canonical, long pivot) {\n    return canonical != pivot;\n  }\n}\n")
    write(os.path.join(upstream, "src/test/java/org/example/sync/PivotReorgDetectorTest.java"), "class PivotReorgDetectorTest {}\n")
    write(os.path.join(upstream, "build.gradle"), "dependencies {\n  implementation 'a:b:1'\n  testImplementation 'c:d:2'\n}\n")
    commit(upstream, "Detect a reorged pivot")
    git(upstream, "update-ref", "refs/pull/8012/head", "snap-pivot-reorg")
    sha = git(upstream, "rev-parse", "snap-pivot-reorg")
    git(upstream, "checkout", "-q", "main")
    subprocess.run(["git", "clone", "-q", upstream, clone], check=True)
    git(clone, "config", f"url.{upstream}.insteadOf", GITHUB)
    git(clone, "remote", "set-url", "origin", GITHUB)

    hunk = "@@ -33,3 +33,4 @@\n   public boolean isHealing() {\n-    return healing;\n+    // A reorged pivot restarts healing.\n+    return healing && !pivotReorged;"
    pr = {"head_ref": "snap-pivot-reorg", "head_sha": sha, "base_ref": "main"}
    json.dump({"viewer": "me", "upstream": upstream, "prs": [
        {**pr, "number": 8012, "title": "Fix snap sync stall when pivot block is reorged", "author": "me", "ci": "PENDING", "decision": "CHANGES_REQUESTED",
         "body_html": "<h2>Summary</h2><p>When the pivot block chosen for snap sync is reorged out, the world-state download keeps requesting "
                      "ranges for a state root that no peer serves any more.</p><ul><li>detects a reorged pivot</li><li>re-selects a pivot</li>"
                      "<li>adds a metric for pivot reorgs</li></ul><p>New unit tests in <code>PivotReorgDetectorTest</code>.</p>",
         "comments": [{"author": "carol", "html": "<p>Thanks, looking at it today.</p>"}],
         "threads": [{"id": "T1", "path": "src/main/java/org/example/sync/SnapSyncState.java", "line": 35, "comment_id": 101, "author": "carol",
                      "html": "<p>Does resetting the heal flag throw away healing progress?</p>", "diff_hunk": hunk}]},
        {**pr, "number": 8007, "title": "Bound the transaction pool sender cache", "author": "me", "ci": "FAILURE", "failing": "unit-tests"},
        {**pr, "number": 7998, "title": "Add block overrides to eth_simulateV1", "author": "me", "draft": True},
        {**pr, "number": 7973, "title": "Move Bonsai trie log pruning to a background task", "author": "carol", "requested": True,
         "decision": "CHANGES_REQUESTED", "threads": [{"id": "T2", "path": "build.gradle", "line": 2, "comment_id": 102, "author": "dmitri", "html": "<p>Why?</p>"}]},
        {**pr, "number": 8015, "title": "Reduce allocations when decoding receipts", "author": "dmitri", "requested": True},
        {**pr, "number": 8020, "title": "Expose the trie log pruning backlog as a metric", "author": "carol", "assigned": True},
        {**pr, "number": 8003, "title": "Add the Prague blob schedule to the genesis config", "author": "me", "decision": "APPROVED"},
        {**pr, "number": 7960, "title": "Back off from peers that send bad headers", "author": "dmitri", "reviewed": True, "decision": "APPROVED"},
        {**pr, "number": 7941, "title": "Cache the parsed genesis file", "author": "alice"},
    ], "issues": [{"number": 7990, "title": "Snap sync never completes after a pivot reorg", "author": "carol", "body": "Seen on Holesky.", "assigned": True},
                  {"number": 7995, "title": "eth_simulateV1 ignores the block overrides' gas limit", "author": "alice"},
                  {"number": 7982, "title": "Document the snap sync pivot distance option", "author": "carol"}]},
        open(os.path.join(gh_dir, "data.json"), "w"))


# --- helpers ----------------------------------------------------------------------------
def shot(s, name):
    if wanted is None or name in wanted:
        time.sleep(0.6)
        path = os.path.join(shots, f"{name}.png")
        print(f"[shot] {name}:", "saved" if s.screenshot(path) else "skipped", flush=True)


def need(*names):
    return wanted is None or any(n in wanted for n in names)


def row(s, number):
    s.wait(f".inbox .row[data-number='{number}']", timeout=60)
    s.js("document.querySelector(`.inbox .row[data-number='${arguments[0]}']`).click()", number)


def tab(s, name):
    s.js("[...document.querySelectorAll('[role=tab]')].find(t => t.textContent.trim().startsWith(arguments[0]))?.click()", name)


def click(s, css):
    s.wait(css, timeout=30)
    s.js("document.querySelector(arguments[0]).click()", css)


def hover(s, css):
    """Moves the pointer over `css`, so :hover styles show."""
    # Below the fold in a small window, the pointer can't reach it.
    s.js("document.querySelector(arguments[0]).scrollIntoView({ block: 'nearest' })", css)
    el = s.find(css)[0]
    s.s("POST", "/actions", {"actions": [{"type": "pointer", "id": "mouse", "parameters": {"pointerType": "mouse"},
                                          "actions": [{"type": "pointerMove", "duration": 0, "origin": {ELEMENT: el}, "x": 0, "y": 0}]}]})


def key(s, k):
    s.js("document.querySelector('.diff')?.dispatchEvent(new KeyboardEvent('keydown', { key: arguments[0], bubbles: true }))", k)


s = Session(app)
try:
    try:
        s.s("POST", "/window/rect", {"width": width, "height": height})
    except Exception as e:
        print("   (resize failed:", e, ")")
    print("[viewport]", s.js("return [innerWidth, innerHeight, matchMedia('(prefers-color-scheme: dark)').matches]"))
    s.wait_js("return !!document.querySelector('.connect form input, .inbox')", timeout=60)
    if s.js("return !!document.querySelector('.connect form input')"):
        shot(s, "connect")
        s.type(s.wait(".connect form input"), clone)
        s.js("document.querySelector('.connect form').requestSubmit()")
    s.wait(".inbox .row[data-number='8012']", timeout=60)
    if need("empty") and s.find(".empty"):
        shot(s, "empty")
    row(s, 8012)
    s.wait("[role=tab]", timeout=60)
    tab(s, "Overview")
    s.wait(".overview", timeout=60)
    shot(s, "overview")
    if need("inbox", "browse"):
        # Star an item and add one to Open, as from the Inbox and the browser; the pointer rests
        # on the added one (its star and Remove show).
        s.js("""const rows = await window.__TAURI_INTERNALS__.invoke('inbox');
          await window.__TAURI_INTERNALS__.invoke('set_starred', { id: rows.find((r) => r.number === 7973).id, starred: true });
          await window.__TAURI_INTERNALS__.invoke('track_item', { kind: 'pr', number: 7941 });""")
        s.wait(".inbox [data-section='starred']", timeout=20)
        s.wait(".inbox [data-section='open'] .row[data-number='7941']", timeout=20)
        hover(s, ".inbox .row[data-number='7941']")
        shot(s, "inbox")
        s.js("document.body.dispatchEvent(new KeyboardEvent('keydown', { key: 'k', ctrlKey: true, bubbles: true }))")
        s.wait("[data-browse-hit]", timeout=20)
        shot(s, "browse")
        s.js("document.querySelector(\"[data-browse-kind] [data-value='issue']\").click()")
        s.wait("[data-browse-hit='7995']", timeout=20)
        shot(s, "browse-issues")
        s.js("document.querySelector(\"[data-browse-search]\").dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))")
    if need("new-task"):
        s.click(s.wait("[data-new-task]"))
        s.type(s.wait("[data-task-branch]"), "friend:feat/evm-v2-all-opcodes")
        s.type(s.wait("[data-task-form] textarea"), "Finish the opcodes the branch is missing.")
        shot(s, "new-task")
        s.js("document.querySelector('[data-task-form] button[type=button]').click()")
    if need("menu", "repo-remove"):
        click(s, ".inbox [data-repo-menu]")
        s.wait(".menu [data-repo-id]")
        shot(s, "menu")
        if need("repo-remove"):
            hover(s, ".menu .entry")
            click(s, ".menu [data-remove-repo]")
            s.wait("[data-confirm-remove]")
            shot(s, "repo-remove")
        s.js("document.body.click()")
    if need("comments"):
        tab(s, "Comments")
        s.wait(".comments", timeout=30)
        shot(s, "comments")
    if need("waiting", "terminal", "collapsed"):
        click(s, "[data-start]")
        s.wait(".terminal .xterm", timeout=60)
        s.wait_js("return Object.keys(window.__ariekeiTerminals || {}).length > 0", timeout=30)
        time.sleep(1)
        s.js("const t = Object.values(window.__ariekeiTerminals)[0]; t.focus(); t.input('please ask\\r')")
        s.wait("[data-waiting]", timeout=30)
        shot(s, "terminal")
        tab(s, "Overview")
        shot(s, "waiting")
        s.js("document.body.dispatchEvent(new KeyboardEvent('keydown', { key: 'b', ctrlKey: true, bubbles: true }))")
        s.wait(".inbox.collapsed", timeout=10)
        shot(s, "collapsed")
        s.js("document.body.dispatchEvent(new KeyboardEvent('keydown', { key: 'b', ctrlKey: true, bubbles: true }))")
    if need("review", "scope-menu", "change-end", "selection", "drafts", "submit", "code-menu", "ask-agent", "shortcuts", "review-hidden"):
        tab(s, "Review")
        s.wait("[data-file]", timeout=60)
        shot(s, "review")
        if need("scope-menu"):
            click(s, "[data-scope-menu]")
            s.wait("[data-scope='all']", timeout=10)
            shot(s, "scope-menu")
            key(s, "Escape")
            s.js("document.querySelector('[data-scope-menu]').click()") if s.find("[role=menu][aria-label=Scope]") else None
        if need("change-end"):
            # n past the last change of the last file: the note at the end of the diff.
            s.js("[...document.querySelectorAll('.side .files [data-file]')].at(-1).click()")
            # The file's diff first: n before it shows would already be past its last change.
            s.wait_js("return [...document.querySelectorAll('.diff .cm-content')].at(-1)?.textContent.includes('testImplementation')", timeout=30)
            for _ in range(20):
                key(s, "n")
                time.sleep(0.3)
                if s.find("[data-edge]"):
                    break
            time.sleep(0.3)
            shot(s, "change-end")
        if need("selection", "drafts", "submit"):
            click(s, "[data-file$='SnapSyncState.java']")
            s.wait_js("return [...document.querySelectorAll('.diff .cm-content')].at(-1)?.cmView?.rootView?.view?.state.doc.lines > 30 "
                      "|| [...document.querySelectorAll('.diff .cm-content')].at(-1)?.textContent.includes('field29')", timeout=30)
            s.js("""const el = [...document.querySelectorAll('.diff .cm-content')].at(-1);
              const view = el && (el.cmView?.rootView?.view ?? el.cmTile?.root?.view ?? el.cmView?.view);
              const doc = view.state.doc; view.dispatch({ selection: { anchor: doc.line(34).from, head: doc.line(36).to } });""")
            s.type(s.wait("[data-composer] input"), "Keep a one-argument constructor for the old tests?")
            shot(s, "selection")
            click(s, "[data-add-comment]")
            shot(s, "drafts")
            click(s, "[data-open-submit]")
            s.wait("[data-submit]", timeout=30)
            shot(s, "submit")
        if need("code-menu", "ask-agent"):
            # Without the submit panel, which leaves little room for the diff in a small window.
            s.js("[...document.querySelectorAll('[data-submit] button')].find((b) => b.textContent.trim() === 'Cancel')?.click()")
            click(s, "[data-file$='SnapSyncState.java']")
            s.wait_js("return [...document.querySelectorAll('.diff .cm-content')].at(-1)?.textContent.includes('field29')", timeout=30)
            # Right-click in a selection of three lines.
            s.js("""const el = [...document.querySelectorAll('.diff .cm-content')].at(-1);
              const view = el && (el.cmView?.rootView?.view ?? el.cmTile?.root?.view ?? el.cmView?.view);
              const doc = view.state.doc; view.dispatch({ selection: { anchor: doc.line(34).from, head: doc.line(36).to }, scrollIntoView: true });
              await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
              const c = view.coordsAtPos(doc.line(35).from + 4), x = c.left, y = (c.top + c.bottom) / 2;
              document.elementFromPoint(x, y).dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: x, clientY: y }));""")
            s.wait("[data-code-menu]", timeout=10)
            shot(s, "code-menu")
            click(s, "[data-code-menu] [data-ask-agent]")
            s.type(s.wait("[data-ask] textarea"), "Why is the pivot block reset here and not in the constructor?")
            shot(s, "ask-agent")
            s.js("document.querySelector('[data-ask] textarea').dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))")
        if need("shortcuts"):
            key(s, "?")
            shot(s, "shortcuts")
            key(s, "Escape")
        if need("review-hidden"):
            s.js("document.body.dispatchEvent(new KeyboardEvent('keydown', { key: 'b', ctrlKey: true, bubbles: true }))")
            click(s, "[data-toggle='side']")
            shot(s, "review-hidden")
    if need("task"):
        # A task whose agent opened a PR: the task's Overview, and the PR an item of its own in
        # My PRs.
        s.js("document.body.dispatchEvent(new KeyboardEvent('keydown', { key: 'n', ctrlKey: true, bubbles: true }))")
        # The form keeps what the new-task screen typed.
        s.wait("[data-task-form]")
        s.js("""document.querySelectorAll('[data-task-form] input, [data-task-form] textarea').forEach((f) => {
          f.value = ''; f.dispatchEvent(new Event('input', { bubbles: true })); });""")
        s.type(s.wait("[data-task-form] input"), "Peers drop after restart")
        s.type(s.wait("[data-task-form] textarea"), "Look at how the peer table is reset when the node restarts.")
        s.click(s.wait("[data-task-form] button[type=submit]"))
        s.wait(".terminal .xterm", timeout=120)
        s.wait(".inbox .row[data-number='-1'] [data-agent='ready'], .inbox .row[data-number='-1'] [data-agent='finished']", timeout=60)
        wt = os.path.join(work, "proj_peers-drop-after-restart")
        write(os.path.join(wt, "src/main/java/org/example/p2p/PeerTable.java"), "package org.example.p2p;\n\nclass PeerTable {}\n")
        commit(wt, "Reset the peer table once")
        s.js("const t = Object.values(window.__ariekeiTerminals).at(-1); t.focus(); "
             "t.input(\"Open a pull request titled 'Reset the peer table once'\\r\")")
        s.wait(".inbox .row[data-number='8021']", timeout=60)
        tab(s, "Overview")
        s.wait(".overview .notes", timeout=30)
        shot(s, "task")
    if need("notice-arriving", "notices"):
        # One notice of each kind, as the backend sends them (notify.rs).
        s.js("""const rows = (await window.__TAURI_INTERNALS__.invoke('inbox')).filter((r) => r.kind !== 'task');
          const emit = (i, kind, title, body) => window.__TAURI_INTERNALS__.invoke('plugin:event|emit',
            { event: 'notice', payload: { item: rows[i]?.id ?? null, kind, title, body } });
          await emit(1, 'finished', '#' + rows[1]?.number + ' ' + rows[1]?.title, 'the agent finished');
          await emit(2, 'ci-failing', '#' + rows[2]?.number + ' ' + rows[2]?.title, 'build (ubuntu-24.04)');
          await emit(3, 'activity', '#' + rows[3]?.number + ' ' + rows[3]?.title, '');""")
        s.wait("[data-notice='activity']", timeout=10)
        s.js("""const rows = (await window.__TAURI_INTERNALS__.invoke('inbox')).filter((r) => r.kind !== 'task');
          await window.__TAURI_INTERNALS__.invoke('plugin:event|emit', { event: 'notice', payload: { item: null,
            kind: 'needs-you', title: '#' + rows[4]?.number + ' ' + rows[4]?.title, body: 'Bash(./gradlew test)' } });""")
        time.sleep(0.15)
        if wanted is None or "notice-arriving" in wanted:
            print("[shot] notice-arriving:", "saved" if s.screenshot(os.path.join(shots, "notice-arriving.png")) else "skipped", flush=True)
        time.sleep(1)
        shot(s, "notices")
    if need("quit"):
        # As quit.rs asks when the last window closes while agents run.
        s.js("""const rows = (await window.__TAURI_INTERNALS__.invoke('inbox')).filter((r) => r.worktree);
          const at = (r) => ({ item: r.id, name: (r.kind === 'task' ? 'T' + -r.number : '#' + r.number) + ' ' + r.title,
            repo: 'hyperledger/besu', worktree: r.worktree });
          await window.__TAURI_INTERNALS__.invoke('plugin:event|emit', { event: 'confirm-quit', payload: [
            { ...at(rows[0]), busy: false, processes: [{ pid: 48211, command: 'npm run dev -- --port 5173' },
              { pid: 48377, command: './gradlew test --continuous' }] },
            ...(rows[1] ? [{ ...at(rows[1]), busy: true, processes: [] }] : []) ] });""")
        s.wait("[data-confirm-quit]", timeout=10)
        shot(s, "quit")
        s.click(s.wait("[data-cancel-quit]"))
except Exception:
    print("[page]", s.js("return document.body.innerText.slice(0, 1500)"))
    raise
finally:
    s.quit()
