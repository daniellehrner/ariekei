//! The start and end of a work item's life:
//! - an issue whose branch was pushed gets a draft PR, and the item becomes that PR;
//! - a finished item (PR merged/closed, issue closed, or marked done) is cleaned up:
//!   agent, transcript, worktree and branch (if Ariekei created them), refs and context
//!   are removed. Cleanup never loses work: it is refused while anything would be lost;
//! - a task is never finished by GitHub (it may still open more PRs): it ends when the user
//!   marks it done (cleaned up like the rest, then deleted) or discards it (its work is
//!   deleted too, after the user confirmed what is lost).

use crate::AppState;
use crate::git::{self, git};
use crate::github;
use crate::hooks::AgentStatus;
use crate::store::{Repo, WorkItem};
use crate::worktree;
use serde::Serialize;
use std::path::Path;

/// Why the item cannot be cleaned up without losing something; empty if it can.
pub fn blockers(repo: &Repo, item: &WorkItem, agent: Option<AgentStatus>) -> Vec<String> {
    let mut found = vec![];
    if matches!(
        agent,
        Some(AgentStatus::Working | AgentStatus::WaitingForPermission)
    ) {
        found.push("The agent is in the middle of a turn".to_owned());
    }
    let Some(dir) = item.worktree_path.as_deref().filter(|p| p.exists()) else {
        return found;
    };
    match git::git_str(dir, &["status", "--porcelain"]) {
        Ok(status) if !status.is_empty() => {
            let files: Vec<&str> = status.lines().map(|l| l.get(3..).unwrap_or(l)).collect();
            let more = if files.len() > 5 {
                format!(" and {} more", files.len() - 5)
            } else {
                String::new()
            };
            found.push(format!(
                "{} uncommitted change{}: {}{more}",
                files.len(),
                if files.len() == 1 { "" } else { "s" },
                files[..files.len().min(5)].join(", ")
            ));
        }
        Ok(_) => {}
        Err(e) => found.push(format!("Could not check for uncommitted changes: {e}")),
    }
    // Commits are safe when a remote branch or GitHub's copy of a PR head has them.
    if item.kind == "pr" {
        let _ = worktree::fetch_pr_head(
            &repo.path,
            &repo.base_remote,
            item.number,
            &summary_base(item),
        );
    }
    match git::git_str(
        dir,
        &[
            "log",
            "--format=%h %s",
            "HEAD",
            "--not",
            "--remotes",
            "--glob=refs/ariekei/pull/*",
        ],
    ) {
        Ok(log) if !log.is_empty() => {
            let commits: Vec<&str> = log.lines().collect();
            found.push(format!(
                "{} commit{} not on GitHub: {}",
                commits.len(),
                if commits.len() == 1 { " is" } else { "s are" },
                commits[..commits.len().min(5)].join("; ")
            ));
        }
        Ok(_) => {}
        Err(e) => found.push(format!("Could not check for unpushed commits: {e}")),
    }
    found
}

fn summary_base(item: &WorkItem) -> String {
    item.snapshot["baseRef"]
        .as_str()
        .unwrap_or("main")
        .to_owned()
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "outcome", rename_all = "camelCase")]
pub enum Cleanup {
    /// Everything listed was removed.
    Cleaned { removed: Vec<String> },
    /// Nothing was removed.
    Blocked { blockers: Vec<String> },
}

/// Cleans up item `id` if nothing would be lost; otherwise records and returns why not.
/// A task is deleted afterwards (its PRs and issues stay, unlinked).
pub fn clean_up(state: &AppState, id: i64) -> Result<Cleanup, String> {
    let item = state.store.item(id)?;
    let repo = state
        .store
        .repo(item.repo_id)?
        .ok_or("repository not found")?;
    let found = blockers(&repo, &item, state.terminals.status(&id.to_string()));
    if !found.is_empty() {
        state.store.set_cleanup_blockers(id, Some(&found))?;
        return Ok(Cleanup::Blocked { blockers: found });
    }
    let removed = remove(state, &repo, &item, false)?;
    if item.is_task() {
        state.store.delete_item(id)?;
    } else {
        state.store.mark_cleaned(id)?;
    }
    Ok(Cleanup::Cleaned { removed })
}

/// What discarding task `id` would lose besides the agent session: uncommitted changes and
/// commits that are not on GitHub.
pub fn discard_losses(state: &AppState, id: i64) -> Result<Vec<String>, String> {
    let item = state.store.item(id)?;
    let repo = state
        .store
        .repo(item.repo_id)?
        .ok_or("repository not found")?;
    Ok(blockers(&repo, &item, None))
}

/// Deletes task `id` with its worktree and branch even if that loses work (the user
/// confirmed `discard_losses`); refused only while its agent is in the middle of a turn.
pub fn discard(state: &AppState, id: i64) -> Result<Cleanup, String> {
    let item = state.store.item(id)?;
    if !item.is_task() {
        return Err("only a task can be discarded".into());
    }
    let repo = state
        .store
        .repo(item.repo_id)?
        .ok_or("repository not found")?;
    if matches!(
        state.terminals.status(&id.to_string()),
        Some(AgentStatus::Working | AgentStatus::WaitingForPermission)
    ) {
        return Ok(Cleanup::Blocked {
            blockers: vec!["The agent is in the middle of a turn".to_owned()],
        });
    }
    let removed = remove(state, &repo, &item, true)?;
    state.store.delete_item(id)?;
    Ok(Cleanup::Cleaned { removed })
}

/// Removes the item's agent, transcript, worktree and branch (if Ariekei created them),
/// refs and context files; `force`: even with uncommitted changes.
fn remove(
    state: &AppState,
    repo: &Repo,
    item: &WorkItem,
    force: bool,
) -> Result<Vec<String>, String> {
    let key = item.id.to_string();
    let mut removed = vec![];
    if state.terminals.status(&key).is_some() {
        state.terminals.close(&key);
        removed.push("the agent process".to_owned());
    }
    if let Some(transcript) = item.transcript_path.as_deref()
        && remove_transcript(transcript)
    {
        removed.push("the Claude session transcript".to_owned());
    }
    if let (Some(dir), true) = (
        item.worktree_path.as_deref().filter(|p| p.exists()),
        item.worktree_created,
    ) {
        // Without --force (unless discarding): git refuses if the worktree has changes after all.
        let dir_arg = dir.to_string_lossy();
        let mut args = vec!["worktree", "remove", dir_arg.as_ref()];
        if force {
            args.push("--force");
        }
        git(&repo.path, &args)?;
        removed.push(format!("the worktree {}", dir.display()));
        if let Some(branch) = item.branch.as_deref().filter(|b| !b.is_empty()) {
            // Its commits are on GitHub (checked above, unless discarding), so deleting the
            // branch loses nothing.
            if git(&repo.path, &["branch", "-D", branch]).is_ok() {
                removed.push(format!("the branch {branch}"));
            }
        }
    } else if let Some(dir) = item.worktree_path.as_deref() {
        removed.push(format!(
            "Ariekei's link to {} (your worktree is kept)",
            dir.display()
        ));
    }
    let mut refs = vec![crate::workitem::reviewed_ref(item)];
    if !item.is_task() {
        refs.push(worktree::pull_ref(item.number));
    }
    for name in refs {
        if git::commit_exists(&repo.path, &name)
            && git(&repo.path, &["update-ref", "-d", &name]).is_ok()
        {
            removed.push(name);
        }
    }
    let mut contexts = vec![item.slug()];
    if !item.is_task() {
        for number in [Some(item.number), item.issue_number].into_iter().flatten() {
            contexts.extend(["pr", "issue"].map(|kind| format!("{kind}{number}")));
        }
    }
    for name in contexts {
        let _ = std::fs::remove_file(
            state
                .data_dir
                .join("context")
                .join(format!("{}-{}-{name}.md", repo.owner, repo.name)),
        );
    }
    Ok(removed)
}

/// Deletes a session transcript (`<id>.jsonl`) and its `<id>/` folder of subagent
/// transcripts, if present.
fn remove_transcript(path: &Path) -> bool {
    let removed = std::fs::remove_file(path).is_ok();
    if let Some(dir) = path.file_stem().map(|stem| path.with_file_name(stem)) {
        let _ = std::fs::remove_dir_all(dir);
    }
    removed
}

/// Run after every poll: open draft PRs for pushed issue branches, and clean up items
/// that are finished.
pub fn after_poll(state: &AppState, app: &tauri::AppHandle, repo: &Repo) -> Result<(), String> {
    for item in state.store.items(repo.id)? {
        if item.kind == "issue" && item.open && item.worktree_path.is_some() {
            match open_draft_pr(state, repo, &item) {
                Ok(Some(number)) => crate::notify::lifecycle(
                    app,
                    &item.id.to_string(),
                    &format!("#{number} {}", item.title()),
                    "Opened a draft pull request for the pushed branch.",
                ),
                Ok(None) => {}
                Err(e) => tracing::warn!("issue #{}: draft PR: {e}", item.number),
            }
        }
        // Only GitHub's own state counts: a PR that merely dropped out of a search (e.g.
        // past its 50 results) is not finished. Items with a worktree or session are
        // tracked, so they are always fetched by number and have a real state.
        let state_on_github = item.snapshot["state"].as_str().unwrap_or("OPEN");
        let finished =
            !item.hidden && !item.is_task() && matches!(state_on_github, "MERGED" | "CLOSED");
        if !finished {
            continue;
        }
        let had_blockers = item.cleanup_blockers.is_some();
        match clean_up(state, item.id)? {
            Cleanup::Cleaned { removed } if !removed.is_empty() => crate::notify::lifecycle(
                app,
                &item.id.to_string(),
                &item.label(),
                &format!("Cleaned up: {}.", removed.join(", ")),
            ),
            Cleanup::Blocked { blockers } if !had_blockers => crate::notify::lifecycle(
                app,
                &item.id.to_string(),
                &item.label(),
                &format!(
                    "Finished, but kept so no work is lost: {}.",
                    blockers.join("; ")
                ),
            ),
            _ => {}
        }
        if let Some(task) = item.task_id.filter(|_| !had_blockers) {
            outcome_closed(state, app, task)?;
        }
    }
    Ok(())
}

/// One of task `task`'s PRs or issues just closed: says so when it was the last open one.
/// The task is not cleaned up by itself: it may still open more, and cleanup would delete
/// its agent's conversation.
fn outcome_closed(state: &AppState, app: &tauri::AppHandle, task: i64) -> Result<(), String> {
    let Ok(task) = state.store.item(task) else {
        return Ok(());
    };
    let all_closed = state
        .store
        .items(task.repo_id)?
        .iter()
        .filter(|i| i.task_id == Some(task.id))
        .all(|i| !i.open);
    if all_closed {
        crate::notify::lifecycle(
            app,
            &task.id.to_string(),
            &task.label(),
            "All its PRs and issues are closed. Mark the task done when you are finished with it.",
        );
    }
    Ok(())
}

/// If the issue's branch is on its push remote, makes sure it has a PR (opening a draft
/// if there is none) and turns the item into that PR. Returns the PR number when the
/// item was converted.
fn open_draft_pr(state: &AppState, repo: &Repo, item: &WorkItem) -> Result<Option<i64>, String> {
    let Some(branch) = item.branch.as_deref() else {
        return Ok(None);
    };
    let remote = git::git_str(&repo.path, &["config", &format!("branch.{branch}.remote")])?;
    if !git::commit_exists(&repo.path, &format!("refs/remotes/{remote}/{branch}")) {
        return Ok(None); // not pushed yet
    }
    let url = git::git_str(
        &repo.path,
        &["config", "--get", &format!("remote.{remote}.url")],
    )?;
    let (head_owner, _) =
        git::github_slug(&url).ok_or_else(|| format!("remote {remote} is not on GitHub"))?;
    let base = github::default_branch(&repo.owner, &repo.name)?;
    let number =
        match github::pull_for_head(&repo.owner, &repo.name, &repo.viewer, &head_owner, branch)? {
            Some(pr) => pr.number,
            None => {
                let head = if head_owner.eq_ignore_ascii_case(&repo.owner) {
                    branch.to_owned()
                } else {
                    format!("{head_owner}:{branch}")
                };
                let title = item.snapshot["title"].as_str().unwrap_or(branch);
                let body = format!("Closes #{}.", item.number);
                github::create_draft_pull(&repo.owner, &repo.name, &head, &base, title, &body)?
            }
        };
    let pr = github::prs_by_number(&repo.owner, &repo.name, &repo.viewer, &[number])?
        .into_iter()
        .next()
        .ok_or_else(|| format!("PR #{number} not found"))?;
    // The poll may already have listed the new PR as an item of its own: merge into ours.
    if let Some(other) = state
        .store
        .items(repo.id)?
        .into_iter()
        .find(|i| i.number == number && i.id != item.id)
    {
        if other.worktree_path.is_some() {
            return Err(format!("PR #{number} already has its own worktree"));
        }
        state.store.delete_item(other.id)?;
    }
    state.store.convert_to_pr(
        item.id,
        number,
        &serde_json::to_value(&pr).unwrap_or_default(),
    )?;
    Ok(Some(number))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::tests::{commit, repo as git_repo};

    fn item(worktree: &Path) -> WorkItem {
        WorkItem {
            id: 1,
            repo_id: 1,
            number: 7,
            snapshot: serde_json::json!({ "baseRef": "main" }),
            open: false,
            worktree_path: Some(worktree.to_owned()),
            branch: Some("feature".into()),
            session_id: None,
            session_started: false,
            transcript_path: None,
            reviewed_sha: None,
            last_seen: None,
            ui_state: None,
            role: "author".into(),
            tracked: true,
            kind: "issue".into(),
            issue_number: None,
            worktree_created: true,
            hidden: false,
            cleanup_blockers: None,
            task_id: None,
        }
    }

    #[test]
    fn blocks_on_uncommitted_changes_unpushed_commits_and_a_busy_agent() {
        let upstream = git_repo();
        let root = tempfile::tempdir().unwrap();
        let clone = root.path().join("c");
        git(
            root.path(),
            &["clone", "-q", &upstream.path().to_string_lossy(), "c"],
        )
        .unwrap();
        let repo = Repo {
            id: 1,
            path: clone.clone(),
            owner: "o".into(),
            name: "c".into(),
            base_remote: "origin".into(),
            viewer: "me".into(),
        };
        let it = item(&clone);

        assert!(
            blockers(&repo, &it, None).is_empty(),
            "a clean clone at origin/main loses nothing"
        );
        assert_eq!(blockers(&repo, &it, Some(AgentStatus::Working)).len(), 1);

        std::fs::write(clone.join("new.txt"), "x").unwrap();
        let b = blockers(&repo, &it, Some(AgentStatus::Finished));
        assert!(
            b.len() == 1 && b[0].starts_with("1 uncommitted change: new.txt"),
            "{b:?}"
        );

        commit(&clone, "local work");
        let b = blockers(&repo, &it, None);
        assert!(
            b.len() == 1
                && b[0].contains("1 commit is not on GitHub")
                && b[0].contains("local work"),
            "{b:?}"
        );

        git(&clone, &["push", "-q", "origin", "HEAD:refs/heads/feature"]).unwrap();
        git(&clone, &["fetch", "-q", "origin"]).unwrap();
        assert!(
            blockers(&repo, &it, None).is_empty(),
            "pushed commits are safe"
        );
    }

    #[test]
    fn removes_transcripts_with_their_subagent_folder() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("abc.jsonl");
        std::fs::write(&file, "{}").unwrap();
        std::fs::create_dir(dir.path().join("abc")).unwrap();
        std::fs::write(dir.path().join("abc").join("sub.jsonl"), "{}").unwrap();
        assert!(remove_transcript(&file));
        assert!(!file.exists() && !dir.path().join("abc").exists());
        assert!(!remove_transcript(&file), "already gone");
    }
}
