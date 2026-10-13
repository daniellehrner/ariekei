//! Work item operations behind the UI commands: connecting a repo, the Inbox rows,
//! preparing a PR's worktree, starting its agent with context, and review markers.

use crate::AppState;
use crate::git::{self, git};
use crate::github::{self, PrSummary};
use crate::inbox::{self, Row};
use crate::store::{Repo, WorkItem};
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
            state.store.set_worktree(item.id, None, None)?;
        }
    }
    Ok(())
}

fn reviewed_marker(cwd: &Path, item: &WorkItem) -> Option<String> {
    item.reviewed_sha
        .clone()
        .filter(|sha| git::commit_exists(cwd, sha))
}

fn unreviewed(cwd: &Path, item: &WorkItem, pr: &PrSummary, repo: &Repo) -> bool {
    let Ok(head) = git::head(cwd) else {
        return false;
    };
    let marker = reviewed_marker(cwd, item).or_else(|| {
        git::merge_base(
            cwd,
            "HEAD",
            &format!("{}/{}", repo.base_remote, pr.base_ref),
        )
        .ok()
    });
    marker.as_deref() != Some(head.as_str()) || git::is_dirty(cwd).unwrap_or(false)
}

pub fn inbox(state: &AppState) -> Result<Vec<Row>, String> {
    let Some(repo) = state.store.active_repo()? else {
        return Ok(vec![]);
    };
    let rows = state
        .store
        .items(repo.id)?
        .into_iter()
        // Closed items stay listed only while they still have a worktree (cleanup is later).
        .filter(|item| item.open || item.worktree_path.is_some())
        .filter_map(|item| {
            let pr = summary(&item).ok()?;
            let worktree = item.worktree_path.clone().filter(|p| p.exists());
            Some(Row {
                id: item.id,
                number: item.number,
                title: pr.title.clone(),
                url: pr.url.clone(),
                is_draft: pr.is_draft,
                open: item.open,
                updated_at: pr.updated_at.clone(),
                ci: pr.ci.clone(),
                failing_checks: pr.failing_checks.clone(),
                review_decision: pr.review_decision.clone(),
                unresolved_threads: pr.unresolved_threads,
                new_activity: pr.latest_activity.as_deref().unwrap_or("")
                    > item.last_seen.as_deref().unwrap_or(""),
                agent: state.terminals.status(&item.id.to_string()),
                unreviewed: worktree
                    .as_deref()
                    .is_some_and(|w| unreviewed(w, &item, &pr, &repo)),
                worktree: worktree.map(|p| p.to_string_lossy().into_owned()),
                attention: None,
            })
        })
        .collect();
    Ok(inbox::order(rows))
}

/// Opening an item counts as seeing its current GitHub activity.
pub fn mark_seen(state: &AppState, id: i64) -> Result<(), String> {
    let item = state.store.item(id)?;
    if let Ok(pr) = summary(&item)
        && let Some(at) = pr.latest_activity
    {
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
    let prepared = worktree::prepare(&repo, &summary(&item)?)?;
    state
        .store
        .set_worktree(id, Some(&prepared.path), Some(&prepared.branch))?;
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
    let context_file = match write_context(data_dir, &repo, item.number) {
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

// --- review -------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewState {
    pub worktree: String,
    /// The commit the review diff starts from.
    pub marker: String,
    /// "reviewed" when the user marked it, "base" for the PR's merge base.
    pub marker_kind: &'static str,
    pub head: String,
    pub commits: Vec<git::Commit>,
    pub dirty: bool,
}

pub fn review_state(state: &AppState, id: i64) -> Result<ReviewState, String> {
    let (item, repo) = item_repo(state, id)?;
    let cwd = worktree_of(&item)?;
    let pr = summary(&item)?;
    let head = git::head(&cwd)?;
    let (marker, marker_kind) = match reviewed_marker(&cwd, &item) {
        Some(sha) => (sha, "reviewed"),
        None => (
            git::merge_base(
                &cwd,
                "HEAD",
                &format!("{}/{}", repo.base_remote, pr.base_ref),
            )
            .unwrap_or_else(|_| head.clone()),
            "base",
        ),
    };
    Ok(ReviewState {
        worktree: cwd.to_string_lossy().into_owned(),
        commits: git::commits_since(&cwd, &marker)?,
        dirty: git::is_dirty(&cwd)?,
        marker,
        marker_kind,
        head,
    })
}

/// Marks HEAD as reviewed. Also kept as `refs/ariekei/reviewed/pr-<N>` in the clone so the
/// commit stays reachable when the agent rebases or amends.
pub fn mark_reviewed(state: &AppState, id: i64) -> Result<String, String> {
    let (item, repo) = item_repo(state, id)?;
    let cwd = worktree_of(&item)?;
    let head = git::head(&cwd)?;
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

pub fn review_files(
    state: &AppState,
    id: i64,
    from: &str,
    to: Option<&str>,
) -> Result<Vec<git::ChangedFile>, String> {
    git::changed_files(&worktree_of(&state.store.item(id)?)?, from, to)
}

pub fn review_file(
    state: &AppState,
    id: i64,
    from: &str,
    to: Option<&str>,
    path: &str,
) -> Result<git::FileDiff, String> {
    git::file_diff(&worktree_of(&state.store.item(id)?)?, from, to, path)
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
