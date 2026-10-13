"""Spike scenario: open worktrees, run an agent turn with a permission prompt,
review a large diff, then restart the app and check the session comes back.

Runs against a release build through tauri-driver (Linux, Xvfb):
  xvfb-run -a tauri-driver --port 4444 &
  python3 -I e2e/spike_scenario.py <app-binary> <out-dir> <ui-worktree> <big-worktree>
"""

import os
import subprocess
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from wd import Session  # noqa: E402

app, out, wt_ui, wt_big = sys.argv[1:5]
os.makedirs(out, exist_ok=True)
shot = lambda s, name: s.screenshot(os.path.join(out, name + ".png"))


def status_of(s, wt):
    return s.js(
        "const li=[...document.querySelectorAll('aside li')].find(l=>l.querySelector('.item').title===arguments[0]);"
        "return li ? li.querySelector('.status').textContent : null;",
        wt,
    )


def wait_status(s, wt, wanted, timeout=120):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        st = status_of(s, wt)
        if st in wanted:
            return st
        time.sleep(0.3)
    raise TimeoutError(f"{wt}: status {status_of(s, wt)!r}, wanted {wanted}")


def tree_pss_mb(name="ariekei"):
    """PSS of the app process and all of its descendants (WebKit + agents)."""
    pids = subprocess.run(["pgrep", "-x", name], capture_output=True, text=True).stdout.split()
    if not pids:
        return None
    todo, seen = list(pids), set()
    while todo:
        p = todo.pop()
        if p in seen:
            continue
        seen.add(p)
        todo += subprocess.run(["pgrep", "-P", p], capture_output=True, text=True).stdout.split()
    total = 0
    for p in seen:
        try:
            for line in open(f"/proc/{p}/smaps_rollup"):
                if line.startswith("Pss:"):
                    total += int(line.split()[1])
        except OSError:
            pass
    return total // 1024


def open_worktree(s, path):
    inp = s.wait("aside form input")
    s.type(inp, path)
    s.click(s.wait("aside form button"))


def select_item(s, path):
    # WebKitWebDriver's element text omits the ellipsized name, so match on title.
    s.wait_js("return [...document.querySelectorAll('aside .item')].some(e=>e.title===%r)" % path)
    s.js("[...document.querySelectorAll('aside .item')].find(e=>e.title===arguments[0]).click()", path)


def tab(s, name):
    s.click(s.wait("nav button", text=name))


def on_failure(s):
    try:
        shot(s, "zz-failure")
        print("[failure] page text:", s.js("return document.body.innerText")[:1500])
    finally:
        s.quit()


# --- run 1 -------------------------------------------------------------------
s = Session(app)
_orig_wait = s.wait


def wait_or_dump(*a, **k):
    try:
        return _orig_wait(*a, **k)
    except TimeoutError:
        on_failure(s)
        raise


s.wait = wait_or_dump
s.wait("aside h1")
print(f"[start] session+first paint in {s.started_in:.2f}s; idle tree PSS {tree_pss_mb()} MB")
shot(s, "01-empty")

open_worktree(s, wt_ui)
wait_status(s, wt_ui, {"ready"})
print(f"[agent] ready; tree PSS {tree_pss_mb()} MB")

prompt = s.wait(".agent form textarea")
s.type(prompt, "Create a file named hello.txt containing exactly the word hi. Do nothing else.")
s.click(s.wait(".agent form button[type=submit]"))
# Sessions inherit the user's Claude permission mode: depending on it, the write
# either asks for permission or is approved without asking.
outcome = s.wait_js(
    "return document.querySelector('.permission') ? 'permission'"
    " : (['finished','error'].includes(document.querySelector('aside li.active .status')?.textContent) ? 'done' : null)",
    timeout=120,
)
if outcome == "permission":
    print("[agent] permission prompt shown:", s.text(s.wait(".permission")).replace("\n", " | "))
    shot(s, "02-permission")
    s.click(s.wait(".permission button.allow_once"))
else:
    print("[agent] no permission prompt (approved by the user's Claude permission mode)")
wait_status(s, wt_ui, {"finished", "error"})
print("[agent] turn ended:", status_of(s, wt_ui), "| hello.txt exists:", os.path.exists(os.path.join(wt_ui, "hello.txt")))
print(f"[agent] busy tree PSS {tree_pss_mb()} MB")
shot(s, "03-finished")

tab(s, "Review")
s.wait(".files button", text="hello.txt")
shot(s, "04-review-small")

open_worktree(s, wt_big)
wait_status(s, wt_big, {"ready"})
tab(s, "Review")
metric = s.wait(".metric", timeout=60, text="rendered")
print("[diff]", s.text(metric), "| files:", [s.text(e) for e in s.find(".files button")])
# Scroll through the whole merge view to see whether it stays responsive.
t0 = time.monotonic()
s.js("""const sc=document.querySelector('.diff'); for (let i=0;i<40;i++){ sc.scrollTop += 2000; } return sc.scrollTop;""")
print(f"[diff] scripted scroll of 40 steps took {time.monotonic() - t0:.2f}s")
shot(s, "05-review-big")

# Switch back: the first work item's transcript should still be there.
select_item(s, wt_ui)
tab(s, "Agent")
n = len(s.find(".log .msg, .log .tool"))
print(f"[switch] back to wt-ui, {n} transcript entries visible")
print(f"[mem] two agents tree PSS {tree_pss_mb()} MB")
s.quit()
time.sleep(2)
print("[quit] app processes left:", subprocess.run(["pgrep", "-a", "-f", "claude-agent-acp"], capture_output=True, text=True).stdout.strip() or "none")

# --- run 2: restart ------------------------------------------------------------
s = Session(app)
s.wait("aside h1")
items = s.js("return [...document.querySelectorAll('aside .item')].map(e=>e.title)")
print(f"[restart] started in {s.started_in:.2f}s, restored items: {items}")
if wt_ui not in (items or []):
    open_worktree(s, wt_ui)
else:
    select_item(s, wt_ui)
wait_status(s, wt_ui, {"ready"})
tab(s, "Agent")
entries = s.js("return [...document.querySelectorAll('.log .msg, .log .tool')].map(e=>e.className.split(' ')[1]+': '+e.textContent.trim().slice(0,60))")
print("[restart] replayed transcript:", entries)
prompt = s.wait(".agent form textarea")
s.type(prompt, "Reply with only the name of the file you created earlier in this conversation.")
s.click(s.wait(".agent form button[type=submit]"))
wait_status(s, wt_ui, {"finished", "error"})
reply = s.js("const m=[...document.querySelectorAll('.log .msg.agent')]; return m.length ? m[m.length-1].textContent : null")
print("[restart] agent reply:", reply)
shot(s, "06-resumed")
s.quit()
