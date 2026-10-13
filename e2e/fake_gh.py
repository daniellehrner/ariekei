#!/usr/bin/env -S python3 -I
"""A stand-in for the `gh` CLI, for E2E tests (point ARIEKEI_GH at this file).

Serves the PRs described in $FAKE_GH_DIR/data.json and appends every write (REST POSTs and
GraphQL mutations) to $FAKE_GH_DIR/writes.jsonl, so a test can check what would have been
sent to GitHub. Supports only the calls Ariekei makes.

data.json: {"viewer": "me", "prs": [{number, title, author, head_ref, head_sha, base_ref,
            requested (bool), body_html, comments: [...], threads: [...]}]}
"""

import json
import os
import re
import sys

DIR = os.environ["FAKE_GH_DIR"]
DATA = json.load(open(os.path.join(DIR, "data.json")))
ME = DATA["viewer"]


def record(entry):
    with open(os.path.join(DIR, "writes.jsonl"), "a") as f:
        f.write(json.dumps(entry) + "\n")


def nodes(items):
    return {"nodes": items}


def summary(pr):
    return {
        "number": pr["number"], "title": pr["title"], "url": f"https://github.com/owner/proj/pull/{pr['number']}",
        "state": pr.get("state", "OPEN"), "isDraft": False, "updatedAt": "2026-10-13T10:00:00Z",
        "author": {"login": pr["author"]}, "headRefName": pr["head_ref"], "headRefOid": pr["head_sha"],
        "baseRefName": pr["base_ref"], "reviewDecision": "REVIEW_REQUIRED",
        "headRepository": {"name": "proj", "owner": {"login": "owner"}, "sshUrl": "git@github.com:owner/proj.git", "url": "https://github.com/owner/proj"},
        "reviews": nodes([]), "comments": nodes([]),
        "reviewThreads": nodes([{"isResolved": t.get("resolved", False), "comments": nodes([])} for t in pr.get("threads", [])]),
        "commits": nodes([{"commit": {"statusCheckRollup": {"state": "SUCCESS", "contexts": nodes([
            {"__typename": "CheckRun", "name": "build", "conclusion": "SUCCESS", "status": "COMPLETED", "detailsUrl": "https://example.com/build"}])}}}]),
    }


def conversation(pr):
    c = summary(pr)
    c.update({
        "createdAt": "2026-10-10T09:00:00Z", "body": "", "bodyHTML": pr.get("body_html", ""),
        "labels": nodes([{"name": "review"}]), "reviewRequests": nodes([]),
        "comments": nodes([{"databaseId": i + 1, "author": {"login": cm["author"]}, "bodyHTML": cm["html"], "createdAt": "2026-10-11T10:00:00Z", "url": ""} for i, cm in enumerate(pr.get("comments", []))]),
        "reviews": nodes([]),
        "reviewThreads": nodes([{
            "id": t["id"], "isResolved": t.get("resolved", False), "isOutdated": False, "path": t["path"], "line": t["line"], "originalLine": t["line"],
            "comments": nodes([{"databaseId": t["comment_id"], "author": {"login": t["author"]}, "body": "", "bodyHTML": t["html"],
                                "createdAt": "2026-10-11T11:00:00Z", "diffHunk": t.get("diff_hunk", ""), "url": ""}]),
        } for t in pr.get("threads", [])]),
    })
    return c


def graphql(fields):
    query = fields.get("query", "")
    if query.lstrip().startswith("mutation"):
        record({"graphql": query.split("(")[1].split("{")[-1].strip() if "{" in query else query, "vars": {k: v for k, v in fields.items() if k != "query"}})
        return {"data": {}}
    if "search(" in query:
        q = fields["q"]
        if f"author:{ME}" in q:
            found = [p for p in DATA["prs"] if p["author"] == ME]
        elif f"review-requested:{ME}" in q:
            found = [p for p in DATA["prs"] if p.get("requested")]
        else:
            words = [w for w in q.split() if not re.match(r"^(repo|is|sort):", w)]
            found = [p for p in DATA["prs"] if all(w.lower() in (p["title"] + " " + p["author"] + " " + str(p["number"])).lower() for w in words)]
        return {"data": {"search": {"nodes": [summary(p) for p in found]}}}
    by_number = {p["number"]: p for p in DATA["prs"]}
    if "pullRequest(number: $number)" in query:
        return {"data": {"repository": {"pullRequest": conversation(by_number[int(fields["number"])])}}}
    aliases = re.findall(r"p(\d+): pullRequest\(number: (\d+)\)", query)
    return {"data": {"repository": {f"p{n}": summary(by_number[int(n)]) for n, _ in aliases if int(n) in by_number}}}


def main(argv):
    if argv[:2] == ["api", "user"]:
        print(ME)
        return 0
    if argv[:2] == ["api", "graphql"]:
        fields, i = {}, 2
        while i < len(argv):
            if argv[i] in ("-f", "-F"):
                k, _, v = argv[i + 1].partition("=")
                fields[k] = v
                i += 2
            else:
                i += 1
        print(json.dumps(graphql(fields)))
        return 0
    if argv[0] == "api" and "--method" in argv and "--input" in argv:
        path = next(a for a in argv[1:] if "/" in a)
        body = json.loads(sys.stdin.read() or "{}")
        record({"post": path, "body": body})
        print(json.dumps({"html_url": f"https://github.com/{path}"}))
        return 0
    if argv[0] == "api" and "/actions/jobs/" in argv[1]:
        return 0
    print(f"fake gh: unsupported call {argv}", file=sys.stderr)
    return 1


sys.exit(main(sys.argv[1:]))
