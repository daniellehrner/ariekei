#!/usr/bin/env -S python3 -I
"""A stand-in for `claude`, for E2E tests (point ARIEKEI_CLAUDE at this file).

With -p it writes a review guide: reads the guide prompt on stdin and answers in
`--output-format json` form: the changed files (from the diff) in one high-risk group, the PR
title as an implemented claim backed by the first file, a claim nothing implements, and the
last file as unexplained.

Without -p it is a minimal terminal agent that reports through the hooks of `--settings` like
Claude Code does (UserPromptSubmit, PermissionRequest, PostToolUse, Stop, SessionEnd):
- a `❯` prompt with "? for shortcuts", and `/code-review` offered while a `/` command is typed;
- "Append a line with the word W to F, then commit with the message 'M'" asks permission to
  run git (Enter allows), then appends, commits and finishes the turn;
- a line containing "ask" asks permission for a Bash command and waits;
- anything else finishes the turn at once.
Turns are kept in a transcript ($FAKE_CLAUDE_DIR, default <tmp>/ariekei-fake-claude), which
`--resume <session>` prints again. Works in a Unix PTY and in a Windows ConPTY.
"""

import json
import os
import re
import subprocess
import sys
import tempfile
import time
import urllib.request

PROMPT = "\x1b[38;5;173m❯\x1b[0m "


def guide():
    prompt = sys.stdin.read()
    files = list(dict.fromkeys(re.findall(r"^diff --git a/(\S+) b/", prompt, re.M)))
    title = re.search(r"^# Title\n(.*)$", prompt, re.M)
    result = {
        "focus": "Check the parser change first: it alters how lines are split.",
        "groups": [{"title": "Parser", "intent": "Splits lines differently.", "risk": "high", "files": files}],
        "claims": [
            {"claim": title.group(1) if title else "the change", "status": "implemented", "files": files[:1]},
            {"claim": "Adds metrics", "status": "missing", "files": []},
        ],
        "unexplained": files[-1:] if len(files) > 1 else [],
    }
    print(json.dumps({"type": "result", "subtype": "success", "is_error": False, "structured_output": result}))


class Keys:
    """Key presses from the terminal, without echo: characters, "\\r", "\\b", "esc"."""

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
            c = self.msvcrt.getwch()
            if c in ("\x00", "\xe0"):
                self.msvcrt.getwch()  # arrow and function keys: ignored
                return ""
            return {"\x08": "\b", "\x1b": "esc", "\x03": "\x03", "\x04": "\x04"}.get(c, c)
        if not self.pending:
            data = os.read(self.fd, 1024).decode(errors="replace")
            if not data:
                return "\x04"
            data = re.sub(r"\x1b\[[0-9;]*[A-Za-z]", "", data)  # arrows etc.: ignored
            self.pending = list(data)
        c = self.pending.pop(0)
        return {"\x7f": "\b", "\x1b": "esc", "\n": "\r"}.get(c, c)

    def restore(self):
        if os.name != "nt":
            import termios

            termios.tcsetattr(self.fd, termios.TCSADRAIN, self.saved)


def out(text=""):
    sys.stdout.write(text.replace("\n", "\r\n"))
    sys.stdout.flush()


def interactive(argv):
    settings = json.load(open(argv[argv.index("--settings") + 1]))
    url = settings["hooks"]["Stop"][0]["hooks"][0]["url"]
    resume = "--resume" in argv
    session = argv[argv.index("--resume" if resume else "--session-id") + 1]
    store = os.environ.get("FAKE_CLAUDE_DIR") or os.path.join(tempfile.gettempdir(), "ariekei-fake-claude")
    os.makedirs(store, exist_ok=True)
    transcript = os.path.join(store, f"{session}.txt")

    def hook(name, **fields):
        body = json.dumps({"hook_event_name": name, "session_id": session, "cwd": os.getcwd(),
                           "transcript_path": transcript, **fields}).encode()
        try:
            req = urllib.request.Request(url, body, {"Content-Type": "application/json"})
            urllib.request.urlopen(req, timeout=5).read()
        except OSError:
            pass  # the app is gone or not listening; the TUI goes on

    def say(text):
        out(text + "\n")
        with open(transcript, "a", encoding="utf-8") as f:
            f.write(text + "\n")

    keys = Keys()
    try:
        out("\x1b[1mfake claude\x1b[0m\n")
        if resume and os.path.exists(transcript):
            out(open(transcript, encoding="utf-8").read())
            with open(os.path.join(store, "fake-claude.log"), "a", encoding="utf-8") as log:
                log.write(f"{time.strftime('%H:%M:%S')} resumed {session}, printed the transcript\n")
        line = ""
        while True:
            out("\r\x1b[2K" + PROMPT + line + "\x1b7\n\x1b[2K  " +
                ("/code-review   Review the change" if line.startswith("/") else "? for shortcuts") + "\x1b8")
            c = keys.read()
            if c in ("\x03", "\x04"):
                hook("SessionEnd")
                return
            if c == "\b":
                line = line[:-1]
            elif c == "esc":
                line = ""
            elif c == "\r":
                text, line = line.strip(), ""
                out("\x1b7\n\x1b[2K\x1b8\n")
                if text:
                    turn(text, say, hook, keys)
            elif c >= " ":
                line += c
    finally:
        keys.restore()


def turn(text, say, hook, keys):
    say(f"> {text}")
    hook("UserPromptSubmit", prompt=text)
    time.sleep(0.8)  # a turn takes a moment, as the real one does
    task = re.search(r"word (\S+) to (\S+?),? then commit with the message '([^']+)'", text)
    if task:
        word, path, message = task.groups()
        command = f"git commit -am '{message}'"
        out(f"Bash({command})\n  Do you want to proceed?\n  ❯ 1. Yes\n    2. No\n")
        hook("PermissionRequest", tool_name="Bash", tool_input={"command": command})
        while (c := keys.read()) not in ("\r", "esc", "\x03", "\x04"):
            pass
        if c != "\r":
            say("Not run.")
            hook("Stop")
            return
        hook("PreToolUse", tool_name="Bash", tool_input={"command": command})
        time.sleep(0.8)
        with open(path, "a", encoding="utf-8") as f:
            f.write(word + "\n")
        git = ["git", "-c", "user.name=agent", "-c", "user.email=agent@example.com", "-c", "commit.gpgsign=false"]
        subprocess.run([*git, "add", path], check=True, capture_output=True)
        subprocess.run([*git, "commit", "-qm", message], check=True, capture_output=True)
        hook("PostToolUse", tool_name="Bash", tool_input={"command": command})
        say(f"⏺ Committed {message}")
    elif "ask" in text:
        command = "./gradlew :ethereum:eth:test --tests '*SnapSync*'"
        out(f"Bash({command})\n  Do you want to proceed?\n  ❯ 1. Yes\n    2. No\n")
        hook("PermissionRequest", tool_name="Bash", tool_input={"command": command})
        return  # waits like the real one; the next prompt goes on
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


if "-p" in sys.argv[1:]:
    guide()
else:
    logged(interactive, sys.argv[1:])
