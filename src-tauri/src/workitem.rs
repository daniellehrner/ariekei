// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

//! Work item operations behind the UI commands: connecting a repo, the Inbox rows,
//! preparing a PR's worktree, starting its agent with context, and review markers.

use crate::AppState;
use crate::git;
use crate::github::{self, PrSummary};
use crate::inbox::{self, Row};
use crate::store::{NOTHING_SEEN, PolledPr, Repo, WorkItem};
use crate::terminal::{self, SessionPlan};
use crate::worktree::{self, Prepared};
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub fn summary(item: &WorkItem) -> Result<PrSummary, String> {
    serde_json::from_value(item.snapshot.clone())
        .map_err(|e| format!("PR #{}: bad snapshot: {e}", item.number))
}

pub(crate) fn item_repo(state: &AppState, id: i64) -> Result<(WorkItem, Repo), String> {
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

/// The default branch to start new work from: GitHub's, which may have been renamed since the
/// clone was made (fetching never updates `<remote>/HEAD`); `<remote>/HEAD` is moved to it, so
/// the review compares with it too. The clone's if GitHub cannot be asked.
fn current_default_branch(repo: &Repo) -> Result<String, String> {
    let branch = match github::default_branch(&repo.owner, &repo.name) {
        Ok(branch) => branch,
        Err(e) => {
            tracing::warn!("the default branch of {}/{}: {e}", repo.owner, repo.name);
            return default_branch(repo);
        }
    };
    let head = format!("refs/remotes/{}/HEAD", repo.base_remote);
    let tracking = format!("refs/remotes/{}/{branch}", repo.base_remote);
    if git::git_str(&repo.path, &["symbolic-ref", &head]).ok() != Some(tracking.clone())
        && let Err(e) = git::git(&repo.path, &["symbolic-ref", &head, &tracking])
    {
        tracing::warn!("could not point {head} at {tracking}: {e}");
    }
    Ok(branch)
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
            state
                .store
                .set_worktree(item.id, None, None, false, false)?;
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
    let rows = state
        .store
        .items(repo.id)?
        .into_iter()
        .filter(listed)
        .filter_map(|item| row(state, &repo, item))
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
            ready_for_review: false,
            worktree: None,
            branch: item.branch.clone(),
            moved_to: None,
            can_follow: false,
            update: None,
            adopted: false,
            attention: None,
            finished: None,
            kept: false,
            cleanup_blockers: None,
            issue_number: None,
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
            ready_for_review: false,
            worktree: None,
            branch: item.branch.clone(),
            moved_to: None,
            can_follow: false,
            update: None,
            adopted: false,
            attention: None,
            finished: None,
            kept: false,
            cleanup_blockers: None,
            issue_number: None,
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
            ready_for_review: false,
            worktree: None,
            branch: item.branch.clone(),
            moved_to: None,
            can_follow: false,
            update: None,
            adopted: false,
            attention: None,
            finished: None,
            kept: false,
            cleanup_blockers: None,
            issue_number: item.issue_number,
            starred: item.starred,
            removable: false,
            notes: None,
        }
    };
    row.ready_for_review = state.terminals.ready_for_review(&item.id.to_string());
    row.moved_to = moved_to(&item);
    row.can_follow = followable(&item, repo).is_ok();
    if worktree.is_some() && row.moved_to.is_none() {
        row.update = update_of(&item);
    }
    row.adopted = worktree.is_some() && !item.worktree_created;
    row.removable = removable(&item);
    row.worktree = worktree.map(|p| p.to_string_lossy().into_owned());
    if !item.is_task() {
        row.finished = match item.snapshot["state"].as_str() {
            Some("MERGED") => Some("merged"),
            Some("CLOSED") => Some("closed"),
            _ => None,
        };
    }
    row.kept = item.kept;
    row.cleanup_blockers = item.cleanup_blockers;
    Some(row)
}

/// Whether the user can take the item out of the Inbox: only a PR or issue they added, which
/// no query finds and nothing local hangs on (a worktree is finished with Done instead).
fn removable(item: &WorkItem) -> bool {
    !item.is_task() && item.role == "other" && item.worktree_path.is_none()
}

/// Marks an item done: cleans it up now if nothing would be lost.
pub fn mark_done(state: &AppState, id: i64) -> Result<crate::lifecycle::Cleanup, String> {
    crate::lifecycle::clean_up(state, id)
}

/// How far the user had seen an item's GitHub activity (its `latest_activity`, or `NOTHING_SEEN`), before and
/// after `mark_seen`.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Seen {
    pub before: Option<String>,
    pub after: String,
}

/// Opening an item counts as seeing its current GitHub activity, and keeps it in the
/// Inbox from now on (e.g. after the review request is gone because the user reviewed). So
/// does loading its conversation while it is open. `before` tells the item which comments are
/// new to the user.
pub fn mark_seen(state: &AppState, id: i64) -> Result<Seen, String> {
    let item = state.store.item(id)?;
    if !item.tracked {
        state.store.set_tracked(id)?;
    }
    let latest = match item.kind.as_str() {
        "issue" => issue_summary(&item).ok().and_then(|i| i.latest_activity),
        _ => summary(&item).ok().and_then(|p| p.latest_activity),
    };
    let before = item.last_seen.filter(|s| !s.is_empty());
    // Seen with nothing on it: whatever comes later is new.
    let after = latest
        .or_else(|| before.clone())
        .unwrap_or_else(|| NOTHING_SEEN.to_owned());
    if before.as_deref() != Some(after.as_str()) {
        state.store.set_last_seen(id, &after)?;
    }
    Ok(Seen { before, after })
}

pub fn prepare(state: &AppState, id: i64) -> Result<Prepared, String> {
    let (item, repo) = item_repo(state, id)?;
    if let Some(path) = item.worktree_path.as_ref().filter(|p| p.exists()) {
        return Ok(Prepared {
            path: path.clone(),
            branch: item.branch.clone().unwrap_or_default(),
            adopted: true,
            branch_created: false,
            note: None,
        });
    }
    // The branch a task was given before, if it had one.
    let own = item.snapshot["branch"].as_str().filter(|_| item.is_task());
    let prepared = if let Some(head) = task_head(&item) {
        worktree::prepare_task_on(
            &repo,
            -item.number,
            item.title(),
            &head,
            &current_default_branch(&repo)?,
            own,
        )?
    } else if item.is_task() {
        let default = current_default_branch(&repo)?;
        worktree::prepare_task(&repo, -item.number, item.title(), &default, own)?
    } else if item.kind == "issue" {
        let issue = issue_summary(&item)?;
        let default = current_default_branch(&repo)?;
        worktree::prepare_issue(&repo, item.number, &issue.title, &default)?
    } else {
        worktree::prepare(&repo, &summary(&item)?)?
    };
    state.store.set_worktree(
        id,
        Some(&prepared.path),
        Some(&prepared.branch),
        !prepared.adopted,
        prepared.branch_created,
    )?;
    if item.is_task() && own.is_none() {
        state.store.set_task_branch(id, &prepared.branch)?;
    }
    Ok(prepared)
}

/// Resume the item's conversation if it has one; otherwise start a new one with the PR's
/// current context appended to the system prompt. Also while its worktree is on another
/// branch (`moved_to`): a start sends the agent no work, and the user may want to talk to it.
pub fn session_plan(state: &AppState, data_dir: &Path, id: i64) -> Result<SessionPlan, String> {
    let (mut item, repo) = item_repo(state, id)?;
    // Hooks may never have said that it was prompted (hooks turned off, or another Ariekei's
    // settings file): a transcript on disk says so. Claude Code refuses to start a new
    // conversation under the id of one it saved.
    if let Some(session_id) = item.session_id.as_deref().filter(|_| !item.session_started)
        && let Some(transcript) = state.terminals.find_transcript(session_id)
    {
        state.store.mark_session_started(id, Some(&transcript))?;
        (item.session_started, item.transcript_path) = (true, Some(transcript));
    }
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
        write_context(data_dir, &repo, &item)
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

/// The user wrote the PR (else they review someone else's).
fn by_viewer(item: &WorkItem, repo: &Repo) -> bool {
    item.snapshot["author"]
        .as_str()
        .is_some_and(|a| a.eq_ignore_ascii_case(&repo.viewer))
}

fn write_context(data_dir: &Path, repo: &Repo, item: &WorkItem) -> Result<PathBuf, String> {
    let number = item.number;
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
    let mut md = crate::context::render(number, &detail, &logs);
    if !by_viewer(item, repo) {
        md.push_str(crate::context::REVIEWING);
    }
    std::fs::write(&file, with_origin(md, item)).map_err(|e| e.to_string())?;
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
    std::fs::write(&file, with_origin(md, item)).map_err(|e| e.to_string())?;
    Ok(file)
}

/// The context `md`, then how the item came about, if a task's agent opened it.
fn with_origin(md: String, item: &WorkItem) -> String {
    match item.origin.as_deref() {
        Some(origin) => format!("{md}\n{origin}"),
        None => md,
    }
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
        item.branch.as_deref().is_some_and(|b| {
            crate::lifecycle::pushed_head(repo, b)
                .ok()
                .flatten()
                .is_some()
        }),
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
        .create_task(repo.id, title, notes.trim(), snapshot.as_ref(), |n| {
            worktree::task_number_taken(&repo, n)
        })?;
    if let Err(e) = prepare(state, id) {
        state.store.delete_item(id)?;
        return Err(e);
    }
    // The branch may have a PR already.
    if head.is_some()
        && let Err(e) = crate::lifecycle::track_branch_pr(state, &repo, &state.store.item(id)?)
    {
        tracing::warn!("task {id}: its branch's PR: {e}");
    }
    Ok(id)
}

/// Adds PR or issue `number` (`kind`), which the agent of task `task_id` opened (or opened
/// for its branch), to the Inbox as an item of its own. It is tracked, so the poller keeps it
/// up to date (an issue the user opened is not assigned to them, so no other query finds it),
/// and keeps how it came about for the agent that later works on it (`origin_of`).
pub fn track_opened(state: &AppState, task_id: i64, kind: &str, number: i64) -> Result<(), String> {
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
    state.store.set_origin(id, &origin_of(state, &task, kind))
}

/// How a PR or issue (`kind`) the agent of `task` opened came about: the task and its
/// conversation so far.
fn origin_of(state: &AppState, task: &WorkItem, kind: &str) -> String {
    let transcript = task.transcript_path.clone().or_else(|| {
        task.session_id
            .as_deref()
            .and_then(|s| state.terminals.find_transcript(s))
    });
    crate::context::render_origin(
        if kind == "issue" {
            "issue"
        } else {
            "pull request"
        },
        &task.short_name(),
        task.title(),
        task.snapshot["notes"].as_str().unwrap_or_default(),
        task.branch.as_deref().unwrap_or_default(),
        &transcript
            .map(|t| crate::context::conversation(&t))
            .unwrap_or_default(),
    )
}

/// PRs and issues used to be linked to the task whose agent opened them; now they are items of
/// their own, which keep how they came about instead.
pub fn unlink_from_tasks(state: &AppState) -> Result<(), String> {
    for (id, task) in state.store.linked_to_tasks()? {
        let item = state.store.item(id)?;
        if let Ok(task) = state.store.item(task)
            && task.is_task()
        {
            state
                .store
                .set_origin(id, &origin_of(state, &task, &item.kind))?;
        }
        state.store.unlink_from_task(id)?;
    }
    Ok(())
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

/// The branch the item's worktree is on instead of the item's (`""`: a detached HEAD), if
/// it was switched. Mostly the user's own checkout, which Ariekei adopts but the user also
/// works in. Not once the PR or issue is finished: its branch is done with (often the agent
/// that merged it switched back to the base branch). A rebase or bisect of the item's branch
/// (e.g. the agent's, stopped on a conflict) detaches HEAD but has not moved it: the agent
/// still works on the branch.
pub fn moved_to(item: &WorkItem) -> Option<String> {
    if !item.open {
        return None;
    }
    let dir = item.worktree_path.as_deref().filter(|p| p.exists())?;
    let branch = item.branch.as_deref().filter(|b| !b.is_empty())?;
    let now = match git::current_branch(dir).ok()? {
        Some(now) => now,
        None => git::operation_branch(dir).unwrap_or_default(),
    };
    (now != branch).then_some(now)
}

/// Whether GitHub's head of PR `item` is one its worktree's branch lacks: "new" when the clone
/// has not fetched it, "fast-forward" when the branch can move up to it, "rewritten" when the
/// author rewrote the PR's history since (e.g. rebased it). Not when the branch is ahead with
/// commits of its own: those are for pushing. Nor when the branch had GitHub's head before
/// (its reflog says so) and was rewritten here since, e.g. rebased by the agent: GitHub has
/// nothing it lacks, the rewrite is for pushing too. Nor while GitHub has the head it had when
/// the item followed another branch (`follow_branch`): that one is the PR's next version.
pub fn update_of(item: &WorkItem) -> Option<&'static str> {
    if item.kind != "pr" || !item.open {
        return None;
    }
    let dir = item.worktree_path.as_deref().filter(|p| p.exists())?;
    let branch = item.branch.as_deref().filter(|b| !b.is_empty())?;
    let head = summary(item).ok()?.head_sha;
    if item.followed_head.as_deref() == Some(head.as_str()) {
        return None;
    }
    let tip = git::git_str(
        dir,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ],
    )
    .ok()?;
    if head.is_empty() || head == tip {
        None
    } else if !git::commit_exists(dir, &head) {
        Some("new")
    } else if git::is_ancestor(dir, &head, &tip) || git::branch_had(dir, branch, &head) {
        None
    } else if git::is_ancestor(dir, &tip, &head) {
        Some("fast-forward")
    } else {
        Some("rewritten")
    }
}

/// "N commits are …: a; b": the commits an action will not leave behind.
fn commits_list(commits: &[String]) -> String {
    let more = match commits.len() {
        n if n > 5 => format!("; and {} more", n - 5),
        _ => String::new(),
    };
    format!(
        "{} commit{} ({}{more})",
        commits.len(),
        if commits.len() == 1 { "" } else { "s" },
        commits[..commits.len().min(5)].join("; ")
    )
}

/// Puts item `id`'s worktree back on its branch after someone switched it (`moved_to`). A
/// detached HEAD's commits that no branch or remote has go along when they build on the
/// branch (it moves up to them); else switching would leave them in the reflog only, and it
/// refuses. Uncommitted changes go along, or git refuses.
pub fn switch_back(state: &AppState, id: i64) -> Result<(), String> {
    let item = state.store.item(id)?;
    let Some(now) = moved_to(&item) else {
        return Ok(());
    };
    let dir = worktree_of(&item)?;
    let branch = item.branch.clone().unwrap_or_default();
    if now.is_empty() {
        let head = git::head(&dir)?;
        let loose = git::git_str(
            &dir,
            &[
                "log",
                "--format=%h %s",
                "HEAD",
                "--not",
                "--branches",
                "--remotes",
                "--glob=refs/ariekei/pull/*",
            ],
        )?;
        if !loose.is_empty() {
            let loose: Vec<String> = loose.lines().map(str::to_owned).collect();
            if !git::is_ancestor(&dir, &format!("refs/heads/{branch}"), "HEAD") {
                return Err(format!(
                    "HEAD has {} on no branch: switching to {branch} would leave them in the \
                     reflog only. Move {branch} here, or put them on a branch first.",
                    commits_list(&loose)
                ));
            }
            git::git(&dir, &["switch", "--quiet", &branch])?;
            return git::git(&dir, &["merge", "--ff-only", "--quiet", &head]).map(|_| ());
        }
    }
    git::git(&dir, &["switch", "--quiet", &branch]).map(|_| ())
}

/// The branch `item`'s worktree was switched to, if the item can take it as its own: not a
/// detached HEAD, nor the default branch (e.g. the user's checkout, switched back to it).
fn followable(item: &WorkItem, repo: &Repo) -> Result<String, String> {
    match moved_to(item) {
        None => Err("its worktree is on its branch".into()),
        Some(now) if now.is_empty() => {
            Err("its worktree is on a detached HEAD, not on a branch".into())
        }
        Some(now) if default_branch(repo).is_ok_and(|d| d == now) => Err(format!(
            "{now} is the default branch: the agent works on a branch of its own"
        )),
        Some(now) => Ok(now),
    }
}

/// Makes the branch item `id`'s worktree was switched to (`moved_to`) the item's branch, e.g.
/// one a task's agent started for the next part of the work (the branch's PR is linked to the
/// task), or a rewrite of a PR that the user pushes to it later: the agent gets work again.
/// The PR on GitHub stays as it is, and the Update bar waits until GitHub has another head
/// (`update_of`). Cleanup keeps both branches (Ariekei created neither of them now).
pub fn follow_branch(state: &AppState, id: i64) -> Result<(), String> {
    let (item, repo) = item_repo(state, id)?;
    let now = followable(&item, &repo)?;
    let head = match item.kind.as_str() {
        "pr" => Some(summary(&item)?.head_sha),
        _ => None,
    };
    state.store.set_worktree(
        id,
        item.worktree_path.as_deref(),
        Some(&now),
        item.worktree_created,
        false,
    )?;
    state.store.set_followed_head(id, head.as_deref())
}

/// Moves item `id`'s branch to its worktree's detached HEAD and puts the worktree on it: for
/// a checkout of a newer version (e.g. the author's rebased PR). Refuses while the branch has
/// commits HEAD lacks that GitHub does not have.
pub fn move_branch_here(state: &AppState, id: i64) -> Result<(), String> {
    let (item, repo) = item_repo(state, id)?;
    if moved_to(&item).as_deref() != Some("") {
        return Err("its worktree is not on a detached HEAD".into());
    }
    let dir = worktree_of(&item)?;
    let branch = item.branch.clone().unwrap_or_default();
    let tip = format!("refs/heads/{branch}");
    let lost =
        crate::lifecycle::not_on_github(&repo.path, &dir, &[&tip], Some(&branch), &["HEAD"])?;
    if !lost.is_empty() {
        return Err(format!(
            "{branch} has {} that HEAD lacks and GitHub does not have: moving it would leave \
             them in the reflog only. Push them, or switch back to {branch}.",
            commits_list(&lost)
        ));
    }
    git::git(&dir, &["switch", "--quiet", "-C", &branch]).map(|_| ())
}

/// Brings item `id`'s branch up to GitHub's head of the PR (`update_of`): fast-forwards it,
/// or, after the author rewrote the PR's history, resets it there when that drops nothing
/// GitHub does not have. Uncommitted changes stay (`reset --keep`), or git refuses. Resets
/// only with `rewrite` (the user was told the history was rewritten); else a rewrite that
/// only the fetch revealed returns `false` and changes nothing: the Update bar says so now.
pub fn update_branch(state: &AppState, id: i64, rewrite: bool) -> Result<bool, String> {
    let (item, repo) = item_repo(state, id)?;
    if item.kind != "pr" {
        return Err("only a PR's branch follows GitHub".into());
    }
    ensure_on_branch(state, id)?;
    let dir = worktree_of(&item)?;
    let branch = item.branch.clone().unwrap_or_default();
    let pr = summary(&item)?;
    let head = worktree::fetch_pr_head(&repo.path, &repo.base_remote, item.number, &pr.base_ref)?;
    let tip = format!("refs/heads/{branch}");
    // By the ref, not the commit: the branch's reflog then says it came from GitHub.
    let pulled = worktree::pull_ref(item.number);
    if git::is_ancestor(&dir, &head, &tip) {
        return Ok(true);
    }
    if git::is_ancestor(&dir, &tip, &head) {
        return git::git(&dir, &["merge", "--ff-only", "--quiet", &pulled]).map(|_| true);
    }
    if !rewrite {
        return Ok(false);
    }
    let lost = crate::lifecycle::not_on_github(&repo.path, &dir, &[&tip], Some(&branch), &[&head])?;
    if !lost.is_empty() {
        return Err(format!(
            "{branch} has {} that GitHub does not have, and the PR's history was rewritten: \
             updating would drop them. Rebase them onto the PR's head, or push them first.",
            commits_list(&lost)
        ));
    }
    git::git(&dir, &["reset", "--keep", "--quiet", &pulled]).map(|_| true)
}

/// For a PR the user reviewed: brings its worktree's branch up to what the author pushed since
/// (`update_of`), most likely the fixes for the review, without the user pressing Update.
/// Returns whether it did. Not while the worktree is on another branch, nor when it would drop
/// commits GitHub does not have (the Update bar says why), nor while the agent is in the middle
/// of a turn (the caller's check): its files would change under it.
pub fn pull_reviewed(state: &AppState, repo: &Repo, item: &WorkItem) -> Result<bool, String> {
    if item.role != "reviewed" || moved_to(item).is_some() {
        return Ok(false);
    }
    let Some(update) = update_of(item) else {
        return Ok(false);
    };
    // Fetched already: whether updating would drop anything is known without asking GitHub,
    // so a branch that cannot follow does not fetch on every poll.
    if update == "rewritten" {
        let dir = worktree_of(item)?;
        let branch = item.branch.clone().unwrap_or_default();
        let head = summary(item)?.head_sha;
        let tip = format!("refs/heads/{branch}");
        if !crate::lifecycle::not_on_github(&repo.path, &dir, &[&tip], Some(&branch), &[&head])?
            .is_empty()
        {
            return Ok(false);
        }
    }
    update_branch(state, item.id, true)
}

/// Refuses to send work to the agent of item `id` while its worktree is on another branch:
/// it would land there. What the user types in its terminal still reaches it.
pub fn ensure_on_branch(state: &AppState, id: i64) -> Result<(), String> {
    let item = state.store.item(id)?;
    match moved_to(&item) {
        None => Ok(()),
        Some(now) => Err(format!(
            "{} is on {} now, not on {}; switch it back to give the agent work",
            item.worktree_path.unwrap_or_default().display(),
            if now.is_empty() {
                "a detached HEAD".to_owned()
            } else {
                now
            },
            item.branch.unwrap_or_default()
        )),
    }
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
    let conv = if item.kind == "issue" {
        github::issue_conversation(&repo.owner, &repo.name, item.number).map(|(c, _)| c)?
    } else {
        github::conversation(&repo.owner, &repo.name, item.number)?
    };
    // Kept for the next time the item opens (`cached_conversation`).
    match serde_json::to_value(&conv) {
        Ok(json) => state.store.set_conversation(id, &json)?,
        Err(e) => tracing::warn!("item {id}: could not keep its conversation: {e}"),
    }
    Ok(conv)
}

/// The item's conversation as GitHub last gave it, if it was loaded before: the item shows it
/// at once while `conversation` loads the current one.
pub fn cached_conversation(state: &AppState, id: i64) -> Result<Option<serde_json::Value>, String> {
    state.store.conversation(id)
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
    // Comparing with the head itself would show no changes at all, and not why.
    let base = git::merge_base(&dir, &head, &tracking).map_err(|_| {
        if git::commit_exists(&dir, &tracking) {
            format!("Nothing to compare with: the head has no commit in common with {tracking}")
        } else {
            format!("Nothing to compare with: {tracking} is not fetched")
        }
    })?;
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

/// Fetches what the review of item `id` compares, when GitHub may have newer, and says whether
/// that moved it. Without a worktree that is the PR head (into `refs/ariekei/pull/<N>/head`)
/// and its base; with one, the base branch only: it was fetched when the worktree was made, and
/// a `git pull` there need not update it (for a fork's PR it fetches the fork), so a PR rebased
/// onto a newer base would show the base's new commits as its own, and a retargeted one (a
/// stacked PR whose parent merged) be compared with a branch it no longer targets.
///
/// The review shows what the clone has at once and calls this in the background (whenever the
/// tab is shown, and after each poll): so it goes to GitHub only when the poll saw a head the
/// clone lacks, when the base was not fetched in the last `BASE_FETCH_EVERY` (or it is another
/// branch now), or `force` (the user's Reload).
pub fn review_fetch(state: &AppState, id: i64, force: bool) -> Result<bool, String> {
    // When each (clone, item, base branch) was fetched.
    type Fetched = HashMap<(PathBuf, i64, String), Instant>;
    static FETCHED: Mutex<Option<Fetched>> = Mutex::new(None);
    let (item, repo) = item_repo(state, id)?;
    let worktree = item.worktree_path.as_ref().is_some_and(|p| p.exists());
    if !worktree && item.kind != "pr" {
        return Ok(false);
    }
    let base = base_ref_of(&item, &repo)?;
    let key = (repo.path.clone(), id, base.clone());
    let recent = FETCHED
        .lock()
        .unwrap()
        .get_or_insert_with(HashMap::new)
        .get(&key)
        .is_some_and(|at| at.elapsed() < BASE_FETCH_EVERY);
    let at = |r: &str| git::git_str(&repo.path, &["rev-parse", "--verify", "--quiet", r]).ok();
    let head = (!worktree).then(|| pull_ref(item.number));
    let tracking = format!("refs/remotes/{}/{base}", repo.base_remote);
    let before = (head.as_deref().and_then(at), at(&tracking));
    let new_head = match &head {
        Some(_) => Some(summary(&item)?.head_sha).filter(|h| before.0.as_ref() != Some(h)),
        None => None,
    };
    if recent && new_head.is_none() && !force {
        return Ok(false);
    }
    if worktree {
        worktree::fetch(&repo.path, &repo.base_remote, &base)?;
    } else {
        worktree::fetch_pr_head(&repo.path, &repo.base_remote, item.number, &base)?;
    }
    FETCHED
        .lock()
        .unwrap()
        .get_or_insert_with(HashMap::new)
        .insert(key, Instant::now());
    Ok((head.as_deref().and_then(at), at(&tracking)) != before)
}

/// How long a review's base counts as fresh (see `review_fetch`).
const BASE_FETCH_EVERY: Duration = Duration::from_secs(10 * 60);

/// The review of item `id` as the clone has it; `refresh`: as GitHub has it now, fetched first
/// (the user's Reload, or nothing to show yet).
pub fn review_state(state: &AppState, id: i64, refresh: bool) -> Result<ReviewState, String> {
    // Not fatal (e.g. offline): what was fetched before will do.
    let fetched = if refresh {
        review_fetch(state, id, true).map(|_| ())
    } else {
        Ok(())
    };
    if let Err(e) = &fetched {
        tracing::warn!("review of item {id}: could not fetch: {e}");
    }
    let r = review(state, id).map_err(|e| match &fetched {
        Err(f) => format!("{e} ({f})"),
        Ok(()) => e,
    })?;
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
        by_viewer(&r.item, &repo)
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

/// For each path, the line ranges (new side) of the PR diff at `commit` that GitHub accepts
/// line comments on: its hunks with GitHub's three lines of context. `commit` is the PR head
/// the drafts were made on (see `submit_review`). A renamed file's hunks are those of the
/// rename, as GitHub shows it, not of a new file.
pub fn comment_ranges(
    state: &AppState,
    id: i64,
    paths: &[String],
    commit: &str,
) -> Result<CommentRanges, String> {
    let r = review(state, id)?;
    let head = if git::commit_exists(&r.dir, commit) {
        commit.to_owned()
    } else {
        r.head
    };
    let renames = git::renames(&r.dir, &r.base, &head)?;
    paths
        .iter()
        .map(|path| {
            let mut args = vec![
                "diff",
                "-U3",
                "--no-color",
                "--no-ext-diff",
                "-M",
                &r.base,
                &head,
                "--",
                path,
            ];
            // Both paths, or git cannot pair them.
            args.extend(renames.get(path).map(String::as_str));
            let diff = git::git_str(&r.dir, &args)?;
            let ranges = git::parse_hunks(&diff)
                .into_iter()
                .filter(|h| h.new_lines > 0)
                .map(|h| (h.new_start, h.new_start + h.new_lines - 1))
                .collect();
            Ok((path.clone(), ranges))
        })
        .collect()
}

/// Submits a review on GitHub of the PR at `commit`, the head the review showed (empty: the
/// PR's head now). `comments` must already be on lines of the PR diff at that commit (see
/// `comment_ranges`); everything else belongs in `body`; `replies` go in existing threads.
/// The author may have pushed since: against the newer head the comments' line numbers would
/// point at other lines.
pub fn submit_review(
    state: &AppState,
    id: i64,
    event: &str,
    body: &str,
    comments: &[github::LineComment],
    replies: &[github::ThreadReply],
    commit: &str,
) -> Result<String, String> {
    let (item, repo) = item_repo(state, id)?;
    if !matches!(event, "COMMENT" | "APPROVE" | "REQUEST_CHANGES") {
        return Err(format!("unknown review event {event}"));
    }
    let commit = match commit {
        "" => summary(&item)?.head_sha,
        commit => commit.to_owned(),
    };
    let payload = github::review_payload(&commit, event, body, comments);
    let url = if replies.is_empty() {
        github::submit_review(&repo.owner, &repo.name, item.number, &payload)?
    } else {
        github::submit_review_with_replies(&repo.owner, &repo.name, item.number, &payload, replies)?
    };
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

/// Writes a file edited in the review view. `path` must stay inside the worktree, and the
/// file must still be as the editor loaded it (`loaded`; a missing file counts as empty):
/// the diff does not follow the disk, so the agent may have changed it meanwhile, and
/// writing would throw its edits away.
pub fn save_file(
    state: &AppState,
    id: i64,
    path: &str,
    content: &str,
    loaded: &str,
) -> Result<(), String> {
    let root = worktree_of(&state.store.item(id)?)?;
    if !inside_worktree(path) {
        return Err(format!("refusing to write outside the worktree: {path}"));
    }
    let file = root.join(path);
    let now = match std::fs::read(&file) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => vec![],
        Err(e) => return Err(format!("{path}: {e}")),
    };
    // The editor got U+FFFD for what is not UTF-8, and would write that back.
    if std::str::from_utf8(&now).is_err() {
        return Err(format!(
            "Not saved: {path} is not valid UTF-8 text, which the review cannot edit."
        ));
    }
    if now != loaded.as_bytes() {
        return Err(format!(
            "Not saved: {path} changed on disk since the review loaded it (the agent may have edited it). Reload to see it as it is now; your edits are dropped."
        ));
    }
    std::fs::write(file, content).map_err(|e| format!("{path}: {e}"))
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
    use serde_json::{Value, json};

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
    fn save_refuses_to_overwrite_what_changed_on_disk_since_it_was_loaded() {
        let app = TestApp::new();
        let task = create_task(&app.state, Some(app.repo.clone()), "Edit", "", "").unwrap();
        let file = app.path("proj_edit").join("a.txt");
        let loaded = std::fs::read_to_string(&file).unwrap();
        save_file(&app.state, task, "a.txt", "mine\n", &loaded).unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "mine\n");

        // The agent edits it while the review still shows "mine".
        write(&file, "the agent's\n");
        let err = save_file(&app.state, task, "a.txt", "mine again\n", "mine\n").unwrap_err();
        assert!(err.contains("a.txt changed on disk"), "{err}");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "the agent's\n");

        // A file that is not there yet was loaded as empty.
        save_file(&app.state, task, "new.txt", "hi\n", "").unwrap();
        let err = save_file(&app.state, task, "b.txt", "hi\n", "gone\n").unwrap_err();
        assert!(err.contains("changed on disk"), "{err}");

        // Nor is a file that is not UTF-8 (the editor has U+FFFD in it).
        std::fs::write(&file, b"caf\xe9\n").unwrap();
        let err =
            save_file(&app.state, task, "a.txt", "caf\u{fffd}!\n", "caf\u{fffd}\n").unwrap_err();
        assert!(err.contains("not valid UTF-8"), "{err}");
        assert_eq!(std::fs::read(&file).unwrap(), b"caf\xe9\n");
    }

    #[test]
    fn a_review_goes_to_the_commit_its_comments_were_made_on() {
        let app = TestApp::new();
        let lines = |changed: &[usize]| {
            (1..=20)
                .map(|i| match changed.contains(&i) {
                    true => format!("line {i} changed\n"),
                    false => format!("line {i}\n"),
                })
                .collect::<String>()
        };
        // bob's PR #5, as GitHub has it (refs/pull/5/head).
        let push = |changed: &[usize]| {
            app.push_to("feat", &[("f.txt", &lines(changed))], "bob");
            git(&app.upstream, &["update-ref", "refs/pull/5/head", "feat"]);
        };
        app.push_to("main", &[("f.txt", &lines(&[]))], "me");
        push(&[2]);
        app.add_pr(
            json!({"number": 5, "title": "Feat", "head_ref": "feat", "author": "bob",
                          "requested": true}),
        );
        app.poll().unwrap();
        let id = app.item(5).id;
        assert!(review_fetch(&app.state, id, false).unwrap());
        let seen = review_state(&app.state, id, false).unwrap().pr_head;

        // The author pushes again while the review is open, and the poll sees it.
        push(&[2, 15]);
        app.poll().unwrap();
        assert!(review_fetch(&app.state, id, false).unwrap());
        assert_ne!(summary(&app.item(5)).unwrap().head_sha, seen);

        let paths = ["f.txt".to_owned()];
        assert_eq!(
            comment_ranges(&app.state, id, &paths, &seen).unwrap(),
            [("f.txt".to_owned(), vec![(1, 5)])],
            "the diff at the commit the drafts are on"
        );
        let comment = github::LineComment {
            path: "f.txt".into(),
            line: 2,
            start_line: None,
            side: "RIGHT".into(),
            body: "Why?".into(),
        };
        submit_review(&app.state, id, "COMMENT", "", &[comment], &[], &seen).unwrap();
        let posted = app.writes().pop().unwrap();
        assert_eq!(posted["body"]["commit_id"], seen.as_str());
    }

    #[test]
    fn a_review_without_a_worktree_fetches_only_what_github_has_new() {
        let app = TestApp::new();
        let push = |content: &str| {
            app.push_to("feat", &[("f.txt", content)], "bob");
            git(&app.upstream, &["update-ref", "refs/pull/5/head", "feat"]);
        };
        push("one\n");
        app.add_pr(json!({"number": 5, "title": "Feat", "head_ref": "feat", "author": "bob", "requested": true}));
        app.poll().unwrap();
        let id = app.item(5).id;
        // Nothing to show yet: the review fetches first.
        let err = review_state(&app.state, id, false).unwrap_err();
        assert!(err.contains("not fetched yet"), "{err}");
        let first = review_state(&app.state, id, true).unwrap().head;
        assert!(
            !review_fetch(&app.state, id, false).unwrap(),
            "the clone has the head"
        );

        // The author pushes, and the poll sees it: the review still shows what it showed, and
        // the fetch after it says there is newer.
        push("two\n");
        app.poll().unwrap();
        assert_eq!(review_state(&app.state, id, false).unwrap().head, first);
        assert!(review_fetch(&app.state, id, false).unwrap());
        assert_eq!(
            review_state(&app.state, id, false).unwrap().head,
            git(&app.upstream, &["rev-parse", "feat"])
        );
    }

    #[test]
    fn seeing_an_item_says_how_far_the_user_had_seen_it() {
        let app = TestApp::new();
        let thread = |replies: Value| {
            json!([{"id": "T1", "path": "a.txt", "line": 1, "comment_id": 1, "author": "me",
                    "html": "<p>Why?</p>", "replies": replies}])
        };
        let carol =
            |id: i64, at: &str| json!({"comment_id": id, "author": "carol", "html": "", "at": at});
        app.add_pr(json!({"number": 5, "title": "Feat", "head_ref": "main", "author": "bob", "requested": true,
                          "threads": thread(json!([carol(2, "2026-10-12T08:00:00Z")]))}));
        app.poll().unwrap();
        let id = app.item(5).id;
        let new_activity = || {
            app.rows()
                .into_iter()
                .find(|r| r.id == id)
                .unwrap()
                .new_activity
        };
        // Found with Carol's reply: that is not new to the user.
        let first = Some("2026-10-12T08:00:00Z".to_owned());
        assert_eq!(
            mark_seen(&app.state, id).unwrap(),
            Seen {
                before: first.clone(),
                after: first.clone().unwrap()
            }
        );

        // Carol replies again while the item is open; the poll says so, and the conversation
        // loading after it clears that.
        let replies = json!([
            carol(2, "2026-10-12T08:00:00Z"),
            carol(3, "2026-10-13T08:00:00Z")
        ]);
        app.set("prs", 5, json!({"threads": thread(replies)}));
        app.poll().unwrap();
        assert!(new_activity());
        assert_eq!(
            mark_seen(&app.state, id).unwrap(),
            Seen {
                before: first,
                after: "2026-10-13T08:00:00Z".into()
            }
        );
        assert!(!new_activity());
    }

    #[test]
    fn comments_on_an_item_seen_without_any_are_new() {
        let app = TestApp::new();
        let carol = json!([{"author": "carol", "html": "", "at": "2026-10-12T08:00:00Z"}]);
        let nothing = Seen {
            before: Some(NOTHING_SEEN.into()),
            after: NOTHING_SEEN.into(),
        };
        let after_carol = Seen {
            before: Some(NOTHING_SEEN.into()),
            after: "2026-10-12T08:00:00Z".into(),
        };
        // Found by a poll with no comments yet.
        app.add_pr(json!({"number": 5, "title": "Feat", "head_ref": "main", "author": "bob", "requested": true}));
        app.poll().unwrap();
        let id = app.item(5).id;
        assert_eq!(mark_seen(&app.state, id).unwrap(), nothing);
        app.set("prs", 5, json!({"comments": carol}));
        app.poll().unwrap();
        assert!(app.row(5).unwrap().new_activity);
        assert_eq!(mark_seen(&app.state, id).unwrap(), after_carol);

        // Added from Browse, never seen: nothing is new on the first look, then the same.
        app.add_pr(json!({"number": 6, "title": "Other", "head_ref": "main", "author": "bob"}));
        let id = track(&app.state, Some(app.repo.clone()), "pr", 6).unwrap();
        assert_eq!(
            mark_seen(&app.state, id).unwrap(),
            Seen {
                before: None,
                after: NOTHING_SEEN.into()
            }
        );
        app.set("prs", 6, json!({"comments": carol}));
        app.poll().unwrap();
        assert_eq!(mark_seen(&app.state, id).unwrap(), after_carol);
    }

    #[test]
    fn a_conversation_loaded_once_shows_at_once_next_time() {
        let app = TestApp::new();
        app.add_pr(json!({"number": 5, "title": "Feat", "head_ref": "main", "author": "bob", "requested": true}));
        app.poll().unwrap();
        let id = app.item(5).id;
        assert_eq!(cached_conversation(&app.state, id).unwrap(), None);
        let conv = conversation(&app.state, id).unwrap();
        assert_eq!(
            cached_conversation(&app.state, id).unwrap(),
            Some(serde_json::to_value(&conv).unwrap())
        );
    }

    #[test]
    fn line_comments_on_a_renamed_file_go_on_the_lines_of_its_rename() {
        let app = TestApp::new();
        let lines = |changed: usize| {
            (1..=40)
                .map(|i| match i == changed {
                    true => format!("line {i} changed\n"),
                    false => format!("line {i}\n"),
                })
                .collect::<String>()
        };
        app.push_to("main", &[("old.txt", &lines(0))], "me");
        let task = create_task(&app.state, Some(app.repo.clone()), "Move", "", "").unwrap();
        let wt = app.path("proj_move");
        git(&wt, &["mv", "old.txt", "new.txt"]);
        write(&wt.join("new.txt"), &lines(20));
        commit(&wt, "move it", "me");
        let head = crate::git::head(&wt).unwrap();
        assert_eq!(
            comment_ranges(&app.state, task, &["new.txt".to_owned()], &head).unwrap(),
            [("new.txt".to_owned(), vec![(17, 23)])],
            "GitHub shows a rename with one changed line, not a new file"
        );
    }

    #[test]
    fn a_worktree_review_compares_with_the_base_as_it_is_now() {
        let app = TestApp::new();
        let (id, wt) = app.pr_with_worktree(6, "feat");
        // main moves on and the PR is rebased onto it; the worktree gets only the PR's branch
        // (as `git pull` does for a fork's PR), so the clone's origin/main stays behind.
        app.push_to("main", &[("m.txt", "carol's\n")], "carol");
        let up = &app.upstream;
        git(up, &["checkout", "-q", "feat"]);
        git(
            up,
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
        git(up, &["checkout", "-q", "main"]);
        git(&wt, &["fetch", "-q", "origin", "feat"]);
        git(&wt, &["reset", "-q", "--hard", "origin/feat"]);

        // Shown as the clone has it, main's new commit as if it were the PR's; the fetch
        // after it says the base moved.
        let stale = review_state(&app.state, id, false).unwrap();
        assert_eq!(stale.commits.len(), 2);
        assert!(review_fetch(&app.state, id, false).unwrap());
        let state = review_state(&app.state, id, false).unwrap();
        assert_eq!(
            state
                .commits
                .iter()
                .map(|c| c.author.as_str())
                .collect::<Vec<_>>(),
            ["me"],
            "only the PR's own commit, not main's new one"
        );
        assert_eq!(state.base, git(up, &["rev-parse", "main"]));

        // Shown again soon after: no fetch (the tab checks whenever it is shown), unless the
        // user asks for it as it is now.
        app.push_to("main", &[("n.txt", "dave's\n")], "dave");
        let main_now = git(up, &["rev-parse", "main"]);
        let fetched = || git(&app.clone, &["rev-parse", "origin/main"]);
        assert!(!review_fetch(&app.state, id, false).unwrap());
        review_state(&app.state, id, false).unwrap();
        assert_ne!(fetched(), main_now);
        review_state(&app.state, id, true).unwrap();
        assert_eq!(fetched(), main_now);
        assert!(
            !review_fetch(&app.state, id, true).unwrap(),
            "nothing new since"
        );

        // A base that cannot be fetched (deleted): the review says so instead of showing nothing.
        app.set("prs", 6, json!({"base_ref": "gone"}));
        app.poll().unwrap();
        let err = review_state(&app.state, id, true).unwrap_err();
        assert!(err.contains("origin/gone is not fetched"), "{err}");
        assert!(
            err.contains("could not fetch") || err.contains("failed"),
            "{err}"
        );
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
    fn what_a_tasks_agent_opens_is_an_item_of_its_own_that_knows_how_it_came_about() {
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
        assert_eq!((t.number, t.branch.as_deref()), (-1, Some("investigate")));
        assert!(app.path("proj_investigate").join("a.txt").exists());
        let context =
            std::fs::read_to_string(app.state.data_dir.join("context/owner-proj-task1.md"));
        assert!(
            context.is_err(),
            "the context is written when the agent starts"
        );
        let transcript = app.path("t.jsonl");
        write(
            &transcript,
            concat!(
                r#"{"type":"user","message":{"content":"Why do peers drop?"}}"#,
                "\n",
                r#"{"type":"assistant","message":{"content":[{"type":"text","text":"The table resets twice."}]}}"#,
                "\n"
            ),
        );
        app.state.store.set_session(task, "s1").unwrap();
        app.state
            .store
            .mark_session_started(task, Some(&transcript))
            .unwrap();

        app.add_pr(json!({"number": 20, "title": "Fix", "head_ref": "main"}));
        app.add_issue(json!({"number": 21, "title": "Found", "author": "me"}));
        track_opened(&app.state, task, "pr", 20).unwrap();
        track_opened(&app.state, task, "issue", 21).unwrap();
        let rows = app.rows();
        let row = |n: i64| rows.iter().find(|r| r.number == n).unwrap();
        assert_eq!(
            (row(-1).kind.as_str(), row(-1).title.as_str()),
            ("task", "Investigate")
        );
        assert!(row(21).removable, "an issue like any other the user added");
        let origin = app.item(20).origin.unwrap();
        assert!(
            origin.contains("task T1 (\"Investigate\", on `investigate`)")
                && origin.contains("**User:** Why do peers drop?")
                && origin.contains("**Agent:** The table resets twice."),
            "{origin}"
        );
        // Its own agent starts with it.
        let file = write_issue_context(&app.state.data_dir, &app.repo, &app.item(21)).unwrap();
        let md = std::fs::read_to_string(file).unwrap();
        assert!(
            md.starts_with("# Work item: issue #21") && md.contains("**User:** Why do peers drop?"),
            "{md}"
        );

        // Merged and cleaned up: gone like any other.
        app.set("prs", 20, json!({"state": "MERGED"}));
        app.poll().unwrap();
        assert!(app.item(20).hidden && app.row(20).is_none());
        assert!(app.row(-1).is_some(), "the task stays");
    }

    #[test]
    fn prs_and_issues_linked_to_a_task_before_are_items_of_their_own() {
        let app = TestApp::new();
        let task = create_task(&app.state, Some(app.repo.clone()), "Old", "", "").unwrap();
        app.add_pr(json!({"number": 20, "title": "Fix", "head_ref": "main"}));
        track_opened(&app.state, task, "pr", 20).unwrap();
        app.add_pr(json!({"number": 22, "title": "Linked", "head_ref": "main"}));
        app.poll().unwrap();
        let (fix, linked) = (app.item(20).id, app.item(22).id);
        app.state.store.link_to_task(fix, task).unwrap();
        app.state.store.link_to_task(linked, task).unwrap();

        let had = app.item(20).origin;
        unlink_from_tasks(&app.state).unwrap();
        assert_eq!(app.state.store.linked_to_tasks().unwrap(), []);
        let origin = |n| app.item(n).origin.unwrap();
        assert!(origin(22).contains("task T1 (\"Old\""), "{}", origin(22));
        assert_eq!(Some(origin(20)), had, "an origin it had stays");
    }

    #[test]
    fn a_task_whose_worktree_cannot_be_made_is_not_left_behind() {
        let app = TestApp::new();
        let err = create_task(
            &app.state,
            Some(app.repo.clone()),
            "Blocked",
            "",
            "no-such-branch",
        )
        .unwrap_err();
        assert!(err.contains("could not fetch no-such-branch"), "{err}");
        assert!(app.items().iter().all(|i| !i.is_task()));
        assert!(
            create_task(&app.state, Some(app.repo.clone()), "  ", "", "").is_err(),
            "no title"
        );
    }

    #[test]
    fn a_task_skips_the_numbers_of_what_earlier_tasks_left() {
        // T1's worktree folder, and a branch of T2 (kept, say, by the PR T2's agent opened,
        // after the task itself was done).
        let app = TestApp::new();
        std::fs::create_dir_all(app.path("proj_task1")).unwrap();
        crate::testkit::git(&app.clone, &["branch", "task-2-old-work", "main"]);
        let id = create_task(&app.state, Some(app.repo.clone()), "Next", "", "").unwrap();
        let task = app.state.store.item(id).unwrap();
        assert_eq!((task.number, task.branch.as_deref()), (-3, Some("next")));
        assert_eq!(
            std::fs::canonicalize(task.worktree_path.unwrap()).unwrap(),
            std::fs::canonicalize(app.path("proj_next")).unwrap()
        );
    }

    #[test]
    fn a_task_reviews_its_worktree_against_the_default_branch() {
        let app = TestApp::new();
        let task = create_task(&app.state, Some(app.repo.clone()), "Try", "", "").unwrap();
        let wt = app.path("proj_try");
        write(&wt.join("a.txt"), "one\ntwo\nthree\nfour\n");
        commit(&wt, "four", "me");
        let state = review_state(&app.state, task, false).unwrap();
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
    fn new_work_starts_from_the_default_branch_github_has_now() {
        let app = TestApp::new();
        // Renamed on GitHub after the clone was made; the clone's origin/HEAD says main.
        git(&app.upstream, &["branch", "-m", "main", "trunk"]);
        let mut data = app.data();
        data["default_branch"] = json!("trunk");
        app.set_data(&data);
        let task = create_task(&app.state, Some(app.repo.clone()), "Renamed", "", "").unwrap();
        let wt = app.path("proj_renamed");
        assert_eq!(
            git(&wt, &["rev-parse", "HEAD"]),
            git(&app.upstream, &["rev-parse", "trunk"])
        );
        assert_eq!(default_branch(&app.repo).unwrap(), "trunk");
        assert_eq!(
            review_state(&app.state, task, false).unwrap().base_ref,
            "trunk"
        );
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
                &app.path("proj_gone").to_string_lossy(),
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
                .ends_with("proj_gone")
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
        app.state
            .store
            .set_worktree(id, None, None, false, false)
            .unwrap();
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
    fn a_task_on_the_branch_of_the_users_checkout_works_there_while_it_stays_on_it() {
        let app = TestApp::new();
        app.fork("friend", "feat/evm");
        git(
            &app.clone,
            &["remote", "add", "mine", "git@github.com:friend/proj.git"],
        );
        git(&app.clone, &["fetch", "-q", "mine", "feat/evm"]);
        git(
            &app.clone,
            &["checkout", "-q", "-b", "feat/evm", "mine/feat/evm"],
        );
        let id = create_task(
            &app.state,
            Some(app.repo.clone()),
            "",
            "",
            "friend:feat/evm",
        )
        .unwrap();
        let item = app.state.store.item(id).unwrap();
        assert_eq!(
            (
                std::fs::canonicalize(item.worktree_path.as_ref().unwrap()).unwrap(),
                item.worktree_created
            ),
            (std::fs::canonicalize(&app.clone).unwrap(), false)
        );
        assert_eq!(moved_to(&item), None);
        assert!(ensure_on_branch(&app.state, id).is_ok());

        // Switched away: the agent gets nothing until it is back.
        git(&app.clone, &["checkout", "-q", "main"]);
        assert_eq!(moved_to(&item).as_deref(), Some("main"));
        let finished = WorkItem {
            open: false,
            ..item.clone()
        };
        assert_eq!(
            moved_to(&finished),
            None,
            "a finished item's branch is done with"
        );
        let err = ensure_on_branch(&app.state, id).unwrap_err();
        assert!(err.contains("is on main now, not on feat/evm"), "{err}");
        assert!(
            session_plan(&app.state, &app.state.data_dir, id).is_ok(),
            "the agent still starts: the user may want to talk to it"
        );

        // Done keeps the checkout, the branch and the user's own changes.
        git(&app.clone, &["checkout", "-q", "feat/evm"]);
        write(&app.clone.join("mine.txt"), "wip\n");
        let crate::lifecycle::Cleanup::Cleaned { removed } = mark_done(&app.state, id).unwrap()
        else {
            panic!("blocked")
        };
        assert!(
            removed.iter().any(|r| r.contains("your worktree is kept")),
            "{removed:?}"
        );
        assert!(app.clone.join("mine.txt").exists());
        assert_eq!(git(&app.clone, &["branch", "--show-current"]), "feat/evm");
    }

    #[test]
    fn a_saved_conversation_is_resumed_though_no_hook_said_it_started() {
        let app = TestApp::new();
        let id = create_task(&app.state, Some(app.repo.clone()), "Hookless", "", "").unwrap();
        let plan = session_plan(&app.state, &app.state.data_dir, id).unwrap();
        assert!(!plan.resume);
        // Not prompted (as far as Ariekei knows): the same id again.
        let again = session_plan(&app.state, &app.state.data_dir, id).unwrap();
        assert_eq!(
            (again.session_id.as_str(), again.resume),
            (plan.session_id.as_str(), false)
        );

        // Claude Code saved it, but no hook arrived.
        let transcript = app
            .path("claude-config/projects/-proj-task1")
            .join(format!("{}.jsonl", plan.session_id));
        write(&transcript, "{}\n");
        let resumed = session_plan(&app.state, &app.state.data_dir, id).unwrap();
        assert_eq!(
            (resumed.session_id.as_str(), resumed.resume),
            (plan.session_id.as_str(), true)
        );
        let item = app.state.store.item(id).unwrap();
        assert!(item.session_started);
        assert_eq!(item.transcript_path, Some(transcript), "deleted on cleanup");
    }

    /// Rebases `branch` of the "GitHub" repo onto `main`, and GitHub's head of PR `number`
    /// with it (the author's force-push).
    fn rebase_upstream(app: &TestApp, branch: &str, number: i64) {
        let up = &app.upstream;
        git(up, &["checkout", "-q", branch]);
        let me = ["-c", "user.name=bob", "-c", "user.email=bob@example.com"];
        git(up, &[&me[..], &["rebase", "-q", "main"]].concat());
        git(up, &["checkout", "-q", "main"]);
        git(
            up,
            &["update-ref", &format!("refs/pull/{number}/head"), branch],
        );
    }

    #[test]
    fn a_worktree_switched_away_goes_back_to_its_branch() {
        let app = TestApp::new();
        let (id, wt) = app.pr_with_worktree(5, "feat");
        let branch = app.state.store.item(id).unwrap().branch.unwrap();
        let on = || crate::git::current_branch(&wt).unwrap();
        // Another branch, as in an IDE.
        git(&wt, &["switch", "-q", "-c", "elsewhere"]);
        switch_back(&app.state, id).unwrap();
        assert_eq!(on(), Some(branch.clone()));

        // A detached HEAD on an older commit.
        git(&wt, &["checkout", "-q", "--detach", "HEAD~1"]);
        switch_back(&app.state, id).unwrap();
        assert_eq!(on(), Some(branch.clone()));

        // A commit on a detached HEAD on top of the branch: the branch moves up to it.
        git(&wt, &["checkout", "-q", "--detach"]);
        write(&wt.join("c.txt"), "on top\n");
        commit(&wt, "on top", "me");
        let top = git(&wt, &["rev-parse", "HEAD"]);
        switch_back(&app.state, id).unwrap();
        assert_eq!(on(), Some(branch.clone()));
        assert_eq!(git(&wt, &["rev-parse", &branch]), top);

        // A commit on a detached HEAD off the branch: switching would leave it behind.
        git(&wt, &["checkout", "-q", "--detach", "HEAD~2"]);
        write(&wt.join("d.txt"), "stray\n");
        commit(&wt, "stray", "me");
        let err = switch_back(&app.state, id).unwrap_err();
        assert!(err.contains("stray"), "{err}");
        assert_eq!(on(), None);
        // Nor can the branch move there: "on top" is not on GitHub.
        let err = move_branch_here(&app.state, id).unwrap_err();
        assert!(err.contains("on top"), "{err}");
        assert_eq!(on(), None);
    }

    #[test]
    fn a_detached_checkout_of_the_authors_rebased_pr_takes_the_branch_along() {
        let app = TestApp::new();
        let (id, wt) = app.pr_with_worktree(5, "feat");
        let branch = app.state.store.item(id).unwrap().branch.unwrap();
        app.push_to("main", &[("m.txt", "carol's\n")], "carol");
        rebase_upstream(&app, "feat", 5);
        // Someone checks out the author's new version: HEAD is detached there.
        git(&wt, &["fetch", "-q", "origin", "feat"]);
        git(&wt, &["checkout", "-q", "--detach", "FETCH_HEAD"]);
        assert_eq!(
            moved_to(&app.state.store.item(id).unwrap()).as_deref(),
            Some("")
        );

        // The branch's old commits came from GitHub: it can move there.
        move_branch_here(&app.state, id).unwrap();
        assert_eq!(
            crate::git::current_branch(&wt).unwrap(),
            Some(branch.clone())
        );
        assert_eq!(
            git(&wt, &["rev-parse", &branch]),
            git(&app.upstream, &["rev-parse", "feat"])
        );
    }

    #[test]
    fn a_prs_branch_follows_the_authors_pushes_and_rebases() {
        let app = TestApp::new();
        let (id, wt) = app.pr_with_worktree(5, "feat");
        let up = &app.upstream;
        let update = || {
            app.poll().unwrap();
            app.rows().into_iter().find(|r| r.id == id).unwrap().update
        };
        let github = || git(up, &["rev-parse", "feat"]);
        let here = || git(&wt, &["rev-parse", "HEAD"]);
        git(up, &["update-ref", "refs/pull/5/head", "feat"]);
        assert_eq!(update(), None);

        // The author pushes: the branch can follow.
        app.push_to("feat", &[("c.txt", "bob's\n")], "bob");
        git(up, &["update-ref", "refs/pull/5/head", "feat"]);
        assert_eq!(update(), Some("new"), "not fetched yet");
        update_branch(&app.state, id, false).unwrap();
        assert_eq!(here(), github());
        assert_eq!(update(), None);

        // A commit of the user's own, not pushed yet: nothing to update.
        write(&wt.join("d.txt"), "mine\n");
        commit(&wt, "mine, not pushed", "me");
        assert_eq!(update(), None);

        // The author rebases the PR. Not fetched yet, the bar could not say so: the first
        // Update only fetches, then the bar says the history was rewritten.
        app.push_to("main", &[("m.txt", "carol's\n")], "carol");
        rebase_upstream(&app, "feat", 5);
        assert_eq!(update(), Some("new"));
        let before = here();
        assert_eq!(update_branch(&app.state, id, false), Ok(false));
        assert_eq!(here(), before);
        assert_eq!(update(), Some("rewritten"));
        // Updating would drop the user's commit, so it refuses.
        let err = update_branch(&app.state, id, true).unwrap_err();
        assert!(err.contains("mine, not pushed"), "{err}");

        // Without it, the branch takes the rebased PR: its old commits came from GitHub.
        git(&wt, &["reset", "-q", "--hard", "HEAD~1"]);
        update_branch(&app.state, id, true).unwrap();
        assert_eq!(here(), github());
        assert_eq!(update(), None);
    }

    #[test]
    fn a_rebase_here_is_no_update() {
        let app = TestApp::new();
        let (id, wt) = app.pr_with_worktree(5, "feat");
        let up = &app.upstream;
        let update = || {
            app.poll().unwrap();
            app.rows().into_iter().find(|r| r.id == id).unwrap().update
        };
        git(up, &["update-ref", "refs/pull/5/head", "feat"]);
        assert_eq!(update(), None);

        // The agent rebases the branch onto a newer main, not pushed yet: GitHub has nothing
        // the branch lacks.
        app.push_to("main", &[("m.txt", "carol's\n")], "carol");
        git(&wt, &["fetch", "-q", "origin", "main"]);
        let me = ["-c", "user.name=me", "-c", "user.email=me@example.com"];
        git(&wt, &[&me[..], &["rebase", "-q", "FETCH_HEAD"]].concat());
        assert_eq!(update(), None);

        // The author pushes to the PR: that is news.
        app.push_to("feat", &[("c.txt", "bob's\n")], "bob");
        git(up, &["update-ref", "refs/pull/5/head", "feat"]);
        assert_eq!(update(), Some("new"));
        git(&wt, &["fetch", "-q", "origin", "refs/pull/5/head"]);
        assert_eq!(update(), Some("rewritten"));
    }

    #[test]
    fn a_reviewed_prs_new_commits_are_pulled() {
        let app = TestApp::new();
        let up = &app.upstream;
        app.push_to("fix", &[("b.txt", "bob's\n")], "bob");
        git(up, &["update-ref", "refs/pull/6/head", "fix"]);
        app.add_pr(
            json!({"number": 6, "title": "Fix", "head_ref": "fix", "author": "bob", "reviewed": true}),
        );
        app.poll().unwrap();
        let id = app.item(6).id;
        mark_seen(&app.state, id).unwrap();
        let wt = prepare(&app.state, id).unwrap().path;
        let branch = app.item(6).branch.unwrap();
        assert_eq!(app.item(6).role, "reviewed");
        let rev = |r: &str| git(&wt, &["rev-parse", r]);
        let github = || git(up, &["rev-parse", "fix"]);

        // The author pushes the fixes for the review: the next poll pulls them.
        app.push_to("fix", &[("c.txt", "fixed\n")], "bob");
        git(up, &["update-ref", "refs/pull/6/head", "fix"]);
        app.poll().unwrap();
        assert_eq!(rev(&branch), github());
        assert_eq!(app.row(6).unwrap().update, None);

        // Rebased by the author: the branch takes that version too.
        app.push_to("main", &[("m.txt", "carol's\n")], "carol");
        rebase_upstream(&app, "fix", 6);
        app.poll().unwrap();
        assert_eq!(rev(&branch), github());

        // A commit of the user's own would be dropped: the branch stays, and the bar says so.
        write(&wt.join("d.txt"), "mine\n");
        commit(&wt, "mine, not pushed", "me");
        let mine = rev("HEAD");
        app.push_to("main", &[("n.txt", "carol's\n")], "carol");
        rebase_upstream(&app, "fix", 6);
        app.poll().unwrap();
        app.poll().unwrap();
        assert_eq!(rev(&branch), mine);
        assert_eq!(app.row(6).unwrap().update, Some("rewritten"));
    }

    #[test]
    fn the_worktree_fixes_move_no_other_branch_and_keep_uncommitted_work() {
        let app = TestApp::new();
        let (id, wt) = app.pr_with_worktree(5, "feat");
        let branch = app.state.store.item(id).unwrap().branch.unwrap();
        let up = &app.upstream;
        let row = || {
            app.poll().unwrap();
            app.rows().into_iter().find(|r| r.id == id).unwrap()
        };
        let rev = |r: &str| git(&wt, &["rev-parse", r]);
        let github = || git(up, &["rev-parse", "feat"]);

        // On another branch with a commit of its own: Move here is for a detached HEAD only,
        // and Update would move that branch.
        git(&wt, &["switch", "-q", "-c", "elsewhere"]);
        write(&wt.join("e.txt"), "elsewhere\n");
        commit(&wt, "elsewhere", "me");
        let tip = rev(&branch);
        let err = move_branch_here(&app.state, id).unwrap_err();
        assert!(err.contains("not on a detached HEAD"), "{err}");
        app.push_to("feat", &[("c.txt", "bob's\n")], "bob");
        git(up, &["update-ref", "refs/pull/5/head", "feat"]);
        let err = update_branch(&app.state, id, true).unwrap_err();
        assert!(err.contains("switch it back"), "{err}");
        assert_eq!((rev(&branch), rev("elsewhere")), (tip, rev("HEAD")));
        let moved = row();
        assert_eq!(
            (moved.moved_to.as_deref(), moved.update),
            (Some("elsewhere"), None)
        );
        switch_back(&app.state, id).unwrap();

        // The author's push, already in the clone: a fast-forward. Uncommitted changes stay.
        git(&wt, &["fetch", "-q", "origin", "refs/pull/5/head"]);
        assert_eq!(row().update, Some("fast-forward"));
        write(&wt.join("a.txt"), "not committed\n");
        update_branch(&app.state, id, false).unwrap();
        assert_eq!(rev("HEAD"), github());
        update_branch(&app.state, id, false).unwrap();
        assert_eq!(rev("HEAD"), github(), "up to date: nothing to do");

        // Rebased by the author onto a main that adds m.txt, which is untracked here: git
        // refuses to overwrite it, and the branch stays.
        app.push_to("main", &[("m.txt", "carol's\n")], "carol");
        rebase_upstream(&app, "feat", 5);
        write(&wt.join("m.txt"), "mine\n");
        let before = rev("HEAD");
        let err = update_branch(&app.state, id, true).unwrap_err();
        assert!(err.contains("m.txt"), "{err}");
        assert_eq!(rev("HEAD"), before);
        std::fs::remove_file(wt.join("m.txt")).unwrap();
        assert_eq!(update_branch(&app.state, id, true), Ok(true));
        assert_eq!(rev("HEAD"), github());
        assert_eq!(
            std::fs::read_to_string(wt.join("a.txt")).unwrap(),
            "not committed\n"
        );
        git(&wt, &["checkout", "-q", "--", "a.txt"]);

        // A detached HEAD that builds on the branch's own unpushed commit: Move here keeps it.
        write(&wt.join("d.txt"), "mine\n");
        commit(&wt, "mine, not pushed", "me");
        git(&wt, &["checkout", "-q", "--detach"]);
        write(&wt.join("d.txt"), "more\n");
        commit(&wt, "more", "me");
        move_branch_here(&app.state, id).unwrap();
        assert_eq!(
            crate::git::current_branch(&wt).unwrap(),
            Some(branch.clone())
        );
        assert_eq!(rev(&branch), rev("HEAD"));

        // Merged (kept: its commits are not on GitHub): nothing to update.
        app.set("prs", 5, json!({"state": "MERGED"}));
        app.push_to("feat", &[("f.txt", "late\n")], "bob");
        git(up, &["update-ref", "refs/pull/5/head", "feat"]);
        assert_eq!(row().update, None);
    }

    #[test]
    fn a_task_follows_the_branch_its_agent_started() {
        let app = TestApp::new();
        let id = create_task(&app.state, Some(app.repo.clone()), "Next", "", "").unwrap();
        let item = app.state.store.item(id).unwrap();
        let wt = item.worktree_path.clone().unwrap();
        let err = follow_branch(&app.state, id).unwrap_err();
        assert!(err.contains("on its branch"), "{err}");

        git(&wt, &["checkout", "-q", "--detach"]);
        let err = follow_branch(&app.state, id).unwrap_err();
        assert!(err.contains("detached HEAD"), "{err}");

        let row = || app.row(item.number).unwrap();
        // The default branch is never a task's.
        git(&wt, &["checkout", "-q", "--ignore-other-worktrees", "main"]);
        let err = follow_branch(&app.state, id).unwrap_err();
        assert!(err.contains("default branch"), "{err}");
        assert!(!row().can_follow);

        git(&wt, &["switch", "-q", "-c", "next-part"]);
        assert!(row().can_follow);
        follow_branch(&app.state, id).unwrap();
        let item = app.state.store.item(id).unwrap();
        assert_eq!(
            (item.branch.as_deref(), item.branch_created),
            (Some("next-part"), false),
            "cleanup keeps the branch Ariekei did not create"
        );
        assert_eq!(moved_to(&item), None);
        assert!(ensure_on_branch(&app.state, id).is_ok());
    }

    #[test]
    fn a_pr_follows_a_rewrite_of_its_branch() {
        let app = TestApp::new();
        let (id, wt) = app.pr_with_worktree(5, "feat");
        let up = &app.upstream;
        let row = || {
            app.poll().unwrap();
            app.rows().into_iter().find(|r| r.id == id).unwrap()
        };
        git(up, &["update-ref", "refs/pull/5/head", "feat"]);

        // The agent rewrites the PR on a new branch: GitHub's head is not on it.
        git(&wt, &["switch", "-q", "-c", "rewrite", "HEAD~1"]);
        write(&wt.join("b.txt"), "rewritten\n");
        commit(&wt, "rewritten", "me");
        let moved = row();
        assert_eq!(
            (moved.moved_to.as_deref(), moved.can_follow),
            (Some("rewrite"), true)
        );
        assert!(ensure_on_branch(&app.state, id).is_err());

        follow_branch(&app.state, id).unwrap();
        let item = app.state.store.item(id).unwrap();
        assert_eq!(
            (item.branch.as_deref(), item.branch_created),
            (Some("rewrite"), false),
            "cleanup keeps both branches"
        );
        assert!(ensure_on_branch(&app.state, id).is_ok());
        let followed = row();
        assert_eq!(
            (followed.moved_to, followed.update),
            (None, None),
            "GitHub still has the version the rewrite replaces"
        );

        // The user pushes it to the PR: nothing to update.
        git(&wt, &["push", "-q", "-f", "origin", "rewrite:feat"]);
        git(up, &["update-ref", "refs/pull/5/head", "feat"]);
        assert_eq!(row().update, None);

        // Someone else pushes on top: the bar is back.
        app.push_to("feat", &[("c.txt", "bob's\n")], "bob");
        git(up, &["update-ref", "refs/pull/5/head", "feat"]);
        assert_eq!(row().update, Some("new"));
    }

    #[test]
    fn a_rebase_or_bisect_of_the_items_branch_has_not_moved_it() {
        let app = TestApp::new();
        let id = create_task(&app.state, Some(app.repo.clone()), "Rebase", "", "").unwrap();
        let item = app.state.store.item(id).unwrap();
        let wt = item.worktree_path.clone().unwrap();
        write(&wt.join("a.txt"), "mine\n");
        commit(&wt, "mine", "me");
        write(&app.clone.join("a.txt"), "theirs\n");
        commit(&app.clone, "theirs", "me");

        // Stopped on a conflict: HEAD is detached, the branch is still the one rebased.
        let rebase = crate::git::git_str(&wt, &["-c", "core.editor=true", "rebase", "main"]);
        assert!(rebase.is_err(), "the rebase conflicts");
        assert_eq!(crate::git::current_branch(&wt).unwrap(), None);
        assert_eq!(moved_to(&item), None);
        assert!(ensure_on_branch(&app.state, id).is_ok());
        git(&wt, &["rebase", "--abort"]);

        git(&wt, &["bisect", "start", "HEAD", "main"]);
        git(&wt, &["checkout", "-q", "--detach", "main"]);
        assert_eq!(moved_to(&item), None, "bisecting the branch");
        git(&wt, &["bisect", "reset"]);

        // A plain detached HEAD is not on the branch.
        git(&wt, &["checkout", "-q", "--detach", "HEAD"]);
        assert_eq!(moved_to(&item).as_deref(), Some(""));
        let err = ensure_on_branch(&app.state, id).unwrap_err();
        assert!(err.contains("a detached HEAD"), "{err}");
    }

    #[test]
    fn a_task_on_a_branch_that_is_only_local_uses_it_as_it_is() {
        let app = TestApp::new();
        git(&app.clone, &["branch", "perf/local", "main"]);
        git(&app.clone, &["checkout", "-q", "perf/local"]);
        write(&app.clone.join("p.txt"), "faster\n");
        commit(&app.clone, "Hot and cold", "me");
        git(&app.clone, &["checkout", "-q", "main"]);

        // The branch name alone: not on GitHub, so the clone's branch, in a new worktree.
        let id = create_task(&app.state, Some(app.repo.clone()), "", "", "perf/local").unwrap();
        let (path, branch, _, _) = checkout(&app, id);
        assert_eq!(branch, "perf/local");
        assert_eq!(git(&path, &["log", "-1", "--format=%s"]), "Hot and cold");
        assert_eq!(
            git(&app.clone, &["config", "branch.perf/local.remote"]),
            "origin",
            "a plain push creates it on GitHub"
        );
        let item = app.state.store.item(id).unwrap();
        let context = std::fs::read_to_string(
            write_task_context(&app.state.data_dir, &app.repo, &item).unwrap(),
        )
        .unwrap();
        assert!(context.contains("it is not on GitHub yet"), "{context}");

        // Pushed, it is on GitHub like any other branch.
        git(&path, &["push", "-q"]);
        let context = std::fs::read_to_string(
            write_task_context(&app.state.data_dir, &app.repo, &item).unwrap(),
        )
        .unwrap();
        assert!(
            context.contains("a plain `git push` updates it"),
            "{context}"
        );
    }

    #[test]
    fn a_local_branch_named_for_a_fork_keeps_the_forks_remote_and_its_checkout() {
        let app = TestApp::new();
        app.fork("friend", "feature-x");
        git(&app.clone, &["checkout", "-q", "-b", "perf/mine"]);
        let id = create_task(
            &app.state,
            Some(app.repo.clone()),
            "",
            "",
            "friend:perf/mine",
        )
        .unwrap();
        let item = app.state.store.item(id).unwrap();
        assert_eq!(
            (
                std::fs::canonicalize(item.worktree_path.unwrap()).unwrap(),
                item.worktree_created
            ),
            (std::fs::canonicalize(&app.clone).unwrap(), false),
            "the clone has it checked out"
        );
        assert_eq!(
            git(&app.clone, &["config", "branch.perf/mine.remote"]),
            "friend",
            "the fork's remote stays, to push to"
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
            ("feature-x-2", "friend/feature-x")
        );
        assert_eq!(
            crate::git::upstream(&app.clone, "feature-x").as_deref(),
            Some("origin/main")
        );
    }
}
