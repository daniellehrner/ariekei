//! The start and end of a work item's life:
//! - an issue whose branch was pushed gets a draft PR, and the item becomes that PR;
//! - a finished item (PR merged/closed, issue closed, or marked done) is cleaned up:
//!   agent, transcript, worktree and branch (if Ariekei created them), refs and context
//!   are removed. Cleanup never loses work: it is refused while anything would be lost.

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
pub fn clean_up(state: &AppState, id: i64) -> Result<Cleanup, String> {
    let item = state.store.item(id)?;
    let repo = state
        .store
        .repo(item.repo_id)?
        .ok_or("repository not found")?;
    let key = id.to_string();
    let found = blockers(&repo, &item, state.terminals.status(&key));
    if !found.is_empty() {
        state.store.set_cleanup_blockers(id, Some(&found))?;
        return Ok(Cleanup::Blocked { blockers: found });
    }

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
        // Without --force: git refuses if the worktree has changes after all.
        git(&repo.path, &["worktree", "remove", &dir.to_string_lossy()])?;
        removed.push(format!("the worktree {}", dir.display()));
        if let Some(branch) = item.branch.as_deref().filter(|b| !b.is_empty()) {
            // Its commits are on GitHub (checked above), so deleting the branch loses nothing.
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
    for name in [
        format!("refs/ariekei/reviewed/pr-{}", item.number),
        worktree::pull_ref(item.number),
    ] {
        if git::commit_exists(&repo.path, &name)
            && git(&repo.path, &["update-ref", "-d", &name]).is_ok()
        {
            removed.push(name);
        }
    }
    for number in [Some(item.number), item.issue_number].into_iter().flatten() {
        for kind in ["pr", "issue"] {
            let _ = std::fs::remove_file(
                state
                    .data_dir
                    .join("context")
                    .join(format!("{}-{}-{kind}{number}.md", repo.owner, repo.name)),
            );
        }
    }
    state.store.mark_cleaned(id)?;
    Ok(Cleanup::Cleaned { removed })
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
                    &format!(
                        "#{number} {}",
                        item.snapshot["title"].as_str().unwrap_or_default()
                    ),
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
        let finished = !item.hidden && matches!(state_on_github, "MERGED" | "CLOSED");
        if !finished {
            continue;
        }
        let had_blockers = item.cleanup_blockers.is_some();
        match clean_up(state, item.id)? {
            Cleanup::Cleaned { removed } if !removed.is_empty() => crate::notify::lifecycle(
                app,
                &item.id.to_string(),
                &format!(
                    "#{} {}",
                    item.number,
                    item.snapshot["title"].as_str().unwrap_or_default()
                ),
                &format!("Cleaned up: {}.", removed.join(", ")),
            ),
            Cleanup::Blocked { blockers } if !had_blockers => crate::notify::lifecycle(
                app,
                &item.id.to_string(),
                &format!(
                    "#{} {}",
                    item.number,
                    item.snapshot["title"].as_str().unwrap_or_default()
                ),
                &format!(
                    "Finished, but kept so no work is lost: {}.",
                    blockers.join("; ")
                ),
            ),
            _ => {}
        }
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
