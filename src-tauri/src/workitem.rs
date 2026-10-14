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
    Ok(repo)
}

/// Clears worktree paths that no longer exist (e.g. removed outside Ariekei).
pub fn reconcile(state: &AppState) -> Result<(), String> {
    for item in state
        .store
        .repos()?
        .iter()
        .map(|repo| state.store.items(repo.id))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .flatten()
    {
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

pub fn inbox(state: &AppState, repo: Option<Repo>) -> Result<Vec<Row>, String> {
    let Some(repo) = repo else {
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

/// Open PRs of `repo` matching `text` (GitHub search syntax: words, `author:x`, …).
pub fn search(state: &AppState, repo: Option<Repo>, text: &str) -> Result<Vec<SearchHit>, String> {
    let repo = repo.ok_or("no repository connected")?;
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
pub fn track(state: &AppState, repo: Option<Repo>, number: i64) -> Result<i64, String> {
    let repo = repo.ok_or("no repository connected")?;
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
    /// Where "since your review" starts: the reviewed commit, or, when the branch was
    /// rebased since, the reviewed state moved onto the new base (a tree id).
    pub since_review_from: Option<String>,
    /// Why "since your review" differs from a plain diff, or why it could not be adjusted.
    pub rebase_note: Option<String>,
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

/// What the review looks at, with the reviewed marker resolved.
struct Review {
    item: WorkItem,
    dir: PathBuf,
    /// The head as reviewed content: `None` for the working tree, else the PR head commit.
    view: Option<String>,
    head: String,
    base: String,
    base_ref: String,
    worktree: bool,
    reviewed: Option<String>,
    since_review_from: Option<String>,
    rebase_note: Option<String>,
}

fn review(state: &AppState, id: i64) -> Result<Review, String> {
    let (item, repo) = item_repo(state, id)?;
    let base_ref = base_ref_of(&item, &repo)?;
    let worktree = item.worktree_path.clone().filter(|p| p.exists());
    // An issue has no PR yet: its review needs the worktree, and nothing goes to GitHub.
    if worktree.is_none() && item.kind == "issue" {
        return Err("start working on the issue to review its changes".into());
    }
    let dir = review_dir(&item, &repo);
    let head = match &worktree {
        Some(w) => git::head(w)?,
        None => git::git_str(&dir, &["rev-parse", &pull_ref(item.number)])
            .map_err(|_| "the PR head is not fetched yet".to_owned())?,
    };
    let tracking = format!("{}/{}", repo.base_remote, base_ref);
    let base = git::merge_base(&dir, &head, &tracking).unwrap_or_else(|_| head.clone());
    let reviewed = reviewed_marker(&dir, &item);
    let (since_review_from, rebase_note) = match &reviewed {
        None => (None, None),
        Some(r) => match git::rebase_reviewed(&dir, r, &head, &tracking) {
            Ok(None) => (Some(r.clone()), None),
            Ok(Some(tree)) => (
                Some(tree),
                Some(format!(
                    "Rebased since your review: changes that came only from {base_ref} are hidden."
                )),
            ),
            Err(e) => (
                Some(r.clone()),
                Some(format!(
                    "Rebased since your review, but {base_ref}'s changes could not be separated ({e}), so this is the plain diff against the commit you reviewed, including changes from {base_ref}."
                )),
            ),
        },
    };
    Ok(Review {
        view: if worktree.is_some() {
            None
        } else {
            Some(head.clone())
        },
        worktree: worktree.is_some(),
        item,
        dir,
        head,
        base,
        base_ref,
        reviewed,
        since_review_from,
        rebase_note,
    })
}

pub fn review_state(state: &AppState, id: i64) -> Result<ReviewState, String> {
    let r = review(state, id)?;
    let pr_head = if r.item.kind == "issue" {
        String::new()
    } else {
        summary(&r.item)?.head_sha
    };
    let new: std::collections::HashSet<String> = match &r.reviewed {
        Some(reviewed) => git::commits_new_since(&r.dir, reviewed, &r.head)?,
        None => Default::default(),
    };
    let commits = git::commits_between(&r.dir, &r.base, &r.head)?
        .into_iter()
        .map(|c| ReviewCommit {
            after_review: new.contains(&c.sha),
            commit: c,
        })
        .collect();
    let own = r.item.kind == "issue" || {
        let repo = state
            .store
            .repo(r.item.repo_id)?
            .ok_or("repository not found")?;
        r.item.snapshot["author"]
            .as_str()
            .is_some_and(|a| a.eq_ignore_ascii_case(&repo.viewer))
    };
    Ok(ReviewState {
        mode: if r.worktree { "worktree" } else { "ref" },
        dir: r.dir.to_string_lossy().into_owned(),
        dirty: if r.worktree {
            git::is_dirty(&r.dir)?
        } else {
            false
        },
        own,
        base: r.base,
        base_ref: r.base_ref,
        reviewed: r.reviewed,
        since_review_from: r.since_review_from,
        rebase_note: r.rebase_note,
        pr_head,
        head: r.head,
        commits,
    })
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewFile {
    pub path: String,
    /// Git status letter: A, M, D, … ("?" for untracked).
    pub status: String,
    pub group: crate::review_order::Group,
    /// The user marked this content of the file reviewed (it has not changed since).
    pub reviewed: bool,
    /// Only whitespace changed (in this scope).
    pub whitespace_only: bool,
}

/// Whether each path's current content is the one the user marked reviewed. After a rebase,
/// a marked file whose only changes came from the new base stays reviewed.
fn reviewed_paths(
    state: &AppState,
    r: &Review,
    paths: &[String],
) -> Result<std::collections::HashSet<String>, String> {
    let marks = state.store.reviewed_files(r.item.id)?;
    let changed_since_review: Option<std::collections::HashSet<String>> =
        match (&r.since_review_from, &r.reviewed) {
            (Some(from), Some(reviewed)) if from != reviewed => Some(
                git::changed_files(&r.dir, from, r.view.as_deref())?
                    .into_iter()
                    .map(|f| f.path)
                    .collect(),
            ),
            _ => None,
        };
    Ok(paths
        .iter()
        .filter(|path| {
            let Some(mark) = marks.get(*path) else {
                return false;
            };
            *mark == git::blob_id(&r.dir, r.view.as_deref(), path)
                || changed_since_review
                    .as_ref()
                    .is_some_and(|changed| !changed.contains(*path))
        })
        .cloned()
        .collect())
}

/// The files changed from `from` to `to` (a commit, or the head when `None`) in review order,
/// with their reviewed state.
pub fn review_files(
    state: &AppState,
    id: i64,
    from: &str,
    to: Option<&str>,
    ignore_ws: bool,
) -> Result<Vec<ReviewFile>, String> {
    let r = review(state, id)?;
    let to = to.map(str::to_owned).or_else(|| r.view.clone());
    let files = git::changed_files(&r.dir, from, to.as_deref())?;
    let paths: Vec<String> = files.iter().map(|f| f.path.clone()).collect();
    let generated = crate::review_order::attributed_generated(&r.dir, &paths);
    let reviewed = reviewed_paths(state, &r, &paths)?;
    let whitespace = if ignore_ws {
        git::whitespace_only_files(&r.dir, from, to.as_deref())?
    } else {
        Default::default()
    };
    let mut out: Vec<ReviewFile> = files
        .into_iter()
        .map(|f| ReviewFile {
            group: crate::review_order::group_of(&f.path, generated.contains(&f.path)),
            reviewed: reviewed.contains(&f.path),
            whitespace_only: whitespace.contains(&f.path),
            path: f.path,
            status: f.status,
        })
        .collect();
    out.sort_by_key(|f| crate::review_order::sort_key(&f.path, f.group));
    Ok(out)
}

pub fn review_file(
    state: &AppState,
    id: i64,
    from: &str,
    to: Option<&str>,
    path: &str,
    ignore_ws: bool,
) -> Result<git::FileDiff, String> {
    let r = review(state, id)?;
    let to = to.map(str::to_owned).or_else(|| r.view.clone());
    git::file_diff(&r.dir, from, to.as_deref(), path, ignore_ws)
}

/// Marks (or unmarks) the current content of `path` as reviewed.
pub fn mark_file_reviewed(
    state: &AppState,
    id: i64,
    path: &str,
    reviewed: bool,
) -> Result<(), String> {
    let r = review(state, id)?;
    let blob = reviewed.then(|| git::blob_id(&r.dir, r.view.as_deref(), path));
    state.store.set_file_reviewed(id, path, blob.as_deref())
}

/// Marks the head (worktree HEAD or fetched PR head) as reviewed, and every file of the PR
/// with its current content. Also kept as `refs/ariekei/reviewed/pr-<N>` in the clone so
/// the commit stays reachable when the agent rebases or amends.
pub fn mark_reviewed(state: &AppState, id: i64) -> Result<String, String> {
    let (item, repo) = item_repo(state, id)?;
    let r = review(state, id)?;
    for file in git::changed_files(&r.dir, &r.base, r.view.as_deref())? {
        state.store.set_file_reviewed(
            id,
            &file.path,
            Some(&git::blob_id(&r.dir, r.view.as_deref(), &file.path)),
        )?;
    }
    git(
        &repo.path,
        &[
            "update-ref",
            &format!("refs/ariekei/reviewed/pr-{}", item.number),
            &r.head,
        ],
    )?;
    state.store.set_reviewed(id, &r.head)?;
    Ok(r.head)
}

// --- review guide --------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GuideView {
    pub guide: serde_json::Value,
    /// The guide was made for another base/head than the current one.
    pub stale: bool,
}

pub fn review_guide(state: &AppState, id: i64) -> Result<Option<GuideView>, String> {
    let Some((base, head, guide)) = state.store.guide(id)? else {
        return Ok(None);
    };
    let current = review(state, id).ok();
    let stale = current.is_none_or(|r| r.base != base || r.head != head);
    Ok(Some(GuideView { guide, stale }))
}

/// Asks Claude (headless, read-only tools) for a guide to the item's "all changes" scope.
pub fn prepare_guide(state: &AppState, id: i64) -> Result<GuideView, String> {
    let (item, repo) = item_repo(state, id)?;
    let r = review(state, id)?;
    let claude = state.terminals.claude()?;
    let title = item.snapshot["title"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let description = match item.kind.as_str() {
        "issue" => github::issue_conversation(&repo.owner, &repo.name, item.number)
            .map(|(_, raw)| raw["body"].as_str().unwrap_or_default().to_owned()),
        _ => github::pr_detail(&repo.owner, &repo.name, item.number).map(|d| d.body),
    }
    .unwrap_or_default();
    let commits: Vec<String> = git::commits_between(&r.dir, &r.base, &r.head)?
        .into_iter()
        .rev()
        .map(|c| c.subject)
        .collect();
    let mut stat_args = vec!["diff", "--stat=200", "--no-color", r.base.as_str()];
    let mut diff_args = vec!["diff", "--no-color", "--no-ext-diff", r.base.as_str()];
    if let Some(v) = r.view.as_deref() {
        stat_args.push(v);
        diff_args.push(v);
    }
    let stat = git::git_str(&r.dir, &stat_args)?;
    let diff = String::from_utf8_lossy(&git(&r.dir, &diff_args)?).into_owned();
    let guide = crate::guide::run(
        &claude,
        &r.dir,
        &crate::guide::Input {
            title: &title,
            description: &description,
            commits: &commits,
            stat: &stat,
            diff: &diff,
        },
    )?;
    let value = serde_json::to_value(&guide).map_err(|e| e.to_string())?;
    state.store.set_guide(id, &r.base, &r.head, &value)?;
    Ok(GuideView {
        guide: value,
        stale: false,
    })
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
