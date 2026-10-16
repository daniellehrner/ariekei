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

/// Where the item's review marker is kept (also deleted by cleanup).
pub fn reviewed_ref(item: &WorkItem) -> String {
    if item.is_task() {
        format!("refs/ariekei/reviewed/task-{}", -item.number)
    } else {
        format!("refs/ariekei/reviewed/pr-{}", item.number)
    }
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

/// Changes since the review (or the branch point) the user has not seen: uncommitted
/// changes, or commits someone else made. The user's own commits on the PR, and what their
/// merges brought in, count as seen; local commits GitHub does not know yet do not.
fn unreviewed(state: &AppState, cwd: &Path, item: &WorkItem, base_ref: &str, repo: &Repo) -> bool {
    let Ok(head) = git::head(cwd) else {
        return false;
    };
    if git::is_dirty(cwd).unwrap_or(false) {
        return true;
    }
    let base = format!("{}/{}", repo.base_remote, base_ref);
    let marker = reviewed_marker(cwd, item).or_else(|| git::merge_base(cwd, "HEAD", &base).ok());
    if marker.as_deref() == Some(head.as_str()) {
        return false;
    }
    let own = state
        .store
        .own_commits(item.id)
        .ok()
        .flatten()
        .unwrap_or_default()
        .1;
    let exclude: Vec<&str> = [Some(base.as_str()), marker.as_deref()]
        .into_iter()
        .flatten()
        .collect();
    git::has_others_commits(cwd, &head, &exclude, &own).unwrap_or(true)
}

/// Fetches which commits of the item's PR are the viewer's, when its head moved since.
pub fn refresh_own_commits(state: &AppState, repo: &Repo, item: &WorkItem) -> Result<(), String> {
    let pr = summary(item)?;
    let known = state.store.own_commits(item.id)?.map(|(head, _)| head);
    if known.as_deref() == Some(pr.head_sha.as_str()) {
        return Ok(());
    }
    let own = github::commits_by(&repo.owner, &repo.name, item.number, &repo.viewer)?;
    state.store.set_own_commits(item.id, &pr.head_sha, &own)
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
    let default = default_branch(&repo).unwrap_or_else(|_| "main".into());
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
            let mut row = row(state, &repo, &default, item.clone())?;
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

fn row(state: &AppState, repo: &Repo, default_branch: &str, item: WorkItem) -> Option<Row> {
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
            unreviewed: worktree
                .as_deref()
                .is_some_and(|w| unreviewed(state, w, &item, default_branch, repo)),
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
            unreviewed: worktree
                .as_deref()
                .is_some_and(|w| unreviewed(state, w, &item, default_branch, repo)),
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
            unreviewed: worktree
                .as_deref()
                .is_some_and(|w| unreviewed(state, w, &item, &pr.base_ref, repo)),
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
    let prepared = if item.is_task() {
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

fn write_task_context(data_dir: &Path, repo: &Repo, item: &WorkItem) -> Result<PathBuf, String> {
    let md = crate::context::render_task(
        &item.short_name(),
        item.title(),
        item.snapshot["notes"].as_str().unwrap_or_default(),
        item.branch.as_deref().unwrap_or_default(),
        &default_branch(repo)?,
    );
    let dir = data_dir.join("context");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let file = dir.join(format!("{}-{}-{}.md", repo.owner, repo.name, item.slug()));
    std::fs::write(&file, md).map_err(|e| e.to_string())?;
    Ok(file)
}

/// Starts a task (work with no PR or issue yet): the item and its worktree on a new branch.
pub fn create_task(
    state: &AppState,
    repo: Option<Repo>,
    title: &str,
    notes: &str,
) -> Result<i64, String> {
    let repo = repo.ok_or("no repository connected")?;
    let title = title.trim();
    if title.is_empty() {
        return Err("a task needs a title".into());
    }
    let id = state.store.create_task(repo.id, title, notes.trim())?;
    if let Err(e) = prepare(state, id) {
        state.store.delete_item(id)?;
        return Err(e);
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
    let pr_head = if r.item.kind == "issue" || r.item.is_task() {
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
    git(&repo.path, &["update-ref", &reviewed_ref(&item), &r.head])?;
    state.store.set_reviewed(id, &r.head)?;
    Ok(r.head)
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
    fn changes_to_review_are_the_worktrees_changes_by_others() {
        let app = TestApp::new();
        let (id, wt) = app.pr_with_worktree(10, "feature/x");
        app.poll().unwrap(); // learns which of the PR's commits are mine
        assert!(
            !app.row(10).unwrap().unreviewed,
            "my own commit needs no review"
        );

        write(&wt.join("scratch.txt"), "draft\n");
        assert!(app.row(10).unwrap().unreviewed, "uncommitted changes do");
        std::fs::remove_file(wt.join("scratch.txt")).unwrap();

        // A teammate pushes to the branch; the worktree gets their commit.
        app.push_to("feature/x", &[("c.txt", "carol's\n")], "carol");
        git(&wt, &["pull", "-q", "--ff-only"]);
        app.poll().unwrap();
        assert!(app.row(10).unwrap().unreviewed, "a teammate's commit does");

        mark_reviewed(&app.state, id).unwrap();
        assert!(!app.row(10).unwrap().unreviewed, "nothing after the review");
    }

    #[test]
    fn the_prs_commits_are_fetched_again_only_when_its_head_moved() {
        let app = TestApp::new();
        app.pr_with_worktree(10, "feature/x");
        app.poll().unwrap();
        app.take_calls();
        app.poll().unwrap();
        assert!(!app.take_calls().iter().any(|c| c["commits"] == true));
        app.push_to("feature/x", &[("c.txt", "more\n")], "me");
        app.poll().unwrap();
        assert!(app.take_calls().iter().any(|c| c["commits"] == true));
    }

    #[test]
    fn after_a_rebase_a_file_only_the_base_changed_stays_reviewed() {
        let app = TestApp::new();
        let d = |last: &str| format!("d1 mine\nd2\nd3\nd4\nd5\n{last}\n");
        write(&app.upstream.join("d.txt"), "d1\nd2\nd3\nd4\nd5\nd6\n");
        commit(&app.upstream, "d", "me");
        git(&app.clone, &["fetch", "-q", "origin"]);
        app.push_to(
            "feature/x",
            &[("a.txt", "one mine\ntwo\nthree\n"), ("d.txt", &d("d6"))],
            "me",
        );
        app.add_pr(json!({"number": 10, "title": "x", "head_ref": "feature/x"}));
        app.poll().unwrap();
        let id = app.item(10).id;
        let wt = prepare(&app.state, id).unwrap().path;
        mark_reviewed(&app.state, id).unwrap();

        // main changes d.txt far from the PR's line; the branch is rebased onto it and then
        // changes a.txt again.
        write(&app.upstream.join("d.txt"), "d1\nd2\nd3\nd4\nd5\nd6 main\n");
        commit(&app.upstream, "main moves on", "carol");
        git(&app.upstream, &["checkout", "-q", "feature/x"]);
        git(
            &app.upstream,
            &[
                "-c",
                "user.name=me",
                "-c",
                "user.email=me@example.com",
                "rebase",
                "-q",
                "main",
            ],
        );
        git(&app.upstream, &["checkout", "-q", "main"]);
        app.push_to(
            "feature/x",
            &[("a.txt", "one mine, better\ntwo\nthree\n")],
            "me",
        );
        git(&wt, &["fetch", "-q", "origin"]);
        git(&wt, &["reset", "-q", "--hard", "origin/feature/x"]);

        let state = review_state(&app.state, id).unwrap();
        assert!(
            state.rebase_note.is_some(),
            "the review knows about the rebase"
        );
        let files = review_files(&app.state, id, &state.base, None, false).unwrap();
        let marks: Vec<(&str, bool)> = files
            .iter()
            .map(|f| (f.path.as_str(), f.reviewed))
            .collect();
        assert_eq!(marks, [("a.txt", false), ("d.txt", true)]);
    }

    #[test]
    fn a_task_lists_what_its_agent_opened_and_lends_it_its_agent() {
        let app = TestApp::new();
        let task = create_task(&app.state, Some(app.repo.clone()), "Investigate", "peers").unwrap();
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
        let err = create_task(&app.state, Some(app.repo.clone()), "Blocked", "").unwrap_err();
        assert!(err.contains("already exists"), "{err}");
        assert!(app.items().iter().all(|i| !i.is_task()));
        assert!(
            create_task(&app.state, Some(app.repo.clone()), "  ", "").is_err(),
            "no title"
        );
    }

    #[test]
    fn a_task_reviews_its_worktree_against_the_default_branch() {
        let app = TestApp::new();
        let task = create_task(&app.state, Some(app.repo.clone()), "Try", "").unwrap();
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
        assert!(
            app.row(-1).unwrap().unreviewed,
            "the agent's commit is to review"
        );
    }

    #[test]
    fn reconcile_forgets_worktrees_removed_outside_ariekei() {
        let app = TestApp::new();
        let task = create_task(&app.state, Some(app.repo.clone()), "Gone", "").unwrap();
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
}
