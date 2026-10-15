#!/usr/bin/env -S python3 -I
"""A stand-in for the `gh` CLI, for E2E tests (point ARIEKEI_GH at this file).

Serves the PRs described in $FAKE_GH_DIR/data.json and appends every write (REST POSTs and
GraphQL mutations) to $FAKE_GH_DIR/writes.jsonl, so a test can check what would have been
sent to GitHub. Supports only the calls Ariekei makes.

data.json: {"viewer": "me", "upstream": <path of the "GitHub" git repo, optional>,
            "prs": [{number, title, author, head_ref, head_sha, base_ref, state, requested (bool),
                     draft (bool), decision, ci, failing, body_html, comments: [...], threads: [...]}],
            "issues": [{number, title, author, body, assigned (bool), state}]}
Creating a PR or an issue (POST .../pulls, .../issues) adds it to data.json, so later polls
see it; a test can set a PR's "state" to MERGED in data.json. A PR or issue with "repo": "<name>" belongs to
owner/<name>; without it, to owner/proj.
"""

import json
import os
import re
import subprocess
import sys
import time
import zlib

DIR = os.environ["FAKE_GH_DIR"]
DATA = json.load(open(os.path.join(DIR, "data.json")))
ME = DATA["viewer"]


def record(entry):
    with open(os.path.join(DIR, "writes.jsonl"), "a") as f:
        f.write(json.dumps(entry) + "\n")


def nodes(items):
    return {"nodes": items}


def save():
    with open(os.path.join(DIR, "data.json"), "w") as f:
        json.dump(DATA, f)


def head_sha(pr):
    upstream = DATA.get("upstream")
    if pr.get("head_sha") or not upstream:
        return pr.get("head_sha", "")
    out = subprocess.run(["git", "-C", upstream, "rev-parse", pr["head_ref"]], capture_output=True, text=True)
    return out.stdout.strip()


def issue_summary(i):
    return {
        "number": i["number"], "title": i["title"], "url": f"https://github.com/owner/proj/issues/{i['number']}",
        "state": i.get("state", "OPEN"), "updatedAt": "2026-10-13T09:00:00Z", "author": {"login": i["author"]},
        "labels": nodes([{"name": "bug"}]), "comments": {"totalCount": 0, "nodes": []},
        "createdAt": "2026-10-12T09:00:00Z", "body": i.get("body", ""), "bodyHTML": f"<p>{i.get('body', '')}</p>",
    }


def checks(pr):
    """CI: "ci" is the rollup state (default SUCCESS); "failing" names the failed check."""
    state = pr.get("ci", "SUCCESS")
    runs = [{"__typename": "CheckRun", "name": "build", "conclusion": "SUCCESS", "status": "COMPLETED", "detailsUrl": "https://example.com/build"}]
    if state == "FAILURE":
        runs.append({"__typename": "CheckRun", "name": pr.get("failing", "unit-tests"), "conclusion": "FAILURE", "status": "COMPLETED", "detailsUrl": ""})
    if state == "PENDING":
        runs.append({"__typename": "CheckRun", "name": "integration-tests", "conclusion": None, "status": "IN_PROGRESS", "detailsUrl": ""})
    return nodes([{"commit": {"statusCheckRollup": {"state": state, "contexts": nodes(runs)}}}])


def updated_at(pr):
    """Moves on whenever the PR changes (its data or its head), as GitHub's updatedAt does:
    the poller fetches a PR's details only when it changed. A change gets a later time than
    any before it ($FAKE_GH_DIR/updated.json keeps them)."""
    path = os.path.join(DIR, "updated.json")
    try:
        seen = json.load(open(path))
    except (OSError, ValueError):
        seen = {}
    # Like GitHub's, it does not move when only the checks change.
    changes = {k: v for k, v in pr.items() if k not in ("ci", "failing")}
    key, fingerprint = f"{pr.get('repo', 'proj')}#{pr['number']}", zlib.crc32((json.dumps(changes, sort_keys=True) + head_sha(pr)).encode())
    if key not in seen or seen[key][0] != fingerprint:
        seen[key] = [fingerprint, max([t for _, t in seen.values()] + [0]) + 1]
        tmp = f"{path}.{os.getpid()}"
        with open(tmp, "w") as f:
            json.dump(seen, f)
        os.replace(tmp, path)
    minutes = seen[key][1]
    return f"2026-10-13T{10 + minutes // 60:02d}:{minutes % 60:02d}:00Z"


def summary(pr):
    return {
        "number": pr["number"], "title": pr["title"], "url": f"https://github.com/owner/proj/pull/{pr['number']}",
        "state": pr.get("state", "OPEN"), "isDraft": pr.get("draft", False), "updatedAt": updated_at(pr),
        "author": {"login": pr["author"]}, "headRefName": pr["head_ref"], "headRefOid": head_sha(pr),
        "baseRefName": pr["base_ref"], "reviewDecision": pr.get("decision", "REVIEW_REQUIRED"),
        "headRepository": {"name": "proj", "owner": {"login": "owner"}, "sshUrl": "git@github.com:owner/proj.git", "url": "https://github.com/owner/proj"},
        "reviews": nodes([]), "comments": nodes([]),
        "reviewThreads": nodes([{"isResolved": t.get("resolved", False), "comments": nodes([])} for t in pr.get("threads", [])]),
        "commits": checks(pr),
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
            "comments": nodes([{"databaseId": t["comment_id"], "author": {"login": t["author"]}, "body": t.get("body", ""), "bodyHTML": t["html"],
                                "createdAt": "2026-10-11T11:00:00Z", "diffHunk": t.get("diff_hunk", ""), "url": ""}]),
        } for t in pr.get("threads", [])]),
    })
    return c


def pr_commits(pr):
    """The PR's commits in the upstream repo; a commit's git author and committer names are
    their GitHub logins."""
    upstream = DATA.get("upstream")
    if not upstream:
        return {"pageInfo": {"hasNextPage": False}, "nodes": []}
    log = subprocess.run(["git", "-C", upstream, "log", "--format=%H%x1f%an%x1f%cn", f"{pr.get('base_ref', 'main')}..{pr['head_ref']}"],
                         capture_output=True, text=True).stdout.splitlines()
    user = lambda login: {"user": {"login": login}}
    commits = [line.split("\x1f") for line in log]
    return {"pageInfo": {"hasNextPage": False},
            "nodes": [{"commit": {"oid": sha, "author": user(author), "committer": user(committer)}} for sha, author, committer in commits]}


def of_repo(items, name):
    return [i for i in items if i.get("repo", "proj") == name]


def graphql(fields):
    query = fields.get("query", "")
    in_q = re.search(r"repo:[^/\s]+/(\S+)", fields.get("q", ""))
    repo = fields.get("name") or (in_q.group(1) if in_q else "proj")
    prs, all_issues = of_repo(DATA["prs"], repo), of_repo(DATA.get("issues", []), repo)
    if query.lstrip().startswith("mutation"):
        record({"graphql": query.split("(")[1].split("{")[-1].strip() if "{" in query else query, "vars": {k: v for k, v in fields.items() if k != "query"}})
        return {"data": {}}
    if "defaultBranchRef" in query:
        return {"data": {"repository": {"defaultBranchRef": {"name": "main"}}}}
    if "search(" in query:
        q = fields["q"]
        if "is:issue" in q:
            found = [i for i in all_issues if i.get("assigned") and i.get("state", "OPEN") == "OPEN"]
            return {"data": {"search": {"nodes": [issue_summary(i) for i in found]}}}
        head = re.search(r"head:(\S+)", q)
        if head:
            found = [p for p in prs if p["head_ref"] == head.group(1) and p.get("state", "OPEN") == "OPEN"]
        elif f"author:{ME}" in q:
            found = [p for p in prs if p["author"] == ME and p.get("state", "OPEN") == "OPEN"]
        elif f"review-requested:{ME}" in q:
            found = [p for p in prs if p.get("requested") and p.get("state", "OPEN") == "OPEN"]
        else:
            words = [w for w in q.split() if not re.match(r"^(repo|is|sort):", w)]
            found = [p for p in prs if all(w.lower() in (p["title"] + " " + p["author"] + " " + str(p["number"])).lower() for w in words)]
        return {"data": {"search": {"nodes": [summary(p) for p in found]}}}
    by_number = {p["number"]: p for p in prs}
    issues = {i["number"]: i for i in all_issues}
    if "issue(number: $number)" in query:
        return {"data": {"repository": {"issue": issue_summary(issues[int(fields["number"])])}}}
    issue_aliases = re.findall(r"i(\d+): issue\(number: (\d+)\)", query)
    if issue_aliases:
        return lookups("i", "Issue", [int(n) for n, _ in issue_aliases], issues, issue_summary)
    if "commits(first: 100, after: $after)" in query:
        return {"data": {"repository": {"pullRequest": {"commits": pr_commits(by_number[int(fields["number"])])}}}}
    if "pullRequest(number: $number)" in query:
        return {"data": {"repository": {"pullRequest": conversation(by_number[int(fields["number"])])}}}
    aliases = re.findall(r"p(\d+): pullRequest\(number: (\d+)\)", query)
    return lookups("p", "PullRequest", [int(n) for n, _ in aliases], by_number, summary)


def lookups(prefix, kind, numbers, known, render):
    """Aliased lookups by number, answered as GitHub does: an unknown number is null, with a
    NOT_FOUND error (and gh exits 1)."""
    answer = {"data": {"repository": {f"{prefix}{n}": render(known[n]) if n in known else None for n in numbers}}}
    missing = [n for n in numbers if n not in known]
    if missing:
        answer["errors"] = [{"type": "NOT_FOUND", "path": ["repository", f"{prefix}{n}"],
                             "message": f"Could not resolve to a {kind} with the number of {n}."} for n in missing]
    return answer


def main(argv):
    # While $FAKE_GH_DIR/rate-limited exists, GitHub's API budget is used up (resetting in two
    # minutes); `gh api rate_limit` itself always answers, as on GitHub.
    if argv[:2] == ["api", "rate_limit"]:
        limited = os.path.exists(os.path.join(DIR, "rate-limited"))
        reset = int(time.time()) + 120
        print(json.dumps({"resources": {r: {"limit": 5000, "remaining": 0 if limited else 5000, "reset": reset} for r in ("graphql", "core")}}))
        return 0
    if os.path.exists(os.path.join(DIR, "rate-limited")):
        print("gh: API rate limit exceeded for user ID 1.", file=sys.stderr)
        return 1
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
        answer = graphql(fields)
        print(json.dumps(answer))
        if answer.get("errors"):
            print(f"gh: {answer['errors'][0]['message']}", file=sys.stderr)
            return 1
        return 0
    if argv[0] == "api" and "--method" in argv and "--input" in argv:
        path = next(a for a in argv[1:] if "/" in a)
        body = json.loads(sys.stdin.read() or "{}")
        record({"post": path, "body": body})
        # PRs and issues share their numbers, as on GitHub.
        number = max([i["number"] for i in DATA["prs"] + DATA.get("issues", [])] + [99]) + 1
        if path.endswith("/issues"):
            DATA.setdefault("issues", []).append({"number": number, "title": body["title"], "author": ME, "body": body.get("body", "")})
            save()
            print(json.dumps({"number": number, "html_url": f"https://github.com/owner/proj/issues/{number}"}))
            return 0
        if path.endswith("/pulls"):
            DATA["prs"].append({"number": number, "title": body["title"], "author": ME, "head_ref": body["head"].split(":")[-1],
                                "base_ref": body["base"], "state": "OPEN", "body_html": body.get("body", "")})
            save()
            print(json.dumps({"number": number, "html_url": f"https://github.com/owner/proj/pull/{number}"}))
            return 0
        print(json.dumps({"html_url": f"https://github.com/{path}"}))
        return 0
    if argv[0] == "api" and "/actions/jobs/" in argv[1]:
        return 0
    print(f"fake gh: unsupported call {argv}", file=sys.stderr)
    return 1


sys.exit(main(sys.argv[1:]))
