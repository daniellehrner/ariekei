#!/usr/bin/env -S python3 -I
# Copyright contributors to Ariekei.
# SPDX-License-Identifier: Apache-2.0

"""A stand-in for `claude`, for E2E tests (point ARIEKEI_CLAUDE at this file).

A minimal terminal agent that reports through the hooks of `--settings` like Claude Code does (UserPromptSubmit, PreToolUse, PermissionRequest, PostToolUse, Stop, SessionEnd), with its spinner redrawn while it works:
- a `❯` prompt with "? for shortcuts", and `/code-review` offered while a `/` command is typed;
- "Append a line with the word W to F, then commit with the message 'M'" asks permission to
  run git (Enter allows; Esc says No, which ends the turn with no hook, as in Claude Code), then
  appends, commits and finishes the turn;
- "Run a N second job" asks permission to run it (likewise), then works for N seconds;
- "Open a pull request titled 'T'" pushes the branch and opens the PR, "Open an issue titled
  'T'" opens the issue (both through $ARIEKEI_GH, the fake gh), reporting them as the
  `gh pr create` / `gh issue create` Bash call it stands for, with the URL it printed;
- "Run a N second job in the background" starts a process that sleeps N seconds, as a
  background Bash call does (a child of the agent), and finishes the turn;
- "Exit with code N" crashes: exits with N at once, without SessionEnd;
- with Ariekei's MCP server (`--mcp-config`), "Draft a review comment on P line N: T" drafts
  one with its `add_review_comment` tool, and the Review tab's message about review comment #N
  with "say 'T'" in its prompt changes the draft to T (`update_review_comment`);
- a line containing "ask" asks permission for a Bash command and waits;
- `/clear` ends the conversation (SessionEnd, reason "clear") and goes on in a new one, with a
  new session id;
- anything else finishes the turn at once.
Hook requests carry the hook's `headers`, with the `allowedEnvVars` filled in, as Claude Code's do.
Turns are kept in a transcript ($FAKE_CLAUDE_DIR, default <tmp>/ariekei-fake-claude), which
`--resume <session>` prints again, and as Claude Code keeps them (JSON lines: the prompts as
user messages, the rest as the assistant's text), which the hooks name as `transcript_path`. Works in a Unix PTY and in a Windows ConPTY.
"""

import os
import time

# First of all, for CI runs where a resumed agent seemed never to start (Windows): note that
# Python itself is up, before any other import.
_store = os.environ.get("FAKE_CLAUDE_DIR") or os.path.join(os.environ.get("TEMP", "/tmp"), "ariekei-fake-claude")
os.makedirs(_store, exist_ok=True)
with open(os.path.join(_store, "fake-claude.log"), "a", encoding="utf-8") as _log:
    _log.write(f"{time.strftime('%H:%M:%S')} python up (pid {os.getpid()})\n")

import json  # noqa: E402
import re  # noqa: E402
import subprocess  # noqa: E402
import sys  # noqa: E402
import tempfile  # noqa: E402
import threading  # noqa: E402
import urllib.request  # noqa: E402
import uuid  # noqa: E402

PROMPT = "\x1b[38;5;173m❯\x1b[0m "


class Keys:
    """Key presses from the terminal, without echo: characters, "\\r", "\\b", "esc". A bracketed
    paste (ESC[200~ … ESC[201~, how Ariekei sends text to the agent) arrives as its characters,
    line breaks as spaces (one prompt, as Claude Code takes a paste)."""

    def __init__(self):
        if os.name == "nt":
            import ctypes
            import msvcrt

            self.msvcrt = msvcrt
            kernel32 = ctypes.windll.kernel32
            out = kernel32.GetStdHandle(-11)
            mode = ctypes.c_uint32()
            kernel32.GetConsoleMode(out, ctypes.byref(mode))
            kernel32.SetConsoleMode(out, mode.value | 0x0004)  # ENABLE_VIRTUAL_TERMINAL_PROCESSING
            kernel32.SetConsoleOutputCP(65001)
        else:
            import termios
            import tty

            self.fd = sys.stdin.fileno()
            self.saved = termios.tcgetattr(self.fd)
            tty.setraw(self.fd)
        self.pending = []

    def read(self):
        if os.name == "nt":
            if self.pending:
                return self.pending.pop(0)
            c = self.msvcrt.getwch()
            if c in ("\x00", "\xe0"):
                self.msvcrt.getwch()  # arrow and function keys: ignored
                return ""
            if c == "\x1b" and self.msvcrt.kbhit():
                seq = ""
                while self.msvcrt.kbhit() and len(seq) < 5:
                    seq += self.msvcrt.getwch()
                if seq != "[200~":
                    return ""  # another escape sequence: ignored
                text = ""
                while not text.endswith("\x1b[201~"):
                    text += self.msvcrt.getwch()
                self.pending = list(pasted(text[:-6]))
                return self.pending.pop(0) if self.pending else ""
            return {"\x08": "\b", "\x1b": "esc", "\x03": "\x03", "\x04": "\x04"}.get(c, c)
        if not self.pending:
            data = os.read(self.fd, 65536).decode(errors="replace")
            if not data:
                return "\x04"
            while "\x1b[200~" in data and "\x1b[201~" not in data:  # the rest of a long paste
                data += os.read(self.fd, 65536).decode(errors="replace")
            data = re.sub(r"\x1b\[200~(.*?)\x1b\[201~", lambda m: pasted(m.group(1)), data, flags=re.S)
            data = re.sub(r"\x1b\[[0-9;]*[A-Za-z]", "", data)  # arrows etc.: ignored
            self.pending = list(data)
        c = self.pending.pop(0)
        return {"\x7f": "\b", "\x1b": "esc", "\n": "\r"}.get(c, c)

    def restore(self):
        if os.name != "nt":
            import termios

            termios.tcsetattr(self.fd, termios.TCSADRAIN, self.saved)


def pasted(text):
    """A pasted text as one line of the prompt."""
    return re.sub(r"\s*[\r\n]+\s*", " ", text).strip()


def out(text=""):
    sys.stdout.write(text.replace("\n", "\r\n"))
    sys.stdout.flush()


def interactive(argv):
    settings = json.load(open(argv[argv.index("--settings") + 1]))
    config = settings["hooks"]["Stop"][0]["hooks"][0]
    url = config["url"]
    # Header values with $VAR filled in from the environment, for the variables allowed.
    allowed = config.get("allowedEnvVars", [])
    headers = {k: re.sub(r"\$\{?(\w+)\}?", lambda m: os.environ.get(m.group(1), "") if m.group(1) in allowed else "", v)
               for k, v in config.get("headers", {}).items()}
    # Ariekei's MCP server (--mcp-config), its headers' ${VAR} filled in as Claude Code does.
    mcp = None
    if "--mcp-config" in argv:
        server = json.load(open(argv[argv.index("--mcp-config") + 1]))["mcpServers"]["ariekei"]
        mcp = {"url": server["url"], "headers": {k: re.sub(r"\$\{(\w+)\}", lambda m: os.environ.get(m.group(1), ""), v)
                                                 for k, v in server.get("headers", {}).items()}}
    resume = "--resume" in argv
    store = os.environ.get("FAKE_CLAUDE_DIR") or os.path.join(tempfile.gettempdir(), "ariekei-fake-claude")
    os.makedirs(store, exist_ok=True)
    # The conversation: `/clear` starts a new one, with a new session id and transcript.
    conv = {"session": argv[argv.index("--resume" if resume else "--session-id") + 1]}

    def transcript():
        return os.path.join(store, f"{conv['session']}.txt")

    def jsonl():
        return os.path.join(store, f"{conv['session']}.jsonl")

    def hook(name, **fields):
        body = json.dumps({"hook_event_name": name, "session_id": conv["session"], "cwd": os.getcwd(),
                           "transcript_path": jsonl(), **fields}).encode()
        try:
            req = urllib.request.Request(url, body, {"Content-Type": "application/json", **headers})
            urllib.request.urlopen(req, timeout=5).read()
        except OSError:
            pass  # the app is gone or not listening; the TUI goes on

    def say(text):
        out(text + "\n")
        with open(transcript(), "a", encoding="utf-8") as f:
            f.write(text + "\n")
        prompt = text.startswith("> ")
        entry = {"type": "user", "message": {"content": text[2:]}} if prompt else \
            {"type": "assistant", "message": {"content": [{"type": "text", "text": text}]}}
        with open(jsonl(), "a", encoding="utf-8") as f:
            f.write(json.dumps(entry) + "\n")

    keys = Keys()
    try:
        out("\x1b[1mfake claude\x1b[0m\n")
        if resume and os.path.exists(transcript()):
            out(open(transcript(), encoding="utf-8").read())
            with open(os.path.join(store, "fake-claude.log"), "a", encoding="utf-8") as log:
                log.write(f"{time.strftime('%H:%M:%S')} resumed {conv['session']}, printed the transcript\n")
        line = ""
        while True:
            out("\r\x1b[2K" + PROMPT + line + "\x1b7\n\x1b[2K  " +
                ("/code-review   Review the change" if line.startswith("/") else "? for shortcuts") + "\x1b8")
            c = keys.read()
            if c in ("\x03", "\x04"):
                hook("SessionEnd", reason="prompt_input_exit")
                return
            if c == "\b":
                line = line[:-1]
            elif c == "esc":
                line = ""
            elif c == "\r":
                text, line = line.strip(), ""
                out("\x1b7\n\x1b[2K\x1b8\n")
                if text == "/clear":
                    hook("SessionEnd", reason="clear")
                    conv["session"] = str(uuid.uuid4())
                    out("\x1b[2J\x1b[H(no content)\n")
                elif text:
                    turn(text, say, hook, keys, mcp)
            elif c >= " ":
                line += c
    finally:
        keys.restore()


def busy(seconds):
    """Waits as a working Claude Code does: redrawing its spinner and timer all the while."""
    start = time.monotonic()
    frames = "·✢✳✶✻✽"
    while (spent := time.monotonic() - start) < seconds:
        out(f"\r\x1b[2K{frames[int(spent * 5) % len(frames)]} Working… ({int(spent)}s · esc to interrupt)")
        time.sleep(min(0.2, seconds - spent))
    out("\r\x1b[2K")


def allowed(command, hook, keys):
    """Asks to run `command` as Claude Code does (PreToolUse, then PermissionRequest) and waits
    for the answer: Enter allows. No (Esc) interrupts the turn, which Claude Code reports with
    no hook at all."""
    hook("PreToolUse", tool_name="Bash", tool_input={"command": command})
    out(f"Bash({command})\n  Do you want to proceed?\n  ❯ 1. Yes\n    2. No\n")
    hook("PermissionRequest", tool_name="Bash", tool_input={"command": command})
    while (c := keys.read()) not in ("\r", "esc", "\x03", "\x04"):
        pass
    if c != "\r":
        out("  ⎿  Interrupted · What should Claude do instead?\n")
    return c == "\r"


def tool(mcp, hook, name, args):
    """Calls a tool of Ariekei's MCP server as Claude Code does (initialize first) and returns
    its answer."""
    def post(message):
        req = urllib.request.Request(mcp["url"], json.dumps(message).encode(),
                                     {"Content-Type": "application/json", "Accept": "application/json, text/event-stream", **mcp["headers"]})
        with urllib.request.urlopen(req, timeout=10) as r:
            data = r.read()
        return json.loads(data) if data else None

    post({"jsonrpc": "2.0", "id": 1, "method": "initialize",
          "params": {"protocolVersion": "2025-11-25", "capabilities": {}, "clientInfo": {"name": "fake claude", "version": "0"}}})
    post({"jsonrpc": "2.0", "method": "notifications/initialized"})
    hook("PreToolUse", tool_name=f"mcp__ariekei__{name}", tool_input=args)
    result = post({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": name, "arguments": args}})["result"]
    answer = result["content"][0]["text"]
    hook("PostToolUseFailure" if result.get("isError") else "PostToolUse", tool_name=f"mcp__ariekei__{name}", tool_input=args,
         tool_response=answer)
    return answer


def turn(text, say, hook, keys, mcp=None):
    say(f"> {text}")
    hook("UserPromptSubmit", prompt=text)
    busy(0.8)  # a turn takes a moment, as the real one does
    drafting = re.search(r"draft a review comment on (\S+) line (\d+): (.+)", text, re.I)
    # The Review tab's message about one draft (lib/reviewComments.ts draftMessage).
    redraft = re.search(r"About review comment #(\d+) .*?say '([^']+)'", text)
    if mcp and drafting:
        path, line, body = drafting.groups()
        say("⏺ " + tool(mcp, hook, "add_review_comment", {"path": path, "line": int(line), "body": body}))
        hook("Stop")
        return
    if mcp and redraft:
        say("⏺ " + tool(mcp, hook, "update_review_comment", {"id": int(redraft.group(1)), "body": redraft.group(2)}))
        hook("Stop")
        return
    task = re.search(r"word (\S+) to (\S+?),? then commit with the message '([^']+)'", text)
    create = re.search(r"open (a pull request|an issue) titled '([^']+)'", text, re.I)
    if create:
        pr, title = create.group(1).lower() == "a pull request", create.group(2)
        command = f"gh {'pr' if pr else 'issue'} create --title '{title}' --body ''"
        hook("PreToolUse", tool_name="Bash", tool_input={"command": command})
        origin = subprocess.run(["git", "config", "remote.origin.url"], capture_output=True, text=True).stdout  # not rewritten by insteadOf
        slug = re.search(r"github\.com[:/](.+?)(?:\.git)?$", origin.strip()).group(1)
        body = {"title": title, "body": ""}
        if pr:
            branch = subprocess.run(["git", "rev-parse", "--abbrev-ref", "HEAD"], capture_output=True, text=True).stdout.strip()
            subprocess.run(["git", "push", "-q"], check=True, capture_output=True)
            body.update(head=branch, base="main")
        created = subprocess.run(
            [sys.executable, os.environ["ARIEKEI_GH"], "api", f"repos/{slug}/{'pulls' if pr else 'issues'}", "--method", "POST", "--input", "-"],
            input=json.dumps(body), capture_output=True, text=True, check=True)
        url = json.loads(created.stdout)["html_url"]
        hook("PostToolUse", tool_name="Bash", tool_input={"command": command},
             tool_response={"stdout": url + "\n", "stderr": "", "interrupted": False})
        say(f"⏺ Opened {url}")
    elif task:
        word, path, message = task.groups()
        command = f"git commit -am '{message}'"
        if not allowed(command, hook, keys):
            return
        busy(0.8)
        with open(path, "a", encoding="utf-8") as f:
            f.write(word + "\n")
        git = ["git", "-c", "user.name=agent", "-c", "user.email=agent@example.com", "-c", "commit.gpgsign=false"]
        subprocess.run([*git, "add", path], check=True, capture_output=True)
        subprocess.run([*git, "commit", "-qm", message], check=True, capture_output=True)
        hook("PostToolUse", tool_name="Bash", tool_input={"command": command})
        say(f"⏺ Committed {message}")
    elif made := re.search(r"make a worktree (\S+)( with a draft)?", text, re.I):
        # Next to the one it works in, on a new branch: as an agent trying something aside.
        name, draft = made.group(1), made.group(2)
        command = f"git worktree add -q -b {name} ../{name}"
        hook("PreToolUse", tool_name="Bash", tool_input={"command": command})
        path = os.path.join(os.path.dirname(os.getcwd()), name)
        subprocess.run(["git", "worktree", "add", "-q", "-b", name, path], check=True, capture_output=True)
        if draft:
            with open(os.path.join(path, "draft.txt"), "w", encoding="utf-8") as f:
                f.write("draft\n")
        hook("PostToolUse", tool_name="Bash", tool_input={"command": command},
             tool_response={"stdout": "", "stderr": "", "interrupted": False})
        say(f"⏺ Made the worktree ../{name}")
    elif job := re.search(r"(\d+) second job in the background", text):
        command = f"sleep {job.group(1)}"
        hook("PreToolUse", tool_name="Bash", tool_input={"command": command, "run_in_background": True})
        proc = subprocess.Popen([sys.executable, "-c", f"import time; time.sleep({job.group(1)})"])
        threading.Thread(target=proc.wait, daemon=True).start()  # reaped when it ends, as Claude Code does
        hook("PostToolUse", tool_name="Bash", tool_input={"command": command, "run_in_background": True})
        say(f"⏺ Started {command} in the background")
    elif job := re.search(r"(\d+) second job", text):
        command = f"sleep {job.group(1)}"
        if not allowed(command, hook, keys):
            return
        busy(int(job.group(1)))
        hook("PostToolUse", tool_name="Bash", tool_input={"command": command})
        say(f"⏺ Ran {command}")
    elif crash := re.search(r"exit with code (\d+)", text, re.I):
        out("fake claude crashed\n")
        os._exit(int(crash.group(1)))  # no SessionEnd, as a crash sends none
    elif "ask" in text:
        command = "./gradlew :ethereum:eth:test --tests '*SnapSync*'"
        hook("PreToolUse", tool_name="Bash", tool_input={"command": command})
        out(f"Bash({command})\n  Do you want to proceed?\n  ❯ 1. Yes\n    2. No\n")
        hook("PermissionRequest", tool_name="Bash", tool_input={"command": command})
        return  # waits; unlike the real one, the next prompt goes on
    else:
        say("⏺ Done.")
    hook("Stop")


def logged(f, argv):
    """Runs f, noting the start and any failure in <store>/fake-claude.log (for CI runs)."""
    store = os.environ.get("FAKE_CLAUDE_DIR") or os.path.join(tempfile.gettempdir(), "ariekei-fake-claude")
    os.makedirs(store, exist_ok=True)
    with open(os.path.join(store, "fake-claude.log"), "a", encoding="utf-8") as log:
        log.write(f"{time.strftime('%H:%M:%S')} start {argv}\n")
    try:
        f(argv)
    except BaseException:
        import traceback

        with open(os.path.join(store, "fake-claude.log"), "a", encoding="utf-8") as log:
            log.write(traceback.format_exc())
        raise


logged(interactive, sys.argv[1:])
