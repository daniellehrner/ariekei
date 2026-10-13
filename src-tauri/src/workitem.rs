//! Work item operations behind the UI commands: connecting a repo, the Inbox rows,
//! preparing a PR's worktree, starting its agent with context, and review markers.

use crate::AppState;
use crate::git::{self, git};
use crate::github::{self, PrSummary};
use crate::inbox::{self, Row};
use crate::store::{PolledPr, Repo, WorkItem};
use crate::terminal::{self, SessionPlan};
use crate::worktree::{self, Prepared};
use serde::Serialize;
use std::path::{Path, PathBuf};

pub fn summary(item: &WorkItem) -> Result<PrSummary, String> {
    serde_json::from_value(item.snapshot.clone())
        .map_err(|e| format!("PR #{}: bad snapshot: {e}", item.number))
}

fn item_repo(state: &AppState, id: i64) -> Result<(WorkItem, Repo), String> {
    let item = state.store.item(id)?;
    let repo = state
        .store
        .repo(item.repo_id)?
        .ok_or("repository not found")?;
    Ok((item, repo))
}

fn issue_summary(item: &WorkItem) -> Result<github::IssueSummary, String> {
    serde_json::from_value(item.snapshot.clone())
        .map_err(|e| format!("issue #{}: bad snapshot: {e}", item.number))
}

/// The repo's default branch, from the clone's `<remote>/HEAD` (set by `git clone`), or
/// GitHub if the clone does not know it.
pub fn default_branch(repo: &Repo) -> Result<String, String> {
    let head = git::git_str(
        &repo.path,
        &[
            "symbolic-ref",
            "--short",
            &format!("refs/remotes/{}/HEAD", repo.base_remote),
        ],
    );
    match head
        .ok()
        .and_then(|h| h.split_once('/').map(|(_, b)| b.to_owned()))
    {
        Some(branch) => Ok(branch),
        None => github::default_branch(&repo.owner, &repo.name),
    }
}

/// The branch the item's changes are compared to: a PR's base, or the default branch for
/// an issue.
fn base_ref_of(item: &WorkItem, repo: &Repo) -> Result<String, String> {
    if item.kind == "issue" {
        default_branch(repo)
    } else {
        Ok(summary(item)?.base_ref)
    }
}

fn worktree_of(item: &WorkItem) -> Result<PathBuf, String> {
    item.worktree_path
        .clone()
        .ok_or_else(|| format!("PR #{} has no worktree yet", item.number))
}

/// Connects the clone at `path` (any of its worktrees) as the active repo.
pub fn connect(state: &AppState, path: &str) -> Result<Repo, String> {
    let main = git::main_worktree(Path::new(path))?;
    let remotes = git::remotes(&main)?;
    let github_remote = |name: &str| {
        remotes
            .iter()
            .find(|(n, u)| n == name && git::github_slug(u).is_some())
    };
    let (remote, url) = github_remote("origin")
        .or_else(|| github_remote("upstream"))
        .or_else(|| remotes.iter().find(|(_, u)| git::github_slug(u).is_some()))
        .ok_or_else(|| format!("{} has no GitHub remote", main.display()))?;
    let (owner, name) = git::github_slug(url).ok_or("unrecognized GitHub URL")?;
    let viewer = github::viewer()?;
    let repo = state
        .store
        .add_repo(&main, &owner, &name, remote, &viewer)?;
    state
        .store
        .set_setting("active_repo", &repo.id.to_string())?;
    Ok(repo)
}

/// Clears worktree paths that no longer exist (e.g. removed outside Ariekei).
pub fn reconcile(state: &AppState) -> Result<(), String> {
    let Some(repo) = state.store.active_repo()? else {
        return Ok(());
    };
    for item in state.store.items(repo.id)? {
        if item.worktree_path.as_deref().is_some_and(|p| !p.exists()) {
            tracing::info!("worktree of PR #{} is gone", item.number);
            state.store.set_worktree(item.id, None, None, false)?;
        }
    }
    Ok(())
}

fn reviewed_marker(cwd: &Path, item: &WorkItem) -> Option<String> {
    item.reviewed_sha
        .clone()
        .filter(|sha| git::commit_exists(cwd, sha))
}

fn unreviewed(cwd: &Path, item: &WorkItem, base_ref: &str, repo: &Repo) -> bool {
    let Ok(head) = git::head(cwd) else {
        return false;
    };
    let marker = reviewed_marker(cwd, item).or_else(|| {
        git::merge_base(cwd, "HEAD", &format!("{}/{}", repo.base_remote, base_ref)).ok()
    });
    marker.as_deref() != Some(head.as_str()) || git::is_dirty(cwd).unwrap_or(false)
}

pub fn inbox(state: &AppState) -> Result<Vec<Row>, String> {
    let Some(repo) = state.store.active_repo()? else {
        return Ok(vec![]);
    };
    let default = default_branch(&repo).unwrap_or_else(|_| "main".into());
    let rows = state
        .store
        .items(repo.id)?
        .into_iter()
        // Finished items stay listed only while cleanup is blocked (they keep a worktree).
        .filter(|item| !item.hidden && (item.open || item.worktree_path.is_some()))
        .filter_map(|item| row(state, &repo, &default, item))
        .collect();
    Ok(inbox::order(rows))
}

fn row(state: &AppState, repo: &Repo, default_branch: &str, item: WorkItem) -> Option<Row> {
    let worktree = item.worktree_path.clone().filter(|p| p.exists());
    let agent = state.terminals.status(&item.id.to_string());
    let seen = item.last_seen.clone().unwrap_or_default();
    let mut row = if item.kind == "issue" {
        let issue = issue_summary(&item).ok()?;
        Row {
            id: item.id,
            number: item.number,
            kind: "issue".into(),
            title: issue.title,
            author: issue.author,
            role: item.role.clone(),
            url: issue.url,
            is_draft: false,
            open: item.open,
            updated_at: issue.updated_at,
            ci: None,
            failing_checks: vec![],
            review_decision: None,
            unresolved_threads: 0,
            new_activity: issue.latest_activity.unwrap_or_default() > seen,
            agent,
            worktree: None,
            unreviewed: worktree
                .as_deref()
                .is_some_and(|w| unreviewed(w, &item, default_branch, repo)),
            attention: None,
            cleanup_blockers: None,
            issue_number: None,
        }
    } else {
        let pr = summary(&item).ok()?;
        Row {
            id: item.id,
            number: item.number,
            kind: "pr".into(),
            title: pr.title.clone(),
            author: pr.author.clone(),
            role: item.role.clone(),
            url: pr.url.clone(),
            is_draft: pr.is_draft,
            open: item.open,
            updated_at: pr.updated_at.clone(),
            ci: pr.ci.clone(),
            failing_checks: pr.failing_checks.clone(),
            review_decision: pr.review_decision.clone(),
            unresolved_threads: pr.unresolved_threads,
            new_activity: pr.latest_activity.clone().unwrap_or_default() > seen,
            agent,
            worktree: None,
            unreviewed: worktree
                .as_deref()
                .is_some_and(|w| unreviewed(w, &item, &pr.base_ref, repo)),
            attention: None,
            cleanup_blockers: None,
            issue_number: item.issue_number,
        }
    };
    row.worktree = worktree.map(|p| p.to_string_lossy().into_owned());
    row.cleanup_blockers = item.cleanup_blockers;
    Some(row)
}

/// Marks an item done: cleans it up now if nothing would be lost.
pub fn mark_done(state: &AppState, id: i64) -> Result<crate::lifecycle::Cleanup, String> {
    crate::lifecycle::clean_up(state, id)
}

/// Opening an item counts as seeing its current GitHub activity, and keeps it in the
/// Inbox from now on (e.g. after the review request is gone because the user reviewed).
pub fn mark_seen(state: &AppState, id: i64) -> Result<(), String> {
    let item = state.store.item(id)?;
    if !item.tracked {
        state.store.set_tracked(id)?;
    }
    let latest = match item.kind.as_str() {
        "issue" => issue_summary(&item).ok().and_then(|i| i.latest_activity),
        _ => summary(&item).ok().and_then(|p| p.latest_activity),
    };
    if let Some(at) = latest {
        state.store.set_last_seen(id, &at)?;
    }
    Ok(())
}

pub fn prepare(state: &AppState, id: i64) -> Result<Prepared, String> {
    let (item, repo) = item_repo(state, id)?;
    if let Some(path) = item.worktree_path.as_ref().filter(|p| p.exists()) {
        return Ok(Prepared {
            path: path.clone(),
            branch: item.branch.clone().unwrap_or_default(),
            adopted: true,
            note: None,
        });
    }
    let prepared = if item.kind == "issue" {
        let issue = issue_summary(&item)?;
        worktree::prepare_issue(&repo, item.number, &issue.title, &default_branch(&repo)?)?
    } else {
        worktree::prepare(&repo, &summary(&item)?)?
    };
    state.store.set_worktree(
        id,
        Some(&prepared.path),
        Some(&prepared.branch),
        !prepared.adopted,
    )?;
    Ok(prepared)
}

/// Resume the item's conversation if it has one; otherwise start a new one with the PR's
/// current context appended to the system prompt.
pub fn session_plan(state: &AppState, data_dir: &Path, id: i64) -> Result<SessionPlan, String> {
    let (item, repo) = item_repo(state, id)?;
    if let Some(session_id) = item.session_id.clone()
        && terminal::resumable(item.session_started, item.transcript_path.as_deref())
    {
        return Ok(SessionPlan {
            session_id,
            resume: true,
            context_file: None,
        });
    }
    // Never prompted: reuse its id (nothing was saved under it), else start fresh.
    let session_id = match item.session_id.clone().filter(|_| !item.session_started) {
        Some(id) => id,
        None => uuid::Uuid::new_v4().to_string(),
    };
    state.store.set_session(id, &session_id)?;
    let written = if item.kind == "issue" {
        write_issue_context(data_dir, &repo, &item)
    } else {
        write_context(data_dir, &repo, item.number)
    };
    let context_file = match written {
        Ok(file) => Some(file),
        Err(e) => {
            tracing::warn!(
                "PR #{}: starting the agent without context: {e}",
                item.number
            );
            None
        }
    };
    Ok(SessionPlan {
        session_id,
        resume: false,
        context_file,
    })
}

fn write_context(data_dir: &Path, repo: &Repo, number: i64) -> Result<PathBuf, String> {
    let detail = github::pr_detail(&repo.owner, &repo.name, number)?;
    let logs: Vec<(String, String)> = detail
        .failing
        .iter()
        .filter_map(|c| Some((c, c.job_id?)))
        .take(3)
        .filter_map(
            |(c, job)| match github::job_log_tail(&repo.owner, &repo.name, job, 80) {
                Ok(log) => Some((c.name.clone(), log)),
                Err(e) => {
                    tracing::warn!("no log for {}: {e}", c.name);
                    None
                }
            },
        )
        .collect();
    let dir = data_dir.join("context");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let file = dir.join(format!("{}-{}-pr{number}.md", repo.owner, repo.name));
    std::fs::write(&file, crate::context::render(number, &detail, &logs))
        .map_err(|e| e.to_string())?;
    Ok(file)
}

fn write_issue_context(data_dir: &Path, repo: &Repo, item: &WorkItem) -> Result<PathBuf, String> {
    let (_, raw) = github::issue_conversation(&repo.owner, &repo.name, item.number)?;
    let comments: Vec<(String, String)> = raw["comments"]["nodes"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .map(|c| {
            (
                c["author"]["login"].as_str().unwrap_or_default().to_owned(),
                c["body"].as_str().unwrap_or_default().to_owned(),
            )
        })
        .collect();
    let branch = item.branch.clone().unwrap_or_default();
    let md = crate::context::render_issue(
        item.number,
        raw["title"].as_str().unwrap_or_default(),
        raw["url"].as_str().unwrap_or_default(),
        raw["body"].as_str().unwrap_or_default(),
        &comments,
        &branch,
        &default_branch(repo)?,
    );
    let dir = data_dir.join("context");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let file = dir.join(format!(
        "{}-{}-issue{}.md",
        repo.owner, repo.name, item.number
    ));
    std::fs::write(&file, md).map_err(|e| e.to_string())?;
    Ok(file)
}

/// The PR's unresolved review threads as a message for the agent (newer than its context).
pub fn comments_message(state: &AppState, id: i64) -> Result<String, String> {
    let (item, repo) = item_repo(state, id)?;
    let detail = github::pr_detail(&repo.owner, &repo.name, item.number)?;
    let md = crate::context::render(
        item.number,
        &github::PrDetail {
            body: String::new(),
            ..detail
        },
        &[],
    );
    let start = md.find("## ").unwrap_or(md.len());
    if start == md.len() {
        return Err(
            "the PR has no unresolved review threads, review summaries or failing checks".into(),
        );
    }
    Ok(format!(
        "Here is the current review feedback and CI state of the PR:\n\n{}",
        &md[start..]
    ))
}

// --- other people's PRs -------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub number: i64,
    pub title: String,
    pub author: String,
    pub is_draft: bool,
    pub updated_at: String,
    /// The work item, if the PR is already in the Inbox.
    pub item_id: Option<i64>,
}

/// Open PRs of the active repo matching `text` (GitHub search syntax: words, `author:x`, …).
pub fn search(state: &AppState, text: &str) -> Result<Vec<SearchHit>, String> {
    let repo = state
        .store
        .active_repo()?
        .ok_or("no repository connected")?;
    let items = state.store.items(repo.id)?;
    let terms = text.trim().trim_start_matches('#');
    Ok(
        github::search_prs(&repo.owner, &repo.name, &repo.viewer, terms)?
            .into_iter()
            .map(|p| SearchHit {
                item_id: items.iter().find(|i| i.number == p.number).map(|i| i.id),
                number: p.number,
                title: p.title,
                author: p.author,
                is_draft: p.is_draft,
                updated_at: p.updated_at,
            })
            .collect(),
    )
}

/// Adds PR `number` to the Inbox (tracked) and returns its work item id.
pub fn track(state: &AppState, number: i64) -> Result<i64, String> {
    let repo = state
        .store
        .active_repo()?
        .ok_or("no repository connected")?;
    let pr = github::prs_by_number(&repo.owner, &repo.name, &repo.viewer, &[number])?
        .into_iter()
        .next()
        .ok_or_else(|| format!("PR #{number} not found"))?;
    let role = crate::poller::role_of(&pr, &repo.viewer, &[]);
    state.store.track(
        repo.id,
        &PolledPr {
            number,
            kind: "pr",
            role,
            open: pr.state == "OPEN",
            snapshot: serde_json::to_value(&pr).unwrap_or_default(),
        },
    )
}

pub fn conversation(state: &AppState, id: i64) -> Result<github::Conversation, String> {
    let (item, repo) = item_repo(state, id)?;
    if item.kind == "issue" {
        return github::issue_conversation(&repo.owner, &repo.name, item.number).map(|(c, _)| c);
    }
    github::conversation(&repo.owner, &repo.name, item.number)
}

// --- review -------------------------------------------------------------------------

/// Where the review reads from: the item's worktree, or (without one) the main clone with
/// the PR head fetched to `refs/ariekei/pull/<N>/head`.
fn review_dir(item: &WorkItem, repo: &Repo) -> PathBuf {
    item.worktree_path
        .clone()
        .filter(|p| p.exists())
        .unwrap_or_else(|| repo.path.clone())
}

use crate::worktree::pull_ref;

/// Fetches the PR head and its base branch into the main clone, for reviewing without a
/// worktree. Returns the head commit.
pub fn fetch_pr(state: &AppState, id: i64) -> Result<String, String> {
    let (item, repo) = item_repo(state, id)?;
    let pr = summary(&item)?;
    worktree::fetch_pr_head(&repo.path, &repo.base_remote, item.number, &pr.base_ref)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewCommit {
    #[serde(flatten)]
    pub commit: git::Commit,
    /// Newer than the user's review marker.
    pub after_review: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewState {
    /// "worktree" (diffs reach the working tree) or "ref" (the fetched PR head).
    pub mode: &'static str,
    pub dir: String,
    /// Merge base of the head and the PR's base branch: "all changes" start here.
    pub base: String,
    pub base_ref: String,
    /// The commit the user marked as reviewed, if it still exists.
    pub reviewed: Option<String>,
    /// HEAD of the worktree, or the fetched PR head.
    pub head: String,
    /// The PR's head commit on GitHub (line comments are made against it).
    pub pr_head: String,
    /// All commits of the PR (base..head), newest first.
    pub commits: Vec<ReviewCommit>,
    pub dirty: bool,
    /// The viewer wrote this PR (GitHub does not let them approve it).
    pub own: bool,
}

pub fn review_state(state: &AppState, id: i64) -> Result<ReviewState, String> {
    let (item, repo) = item_repo(state, id)?;
    let base_ref = base_ref_of(&item, &repo)?;
    let worktree = item.worktree_path.clone().filter(|p| p.exists());
    // An issue has no PR yet: its review needs the worktree, and nothing goes to GitHub.
    if worktree.is_none() && item.kind == "issue" {
        return Err("start working on the issue to review its changes".into());
    }
    let pr_head = if item.kind == "issue" {
        String::new()
    } else {
        summary(&item)?.head_sha
    };
    let dir = review_dir(&item, &repo);
    let head = match &worktree {
        Some(w) => git::head(w)?,
        None => git::git_str(&dir, &["rev-parse", &pull_ref(item.number)])
            .map_err(|_| "the PR head is not fetched yet".to_owned())?,
    };
    let base = git::merge_base(&dir, &head, &format!("{}/{}", repo.base_remote, base_ref))
        .unwrap_or_else(|_| head.clone());
    let reviewed = reviewed_marker(&dir, &item);
    let after: Vec<String> = match &reviewed {
        Some(r) => git::commits_between(&dir, r, &head)?
            .into_iter()
            .map(|c| c.sha)
            .collect(),
        None => vec![],
    };
    let commits = git::commits_between(&dir, &base, &head)?
        .into_iter()
        .map(|c| ReviewCommit {
            after_review: after.contains(&c.sha),
            commit: c,
        })
        .collect();
    Ok(ReviewState {
        mode: if worktree.is_some() {
            "worktree"
        } else {
            "ref"
        },
        dir: dir.to_string_lossy().into_owned(),
        dirty: worktree
            .as_deref()
            .map(git::is_dirty)
            .transpose()?
            .unwrap_or(false),
        own: item.kind == "issue"
            || item.snapshot["author"]
                .as_str()
                .is_some_and(|a| a.eq_ignore_ascii_case(&repo.viewer)),
        base,
        base_ref,
        reviewed,
        pr_head,
        head,
        commits,
    })
}

/// Marks the reviewed head (worktree HEAD or fetched PR head) as reviewed. Also kept as
/// `refs/ariekei/reviewed/pr-<N>` in the clone so the commit stays reachable when the
/// agent rebases or amends.
pub fn mark_reviewed(state: &AppState, id: i64) -> Result<String, String> {
    let (item, repo) = item_repo(state, id)?;
    let head = review_state(state, id)?.head;
    git(
        &repo.path,
        &[
            "update-ref",
            &format!("refs/ariekei/reviewed/pr-{}", item.number),
            &head,
        ],
    )?;
    state.store.set_reviewed(id, &head)?;
    Ok(head)
}

/// `to` is a commit, or `None` for the working tree (only with a worktree).
fn review_target(item: &WorkItem, repo: &Repo, to: Option<&str>) -> Result<PathBuf, String> {
    let dir = review_dir(item, repo);
    if to.is_none() && item.worktree_path.as_deref().is_none_or(|p| !p.exists()) {
        return Err("without a worktree, a review needs a commit to compare to".into());
    }
    Ok(dir)
}

pub fn review_files(
    state: &AppState,
    id: i64,
    from: &str,
    to: Option<&str>,
) -> Result<Vec<git::ChangedFile>, String> {
    let (item, repo) = item_repo(state, id)?;
    git::changed_files(&review_target(&item, &repo, to)?, from, to)
}

pub fn review_file(
    state: &AppState,
    id: i64,
    from: &str,
    to: Option<&str>,
    path: &str,
) -> Result<git::FileDiff, String> {
    let (item, repo) = item_repo(state, id)?;
    git::file_diff(&review_target(&item, &repo, to)?, from, to, path)
}

/// Line ranges per path, as `(first, last)` lines of the new side.
pub type CommentRanges = Vec<(String, Vec<(u32, u32)>)>;

/// For each path, the line ranges (new side) of the PR diff that GitHub accepts line
/// comments on: its hunks with GitHub's three lines of context.
pub fn comment_ranges(
    state: &AppState,
    id: i64,
    paths: &[String],
) -> Result<CommentRanges, String> {
    let review = review_state(state, id)?;
    let dir = PathBuf::from(&review.dir);
    let head = if git::commit_exists(&dir, &review.pr_head) {
        review.pr_head
    } else {
        review.head
    };
    paths
        .iter()
        .map(|path| {
            let diff = git::git_str(
                &dir,
                &[
                    "diff",
                    "-U3",
                    "--no-color",
                    "--no-ext-diff",
                    "--no-renames",
                    &review.base,
                    &head,
                    "--",
                    path,
                ],
            )?;
            let ranges = git::parse_hunks(&diff)
                .into_iter()
                .filter(|h| h.new_lines > 0)
                .map(|h| (h.new_start, h.new_start + h.new_lines - 1))
                .collect();
            Ok((path.clone(), ranges))
        })
        .collect()
}

/// Submits a review on GitHub. `comments` must already be on lines of the PR diff
/// (see `comment_ranges`); everything else belongs in `body`.
pub fn submit_review(
    state: &AppState,
    id: i64,
    event: &str,
    body: &str,
    comments: &[github::LineComment],
) -> Result<String, String> {
    let (item, repo) = item_repo(state, id)?;
    if !matches!(event, "COMMENT" | "APPROVE" | "REQUEST_CHANGES") {
        return Err(format!("unknown review event {event}"));
    }
    let pr = summary(&item)?;
    let payload = github::review_payload(&pr.head_sha, event, body, comments);
    let url = github::submit_review(&repo.owner, &repo.name, item.number, &payload)?;
    state.poller.refresh();
    Ok(url)
}

pub fn reply_comment(state: &AppState, id: i64, comment_id: i64, body: &str) -> Result<(), String> {
    let (item, repo) = item_repo(state, id)?;
    github::reply(&repo.owner, &repo.name, item.number, comment_id, body)?;
    state.poller.refresh();
    Ok(())
}

pub fn post_comment(state: &AppState, id: i64, body: &str) -> Result<(), String> {
    let (item, repo) = item_repo(state, id)?;
    github::comment_on(&repo.owner, &repo.name, item.number, body)?;
    state.poller.refresh();
    Ok(())
}

pub fn resolve_thread(state: &AppState, thread_id: &str, resolved: bool) -> Result<(), String> {
    github::resolve_thread(thread_id, resolved)?;
    state.poller.refresh();
    Ok(())
}

/// Writes a file edited in the review view. `path` must stay inside the worktree.
pub fn save_file(state: &AppState, id: i64, path: &str, content: &str) -> Result<(), String> {
    let root = worktree_of(&state.store.item(id)?)?;
    let relative = Path::new(path);
    let inside = relative
        .components()
        .all(|c| matches!(c, std::path::Component::Normal(_)));
    if !inside {
        return Err(format!("refusing to write outside the worktree: {path}"));
    }
    std::fs::write(root.join(relative), content).map_err(|e| format!("{path}: {e}"))
}
