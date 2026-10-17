// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

//! The start and end of a work item's life:
//! - an issue whose pushed branch has a PR (opened by the user or the agent; Ariekei opens
//!   none) becomes that PR;
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
    if !item.is_task() {
        let name = worktree::pull_ref(item.number);
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

/// Run after every poll: follow the PRs opened for pushed issue branches, link those opened
/// for tasks' branches, and clean up items that are finished.
pub fn after_poll(state: &AppState, repo: &Repo) -> Result<(), String> {
    for item in state.store.items(repo.id)? {
        if item.is_task() && item.worktree_path.is_some() {
            match link_branch_pr(state, repo, &item) {
                Ok(Some(number)) => crate::notify::lifecycle(
                    state,
                    &item.id.to_string(),
                    &item.label(),
                    &format!("PR #{number} for its branch is linked to the task."),
                ),
                Ok(None) => {}
                Err(e) => tracing::warn!("task {}: its branch's PR: {e}", item.short_name()),
            }
        }
        if item.kind == "issue" && item.open && item.worktree_path.is_some() {
            match follow_pr(state, repo, &item) {
                Ok(true) => crate::notify::lifecycle(
                    state,
                    &item.id.to_string(),
                    &state.store.item(item.id)?.label(),
                    &format!("Issue #{} now follows its pull request.", item.number),
                ),
                Ok(false) => {}
                Err(e) => tracing::warn!("issue #{}: its PR: {e}", item.number),
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
                state,
                &item.id.to_string(),
                &item.label(),
                &format!("Cleaned up: {}.", removed.join(", ")),
            ),
            Cleanup::Blocked { blockers } if !had_blockers => crate::notify::lifecycle(
                state,
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
            outcome_closed(state, task)?;
        }
    }
    Ok(())
}

/// One of task `task`'s PRs or issues just closed: says so when it was the last open one.
/// The task is not cleaned up by itself: it may still open more, and cleanup would delete
/// its agent's conversation.
fn outcome_closed(state: &AppState, task: i64) -> Result<(), String> {
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
            state,
            &task.id.to_string(),
            &task.label(),
            "All its PRs and issues are closed. Mark the task done when you are finished with it.",
        );
    }
    Ok(())
}

/// If the issue's branch is on its push remote and has a PR, turns the item into that PR.
/// Returns whether the item was converted. Ariekei never opens the PR itself:
/// the user or the agent does, with a title and description of their own.
fn follow_pr(state: &AppState, repo: &Repo, item: &WorkItem) -> Result<bool, String> {
    let Some(branch) = item.branch.as_deref() else {
        return Ok(false);
    };
    let Some((head_owner, head_ref)) = pushed_head(repo, branch)? else {
        return Ok(false); // not pushed yet
    };
    let Some(found) = github::pull_for_head(
        &repo.owner,
        &repo.name,
        &repo.viewer,
        &head_owner,
        &head_ref,
    )?
    else {
        return Ok(false); // no PR yet
    };
    let number = found.number;
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
    Ok(true)
}

/// Where the local `branch` is pushed, if it is: the owner of its remote's GitHub repo and
/// the branch's name there (its upstream, which the remote has).
fn pushed_head(repo: &Repo, branch: &str) -> Result<Option<(String, String)>, String> {
    let config = |key: String| git::git_str(&repo.path, &["config", "--get", &key]);
    let (Ok(remote), Ok(merge)) = (
        config(format!("branch.{branch}.remote")),
        config(format!("branch.{branch}.merge")),
    ) else {
        return Ok(None);
    };
    let head_ref = merge
        .strip_prefix("refs/heads/")
        .unwrap_or(&merge)
        .to_owned();
    if !git::commit_exists(&repo.path, &format!("refs/remotes/{remote}/{head_ref}")) {
        return Ok(None);
    }
    let url = config(format!("remote.{remote}.url"))?;
    let (head_owner, _) =
        git::github_slug(&url).ok_or_else(|| format!("remote {remote} is not on GitHub"))?;
    Ok(Some((head_owner, head_ref)))
}

/// Links the open PR for the task's pushed branch to the task, if it is not linked yet:
/// also one opened outside its agent (in the browser, or by a fork's owner). Returns the
/// PR it linked. No search while a linked open PR has that head already.
pub fn link_branch_pr(
    state: &AppState,
    repo: &Repo,
    task: &WorkItem,
) -> Result<Option<i64>, String> {
    let Some(branch) = task.branch.as_deref() else {
        return Ok(None);
    };
    let Some((head_owner, head_ref)) = pushed_head(repo, branch)? else {
        return Ok(None);
    };
    let linked: Vec<WorkItem> = state
        .store
        .items(repo.id)?
        .into_iter()
        .filter(|i| i.task_id == Some(task.id) && i.kind == "pr")
        .collect();
    let has_head = |i: &WorkItem| {
        i.snapshot["headRef"] == head_ref.as_str()
            && i.snapshot["headOwner"]
                .as_str()
                .is_some_and(|o| o.eq_ignore_ascii_case(&head_owner))
    };
    if linked.iter().any(|i| i.open && has_head(i)) {
        return Ok(None);
    }
    let Some(found) = github::pull_for_head(
        &repo.owner,
        &repo.name,
        &repo.viewer,
        &head_owner,
        &head_ref,
    )?
    else {
        return Ok(None);
    };
    if linked.iter().any(|i| i.number == found.number) {
        return Ok(None);
    }
    crate::workitem::link_created(state, task.id, "pr", found.number)?;
    Ok(Some(found.number))
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
            starred: false,
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

    mod with_app {
        use super::super::*;
        use crate::hooks::AgentStatus;
        use crate::testkit::{TestApp, commit, git, write};
        use crate::workitem::{create_task, link_created, prepare};
        use serde_json::json;

        fn branch_exists(app: &TestApp, branch: &str) -> bool {
            crate::git::git_str(
                &app.clone,
                &[
                    "rev-parse",
                    "--verify",
                    "-q",
                    &format!("refs/heads/{branch}"),
                ],
            )
            .is_ok()
        }

        #[test]
        fn done_on_a_task_deletes_it_and_leaves_its_prs() {
            let app = TestApp::new();
            let task =
                create_task(&app.state, Some(app.repo.clone()), "Done soon", "", "").unwrap();
            app.add_pr(json!({"number": 20, "title": "Fix", "head_ref": "main"}));
            link_created(&app.state, task, "pr", 20).unwrap();
            let wt = app.path("proj_task1");

            write(&wt.join("x.txt"), "work\n");
            let Cleanup::Blocked { blockers } = clean_up(&app.state, task).unwrap() else {
                panic!("uncommitted work must block");
            };
            assert!(blockers[0].contains("x.txt"), "{blockers:?}");
            assert!(
                app.state
                    .store
                    .item(task)
                    .unwrap()
                    .cleanup_blockers
                    .is_some()
            );

            std::fs::remove_file(wt.join("x.txt")).unwrap();
            let Cleanup::Cleaned { removed } = clean_up(&app.state, task).unwrap() else {
                panic!("nothing is lost now");
            };
            assert!(
                removed.iter().any(|r| r.contains("proj_task1")),
                "{removed:?}"
            );
            assert!(app.state.store.item(task).is_err(), "the task is deleted");
            assert!(!wt.exists() && !branch_exists(&app, "task-1-done-soon"));
            assert_eq!(app.item(20).task_id, None, "its PR stays, unlinked");
        }

        #[test]
        fn discard_waits_for_the_agent_then_deletes_the_work() {
            let app = TestApp::new();
            let task =
                create_task(&app.state, Some(app.repo.clone()), "Throwaway", "", "").unwrap();
            let wt = app.path("proj_task1");
            write(&wt.join("a.txt"), "changed\n");
            commit(&wt, "local only", "me");
            write(&wt.join("x.txt"), "uncommitted\n");
            let losses = discard_losses(&app.state, task).unwrap();
            assert_eq!(losses.len(), 2, "{losses:?}");

            app.start_agent(task);
            app.state
                .report(&task.to_string(), AgentStatus::Working, None);
            assert!(matches!(
                discard(&app.state, task).unwrap(),
                Cleanup::Blocked { .. }
            ));
            assert!(wt.exists());

            app.state
                .report(&task.to_string(), AgentStatus::Finished, None);
            assert!(matches!(
                discard(&app.state, task).unwrap(),
                Cleanup::Cleaned { .. }
            ));
            assert!(!wt.exists() && !branch_exists(&app, "task-1-throwaway"));
            assert!(app.state.store.item(task).is_err());
            assert_eq!(
                app.state.terminals.status(&task.to_string()),
                None,
                "its agent is gone"
            );
            app.add_pr(json!({"number": 20, "title": "Not a task", "head_ref": "main"}));
            app.poll().unwrap();
            assert!(
                discard(&app.state, app.item(20).id).is_err(),
                "only a task can be discarded"
            );
        }

        #[test]
        fn a_merged_pr_waits_for_its_uncommitted_work_and_says_so_once() {
            let app = TestApp::new();
            let (id, wt) = app.pr_with_worktree(10, "feature/x");
            write(&wt.join("x.txt"), "not committed\n");
            app.set("prs", 10, json!({"state": "MERGED"}));
            app.poll().unwrap();
            app.poll().unwrap();
            let kept: Vec<_> = app
                .events
                .notices()
                .into_iter()
                .filter(|(_, _, body)| body.contains("kept so no work is lost"))
                .collect();
            assert_eq!(kept.len(), 1, "told once: {kept:?}");
            assert!(
                wt.exists()
                    && app
                        .row(10)
                        .is_some_and(|r| r.attention == Some("finished, cleanup blocked"))
            );

            std::fs::remove_file(wt.join("x.txt")).unwrap();
            app.poll().unwrap();
            assert!(!wt.exists());
            assert!(app.state.store.item(id).unwrap().hidden);
            assert!(
                app.events
                    .notices()
                    .iter()
                    .any(|(_, _, body)| body.starts_with("Cleaned up"))
            );
        }

        #[test]
        fn a_task_says_when_all_it_opened_is_closed() {
            let app = TestApp::new();
            let task = create_task(&app.state, Some(app.repo.clone()), "Outcomes", "", "").unwrap();
            app.add_pr(json!({"number": 20, "title": "One", "head_ref": "main"}));
            app.add_pr(json!({"number": 21, "title": "Two", "head_ref": "main"}));
            link_created(&app.state, task, "pr", 20).unwrap();
            link_created(&app.state, task, "pr", 21).unwrap();
            let closed = |app: &TestApp| {
                app.events
                    .notices()
                    .into_iter()
                    .filter(|(_, _, b)| b.starts_with("All its PRs and issues are closed"))
                    .count()
            };
            app.set("prs", 20, json!({"state": "MERGED"}));
            app.poll().unwrap();
            assert_eq!(closed(&app), 0, "one is still open");
            app.set("prs", 21, json!({"state": "CLOSED"}));
            app.poll().unwrap();
            app.poll().unwrap();
            assert_eq!(closed(&app), 1);
            assert!(app.path("proj_task1").exists(), "the task itself is kept");
        }

        #[test]
        fn the_pr_for_a_task_branch_is_linked_once_found() {
            let app = TestApp::new();
            app.fork("friend", "feature-x");
            let task = crate::workitem::create_task(
                &app.state,
                Some(app.repo.clone()),
                "",
                "",
                "friend:feature-x",
            )
            .unwrap();
            app.poll().unwrap();
            assert!(app.items().iter().all(|i| i.task_id.is_none()));
            // Someone else's branch of the same name, and then the fork's PR.
            app.add_pr(json!({"number": 101, "title": "Other", "author": "bob", "head_ref": "feature-x", "head_sha": "1"}));
            app.add_pr(
                json!({"number": 102, "title": "EVM", "author": "friend", "head_ref": "feature-x",
                              "head_owner": "friend", "head_sha": "2"}),
            );
            app.poll().unwrap();
            let linked: Vec<i64> = app
                .items()
                .iter()
                .filter(|i| i.task_id == Some(task))
                .map(|i| i.number)
                .collect();
            assert_eq!(linked, [102]);
            assert!(
                app.events
                    .notices()
                    .iter()
                    .any(|(_, title, body)| title == "T1 feature-x"
                        && body == "PR #102 for its branch is linked to the task.")
            );
            // Linked: later polls don't search for it again.
            app.take_calls();
            app.poll().unwrap();
            assert!(!app.take_calls().iter().any(|c| c["head"] == true));
        }

        #[test]
        fn a_pushed_issue_branch_opens_no_pr_and_follows_the_one_opened() {
            let app = TestApp::new();
            app.add_issue(json!({"number": 5, "title": "Crash", "assigned": true}));
            app.poll().unwrap();
            let id = app.item(5).id;
            let prepared = prepare(&app.state, id).unwrap();
            write(&prepared.path.join("a.txt"), "fixed\n");
            commit(&prepared.path, "fix", "me");
            git(&prepared.path, &["push", "-q"]);
            app.poll().unwrap();
            assert!(
                app.writes()
                    .iter()
                    .all(|w| w["post"].as_str().is_none_or(|p| !p.ends_with("/pulls"))),
                "Ariekei opens no PR"
            );
            assert_eq!(app.state.store.item(id).unwrap().kind, "issue");
            // The agent (or the user) opens it.
            app.add_pr(
                json!({"number": 100, "title": "Fix the crash", "head_ref": prepared.branch}),
            );
            app.poll().unwrap();
            let pr = app.state.store.item(id).unwrap();
            assert_eq!(
                (pr.kind.as_str(), pr.number, pr.issue_number),
                ("pr", 100, Some(5))
            );
            assert!(
                app.events
                    .notices()
                    .iter()
                    .any(|(_, title, body)| title == "#100 Fix the crash"
                        && body == "Issue #5 now follows its pull request.")
            );
        }

        #[test]
        fn an_issue_branch_with_a_pr_already_becomes_that_pr() {
            let app = TestApp::new();
            app.add_issue(json!({"number": 6, "title": "Typo", "assigned": true}));
            app.poll().unwrap();
            let id = app.item(6).id;
            let prepared = prepare(&app.state, id).unwrap();
            write(&prepared.path.join("a.txt"), "typo fixed\n");
            commit(&prepared.path, "fix", "me");
            git(&prepared.path, &["push", "-q"]);
            // Opened with `gh pr create`: the poll finds it as an item of its own first.
            app.add_pr(
                json!({"number": 130, "title": "Fix the typo", "head_ref": prepared.branch}),
            );
            app.poll().unwrap();
            assert!(
                app.writes()
                    .iter()
                    .all(|w| w["post"].as_str().is_none_or(|p| !p.ends_with("/pulls"))),
                "no draft"
            );
            let numbers: Vec<i64> = app.items().iter().map(|i| i.number).collect();
            assert_eq!(
                numbers.iter().filter(|n| **n == 130).count(),
                1,
                "{numbers:?}"
            );
            let pr = app.state.store.item(id).unwrap();
            assert_eq!(
                (pr.number, pr.issue_number, pr.worktree_path.is_some()),
                (130, Some(6), true)
            );
        }

        #[test]
        fn an_issue_that_became_its_pr_is_not_listed_again_while_the_pr_is_open() {
            let app = TestApp::new();
            app.add_issue(json!({"number": 6, "title": "Typo", "assigned": true}));
            app.poll().unwrap();
            let id = app.item(6).id;
            let prepared = prepare(&app.state, id).unwrap();
            write(&prepared.path.join("a.txt"), "typo fixed\n");
            commit(&prepared.path, "fix", "me");
            git(&prepared.path, &["push", "-q"]);
            app.add_pr(
                json!({"number": 130, "title": "Fix the typo", "head_ref": prepared.branch}),
            );
            app.poll().unwrap();
            app.poll().unwrap();
            assert!(app.row(6).is_none(), "the issue is still open and assigned");
            assert_eq!(app.row(130).unwrap().issue_number, Some(6));
            // Closed without merging: the issue is listed again.
            app.set("prs", 130, json!({"state": "CLOSED"}));
            app.poll().unwrap();
            app.poll().unwrap();
            assert!(app.row(6).is_some());
        }
    }
}
