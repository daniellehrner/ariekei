// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

//! GitHub through the `gh` CLI: reuses the user's login (and any GitHub host it knows),
//! so Ariekei never handles tokens itself.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::io::Write as _;
use std::process::{Command, Stdio};

/// The `gh` binary; `ARIEKEI_GH` replaces it (tests use a fake).
fn gh_command() -> Command {
    #[cfg(test)]
    if let Some(dir) = FAKE_GH_DIR.with(|d| d.borrow().clone()) {
        let fake = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../e2e/fake_gh.py");
        let mut cmd = crate::tools::command(&fake);
        cmd.env("FAKE_GH_DIR", dir);
        return cmd;
    }
    match std::env::var_os("ARIEKEI_GH") {
        Some(fake) => crate::tools::command(std::path::Path::new(&fake)),
        None => crate::tools::quiet("gh"),
    }
}

// Tests: GitHub is e2e/fake_gh.py serving this directory, for calls on this thread (the
// process environment stays as it is, so tests run in parallel).
#[cfg(test)]
thread_local! {
    static FAKE_GH_DIR: std::cell::RefCell<Option<std::path::PathBuf>> = const { std::cell::RefCell::new(None) };
}

/// Makes this thread's GitHub the fake serving `dir` (`None`: the real one again).
#[cfg(test)]
pub fn use_fake(dir: Option<&std::path::Path>) {
    FAKE_GH_DIR.with(|d| *d.borrow_mut() = dir.map(std::path::Path::to_path_buf));
}

fn gh(args: &[&str]) -> Result<Vec<u8>, String> {
    gh_with_input(args, None)
}

/// Runs `gh`, optionally writing `input` to its stdin (for `--input -`).
fn gh_with_input(args: &[&str], input: Option<&[u8]>) -> Result<Vec<u8>, String> {
    let out = gh_output(args, input)?;
    if !out.status.success() {
        return Err(gh_failure(args, &out.stderr));
    }
    Ok(out.stdout)
}

fn gh_failure(args: &[&str], stderr: &[u8]) -> String {
    format!(
        "gh {} failed: {}",
        args.first().unwrap_or(&""),
        String::from_utf8_lossy(stderr).trim()
    )
}

/// Runs `gh` and returns its output whether or not it succeeded.
fn gh_output(args: &[&str], input: Option<&[u8]>) -> Result<std::process::Output, String> {
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
    child.wait_with_output().map_err(|e| e.to_string())
}

fn graphql_args(query: &str, strings: &[(&str, &str)], ints: &[(&str, i64)]) -> Vec<String> {
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
    args
}

/// Runs a GraphQL query; `strings` and `ints` are its variables.
fn graphql(query: &str, strings: &[(&str, &str)], ints: &[(&str, i64)]) -> Result<Value, String> {
    let args = graphql_args(query, strings, ints);
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let v: Value = serde_json::from_slice(&gh(&args)?).map_err(|e| e.to_string())?;
    if let Some(errors) = v.get("errors") {
        return Err(format!("GitHub GraphQL error: {errors}"));
    }
    Ok(v)
}

/// Runs a query of lookups by number (`p12: pullRequest(number: 12) { … }`). A number GitHub
/// does not know (a deleted PR, a transferred issue) comes back as null with a NOT_FOUND
/// error, and gh fails; here that number is only left out, or one such item would make
/// every poll of the repo fail.
fn graphql_lookups(query: &str, strings: &[(&str, &str)]) -> Result<Value, String> {
    let args = graphql_args(query, strings, &[]);
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let out = gh_output(&args, None)?;
    lookup_result(out.status.success(), &out.stdout)
        .ok_or_else(|| gh_failure(&args, &out.stderr))?
}

/// The outcome of a lookup query from gh's exit status and output: `None` when gh failed
/// without a GraphQL answer (e.g. no network), else the data, unless an error other than
/// NOT_FOUND of a looked-up item came with it (the repository itself not found is one).
fn lookup_result(success: bool, stdout: &[u8]) -> Option<Result<Value, String>> {
    let v: Value = match serde_json::from_slice(stdout) {
        Ok(v) => v,
        Err(e) if success => return Some(Err(e.to_string())),
        Err(_) => return None,
    };
    match v.get("errors").and_then(Value::as_array) {
        Some(errors)
            if errors.iter().any(|e| {
                e["type"] != "NOT_FOUND" || e["path"].as_array().is_none_or(|p| p.len() < 2)
            }) =>
        {
            Some(Err(format!("GitHub GraphQL error: {}", v["errors"])))
        }
        _ if v.get("data").is_none_or(Value::is_null) => {
            Some(Err("GitHub answered without data".to_owned()))
        }
        _ => Some(Ok(v)),
    }
}

/// When (Unix seconds) the GitHub rate limit that is used up resets: the later of the
/// GraphQL and REST ones that have nothing left. Asking costs nothing.
pub fn rate_limit_reset() -> Option<u64> {
    let out = gh(&["api", "rate_limit"]).ok()?;
    exhausted_reset(&serde_json::from_slice(&out).ok()?)
}

/// From `gh api rate_limit`'s answer: the reset of the used-up limits (GraphQL, REST).
fn exhausted_reset(v: &Value) -> Option<u64> {
    ["graphql", "core"]
        .iter()
        .map(|k| &v["resources"][k])
        .filter(|r| r["remaining"].as_i64() == Some(0))
        .filter_map(|r| r["reset"].as_u64())
        .max()
}

/// Now by GitHub's clock, as RFC 3339 (from the Date header of an answer that costs nothing).
pub fn server_time() -> Option<String> {
    let out = gh(&["api", "--include", "rate_limit"]).ok()?;
    http_date(&String::from_utf8_lossy(&out))
}

/// The Date header of an HTTP answer (`Date: Mon, 19 Oct 2026 10:00:00 GMT`), as RFC 3339.
fn http_date(answer: &str) -> Option<String> {
    let date = answer
        .lines()
        .take_while(|l| !l.trim().is_empty())
        .find_map(|l| {
            let (name, value) = l.split_once(':')?;
            name.eq_ignore_ascii_case("date").then(|| value.trim())
        })?;
    let mut parts = date.split_whitespace().skip(1);
    let (day, month, year, time) = (parts.next()?, parts.next()?, parts.next()?, parts.next()?);
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let month = MONTHS.iter().position(|m| *m == month)? + 1;
    let day: u32 = day.parse().ok()?;
    (year.len() == 4 && time.len() == 8).then(|| format!("{year}-{month:02}-{day:02}T{time}Z"))
}

/// Where GitHub has repo `owner/name` now: its owner and name, which differ after it was
/// renamed or moved to another owner (the REST API redirects from the old name).
pub fn repo_slug(owner: &str, name: &str) -> Result<(String, String), String> {
    let out = gh(&[
        "api",
        &format!("repos/{owner}/{name}"),
        "--jq",
        ".full_name",
    ])?;
    let full_name = String::from_utf8_lossy(&out).trim().to_owned();
    full_name
        .split_once('/')
        .map(|(o, n)| (o.to_owned(), n.to_owned()))
        .ok_or_else(|| format!("GitHub names {owner}/{name} {full_name:?}"))
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

/// What a PR search or a poll needs to know about a PR to tell whether it changed. Cheap:
/// GitHub charges GraphQL by the most nodes a query could return, so PR_FIELDS in a search
/// of 50 (review threads, checks) costs 53 points of the 5000 an hour, a stamp search 1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrStamp {
    pub number: i64,
    pub title: String,
    pub author: String,
    pub is_draft: bool,
    /// OPEN, CLOSED or MERGED.
    pub state: String,
    pub updated_at: String,
}

const STAMP_FIELDS: &str = "number title isDraft state updatedAt author { login }";

fn parse_stamps(list: &Value) -> Vec<PrStamp> {
    list.as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter(|p| p["number"].is_i64())
        .map(|p| PrStamp {
            number: p["number"].as_i64().unwrap_or_default(),
            title: s(&p["title"]),
            author: s(&p["author"]["login"]),
            is_draft: p["isDraft"].as_bool().unwrap_or_default(),
            state: p["state"].as_str().unwrap_or("OPEN").to_owned(),
            updated_at: s(&p["updatedAt"]),
        })
        .collect()
}

/// What a poll's search found, and whether that is all: it reads at most `SEARCH_PAGES`
/// pages.
#[derive(Debug, Clone)]
pub struct Searched<T> {
    pub found: Vec<T>,
    pub complete: bool,
}

/// The most pages a poll's search reads. A page costs a point; a review request to a team
/// (e.g. through CODEOWNERS) can make for hundreds of results in a busy repo.
const SEARCH_PAGES: usize = 10;

/// All results of GitHub search `q`, `per_page` at a time (`fields` of each node), up to
/// `SEARCH_PAGES` pages.
fn search_pages(q: &str, per_page: usize, fields: &str) -> Result<Searched<Value>, String> {
    let query = format!(
        "query($q: String!, $after: String) {{ search(query: $q, type: ISSUE, first: {per_page}, after: $after) {{ \
         pageInfo {{ hasNextPage endCursor }} nodes {{ {fields} }} }} }}"
    );
    let mut found = vec![];
    let mut after: Option<String> = None;
    for _ in 0..SEARCH_PAGES {
        let mut strings = vec![("q", q)];
        if let Some(cursor) = &after {
            strings.push(("after", cursor));
        }
        let v = graphql(&query, &strings, &[])?;
        let search = &v["data"]["search"];
        found.extend(nodes(search).iter().cloned());
        let next = search["pageInfo"]["endCursor"]
            .as_str()
            .filter(|_| search["pageInfo"]["hasNextPage"] == true);
        match next {
            Some(cursor) => after = Some(cursor.to_owned()),
            None => {
                return Ok(Searched {
                    found,
                    complete: true,
                });
            }
        }
    }
    Ok(Searched {
        found,
        complete: false,
    })
}

/// All open PRs in `owner/name` matching GitHub search `terms` (up to `SEARCH_PAGES` pages
/// of 100, which cost a point each), most recently updated first.
pub fn poll_pr_stamps(owner: &str, name: &str, terms: &str) -> Result<Searched<PrStamp>, String> {
    let q = format!("repo:{owner}/{name} is:pr is:open {terms} sort:updated-desc");
    let pages = search_pages(&q, 100, &format!("... on PullRequest {{ {STAMP_FIELDS} }}"))?;
    Ok(Searched {
        found: parse_stamps(&Value::Array(pages.found)),
        complete: pages.complete,
    })
}

/// Open PRs in `owner/name` matching GitHub search `terms`, most recently updated first:
/// the first 50.
pub fn search_pr_stamps(owner: &str, name: &str, terms: &str) -> Result<Vec<PrStamp>, String> {
    pr_stamps_in(owner, name, "is:open", terms)
}

/// `search_pr_stamps` for PRs in `state` (`is:open`, `is:merged`).
fn pr_stamps_in(owner: &str, name: &str, state: &str, terms: &str) -> Result<Vec<PrStamp>, String> {
    let query = format!(
        "query($q: String!) {{ search(query: $q, type: ISSUE, first: 50) {{ nodes {{ ... on PullRequest {{ {STAMP_FIELDS} }} }} }} }}"
    );
    let q = format!("repo:{owner}/{name} is:pr {state} {terms} sort:updated-desc");
    let v = graphql(&query, &[("q", &q)], &[])?;
    Ok(parse_stamps(&v["data"]["search"]["nodes"]))
}

/// Lookups by number per query: the query is one command-line argument of gh, which Windows
/// limits to 32767 characters (a PR's fields take some 700).
const LOOKUPS_PER_QUERY: usize = 25;

/// The queries that look up `kind` (`pullRequest`, `issue`) with `fields` for `numbers`,
/// `LOOKUPS_PER_QUERY` at a time, with the numbers each one looks up and the alias prefix.
fn lookup_queries<'a>(
    kind: &str,
    fields: &str,
    numbers: &'a [i64],
) -> impl Iterator<Item = (String, &'a [i64])> {
    let prefix = if kind == "issue" { "i" } else { "p" };
    numbers.chunks(LOOKUPS_PER_QUERY).map(move |chunk| {
        let aliases: String = chunk
            .iter()
            .map(|n| format!("{prefix}{n}: {kind}(number: {n}) {{ {fields} }}"))
            .collect::<Vec<_>>()
            .join("\n");
        (
            format!(
                "query($owner: String!, $name: String!) {{ repository(owner: $owner, name: $name) {{ {aliases} }} }}"
            ),
            chunk,
        )
    })
}

/// `kind` (`pullRequest`, `issue`) with `fields` of each of `numbers` in `owner/name`, in
/// that order (null where GitHub does not know the number).
fn look_up(
    owner: &str,
    name: &str,
    kind: &str,
    fields: &str,
    numbers: &[i64],
) -> Result<Value, String> {
    let prefix = if kind == "issue" { "i" } else { "p" };
    let mut found = vec![];
    for (query, chunk) in lookup_queries(kind, fields, numbers) {
        let v = graphql_lookups(&query, &[("owner", owner), ("name", name)])?;
        found.extend(
            chunk
                .iter()
                .map(|n| v["data"]["repository"][format!("{prefix}{n}")].clone()),
        );
    }
    Ok(Value::Array(found))
}

/// Stamps of PRs by number (any state); numbers GitHub does not know are left out.
pub fn pr_stamps(owner: &str, name: &str, numbers: &[i64]) -> Result<Vec<PrStamp>, String> {
    Ok(parse_stamps(&look_up(
        owner,
        name,
        "pullRequest",
        STAMP_FIELDS,
        numbers,
    )?))
}

/// Open PRs in `owner/name` matching GitHub search `terms`, most recently updated first,
/// with their details (a stamp search, then the details by number: 1 point per PR).
/// `viewer` decides what counts as activity by others. (The poll pages through its own
/// searches; this one is for the live tests.)
#[cfg(test)]
pub fn search_prs(
    owner: &str,
    name: &str,
    viewer: &str,
    terms: &str,
) -> Result<Vec<PrSummary>, String> {
    let numbers: Vec<i64> = search_pr_stamps(owner, name, terms)?
        .iter()
        .map(|p| p.number)
        .collect();
    prs_by_number(owner, name, viewer, &numbers)
}

/// PRs by number (any state), e.g. ones the user opened from search.
pub fn prs_by_number(
    owner: &str,
    name: &str,
    viewer: &str,
    numbers: &[i64],
) -> Result<Vec<PrSummary>, String> {
    Ok(parse_prs(
        &look_up(owner, name, "pullRequest", PR_FIELDS, numbers)?,
        viewer,
    ))
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

// --- issues ---------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IssueSummary {
    pub number: i64,
    pub title: String,
    pub url: String,
    pub author: String,
    /// OPEN or CLOSED.
    pub state: String,
    pub updated_at: String,
    pub labels: Vec<String>,
    pub comments: i64,
    /// Newest comment by someone other than the viewer.
    pub latest_activity: Option<String>,
}

const ISSUE_FIELDS: &str = r#"
  number title url state updatedAt author { login }
  labels(first: 20) { nodes { name } }
  comments(last: 20) { totalCount nodes { author { login } createdAt } }
"#;

pub fn parse_issues(list: &Value, viewer: &str) -> Vec<IssueSummary> {
    list.as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter(|i| i["number"].is_i64())
        .map(|i| IssueSummary {
            number: i["number"].as_i64().unwrap_or_default(),
            title: s(&i["title"]),
            url: s(&i["url"]),
            author: s(&i["author"]["login"]),
            state: i["state"].as_str().unwrap_or("OPEN").to_owned(),
            updated_at: s(&i["updatedAt"]),
            labels: nodes(&i["labels"]).iter().map(|l| s(&l["name"])).collect(),
            comments: i["comments"]["totalCount"].as_i64().unwrap_or_default(),
            latest_activity: nodes(&i["comments"])
                .iter()
                .filter(|c| c["author"]["login"].as_str().is_some_and(|a| a != viewer))
                .filter_map(|c| c["createdAt"].as_str())
                .max()
                .map(str::to_owned),
        })
        .collect()
}

/// Open issues in `owner/name` assigned to the viewer (up to `SEARCH_PAGES` pages of 50: with
/// their labels and comments, 50 cost a point).
pub fn assigned_issues(
    owner: &str,
    name: &str,
    viewer: &str,
) -> Result<Searched<IssueSummary>, String> {
    let q = format!("repo:{owner}/{name} is:issue is:open assignee:{viewer} sort:updated-desc");
    let pages = search_pages(&q, 50, &format!("... on Issue {{ {ISSUE_FIELDS} }}"))?;
    Ok(Searched {
        found: parse_issues(&Value::Array(pages.found), viewer),
        complete: pages.complete,
    })
}

/// Open issues in `owner/name` matching GitHub search `terms`, most recently updated first,
/// without their comments (for browsing).
pub fn search_issues(owner: &str, name: &str, terms: &str) -> Result<Vec<IssueSummary>, String> {
    let query = "query($q: String!) { search(query: $q, type: ISSUE, first: 50) { nodes { ... on Issue { \
        number title url state updatedAt author { login } labels(first: 5) { nodes { name } } } } } }";
    let q = format!("repo:{owner}/{name} is:issue is:open {terms} sort:updated-desc");
    let v = graphql(query, &[("q", &q)], &[])?;
    Ok(parse_issues(&v["data"]["search"]["nodes"], ""))
}

/// Issues by number (any state), e.g. ones with a worktree that are no longer assigned.
pub fn issues_by_number(
    owner: &str,
    name: &str,
    viewer: &str,
    numbers: &[i64],
) -> Result<Vec<IssueSummary>, String> {
    Ok(parse_issues(
        &look_up(owner, name, "issue", ISSUE_FIELDS, numbers)?,
        viewer,
    ))
}

const ISSUE_CONVERSATION: &str = r#"query($owner: String!, $name: String!, $number: Int!) {
  repository(owner: $owner, name: $name) { issue(number: $number) {
    number title url state createdAt body bodyHTML author { login }
    labels(first: 20) { nodes { name } }
    comments(first: 100) { nodes { databaseId author { login } body bodyHTML createdAt url } }
  } } }"#;

/// An issue as a `Conversation` (no branches, reviews, threads or checks).
pub fn issue_conversation(
    owner: &str,
    name: &str,
    number: i64,
) -> Result<(Conversation, Value), String> {
    let v = graphql(
        ISSUE_CONVERSATION,
        &[("owner", owner), ("name", name)],
        &[("number", number)],
    )?;
    let issue = v["data"]["repository"]["issue"].clone();
    Ok((parse_conversation(&issue), issue))
}

pub fn default_branch(owner: &str, name: &str) -> Result<String, String> {
    let v = graphql(
        "query($owner: String!, $name: String!) { repository(owner: $owner, name: $name) { defaultBranchRef { name } } }",
        &[("owner", owner), ("name", name)],
        &[],
    )?;
    v["data"]["repository"]["defaultBranchRef"]["name"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("{owner}/{name} has no default branch"))
}

/// The PR in `state` (`is:open`, `is:merged`) whose head is `head_owner:branch`, if any; the
/// most recently updated one.
pub fn pull_for_head(
    owner: &str,
    name: &str,
    viewer: &str,
    head_owner: &str,
    branch: &str,
    state: &str,
) -> Result<Option<PrSummary>, String> {
    let numbers: Vec<i64> = pr_stamps_in(owner, name, state, &format!("head:{branch}"))?
        .iter()
        .map(|p| p.number)
        .collect();
    Ok(prs_by_number(owner, name, viewer, &numbers)?
        .into_iter()
        .find(|p| p.head_ref == branch && p.head_owner.eq_ignore_ascii_case(head_owner)))
}

// --- PR context for the agent ----------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ThreadComment {
    pub author: String,
    pub body: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Thread {
    /// GraphQL node id (a reply in a review needs it).
    pub id: String,
    pub path: String,
    pub line: Option<i64>,
    pub outdated: bool,
    pub resolved: bool,
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
    /// The review threads, resolved ones too.
    pub threads: Vec<Thread>,
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

/// A reply of a review in an existing thread (`thread`: its GraphQL node id).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadReply {
    pub thread: String,
    pub body: String,
}

/// Submits a review with replies in existing threads, which the REST API does not take in a
/// review's comments: the review is made pending (the line comments with it), the replies are
/// added to it, and then it is submitted, as GitHub's own page does. Returns its URL. When a
/// step fails, the pending review is deleted, so nothing was posted.
pub fn submit_review_with_replies(
    owner: &str,
    name: &str,
    number: i64,
    payload: &Value,
    replies: &[ThreadReply],
) -> Result<String, String> {
    let reviews = format!("repos/{owner}/{name}/pulls/{number}/reviews");
    let mut pending = payload.clone();
    let (event, body) = match pending.as_object_mut() {
        Some(p) => (p.remove("event"), p.remove("body")),
        None => (None, None),
    };
    let review = post(&reviews, &pending)?;
    let (id, node) = (
        review["id"].as_i64().unwrap_or_default(),
        s(&review["node_id"]),
    );
    let submitted = (|| {
        for r in replies {
            graphql(
                "mutation($review: ID!, $thread: ID!, $body: String!) {
                  addPullRequestReviewThreadReply(input: { pullRequestReviewId: $review, pullRequestReviewThreadId: $thread, body: $body }) { comment { id } } }",
                &[("review", &node), ("thread", &r.thread), ("body", &r.body)],
                &[],
            )?;
        }
        let mut submit = json!({ "event": event });
        if let Some(body) = body.filter(|b| b.as_str().is_some_and(|b| !b.is_empty())) {
            submit["body"] = body;
        }
        post(&format!("{reviews}/{id}/events"), &submit)
    })();
    match submitted {
        Ok(v) => Ok(s(&v["html_url"])),
        Err(e) => {
            if let Err(gone) = gh(&["api", "--method", "DELETE", &format!("{reviews}/{id}")]) {
                tracing::warn!("could not delete the pending review {id}: {gone}");
            }
            Err(e)
        }
    }
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
    let threads = nodes(&pr["reviewThreads"])
        .iter()
        .map(|t| Thread {
            id: s(&t["id"]),
            path: s(&t["path"]),
            line: t["line"].as_i64().or(t["originalLine"].as_i64()),
            outdated: t["isOutdated"].as_bool().unwrap_or_default(),
            resolved: t["isResolved"].as_bool().unwrap_or_default(),
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
        threads,
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

    #[test]
    fn lookups_fit_on_a_windows_command_line() {
        let numbers: Vec<i64> = (10_000..10_130).collect();
        let queries: Vec<_> = lookup_queries("pullRequest", PR_FIELDS, &numbers).collect();
        assert_eq!(queries.iter().map(|(_, c)| c.len()).sum::<usize>(), 130);
        // gh gets the query as one argument, `query=…`: Windows allows 32767 characters for
        // the whole command line.
        let longest = queries.iter().map(|(q, _)| q.len()).max().unwrap();
        assert!(longest < 30_000, "{longest}");
    }
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
    fn githubs_time_is_read_from_the_date_header() {
        let answer = "HTTP/2.0 200 OK\nContent-Type: application/json\ndate: Mon, 9 Feb 2026 08:05:03 GMT\n\n{\"date\": \"x\"}";
        assert_eq!(http_date(answer).as_deref(), Some("2026-02-09T08:05:03Z"));
        assert_eq!(
            http_date("HTTP/2.0 200 OK\n\nDate: Mon, 9 Feb 2026 08:05:03 GMT"),
            None
        );
        assert_eq!(http_date("Date: yesterday"), None);
    }

    #[test]
    fn the_reset_that_matters_is_that_of_the_used_up_limit() {
        let limits = |graphql: (i64, u64), core: (i64, u64)| {
            json!({"resources": {
                "graphql": {"limit": 5000, "remaining": graphql.0, "reset": graphql.1},
                "core": {"limit": 5000, "remaining": core.0, "reset": core.1},
                "search": {"limit": 30, "remaining": 0, "reset": 9}
            }})
        };
        assert_eq!(exhausted_reset(&limits((0, 500), (4000, 900))), Some(500));
        assert_eq!(exhausted_reset(&limits((0, 500), (0, 900))), Some(900));
        assert_eq!(exhausted_reset(&limits((10, 500), (4000, 900))), None);
    }

    #[test]
    fn lookups_leave_out_numbers_github_does_not_know() {
        // What GitHub answers (and gh exits 1) for an existing and a deleted PR.
        let answer = br#"{"data":{"repository":{"p1":{"number":9000,"state":"CLOSED"},"p2":null}},
            "errors":[{"type":"NOT_FOUND","path":["repository","p2"],"message":"Could not resolve"}]}"#;
        let v = lookup_result(false, answer).unwrap().unwrap();
        let stamps = parse_stamps(&json!([
            v["data"]["repository"]["p1"],
            v["data"]["repository"]["p2"]
        ]));
        assert_eq!(stamps.iter().map(|s| s.number).collect::<Vec<_>>(), [9000]);
        // Other errors still fail, and so does gh failing without an answer.
        let forbidden = br#"{"data":null,"errors":[{"type":"FORBIDDEN","message":"no"}]}"#;
        assert!(lookup_result(false, forbidden).unwrap().is_err());
        // The repository itself is not found (renamed, deleted): nothing in it is known.
        let no_repo = br#"{"data":{"repository":null},"errors":[{"type":"NOT_FOUND","path":["repository"],"message":"Could not resolve"}]}"#;
        assert!(lookup_result(false, no_repo).unwrap().is_err());
        assert!(lookup_result(false, b"").is_none());
        assert!(
            lookup_result(true, br#"{"data":{"repository":{}}}"#)
                .unwrap()
                .is_ok()
        );
    }

    #[test]
    fn stamps_say_when_a_pr_last_changed() {
        let list = json!([
            {"number": 7, "title": "Fix", "isDraft": true, "state": "OPEN", "updatedAt": "2026-10-01T00:00:00Z",
             "author": {"login": "me"}},
            null
        ]);
        assert_eq!(
            parse_stamps(&list),
            [PrStamp {
                number: 7,
                title: "Fix".into(),
                author: "me".into(),
                is_draft: true,
                state: "OPEN".into(),
                updated_at: "2026-10-01T00:00:00Z".into(),
            }]
        );
    }

    #[test]
    fn parses_issues() {
        let list = json!([{
            "number": 7, "title": "Crash on start", "url": "U", "state": "OPEN", "updatedAt": "2026-10-01T00:00:00Z",
            "author": { "login": "carol" }, "labels": { "nodes": [ { "name": "bug" } ] },
            "comments": { "totalCount": 3, "nodes": [
                { "author": { "login": "me" }, "createdAt": "2026-10-03T00:00:00Z" },
                { "author": { "login": "carol" }, "createdAt": "2026-10-02T00:00:00Z" } ] }
        }, null]);
        let issues = parse_issues(&list, "me");
        assert_eq!(issues.len(), 1);
        let i = &issues[0];
        assert_eq!((i.number, i.author.as_str(), i.comments), (7, "carol", 3));
        assert_eq!(i.labels, ["bug"]);
        assert_eq!(
            i.latest_activity.as_deref(),
            Some("2026-10-02T00:00:00Z"),
            "own comments do not count"
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
        let t = &d.threads;
        assert_eq!(t.len(), 3);
        assert_eq!(
            (t[0].path.as_str(), t[0].line, t[0].resolved),
            ("src/A.java", Some(12), false)
        );
        assert_eq!((t[1].line, t[1].outdated), (Some(5), true));
        assert!(t[2].resolved);
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
        println!("default branch: {}", default_branch(&owner, &name).unwrap());
        let issues = assigned_issues(&owner, &name, &me).unwrap().found;
        println!("{} issues assigned to me", issues.len());
        let open_issues = search_issues(&owner, &name, "").unwrap();
        println!("{} open issues (browsing, newest first)", open_issues.len());
        let any_issue = open_issues.first().map(|i| i.number);
        if let Some(n) = any_issue {
            let (c, _) = issue_conversation(&owner, &name, n).unwrap();
            let by_number = issues_by_number(&owner, &name, &me, &[n]).unwrap();
            println!(
                "issue #{n}: {:?} by {}, {} comments; by number: {:?}",
                c.title,
                c.author,
                c.comments.len(),
                by_number.first().map(|i| &i.state)
            );
        }
        println!("{} PRs request my review", requested.len());
        let assigned = poll_pr_stamps(&owner, &name, &format!("assignee:{me}"))
            .unwrap()
            .found;
        let reviewed = poll_pr_stamps(&owner, &name, &format!("reviewed-by:{me}"))
            .unwrap()
            .found;
        println!(
            "{} PRs assigned to me, {} reviewed by me",
            assigned.len(),
            reviewed.len()
        );
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
                d.threads.iter().filter(|t| !t.resolved).count(),
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
