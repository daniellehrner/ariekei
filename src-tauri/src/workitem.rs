//! Work item operations behind the UI commands: connecting a repo, the Inbox rows,
//! preparing a PR's worktree, starting its agent with context, and review markers.

use crate::AppState;
use crate::git;
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
/// an issue or a task.
fn base_ref_of(item: &WorkItem, repo: &Repo) -> Result<String, String> {
    if item.kind == "issue" || item.is_task() {
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

/// Whether the item is in the Inbox (or only under its task). Finished items stay only while
/// cleanup is blocked (they keep a worktree).
fn listed(item: &WorkItem) -> bool {
    !item.hidden && (item.open || item.worktree_path.is_some())
}

pub fn inbox(state: &AppState, repo: Option<Repo>) -> Result<Vec<Row>, String> {
    let Some(repo) = repo else {
        return Ok(vec![]);
    };
    let items = state.store.items(repo.id)?;
    // Finished items stay listed under their task, which shows what came of it.
    let tasks: Vec<&WorkItem> = items.iter().filter(|i| i.is_task() && listed(i)).collect();
    let task_of = |item: &WorkItem| {
        item.task_id
            .and_then(|t| tasks.iter().find(|task| task.id == t).copied())
    };
    let rows = items
        .iter()
        .filter(|item| listed(item) || task_of(item).is_some())
        .filter_map(|item| {
            let mut row = row(state, &repo, item.clone())?;
            if let Some(task) = task_of(item) {
                row.task_id = Some(task.id);
                if row.worktree.is_none() && task.worktree_path.is_some() {
                    row.agent_item = Some(task.id);
                }
            }
            Some(row)
        })
        .collect();
    Ok(inbox::order(rows))
}

fn row(state: &AppState, repo: &Repo, item: WorkItem) -> Option<Row> {
    let worktree = item.worktree_path.clone().filter(|p| p.exists());
    let agent = state.terminals.status(&item.id.to_string());
    let seen = item.last_seen.clone().unwrap_or_default();
    let mut row = if item.is_task() {
        Row {
            id: item.id,
            number: item.number,
            kind: "task".into(),
            title: item.title().to_owned(),
            author: repo.viewer.clone(),
            role: item.role.clone(),
            url: String::new(),
            is_draft: false,
            open: true,
            updated_at: item.snapshot["createdAt"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
            ci: None,
            failing_checks: vec![],
            review_decision: None,
            unresolved_threads: 0,
            new_activity: false,
            agent,
            worktree: None,
            attention: None,
            cleanup_blockers: None,
            issue_number: None,
            task_id: None,
            agent_item: None,
            outcomes_closed: false,
            starred: item.starred,
            removable: false,
            notes: item.snapshot["notes"].as_str().map(str::to_owned),
        }
    } else if item.kind == "issue" {
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
            attention: None,
            cleanup_blockers: None,
            issue_number: None,
            task_id: None,
            agent_item: None,
            outcomes_closed: false,
            starred: item.starred,
            removable: false,
            notes: None,
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
            attention: None,
            cleanup_blockers: None,
            issue_number: item.issue_number,
            task_id: None,
            agent_item: None,
            outcomes_closed: false,
            starred: item.starred,
            removable: false,
            notes: None,
        }
    };
    if worktree.is_some() {
        row.agent_item = Some(item.id);
    }
    row.removable = removable(&item);
    row.worktree = worktree.map(|p| p.to_string_lossy().into_owned());
    row.cleanup_blockers = item.cleanup_blockers;
    Some(row)
}

/// Whether the user can take the item out of the Inbox: only a PR or issue they added, which
/// no query finds and nothing local hangs on (a worktree is finished with Done instead).
fn removable(item: &WorkItem) -> bool {
    !item.is_task()
        && item.role == "other"
        && item.worktree_path.is_none()
        && item.task_id.is_none()
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
    let prepared = if let Some(head) = task_head(&item) {
        worktree::prepare_task_on(
            &repo,
            -item.number,
            item.title(),
            &head,
            &default_branch(&repo)?,
        )?
    } else if item.is_task() {
        worktree::prepare_task(&repo, -item.number, item.title(), &default_branch(&repo)?)?
    } else if item.kind == "issue" {
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
    let written = if item.is_task() {
        write_task_context(data_dir, &repo, &item)
    } else if item.kind == "issue" {
        write_issue_context(data_dir, &repo, &item)
    } else {
        write_context(data_dir, &repo, item.number)
    };
    let context_file = match written {
        Ok(file) => Some(file),
        Err(e) => {
            tracing::warn!(
                "{}: starting the agent without context: {e}",
                item.short_name()
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

/// The existing branch a task works on, if it was started on one.
pub fn task_head(item: &WorkItem) -> Option<worktree::BranchRef> {
    item.is_task()
        .then(|| serde_json::from_value(item.snapshot["head"].clone()).ok())
        .flatten()
}

fn write_task_context(data_dir: &Path, repo: &Repo, item: &WorkItem) -> Result<PathBuf, String> {
    let md = crate::context::render_task(
        &item.short_name(),
        item.title(),
        item.snapshot["notes"].as_str().unwrap_or_default(),
        item.branch.as_deref().unwrap_or_default(),
        &default_branch(repo)?,
        task_head(item).as_ref(),
    );
    let dir = data_dir.join("context");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let file = dir.join(format!("{}-{}-{}.md", repo.owner, repo.name, item.slug()));
    std::fs::write(&file, md).map_err(|e| e.to_string())?;
    Ok(file)
}

/// Starts a task (work with no PR or issue yet): the item and its worktree, on a new branch
/// or on the existing `branch` (see `worktree::parse_branch_ref`; e.g. a fork's), whose
/// name is the title if none is given.
pub fn create_task(
    state: &AppState,
    repo: Option<Repo>,
    title: &str,
    notes: &str,
    branch: &str,
) -> Result<i64, String> {
    let repo = repo.ok_or("no repository connected")?;
    let head = (!branch.trim().is_empty())
        .then(|| worktree::parse_branch_ref(branch, &repo.owner, &repo.name))
        .transpose()?;
    let title = match (title.trim(), &head) {
        ("", Some(head)) => head.branch.as_str(),
        (title, _) => title,
    };
    if title.is_empty() {
        return Err("a task needs a title".into());
    }
    let snapshot = head
        .as_ref()
        .map(|h| serde_json::to_value(h).unwrap_or_default());
    let id = state
        .store
        .create_task(repo.id, title, notes.trim(), snapshot.as_ref())?;
    if let Err(e) = prepare(state, id) {
        state.store.delete_item(id)?;
        return Err(e);
    }
    // The branch may have a PR already.
    if head.is_some()
        && let Err(e) = crate::lifecycle::link_branch_pr(state, &repo, &state.store.item(id)?)
    {
        tracing::warn!("task {id}: its branch's PR: {e}");
    }
    Ok(id)
}

/// Links PR or issue `number` (`kind`), which the agent of task `task_id` opened, to the
/// task. It is tracked, so the poller keeps it up to date (an issue the user opened is not
/// assigned to them, so no other query finds it).
pub fn link_created(state: &AppState, task_id: i64, kind: &str, number: i64) -> Result<(), String> {
    let (task, repo) = item_repo(state, task_id)?;
    if !task.is_task() {
        return Ok(());
    }
    let polled = if kind == "issue" {
        polled_issue(&repo, number)?
    } else {
        polled_pr(&repo, number)?
    };
    let id = state.store.track(repo.id, &polled)?;
    state.store.link_to_task(id, task_id)
}

fn polled_issue(repo: &Repo, number: i64) -> Result<PolledPr, String> {
    let issue = github::issues_by_number(&repo.owner, &repo.name, &repo.viewer, &[number])?
        .into_iter()
        .next()
        .ok_or_else(|| format!("issue #{number} not found"))?;
    // The next poll tells whether it is assigned to the viewer.
    Ok(PolledPr {
        number,
        kind: "issue",
        role: "other",
        open: issue.state == "OPEN",
        snapshot: serde_json::to_value(&issue).unwrap_or_default(),
    })
}

fn polled_pr(repo: &Repo, number: i64) -> Result<PolledPr, String> {
    let pr = github::prs_by_number(&repo.owner, &repo.name, &repo.viewer, &[number])?
        .into_iter()
        .next()
        .ok_or_else(|| format!("PR #{number} not found"))?;
    let role = crate::poller::role_of(&pr, &repo.viewer, &Default::default());
    Ok(PolledPr {
        number,
        kind: "pr",
        role,
        open: pr.state == "OPEN",
        snapshot: serde_json::to_value(&pr).unwrap_or_default(),
    })
}

/// Whose agent works on item `id`: its own, or (a PR or issue without a worktree that a
/// task opened) the task's.
pub fn agent_of(state: &AppState, id: i64) -> Result<i64, String> {
    let item = state.store.item(id)?;
    if item.worktree_path.is_none()
        && let Some(task) = item.task_id
        && state.store.item(task)?.worktree_path.is_some()
    {
        return Ok(task);
    }
    Ok(id)
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
        "Here is the current review feedback and CI state of PR #{}:\n\n{}",
        item.number,
        &md[start..]
    ))
}

// --- browsing open PRs and issues ----------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    /// "pr" or "issue".
    pub kind: &'static str,
    pub number: i64,
    pub title: String,
    pub author: String,
    pub is_draft: bool,
    pub labels: Vec<String>,
    pub updated_at: String,
    /// The work item, if it is in the Inbox.
    pub item_id: Option<i64>,
    /// The user can take it out of the Inbox again (see `removable`).
    pub removable: bool,
}

/// Open PRs or issues (`kind`) of `repo` matching `text` (GitHub search syntax: words,
/// `author:x`, …; empty: the most recently updated).
pub fn search(
    state: &AppState,
    repo: Option<Repo>,
    kind: &str,
    text: &str,
) -> Result<Vec<SearchHit>, String> {
    let repo = repo.ok_or("no repository connected")?;
    let items = state.store.items(repo.id)?;
    let terms = text.trim().trim_start_matches('#');
    let hit = |kind: &'static str, number: i64| {
        let item = items.iter().find(|i| i.number == number && listed(i));
        SearchHit {
            kind,
            number,
            item_id: item.map(|i| i.id),
            removable: item.is_some_and(removable),
            title: String::new(),
            author: String::new(),
            is_draft: false,
            labels: vec![],
            updated_at: String::new(),
        }
    };
    Ok(if kind == "issue" {
        github::search_issues(&repo.owner, &repo.name, terms)?
            .into_iter()
            .map(|i| SearchHit {
                title: i.title,
                author: i.author,
                labels: i.labels,
                updated_at: i.updated_at,
                ..hit("issue", i.number)
            })
            .collect()
    } else {
        github::search_pr_stamps(&repo.owner, &repo.name, terms)?
            .into_iter()
            .map(|p| SearchHit {
                title: p.title,
                author: p.author,
                is_draft: p.is_draft,
                updated_at: p.updated_at,
                ..hit("pr", p.number)
            })
            .collect()
    })
}

/// Adds PR or issue `number` (`kind`) to the Inbox (tracked) and returns its work item id.
pub fn track(state: &AppState, repo: Option<Repo>, kind: &str, number: i64) -> Result<i64, String> {
    let repo = repo.ok_or("no repository connected")?;
    let polled = if kind == "issue" {
        polled_issue(&repo, number)?
    } else {
        polled_pr(&repo, number)?
    };
    let id = state.store.track(repo.id, &polled)?;
    state.events.inbox_changed(repo.id);
    Ok(id)
}

/// Takes an item the user added out of the Inbox again.
pub fn untrack(state: &AppState, id: i64) -> Result<(), String> {
    let item = state.store.item(id)?;
    if !removable(&item) {
        return Err(format!(
            "{} is not one you added, or has a worktree",
            item.short_name()
        ));
    }
    state.store.untrack(id)?;
    state.events.inbox_changed(item.repo_id);
    Ok(())
}

/// Stars or unstars item `id`: starred items head the Inbox.
pub fn set_starred(state: &AppState, id: i64, starred: bool) -> Result<(), String> {
    let item = state.store.item(id)?;
    state.store.set_starred(id, starred)?;
    state.events.inbox_changed(item.repo_id);
    Ok(())
}

pub fn conversation(state: &AppState, id: i64) -> Result<github::Conversation, String> {
    let (item, repo) = item_repo(state, id)?;
    if item.is_task() {
        return Err("a task has no conversation on GitHub".into());
    }
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
pub struct ReviewState {
    /// "worktree" (diffs reach the working tree) or "ref" (the fetched PR head).
    pub mode: &'static str,
    pub dir: String,
    /// Merge base of the head and the PR's base branch: "all changes" start here.
    pub base: String,
    pub base_ref: String,
    /// HEAD of the worktree, or the fetched PR head.
    pub head: String,
    /// The PR's head commit on GitHub (line comments are made against it).
    pub pr_head: String,
    /// All commits of the PR (base..head), newest first.
    pub commits: Vec<git::Commit>,
    pub dirty: bool,
    /// The viewer wrote this PR (GitHub does not let them approve it).
    pub own: bool,
}

/// What the review looks at.
struct Review {
    item: WorkItem,
    dir: PathBuf,
    /// The head's content: `None` for the working tree, else the PR head commit.
    view: Option<String>,
    head: String,
    base: String,
    base_ref: String,
    worktree: bool,
}

fn review(state: &AppState, id: i64) -> Result<Review, String> {
    let (item, repo) = item_repo(state, id)?;
    let base_ref = base_ref_of(&item, &repo)?;
    let worktree = item.worktree_path.clone().filter(|p| p.exists());
    // An issue or task has no PR yet: its review needs the worktree, and nothing goes to
    // GitHub.
    if worktree.is_none() && item.is_task() {
        return Err("the task's worktree is gone".into());
    }
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
    })
}

pub fn review_state(state: &AppState, id: i64) -> Result<ReviewState, String> {
    let r = review(state, id)?;
    let pr_head = if r.item.kind == "issue" || r.item.is_task() {
        String::new()
    } else {
        summary(&r.item)?.head_sha
    };
    let commits = git::commits_between(&r.dir, &r.base, &r.head)?;
    let own = r.item.kind == "issue" || r.item.is_task() || {
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
    /// Only whitespace changed (in this scope).
    pub whitespace_only: bool,
}

/// The files changed from `from` to `to` (a commit, or the head when `None`) in review order.
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
    let whitespace = if ignore_ws {
        git::whitespace_only_files(&r.dir, from, to.as_deref())?
    } else {
        Default::default()
    };
    let mut out: Vec<ReviewFile> = files
        .into_iter()
        .map(|f| ReviewFile {
            group: crate::review_order::group_of(&f.path, generated.contains(&f.path)),
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
    if !inside_worktree(path) {
        return Err(format!("refusing to write outside the worktree: {path}"));
    }
    std::fs::write(root.join(path), content).map_err(|e| format!("{path}: {e}"))
}

/// A path relative to the worktree that stays inside it: plain names only (no `..`, no
/// root or drive, no `.`), and not empty.
fn inside_worktree(path: &str) -> bool {
    let relative = Path::new(path);
    relative.components().next().is_some()
        && relative
            .components()
            .all(|c| matches!(c, std::path::Component::Normal(_)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::{TestApp, commit, git, write};
    use serde_json::json;

    #[test]
    fn edits_are_written_only_inside_the_worktree() {
        assert!(inside_worktree("src/a.rs"));
        assert!(inside_worktree("a.txt"));
        for outside in ["../a.txt", "src/../../a", "/etc/passwd", "", "./a"] {
            assert!(!inside_worktree(outside), "{outside}");
        }
        #[cfg(windows)]
        assert!(!inside_worktree("C:\\Windows\\a.txt"));
    }

    #[test]
    fn each_pr_is_listed_for_its_strongest_reason() {
        let app = TestApp::new();
        let pr = |number: i64, author: &str, flags: serde_json::Value| {
            let mut pr =
                json!({"number": number, "title": "t", "head_ref": "main", "author": author});
            for (k, v) in flags.as_object().unwrap() {
                pr[k] = v.clone();
            }
            app.add_pr(pr);
        };
        pr(1, "me", json!({"assigned": true}));
        pr(2, "me", json!({}));
        pr(3, "bob", json!({"requested": true}));
        pr(4, "bob", json!({"requested": true, "reviewed": true}));
        pr(5, "bob", json!({"reviewed": true}));
        pr(6, "bob", json!({}));
        app.poll().unwrap();
        let role = |n| app.row(n).map(|r| r.role);
        assert_eq!(
            (1..=6).map(role).collect::<Vec<_>>(),
            [
                Some("assignee".into()),
                Some("author".into()),
                Some("requested".into()),
                // Asked to review again.
                Some("requested".into()),
                Some("reviewed".into()),
                None
            ]
        );
        // The review is done.
        app.set("prs", 4, json!({"requested": false}));
        app.poll().unwrap();
        assert_eq!(role(4).as_deref(), Some("reviewed"));
    }

    #[test]
    fn an_added_item_stays_while_starred_and_can_be_removed_again() {
        let app = TestApp::new();
        let repo = || Some(app.repo.clone());
        app.add_pr(
            json!({"number": 9, "title": "Someone's fix", "head_ref": "main", "author": "bob"}),
        );
        let hits = search(&app.state, repo(), "pr", "fix").unwrap();
        assert_eq!(
            hits.iter()
                .map(|h| (h.number, h.item_id))
                .collect::<Vec<_>>(),
            [(9, None)]
        );
        let id = track(&app.state, repo(), "pr", 9).unwrap();
        let row = app.row(9).unwrap();
        assert!(row.role == "other" && row.removable && !row.starred);
        let hit = &search(&app.state, repo(), "pr", "fix").unwrap()[0];
        assert!(hit.item_id == Some(id) && hit.removable);

        set_starred(&app.state, id, true).unwrap();
        app.poll().unwrap();
        assert!(app.row(9).unwrap().starred);

        untrack(&app.state, id).unwrap();
        assert!(app.row(9).is_none());
        app.poll().unwrap();
        assert!(app.row(9).is_none(), "not polled any more");
        assert_eq!(
            search(&app.state, repo(), "pr", "fix").unwrap()[0].item_id,
            None
        );

        // A review request brings it back, and then it is not the user's to remove.
        app.set("prs", 9, json!({"requested": true}));
        app.poll().unwrap();
        let row = app.row(9).unwrap();
        assert!(row.role == "requested" && !row.removable && !row.starred);
        assert!(untrack(&app.state, id).is_err());
    }

    #[test]
    fn open_issues_can_be_browsed_and_added() {
        let app = TestApp::new();
        let repo = || Some(app.repo.clone());
        app.add_issue(json!({"number": 3, "title": "Crash on start"}));
        app.add_issue(json!({"number": 4, "title": "Old crash", "state": "CLOSED"}));
        app.add_issue(json!({"number": 5, "title": "Docs typo"}));
        let numbers = |text: &str| {
            search(&app.state, repo(), "issue", text)
                .unwrap()
                .iter()
                .map(|h| (h.kind, h.number, h.item_id.is_some()))
                .collect::<Vec<_>>()
        };
        assert_eq!(numbers("crash"), [("issue", 3, false)]);
        assert_eq!(numbers("").len(), 2, "every open issue");
        track(&app.state, repo(), "issue", 3).unwrap();
        assert_eq!(numbers("crash"), [("issue", 3, true)]);
        app.poll().unwrap();
        let row = app.row(3).unwrap();
        assert_eq!((row.kind.as_str(), row.role.as_str()), ("issue", "other"));
    }

    #[test]
    fn a_task_lists_what_its_agent_opened_and_lends_it_its_agent() {
        let app = TestApp::new();
        let task = create_task(
            &app.state,
            Some(app.repo.clone()),
            "Investigate",
            "peers",
            "",
        )
        .unwrap();
        let t = app.state.store.item(task).unwrap();
        assert_eq!(
            (t.number, t.branch.as_deref()),
            (-1, Some("task-1-investigate"))
        );
        assert!(app.path("proj_task1").join("a.txt").exists());
        let context =
            std::fs::read_to_string(app.state.data_dir.join("context/owner-proj-task1.md"));
        assert!(
            context.is_err(),
            "the context is written when the agent starts"
        );

        app.add_pr(json!({"number": 20, "title": "Fix", "head_ref": "main"}));
        app.add_issue(json!({"number": 21, "title": "Found", "author": "me"}));
        link_created(&app.state, task, "pr", 20).unwrap();
        link_created(&app.state, task, "issue", 21).unwrap();
        let rows = app.rows();
        let row = |n: i64| rows.iter().find(|r| r.number == n).unwrap();
        assert_eq!(
            (row(-1).kind.as_str(), row(-1).title.as_str()),
            ("task", "Investigate")
        );
        assert_eq!((row(20).task_id, row(21).task_id), (Some(task), Some(task)));
        assert_eq!(
            row(20).agent_item,
            Some(task),
            "the PR has no worktree: the task's agent"
        );
        assert_eq!(agent_of(&app.state, app.item(20).id).unwrap(), task);
        assert!(!row(-1).outcomes_closed);

        // Both closed (the PR merged and cleaned up): still listed under the task.
        app.set("prs", 20, json!({"state": "MERGED"}));
        app.set("issues", 21, json!({"state": "CLOSED"}));
        app.poll().unwrap();
        assert!(app.item(20).hidden, "the merged PR was cleaned up");
        let rows = app.rows();
        let row = |n: i64| rows.iter().find(|r| r.number == n);
        assert!(row(20).is_some_and(|r| !r.open) && row(21).is_some_and(|r| !r.open));
        assert!(row(-1).unwrap().outcomes_closed);
        assert_eq!(
            row(-1).unwrap().attention,
            Some("all its PRs and issues are closed")
        );
    }

    #[test]
    fn a_task_whose_worktree_cannot_be_made_is_not_left_behind() {
        let app = TestApp::new();
        std::fs::create_dir_all(app.path("proj_task1")).unwrap();
        let err = create_task(&app.state, Some(app.repo.clone()), "Blocked", "", "").unwrap_err();
        assert!(err.contains("already exists"), "{err}");
        assert!(app.items().iter().all(|i| !i.is_task()));
        assert!(
            create_task(&app.state, Some(app.repo.clone()), "  ", "", "").is_err(),
            "no title"
        );
    }

    #[test]
    fn a_task_reviews_its_worktree_against_the_default_branch() {
        let app = TestApp::new();
        let task = create_task(&app.state, Some(app.repo.clone()), "Try", "", "").unwrap();
        let wt = app.path("proj_task1");
        write(&wt.join("a.txt"), "one\ntwo\nthree\nfour\n");
        commit(&wt, "four", "me");
        let state = review_state(&app.state, task).unwrap();
        assert_eq!(
            (
                state.mode,
                state.base_ref.as_str(),
                state.pr_head.as_str(),
                state.own
            ),
            ("worktree", "main", "", true)
        );
        assert_eq!(state.commits.len(), 1);
    }

    #[test]
    fn reconcile_forgets_worktrees_removed_outside_ariekei() {
        let app = TestApp::new();
        let task = create_task(&app.state, Some(app.repo.clone()), "Gone", "", "").unwrap();
        git(
            &app.clone,
            &[
                "worktree",
                "remove",
                "--force",
                &app.path("proj_task1").to_string_lossy(),
            ],
        );
        reconcile(&app.state).unwrap();
        let t = app.state.store.item(task).unwrap();
        assert_eq!((t.worktree_path, t.branch), (None, None));
        // Start working makes it again.
        assert!(
            prepare(&app.state, task)
                .unwrap()
                .path
                .ends_with("proj_task1")
        );
    }

    /// The task's worktree, its branch's upstream and the URL of that upstream's remote.
    fn checkout(app: &TestApp, id: i64) -> (PathBuf, String, String, String) {
        let item = app.state.store.item(id).unwrap();
        let path = item.worktree_path.clone().unwrap();
        let branch = item.branch.clone().unwrap();
        let upstream = crate::git::upstream(&app.clone, &branch).unwrap_or_default();
        let remote = upstream.split('/').next().unwrap_or_default().to_owned();
        let url = crate::git::git_str(
            &app.clone,
            &["config", "--get", &format!("remote.{remote}.url")],
        )
        .unwrap_or_default();
        (path, branch, upstream, url)
    }

    #[test]
    fn a_task_on_a_fork_branch_adds_its_remote_and_pushes_back() {
        let app = TestApp::new();
        let fork = app.fork("friend", "feat/evm");
        let id = create_task(
            &app.state,
            Some(app.repo.clone()),
            "",
            "",
            "friend:feat/evm",
        )
        .unwrap();
        let item = app.state.store.item(id).unwrap();
        assert_eq!(item.title(), "feat/evm", "named after the branch");
        assert_eq!(task_head(&item).unwrap().owner, "friend");
        let (path, branch, upstream, url) = checkout(&app, id);
        assert_eq!(
            (branch.as_str(), upstream.as_str(), url.as_str()),
            (
                "feat/evm",
                "friend/feat/evm",
                "git@github.com:friend/proj.git"
            )
        );
        // trim: the checkout may have CRLF (Windows).
        assert_eq!(
            std::fs::read_to_string(path.join("c.txt"))
                .unwrap()
                .trim_end(),
            "friend's work"
        );
        // A plain push updates the fork's branch.
        write(&path.join("c.txt"), "more\n");
        commit(&path, "more", "me");
        git(&path, &["push", "-q"]);
        assert_eq!(
            git(&fork, &["log", "-1", "--format=%s", "feat/evm"]),
            "more"
        );
        // Started again (e.g. after a restart): the same worktree.
        app.state.store.set_worktree(id, None, None, false).unwrap();
        let again = prepare(&app.state, id).unwrap();
        assert!(again.adopted);
    }

    #[test]
    fn a_task_on_a_fork_branch_uses_the_remote_the_clone_has_for_it() {
        let app = TestApp::new();
        app.fork("friend", "feature-x");
        git(
            &app.clone,
            &["remote", "add", "mine", "git@github.com:friend/proj.git"],
        );
        let id = create_task(
            &app.state,
            Some(app.repo.clone()),
            "Finish it",
            "",
            "https://github.com/friend/proj/tree/feature-x",
        )
        .unwrap();
        assert_eq!(checkout(&app, id).2, "mine/feature-x");
        assert!(
            !git(&app.clone, &["remote"]).lines().any(|r| r == "friend"),
            "no second remote"
        );
    }

    #[test]
    fn a_missing_fork_branch_leaves_no_task_and_no_remote() {
        let app = TestApp::new();
        app.fork("friend", "feature-x");
        let err =
            create_task(&app.state, Some(app.repo.clone()), "", "", "friend:nope").unwrap_err();
        assert!(
            err.contains("could not fetch nope from friend/proj"),
            "{err}"
        );
        assert!(!git(&app.clone, &["remote"]).lines().any(|r| r == "friend"));
        assert!(app.items().iter().all(|i| !i.is_task()));
        let err = create_task(&app.state, Some(app.repo.clone()), "", "", "a b").unwrap_err();
        assert!(err.contains("is not a branch"), "{err}");
    }

    #[test]
    fn a_local_branch_of_that_name_tracking_something_else_is_left_alone() {
        let app = TestApp::new();
        app.fork("friend", "feature-x");
        git(
            &app.clone,
            &["branch", "--track", "feature-x", "origin/main"],
        );
        let id = create_task(
            &app.state,
            Some(app.repo.clone()),
            "",
            "",
            "friend:feature-x",
        )
        .unwrap();
        let (_, branch, upstream, _) = checkout(&app, id);
        assert_eq!(
            (branch.as_str(), upstream.as_str()),
            ("task-1-feature-x", "friend/feature-x")
        );
        assert_eq!(
            crate::git::upstream(&app.clone, "feature-x").as_deref(),
            Some("origin/main")
        );
    }
}
