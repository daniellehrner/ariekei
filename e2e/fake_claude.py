#!/usr/bin/env -S python3 -I
"""A stand-in for `claude`, for E2E tests (point ARIEKEI_CLAUDE at this file).

With -p it writes a review guide: reads the guide prompt on stdin and answers in
`--output-format json` form: the changed files (from the diff) in one high-risk group, the PR
title as an implemented claim backed by the first file, a claim nothing implements, and the
last file as unexplained.

Without -p it is a minimal terminal agent that reports through the hooks of `--settings`
like Claude Code does: each line typed is a prompt (UserPromptSubmit), answered with Stop;
a line containing "ask" asks for permission to run a Bash command instead (PermissionRequest).
"""

import json
import re
import sys
import urllib.request


def interactive(argv):
    settings = json.load(open(argv[argv.index("--settings") + 1]))
    url = settings["hooks"]["Stop"][0]["hooks"][0]["url"]
    flag = "--session-id" if "--session-id" in argv else "--resume"
    session = argv[argv.index(flag) + 1]

    def hook(name, **fields):
        body = json.dumps({"hook_event_name": name, "session_id": session, "cwd": ".", **fields}).encode()
        req = urllib.request.Request(url, body, {"Content-Type": "application/json"})
        urllib.request.urlopen(req, timeout=5).read()

    print("\x1b[1mfake claude\x1b[0m — type a prompt; a line with \"ask\" asks for permission")
    while True:
        sys.stdout.write("\x1b[38;5;173m>\x1b[0m ")
        sys.stdout.flush()
        line = sys.stdin.readline()
        if not line:
            hook("SessionEnd")
            return
        hook("UserPromptSubmit")
        if "ask" in line:
            command = "./gradlew :ethereum:eth:test --tests '*SnapSync*'"
            print(f"Bash({command})\n  Do you want to proceed?  1. Yes  2. No")
            hook("PermissionRequest", tool_name="Bash", tool_input={"command": command})
        else:
            print("Done.")
            hook("Stop")


if "-p" not in sys.argv[1:]:
    interactive(sys.argv[1:])
    sys.exit(0)
prompt = sys.stdin.read()
files = list(dict.fromkeys(re.findall(r"^diff --git a/(\S+) b/", prompt, re.M)))
title = re.search(r"^# Title\n(.*)$", prompt, re.M)
guide = {
    "focus": "Check the parser change first: it alters how lines are split.",
    "groups": [{"title": "Parser", "intent": "Splits lines differently.", "risk": "high", "files": files}],
    "claims": [
        {"claim": title.group(1) if title else "the change", "status": "implemented", "files": files[:1]},
        {"claim": "Adds metrics", "status": "missing", "files": []},
    ],
    "unexplained": files[-1:] if len(files) > 1 else [],
}
print(json.dumps({"type": "result", "subtype": "success", "is_error": False, "structured_output": guide}))
