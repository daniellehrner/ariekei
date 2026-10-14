#!/usr/bin/env -S python3 -I
"""A stand-in for `claude -p` that writes a review guide, for E2E tests (point ARIEKEI_CLAUDE
at this file). Reads the guide prompt on stdin and answers in `--output-format json` form: the
changed files (from the diff) in one high-risk group, the PR title as an implemented claim
backed by the first file, a claim nothing implements, and the last file as unexplained.
Not a terminal agent: only the headless guide call is supported."""

import json
import re
import sys

if "-p" not in sys.argv[1:]:
    print("fake claude: only -p is supported", file=sys.stderr)
    sys.exit(1)
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
