"""E2E: the embedded Claude Code terminal, driven through the real app.

Covers: TUI starts in a worktree, slash-command autocomplete, hook-driven status
(working / needs you / finished), answering a permission prompt in the terminal,
agent_send of a multi-line message, switching work items without losing the screen,
and resuming the conversation after an app restart.

Runs against a release build through tauri-driver (Linux, Xvfb). Start the driver with
ARIEKEI_CLAUDE_ARGS="--permission-mode default" so the permission prompt is
deterministic regardless of the user's own permission mode:

  ARIEKEI_CLAUDE_ARGS="--permission-mode default" xvfb-run -a tauri-driver --port 4444 &
  python3 -I e2e/terminal_scenario.py <app-binary> <out-dir> <worktree-a> <worktree-b>
"""

import os
import subprocess
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from wd import Session  # noqa: E402

app, out, wt_a, wt_b = sys.argv[1:5]
os.makedirs(out, exist_ok=True)

ENTER, BACKSPACE = "", ""

SCREEN = """
const t = (window.__ariekeiTerminals || {})[arguments[0]];
if (!t) return null;
const b = t.buffer.active, lines = [];
for (let i = 0; i < b.length; i++) lines.push(b.getLine(i).translateToString(true));
return lines.join('\\n');
"""


def screen(s, key):
    return s.js(SCREEN, key) or ""


def wait_screen(s, key, text, timeout=60):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if text in screen(s, key):
            return
        time.sleep(0.3)
    raise TimeoutError(f"{text!r} never appeared in the terminal of {key}")


def status(s, key):
    return s.js(
        "return [...document.querySelectorAll('aside .item')]"
        ".find(e => e.title === arguments[0])?.querySelector('.status')?.textContent ?? null",
        key,
    )


def wait_status(s, key, wanted, timeout=120):
    deadline, last = time.monotonic() + timeout, None
    while time.monotonic() < deadline:
        last = status(s, key)
        if last in wanted:
            return last
        time.sleep(0.3)
    raise TimeoutError(f"{key}: status {last!r}, wanted {wanted}")


def keys(s, text):
    """Types into the active terminal through xterm's input element (real key path)."""
    s.type(s.wait(".terminal:not(.hidden) .xterm-helper-textarea"), text)


def open_item(s, path):
    s.type(s.wait("aside form input"), path)
    s.click(s.wait("aside form button"))


def select_item(s, path):
    s.wait_js("return [...document.querySelectorAll('aside .item')].some(e => e.title === %r)" % path)
    s.js("[...document.querySelectorAll('aside .item')].find(e => e.title === arguments[0]).click()", path)


def tree_pss_mb():
    pids = subprocess.run(["pgrep", "-x", "ariekei"], capture_output=True, text=True).stdout.split()
    todo, seen = list(pids), set()
    while todo:
        p = todo.pop()
        if p not in seen:
            seen.add(p)
            todo += subprocess.run(["pgrep", "-P", p], capture_output=True, text=True).stdout.split()
    total, mcp = 0, []
    for p in seen:
        try:
            cmd = open(f"/proc/{p}/cmdline", "rb").read().replace(b"\0", b" ").decode()
            # Our own flag mentions MCP; anything else that does is an MCP server.
            if "mcp" in cmd.lower() and "--strict-mcp-config" not in cmd:
                mcp.append(cmd[:100])
            total += next(int(l.split()[1]) for l in open(f"/proc/{p}/smaps_rollup") if l.startswith("Pss:"))
        except (OSError, StopIteration):
            pass
    return total // 1024, mcp


def canonical(path):
    return os.path.realpath(path)


key_a, key_b = canonical(wt_a), canonical(wt_b)

# --- run 1 -------------------------------------------------------------------
s = Session(app)
try:
    s.wait("aside h1")
    print(f"[start] first paint in {s.started_in:.2f}s; tree PSS {tree_pss_mb()[0]} MB")

    open_item(s, wt_a)
    wait_status(s, key_a, {"ready"}, timeout=40)
    wait_screen(s, key_a, "Claude Code")
    wait_screen(s, key_a, "❯")  # the prompt glyph: the TUI accepts input
    pss, mcp = tree_pss_mb()
    print(f"[open] TUI up; tree PSS {pss} MB; MCP processes: {mcp or 'none'}")

    keys(s, "/code-rev")
    wait_screen(s, key_a, "/code-review", timeout=15)
    print("[slash] autocomplete lists /code-review")
    s.screenshot(os.path.join(out, "01-slash.png"))
    keys(s, BACKSPACE * len("/code-rev"))

    keys(s, "Create a file named hello.txt containing exactly the word hi. Do nothing else.")
    keys(s, ENTER)
    wait_status(s, key_a, {"working"}, timeout=30)
    st = wait_status(s, key_a, {"needs you", "finished"}, timeout=120)
    if st == "needs you":
        print("[permission] status 'needs you' while the TUI asks")
        s.screenshot(os.path.join(out, "02-permission.png"))
        keys(s, ENTER)  # accept the highlighted default ("Yes")
        wait_status(s, key_a, {"working"}, timeout=30)
        wait_status(s, key_a, {"finished"}, timeout=120)
    else:
        print("[permission] no prompt (is ARIEKEI_CLAUDE_ARGS set?)")
    print("[turn] finished; hello.txt exists:", os.path.exists(os.path.join(wt_a, "hello.txt")))

    text = "Reply on one line with two words.\nThe first word is alpha and the second word is beta."
    s.js("return window.__TAURI_INTERNALS__.invoke('agent_send', {key: arguments[0], text: arguments[1]})", key_a, text)
    wait_status(s, key_a, {"working"}, timeout=30)
    wait_status(s, key_a, {"finished"}, timeout=120)
    sc = screen(s, key_a)
    print("[send] multi-line message delivered as one prompt:", "second word is beta" in sc,
          "| reply has alpha+beta:", "alpha beta" in sc.lower().replace(",", ""))
    s.screenshot(os.path.join(out, "03-sent.png"))

    open_item(s, wt_b)
    wait_status(s, key_b, {"ready"}, timeout=40)
    select_item(s, key_a)
    wait_screen(s, key_a, "Create a file named hello.txt", timeout=15)
    pss, mcp = tree_pss_mb()
    print(f"[switch] back to A, earlier conversation still on screen; two agents tree PSS {pss} MB; MCP: {mcp or 'none'}")
finally:
    s.quit()
time.sleep(2)
left = subprocess.run(["pgrep", "-a", "-f", "^.*claude --strict-mcp-config"], capture_output=True, text=True).stdout.strip()
print("[quit] claude processes left:", left or "none")

# --- run 2: restart ------------------------------------------------------------
s = Session(app)
try:
    s.wait("aside h1")
    items = s.js("return [...document.querySelectorAll('aside .item')].map(e => e.title)")
    print(f"[restart] first paint in {s.started_in:.2f}s; restored items: {len(items)}")
    if key_a not in items:
        raise AssertionError("work item A was not restored")
    select_item(s, key_a)
    wait_status(s, key_a, {"ready"}, timeout=40)
    wait_screen(s, key_a, "Create a file named hello.txt", timeout=30)
    print("[restart] --resume shows the earlier conversation")
    s.screenshot(os.path.join(out, "04-resumed.png"))
finally:
    s.quit()
