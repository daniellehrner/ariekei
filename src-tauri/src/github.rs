//! GitHub through the `gh` CLI: reuses the user's login (and any GitHub host it knows),
//! so Ariekei never handles tokens itself.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::io::Write as _;
use std::process::{Command, Stdio};

/// The `gh` binary; `ARIEKEI_GH` replaces it (tests use a fake).
fn gh_command() -> Command {
    Command::new(std::env::var_os("ARIEKEI_GH").unwrap_or_else(|| "gh".into()))
}

fn gh(args: &[&str]) -> Result<Vec<u8>, String> {
    gh_with_input(args, None)
}

/// Runs `gh`, optionally writing `input` to its stdin (for `--input -`).
fn gh_with_input(args: &[&str], input: Option<&[u8]>) -> Result<Vec<u8>, String> {
    let mut child = gh_command()
        .args(args)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to run gh (is the GitHub CLI installed?): {e}"))?;
    if let (Some(data), Some(mut stdin)) = (input, child.stdin.take()) {
        stdin.write_all(data).map_err(|e| e.to_string())?;
    }
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!(
            "gh {} failed: {}",
            args.first().unwrap_or(&""),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(out.stdout)
}

/// Runs a GraphQL query; `strings` and `ints` are its variables.
fn graphql(query: &str, strings: &[(&str, &str)], ints: &[(&str, i64)]) -> Result<Value, String> {
    let mut args = vec![
        "api".to_owned(),
        "graphql".to_owned(),
        "-f".to_owned(),
        format!("query={query}"),
    ];
    for (k, v) in strings {
        args.extend(["-f".to_owned(), format!("{k}={v}")]);
    }
    for (k, v) in ints {
        args.extend(["-F".to_owned(), format!("{k}={v}")]);
    }
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let v: Value = serde_json::from_slice(&gh(&args)?).map_err(|e| e.to_string())?;
    if let Some(errors) = v.get("errors") {
        return Err(format!("GitHub GraphQL error: {errors}"));
    }
    Ok(v)
}

pub fn viewer() -> Result<String, String> {
    let out = gh(&["api", "user", "--jq", ".login"])?;
    Ok(String::from_utf8_lossy(&out).trim().to_owned())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Review {
    pub author: String,
    pub state: String,
    pub submitted_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrSummary {
    pub number: i64,
    pub title: String,
    #[serde(default)]
    pub author: String,
    /// OPEN, CLOSED or MERGED.
    #[serde(default = "open_state")]
    pub state: String,
    pub url: String,
    pub is_draft: bool,
    pub updated_at: String,
    pub head_ref: String,
    pub head_sha: String,
    pub head_owner: String,
    pub head_repo: String,
    pub head_ssh_url: String,
    pub head_https_url: String,
    pub base_ref: String,
    /// APPROVED, CHANGES_REQUESTED, REVIEW_REQUIRED or none.
    pub review_decision: Option<String>,
    /// Check rollup of the head commit: SUCCESS, FAILURE, ERROR, PENDING, EXPECTED or none.
    pub ci: Option<String>,
    pub failing_checks: Vec<String>,
    pub unresolved_threads: i64,
    /// Newest review, comment or thread reply by someone other than the viewer.
    pub latest_activity: Option<String>,
    pub reviews: Vec<Review>,
}

fn open_state() -> String {
    "OPEN".into()
}

const PR_FIELDS: &str = r#"
  number title url state isDraft updatedAt author { login } headRefName headRefOid baseRefName reviewDecision
  headRepository { name owner { login } sshUrl url }
  reviews(last: 20) { nodes { author { login } state submittedAt } }
  comments(last: 20) { nodes { author { login } createdAt } }
  reviewThreads(first: 100) { nodes { isResolved comments(last: 1) { nodes { author { login } createdAt } } } }
  commits(last: 1) { nodes { commit { statusCheckRollup { state contexts(first: 100) { nodes {
    __typename
    ... on CheckRun { name conclusion status detailsUrl databaseId }
    ... on StatusContext { context state targetUrl description }
  } } } } } }
"#;

/// Open PRs in `owner/name` matching GitHub search `terms`, most recently updated first.
/// `viewer` decides what counts as activity by others.
pub fn search_prs(
    owner: &str,
    name: &str,
    viewer: &str,
    terms: &str,
) -> Result<Vec<PrSummary>, String> {
    let query = format!(
        "query($q: String!) {{ search(query: $q, type: ISSUE, first: 50) {{ nodes {{ ... on PullRequest {{ {PR_FIELDS} }} }} }} }}"
    );
    let q = format!("repo:{owner}/{name} is:pr is:open {terms} sort:updated-desc");
    let v = graphql(&query, &[("q", &q)], &[])?;
    Ok(parse_prs(&v["data"]["search"]["nodes"], viewer))
}

/// PRs by number (any state), e.g. ones the user opened from search.
pub fn prs_by_number(
    owner: &str,
    name: &str,
    viewer: &str,
    numbers: &[i64],
) -> Result<Vec<PrSummary>, String> {
    if numbers.is_empty() {
        return Ok(vec![]);
    }
    let fields: String = numbers
        .iter()
        .map(|n| format!("p{n}: pullRequest(number: {n}) {{ {PR_FIELDS} }}"))
        .collect::<Vec<_>>()
        .join("\n");
    let query = format!(
        "query($owner: String!, $name: String!) {{ repository(owner: $owner, name: $name) {{ {fields} }} }}"
    );
    let v = graphql(&query, &[("owner", owner), ("name", name)], &[])?;
    let found: Vec<Value> = numbers
        .iter()
        .map(|n| v["data"]["repository"][format!("p{n}")].clone())
        .collect();
    Ok(parse_prs(&Value::Array(found), viewer))
}

fn s(v: &Value) -> String {
    v.as_str().unwrap_or_default().to_owned()
}

fn nodes(v: &Value) -> &[Value] {
    v["nodes"].as_array().map(Vec::as_slice).unwrap_or_default()
}

const FAILED: [&str; 6] = [
    "FAILURE",
    "TIMED_OUT",
    "CANCELLED",
    "ACTION_REQUIRED",
    "STARTUP_FAILURE",
    "ERROR",
];

pub fn parse_prs(list: &Value, viewer: &str) -> Vec<PrSummary> {
    let by_other = |n: &Value| n["author"]["login"].as_str().is_some_and(|a| a != viewer);
    list.as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter(|pr| pr["number"].is_i64())
        .map(|pr| {
            let rollup = &pr["commits"]["nodes"][0]["commit"]["statusCheckRollup"];
            let failing_checks = nodes(&rollup["contexts"])
                .iter()
                .filter(|c| {
                    let outcome = c["conclusion"]
                        .as_str()
                        .or(c["state"].as_str())
                        .unwrap_or("");
                    FAILED.contains(&outcome)
                })
                .map(|c| {
                    c["name"]
                        .as_str()
                        .or(c["context"].as_str())
                        .unwrap_or("?")
                        .to_owned()
                })
                .collect();
            let threads = nodes(&pr["reviewThreads"]);
            let activity = nodes(&pr["reviews"])
                .iter()
                .filter(|r| by_other(r))
                .filter_map(|r| r["submittedAt"].as_str())
                .chain(
                    nodes(&pr["comments"])
                        .iter()
                        .filter(|c| by_other(c))
                        .filter_map(|c| c["createdAt"].as_str()),
                )
                .chain(
                    threads
                        .iter()
                        .flat_map(|t| nodes(&t["comments"]))
                        .filter(|c| by_other(c))
                        .filter_map(|c| c["createdAt"].as_str()),
                )
                .max()
                .map(str::to_owned);
            let head = &pr["headRepository"];
            PrSummary {
                number: pr["number"].as_i64().unwrap_or_default(),
                title: s(&pr["title"]),
                author: s(&pr["author"]["login"]),
                state: pr["state"].as_str().unwrap_or("OPEN").to_owned(),
                url: s(&pr["url"]),
                is_draft: pr["isDraft"].as_bool().unwrap_or_default(),
                updated_at: s(&pr["updatedAt"]),
                head_ref: s(&pr["headRefName"]),
                head_sha: s(&pr["headRefOid"]),
                head_owner: s(&head["owner"]["login"]),
                head_repo: s(&head["name"]),
                head_ssh_url: s(&head["sshUrl"]),
                head_https_url: s(&head["url"]),
                base_ref: s(&pr["baseRefName"]),
                review_decision: pr["reviewDecision"].as_str().map(str::to_owned),
                ci: rollup["state"].as_str().map(str::to_owned),
                failing_checks,
                unresolved_threads: threads.iter().filter(|t| t["isResolved"] == false).count()
                    as i64,
                latest_activity: activity,
                reviews: nodes(&pr["reviews"])
                    .iter()
                    .map(|r| Review {
                        author: s(&r["author"]["login"]),
                        state: s(&r["state"]),
                        submitted_at: s(&r["submittedAt"]),
                    })
                    .collect(),
            }
        })
        .collect()
}

// --- PR context for the agent ----------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ThreadComment {
    pub author: String,
    pub body: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Thread {
    pub path: String,
    pub line: Option<i64>,
    pub outdated: bool,
    pub comments: Vec<ThreadComment>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FailingCheck {
    pub name: String,
    pub url: String,
    /// GitHub Actions job id, when the check is an Actions job (its log can be fetched).
    pub job_id: Option<i64>,
    pub summary: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PrDetail {
    pub title: String,
    pub body: String,
    pub url: String,
    pub head_ref: String,
    pub base_ref: String,
    pub unresolved: Vec<Thread>,
    /// Review bodies (top-level review text), oldest first.
    pub reviews: Vec<(String, String, String)>,
    pub failing: Vec<FailingCheck>,
}

/// Everything shown about one PR: its description, conversation, review threads and
/// checks. GitHub renders the markdown (`bodyHTML`); the UI sanitizes it.
const CONVERSATION: &str = r#"query($owner: String!, $name: String!, $number: Int!) {
  repository(owner: $owner, name: $name) { pullRequest(number: $number) {
    number title url state isDraft createdAt body bodyHTML headRefName headRefOid baseRefName reviewDecision
    author { login }
    labels(first: 20) { nodes { name color } }
    reviewRequests(first: 20) { nodes { requestedReviewer { __typename ... on User { login } ... on Team { name } } } }
    comments(first: 100) { nodes { databaseId author { login } bodyHTML createdAt url } }
    reviews(last: 50) { nodes { author { login } state body bodyHTML submittedAt } }
    reviewThreads(first: 100) { nodes { id isResolved isOutdated path line originalLine
      comments(first: 50) { nodes { databaseId author { login } body bodyHTML createdAt diffHunk url } } } }
    commits(last: 1) { nodes { commit { statusCheckRollup { state contexts(first: 100) { nodes {
      __typename
      ... on CheckRun { name conclusion status detailsUrl databaseId title summary }
      ... on StatusContext { context state targetUrl description }
    } } } } } }
  } } }"#;

fn conversation_json(owner: &str, name: &str, number: i64) -> Result<Value, String> {
    let v = graphql(
        CONVERSATION,
        &[("owner", owner), ("name", name)],
        &[("number", number)],
    )?;
    Ok(v["data"]["repository"]["pullRequest"].clone())
}

pub fn pr_detail(owner: &str, name: &str, number: i64) -> Result<PrDetail, String> {
    Ok(parse_detail(&conversation_json(owner, name, number)?))
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Comment {
    /// REST id (replies and links need it).
    pub id: i64,
    pub author: String,
    pub body_html: String,
    pub created_at: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReviewEntry {
    pub author: String,
    pub state: String,
    pub body_html: String,
    pub submitted_at: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ThreadView {
    /// GraphQL node id (resolve/unresolve need it).
    pub id: String,
    pub path: String,
    pub line: Option<i64>,
    pub resolved: bool,
    pub outdated: bool,
    /// The diff around the commented line, from the thread's first comment.
    pub diff_hunk: String,
    pub comments: Vec<Comment>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Check {
    pub name: String,
    /// SUCCESS, FAILURE, … or the status of a check that has not finished.
    pub outcome: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Conversation {
    pub number: i64,
    pub title: String,
    pub url: String,
    pub author: String,
    pub state: String,
    pub is_draft: bool,
    pub created_at: String,
    pub head_ref: String,
    pub head_sha: String,
    pub base_ref: String,
    pub review_decision: Option<String>,
    pub body_html: String,
    pub labels: Vec<String>,
    pub requested_reviewers: Vec<String>,
    pub comments: Vec<Comment>,
    pub reviews: Vec<ReviewEntry>,
    pub threads: Vec<ThreadView>,
    pub checks: Vec<Check>,
}

pub fn conversation(owner: &str, name: &str, number: i64) -> Result<Conversation, String> {
    Ok(parse_conversation(&conversation_json(owner, name, number)?))
}

fn comment(c: &Value) -> Comment {
    Comment {
        id: c["databaseId"].as_i64().unwrap_or_default(),
        author: s(&c["author"]["login"]),
        body_html: s(&c["bodyHTML"]),
        created_at: s(&c["createdAt"]),
        url: s(&c["url"]),
    }
}

pub fn parse_conversation(pr: &Value) -> Conversation {
    let threads = nodes(&pr["reviewThreads"])
        .iter()
        .map(|t| ThreadView {
            id: s(&t["id"]),
            path: s(&t["path"]),
            line: t["line"].as_i64().or(t["originalLine"].as_i64()),
            resolved: t["isResolved"].as_bool().unwrap_or_default(),
            outdated: t["isOutdated"].as_bool().unwrap_or_default(),
            diff_hunk: s(&t["comments"]["nodes"][0]["diffHunk"]),
            comments: nodes(&t["comments"]).iter().map(comment).collect(),
        })
        .collect();
    let checks = nodes(&pr["commits"]["nodes"][0]["commit"]["statusCheckRollup"]["contexts"])
        .iter()
        .map(|c| Check {
            name: c["name"]
                .as_str()
                .or(c["context"].as_str())
                .unwrap_or("?")
                .to_owned(),
            outcome: c["conclusion"]
                .as_str()
                .or(c["state"].as_str())
                .or(c["status"].as_str())
                .unwrap_or("?")
                .to_owned(),
            url: c["detailsUrl"]
                .as_str()
                .or(c["targetUrl"].as_str())
                .unwrap_or_default()
                .to_owned(),
        })
        .collect();
    Conversation {
        number: pr["number"].as_i64().unwrap_or_default(),
        title: s(&pr["title"]),
        url: s(&pr["url"]),
        author: s(&pr["author"]["login"]),
        state: s(&pr["state"]),
        is_draft: pr["isDraft"].as_bool().unwrap_or_default(),
        created_at: s(&pr["createdAt"]),
        head_ref: s(&pr["headRefName"]),
        head_sha: s(&pr["headRefOid"]),
        base_ref: s(&pr["baseRefName"]),
        review_decision: pr["reviewDecision"].as_str().map(str::to_owned),
        body_html: s(&pr["bodyHTML"]),
        labels: nodes(&pr["labels"]).iter().map(|l| s(&l["name"])).collect(),
        requested_reviewers: nodes(&pr["reviewRequests"])
            .iter()
            .filter_map(|r| {
                let who = &r["requestedReviewer"];
                who["login"]
                    .as_str()
                    .or(who["name"].as_str())
                    .map(str::to_owned)
            })
            .collect(),
        comments: nodes(&pr["comments"]).iter().map(comment).collect(),
        reviews: nodes(&pr["reviews"])
            .iter()
            .map(|r| ReviewEntry {
                author: s(&r["author"]["login"]),
                state: s(&r["state"]),
                body_html: s(&r["bodyHTML"]),
                submitted_at: s(&r["submittedAt"]),
            })
            .collect(),
        threads,
        checks,
    }
}

// --- writing to GitHub (only on the user's explicit action) ---------------------

/// A line comment of a review: `line` (and `start_line` for a range) in the file at
/// the review's commit, on the given diff side ("RIGHT" for new code).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineComment {
    pub path: String,
    pub line: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_line: Option<u32>,
    pub side: String,
    pub body: String,
}

/// The JSON body for `POST /repos/{o}/{r}/pulls/{n}/reviews`.
pub fn review_payload(commit_id: &str, event: &str, body: &str, comments: &[LineComment]) -> Value {
    let comments: Vec<Value> = comments
        .iter()
        .map(|c| {
            let mut v = json!({ "path": c.path, "line": c.line, "side": c.side, "body": c.body });
            if let Some(start) = c.start_line.filter(|s| *s < c.line) {
                v["start_line"] = json!(start);
                v["start_side"] = json!(c.side);
            }
            v
        })
        .collect();
    json!({ "commit_id": commit_id, "event": event, "body": body, "comments": comments })
}

fn post(path: &str, payload: &Value) -> Result<Value, String> {
    let out = gh_with_input(
        &["api", "--method", "POST", path, "--input", "-"],
        Some(payload.to_string().as_bytes()),
    )?;
    serde_json::from_slice(&out).map_err(|e| e.to_string())
}

/// Submits a review (event COMMENT, APPROVE or REQUEST_CHANGES); returns its URL.
pub fn submit_review(
    owner: &str,
    name: &str,
    number: i64,
    payload: &Value,
) -> Result<String, String> {
    let v = post(
        &format!("repos/{owner}/{name}/pulls/{number}/reviews"),
        payload,
    )?;
    Ok(s(&v["html_url"]))
}

/// Replies to the review thread that `comment_id` belongs to.
pub fn reply(
    owner: &str,
    name: &str,
    number: i64,
    comment_id: i64,
    body: &str,
) -> Result<(), String> {
    post(
        &format!("repos/{owner}/{name}/pulls/{number}/comments/{comment_id}/replies"),
        &json!({ "body": body }),
    )
    .map(|_| ())
}

/// Adds a comment to the PR's conversation.
pub fn comment_on(owner: &str, name: &str, number: i64, body: &str) -> Result<(), String> {
    post(
        &format!("repos/{owner}/{name}/issues/{number}/comments"),
        &json!({ "body": body }),
    )
    .map(|_| ())
}

pub fn resolve_thread(thread_id: &str, resolved: bool) -> Result<(), String> {
    let mutation = if resolved {
        "resolveReviewThread"
    } else {
        "unresolveReviewThread"
    };
    let query = format!(
        "mutation($id: ID!) {{ {mutation}(input: {{ threadId: $id }}) {{ thread {{ isResolved }} }} }}"
    );
    graphql(&query, &[("id", thread_id)], &[]).map(|_| ())
}

pub fn parse_detail(pr: &Value) -> PrDetail {
    let unresolved = nodes(&pr["reviewThreads"])
        .iter()
        .filter(|t| t["isResolved"] == false)
        .map(|t| Thread {
            path: s(&t["path"]),
            line: t["line"].as_i64().or(t["originalLine"].as_i64()),
            outdated: t["isOutdated"].as_bool().unwrap_or_default(),
            comments: nodes(&t["comments"])
                .iter()
                .map(|c| ThreadComment {
                    author: s(&c["author"]["login"]),
                    body: s(&c["body"]),
                })
                .collect(),
        })
        .collect();
    let reviews = nodes(&pr["reviews"])
        .iter()
        .filter(|r| r["body"].as_str().is_some_and(|b| !b.trim().is_empty()))
        .map(|r| (s(&r["author"]["login"]), s(&r["state"]), s(&r["body"])))
        .collect();
    let failing = nodes(&pr["commits"]["nodes"][0]["commit"]["statusCheckRollup"]["contexts"])
        .iter()
        .filter(|c| {
            FAILED.contains(
                &c["conclusion"]
                    .as_str()
                    .or(c["state"].as_str())
                    .unwrap_or(""),
            )
        })
        .map(|c| FailingCheck {
            name: c["name"]
                .as_str()
                .or(c["context"].as_str())
                .unwrap_or("?")
                .to_owned(),
            url: c["detailsUrl"]
                .as_str()
                .or(c["targetUrl"].as_str())
                .unwrap_or_default()
                .to_owned(),
            job_id: (c["__typename"] == "CheckRun")
                .then(|| c["databaseId"].as_i64())
                .flatten(),
            summary: c["title"]
                .as_str()
                .or(c["description"].as_str())
                .unwrap_or_default()
                .to_owned(),
        })
        .collect();
    PrDetail {
        title: s(&pr["title"]),
        body: s(&pr["body"]),
        url: s(&pr["url"]),
        head_ref: s(&pr["headRefName"]),
        base_ref: s(&pr["baseRefName"]),
        unresolved,
        reviews,
        failing,
    }
}

/// The part of a GitHub Actions job log that shows the failure: up to `lines` lines
/// ending at the last `##[error]` line (or before the post-job cleanup), without the
/// timestamp prefix.
pub fn job_log_tail(owner: &str, name: &str, job_id: i64, lines: usize) -> Result<String, String> {
    let out = gh(&[
        "api",
        &format!("repos/{owner}/{name}/actions/jobs/{job_id}/logs"),
    ])?;
    Ok(failure_excerpt(&String::from_utf8_lossy(&out), lines))
}

fn failure_excerpt(log: &str, lines: usize) -> String {
    let strip = |l: &str| -> String {
        match l.split_once(' ') {
            // Lines look like "2026-10-13T10:00:00.1234567Z message".
            Some((ts, rest))
                if ts.len() > 20 && ts.ends_with('Z') && ts.as_bytes().get(4) == Some(&b'-') =>
            {
                rest.to_owned()
            }
            _ => l.to_owned(),
        }
    };
    let all: Vec<String> = log.lines().map(strip).collect();
    let end = match all.iter().rposition(|l| l.contains("##[error]")) {
        Some(i) => i + 1,
        None => all
            .iter()
            .position(|l| l.contains("Post job cleanup"))
            .unwrap_or(all.len()),
    };
    all[end.saturating_sub(lines)..end].join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture() -> Value {
        json!([{
            "number": 11574, "title": "Notifier", "url": "https://github.com/o/r/pull/11574",
            "isDraft": false, "updatedAt": "2026-10-13T10:00:00Z",
            "headRefName": "introduce-notifier", "headRefOid": "abc", "baseRefName": "main",
            "reviewDecision": "CHANGES_REQUESTED",
            "headRepository": { "name": "besu", "owner": { "login": "me" }, "sshUrl": "git@github.com:me/besu.git", "url": "https://github.com/me/besu" },
            "reviews": { "nodes": [
                { "author": { "login": "alice" }, "state": "CHANGES_REQUESTED", "submittedAt": "2026-10-12T09:00:00Z" },
                { "author": { "login": "me" }, "state": "COMMENTED", "submittedAt": "2026-10-13T09:00:00Z" }
            ] },
            "comments": { "nodes": [ { "author": { "login": "bob" }, "createdAt": "2026-10-12T12:00:00Z" } ] },
            "reviewThreads": { "nodes": [
                { "isResolved": false, "comments": { "nodes": [ { "author": { "login": "alice" }, "createdAt": "2026-10-12T15:00:00Z" } ] } },
                { "isResolved": true, "comments": { "nodes": [ { "author": { "login": "carol" }, "createdAt": "2026-10-13T08:00:00Z" } ] } },
                { "isResolved": false, "comments": { "nodes": [ { "author": { "login": "me" }, "createdAt": "2026-10-13T11:00:00Z" } ] } }
            ] },
            "commits": { "nodes": [ { "commit": { "statusCheckRollup": { "state": "FAILURE", "contexts": { "nodes": [
                { "__typename": "CheckRun", "name": "unit-tests", "conclusion": "FAILURE", "status": "COMPLETED", "databaseId": 42 },
                { "__typename": "CheckRun", "name": "spotless", "conclusion": "SUCCESS", "status": "COMPLETED" },
                { "__typename": "StatusContext", "context": "DCO", "state": "ERROR" }
            ] } } } } ] }
        }, {}])
    }

    #[test]
    fn parses_pr_summary() {
        let prs = parse_prs(&fixture(), "me");
        assert_eq!(prs.len(), 1, "non-PR search results are skipped");
        let pr = &prs[0];
        assert_eq!(
            (pr.number, pr.head_owner.as_str(), pr.head_ref.as_str()),
            (11574, "me", "introduce-notifier")
        );
        assert_eq!(pr.ci.as_deref(), Some("FAILURE"));
        assert_eq!(pr.failing_checks, ["unit-tests", "DCO"]);
        assert_eq!(pr.unresolved_threads, 2);
        assert_eq!(pr.review_decision.as_deref(), Some("CHANGES_REQUESTED"));
        // The viewer's own activity (09:00 and 11:00 on the 13th) does not count.
        assert_eq!(pr.latest_activity.as_deref(), Some("2026-10-13T08:00:00Z"));
    }

    #[test]
    fn summaries_carry_author_and_state() {
        let mut list = fixture();
        list[0]["author"] = json!({ "login": "alice" });
        list[0]["state"] = json!("OPEN");
        let pr = &parse_prs(&list, "me")[0];
        assert_eq!((pr.author.as_str(), pr.state.as_str()), ("alice", "OPEN"));
        // Snapshots stored before these fields existed still load.
        let mut old = serde_json::to_value(pr).unwrap();
        old.as_object_mut().unwrap().remove("author");
        old.as_object_mut().unwrap().remove("state");
        let loaded: PrSummary = serde_json::from_value(old).unwrap();
        assert_eq!(
            (loaded.author.as_str(), loaded.state.as_str()),
            ("", "OPEN")
        );
    }

    #[test]
    fn parses_conversation() {
        let pr = json!({
            "number": 9, "title": "T", "url": "U", "state": "OPEN", "isDraft": true, "createdAt": "2026-10-01T00:00:00Z",
            "bodyHTML": "<p>Hi</p>", "headRefName": "h", "headRefOid": "abc", "baseRefName": "main", "reviewDecision": null,
            "author": { "login": "alice" },
            "labels": { "nodes": [ { "name": "perf" } ] },
            "reviewRequests": { "nodes": [ { "requestedReviewer": { "__typename": "User", "login": "me" } },
                                           { "requestedReviewer": { "__typename": "Team", "name": "core" } } ] },
            "comments": { "nodes": [ { "databaseId": 1, "author": { "login": "bob" }, "bodyHTML": "<p>LGTM?</p>", "createdAt": "2026-10-02T00:00:00Z", "url": "C1" } ] },
            "reviews": { "nodes": [ { "author": { "login": "bob" }, "state": "COMMENTED", "bodyHTML": "", "submittedAt": "2026-10-02T01:00:00Z" } ] },
            "reviewThreads": { "nodes": [ { "id": "T1", "isResolved": true, "isOutdated": false, "path": "A.java", "line": null, "originalLine": 3,
                "comments": { "nodes": [ { "databaseId": 5, "author": { "login": "bob" }, "bodyHTML": "<p>x</p>", "createdAt": "c", "diffHunk": "@@ -1 +1 @@", "url": "C5" } ] } } ] },
            "commits": { "nodes": [ { "commit": { "statusCheckRollup": { "contexts": { "nodes": [
                { "__typename": "CheckRun", "name": "build", "conclusion": null, "status": "IN_PROGRESS", "detailsUrl": "D" } ] } } } } ] }
        });
        let c = parse_conversation(&pr);
        assert_eq!(
            (c.author.as_str(), c.is_draft, c.head_sha.as_str()),
            ("alice", true, "abc")
        );
        assert_eq!(c.labels, ["perf"]);
        assert_eq!(c.requested_reviewers, ["me", "core"]);
        assert_eq!(
            (c.comments[0].id, c.comments[0].body_html.as_str()),
            (1, "<p>LGTM?</p>")
        );
        let t = &c.threads[0];
        assert_eq!(
            (t.id.as_str(), t.line, t.resolved, t.diff_hunk.as_str()),
            ("T1", Some(3), true, "@@ -1 +1 @@")
        );
        assert_eq!(t.comments[0].id, 5);
        assert_eq!(
            (c.checks[0].name.as_str(), c.checks[0].outcome.as_str()),
            ("build", "IN_PROGRESS")
        );
    }

    #[test]
    fn review_payload_has_line_and_range_comments() {
        let comments = [
            LineComment {
                path: "A.java".into(),
                line: 12,
                start_line: None,
                side: "RIGHT".into(),
                body: "one".into(),
            },
            LineComment {
                path: "B.java".into(),
                line: 9,
                start_line: Some(7),
                side: "RIGHT".into(),
                body: "range".into(),
            },
            LineComment {
                path: "C.java".into(),
                line: 4,
                start_line: Some(4),
                side: "RIGHT".into(),
                body: "same".into(),
            },
        ];
        let p = review_payload("abc", "REQUEST_CHANGES", "Summary", &comments);
        assert_eq!(p["commit_id"], "abc");
        assert_eq!(p["event"], "REQUEST_CHANGES");
        assert_eq!(
            p["comments"][0],
            json!({ "path": "A.java", "line": 12, "side": "RIGHT", "body": "one" })
        );
        assert_eq!(
            (
                p["comments"][1]["start_line"].as_i64(),
                p["comments"][1]["start_side"].as_str()
            ),
            (Some(7), Some("RIGHT"))
        );
        assert!(
            p["comments"][2].get("start_line").is_none(),
            "a one-line range is a plain line comment"
        );
    }

    #[test]
    fn log_excerpt_ends_at_the_last_error() {
        let log = "2026-10-13T10:00:00.0000000Z a\n2026-10-13T10:00:01.0000000Z FooTest > bar FAILED\n\
                   2026-10-13T10:00:02.0000000Z ##[error]Process completed with exit code 1.\n\
                   2026-10-13T10:00:03.0000000Z Post job cleanup.\n2026-10-13T10:00:04.0000000Z Cleaning up orphan processes";
        assert_eq!(
            failure_excerpt(log, 2),
            "FooTest > bar FAILED\n##[error]Process completed with exit code 1."
        );
        assert_eq!(
            failure_excerpt("x\nPost job cleanup.\ny", 5),
            "x",
            "no error line: stop before cleanup"
        );
    }

    #[test]
    fn parses_pr_detail() {
        let pr = json!({
            "title": "T", "body": "B", "url": "U", "headRefName": "h", "baseRefName": "main",
            "reviews": { "nodes": [ { "author": { "login": "a" }, "state": "COMMENTED", "body": "" },
                                    { "author": { "login": "b" }, "state": "CHANGES_REQUESTED", "body": "Please split" } ] },
            "reviewThreads": { "nodes": [
                { "isResolved": false, "isOutdated": false, "path": "src/A.java", "line": 12, "originalLine": 10,
                  "comments": { "nodes": [ { "author": { "login": "b" }, "body": "null check?" } ] } },
                { "isResolved": false, "isOutdated": true, "path": "src/B.java", "line": null, "originalLine": 5,
                  "comments": { "nodes": [] } },
                { "isResolved": true, "path": "x", "comments": { "nodes": [] } }
            ] },
            "commits": { "nodes": [ { "commit": { "statusCheckRollup": { "contexts": { "nodes": [
                { "__typename": "CheckRun", "name": "test", "conclusion": "FAILURE", "detailsUrl": "D", "databaseId": 7, "title": "2 failed" },
                { "__typename": "StatusContext", "context": "ext", "state": "FAILURE", "targetUrl": "E", "description": "boom" }
            ] } } } } ] }
        });
        let d = parse_detail(&pr);
        assert_eq!(d.unresolved.len(), 2);
        assert_eq!(
            (d.unresolved[0].path.as_str(), d.unresolved[0].line),
            ("src/A.java", Some(12))
        );
        assert_eq!(
            (d.unresolved[1].line, d.unresolved[1].outdated),
            (Some(5), true)
        );
        assert_eq!(
            d.reviews,
            [(
                "b".into(),
                "CHANGES_REQUESTED".into(),
                "Please split".into()
            )]
        );
        assert_eq!(d.failing[0].job_id, Some(7));
        assert_eq!(
            (d.failing[1].job_id, d.failing[1].summary.as_str()),
            (None, "boom")
        );
    }
}

/// Read-only checks against the real GitHub API: `ARIEKEI_LIVE_REPO=owner/name cargo test -- --ignored live`.
#[cfg(test)]
mod live {
    use super::*;

    fn repo() -> (String, String) {
        let slug = std::env::var("ARIEKEI_LIVE_REPO").expect("set ARIEKEI_LIVE_REPO=owner/name");
        let (o, n) = slug.split_once('/').expect("owner/name");
        (o.to_owned(), n.to_owned())
    }

    #[test]
    #[ignore]
    fn live_open_prs_and_detail() {
        let (owner, name) = repo();
        let me = viewer().unwrap();
        let prs = search_prs(&owner, &name, &me, &format!("author:{me}")).unwrap();
        let requested = search_prs(&owner, &name, &me, &format!("review-requested:{me}")).unwrap();
        println!("{} PRs request my review", requested.len());
        let others = search_prs(&owner, &name, &me, &format!("-author:{me}")).unwrap();
        println!(
            "{} open PRs by others, e.g. {:?}",
            others.len(),
            others.first().map(|p| (p.number, &p.author))
        );
        if let Some(o) = others.first() {
            let c = conversation(&owner, &name, o.number).unwrap();
            println!(
                "conversation #{}: {} chars of description, {} comments, {} reviews, {} threads, {} checks",
                c.number,
                c.body_html.len(),
                c.comments.len(),
                c.reviews.len(),
                c.threads.len(),
                c.checks.len()
            );
            let by_number = prs_by_number(&owner, &name, &me, &[o.number]).unwrap();
            assert_eq!(by_number[0].number, o.number);
        }
        println!("{} open PRs by {me}", prs.len());
        for p in prs.iter().take(5) {
            println!(
                "#{} ci={:?} failing={:?} decision={:?} unresolved={} activity={:?} head={}:{}",
                p.number,
                p.ci,
                p.failing_checks,
                p.review_decision,
                p.unresolved_threads,
                p.latest_activity,
                p.head_owner,
                p.head_ref
            );
        }
        if let Some(p) = prs.first() {
            let d = pr_detail(&owner, &name, p.number).unwrap();
            println!(
                "detail #{}: {} unresolved, {} reviews with text, {} failing",
                p.number,
                d.unresolved.len(),
                d.reviews.len(),
                d.failing.len()
            );
        }
        let failing = prs.iter().find(|p| !p.failing_checks.is_empty());
        if let Some(p) = failing {
            let d = pr_detail(&owner, &name, p.number).unwrap();
            if let Some(job) = d.failing.iter().find_map(|c| c.job_id) {
                let log = job_log_tail(&owner, &name, job, 5).unwrap();
                println!("log tail of #{} job {job}:\n{log}", p.number);
            }
        }
    }
}
