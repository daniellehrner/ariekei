// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

//! Worktrees the agents make themselves: `git worktree add` in a shell command, Claude Code's
//! `EnterWorktree`, a subagent with `isolation: worktree`. Ariekei notices one after the tool
//! call that made it (`scan`), shows it on the item, and cleans it up with the item
//! (`lifecycle`); the user can also remove it, or make it a task of its own.
//!
//! Whose it is: one inside the agent's own worktree (Claude Code's `.claude/worktrees/…`) is
//! that agent's; one elsewhere is the agent's whose shell command named it (its folder or its
//! branch) and worktrees. Those a clone had when Ariekei first looked are nobody's, and so are
//! those no agent's command named (e.g. the user's own).

use crate::AppState;
use crate::git::{self, git, git_str};
use crate::hooks::HookEvent;
use crate::lifecycle;
use crate::store::{AgentWorktree, Repo, WorkItem};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The worktrees of each clone that are nobody's or already someone's: a new one is not.
#[derive(Default)]
pub struct Watch {
    known: Mutex<HashMap<PathBuf, Known>>,
}

struct Known {
    /// When Ariekei first looked (Unix seconds): a branch made since is the agent's.
    since: i64,
    paths: HashSet<PathBuf>,
}

impl Watch {
    /// Notes the worktrees `clone` has, the first time only: they are nobody's (or someone's
    /// already). Returns when it first looked.
    pub fn baseline(&self, clone: &Path) -> Result<i64, String> {
        let mut known = self.known.lock().unwrap();
        if let Some(k) = known.get(&canon(clone)) {
            return Ok(k.since);
        }
        let since = now();
        let paths = present(clone)?.into_iter().map(|w| w.path).collect();
        known.insert(canon(clone), Known { since, paths });
        Ok(since)
    }
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// The same folder, spelled one way (git and Ariekei spell some paths differently, e.g.
/// `/private/var/…` on macOS).
fn canon(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_owned())
}

/// The clone's worktrees that are there (not the clone itself), spelled by `canon`.
fn present(clone: &Path) -> Result<Vec<git::Worktree>, String> {
    Ok(git::worktrees(clone)?
        .into_iter()
        .filter(|w| !w.main && w.path.exists())
        .map(|w| git::Worktree {
            path: canon(&w.path),
            ..w
        })
        .collect())
}

/// Tools that may make a worktree: a shell command, Claude Code's own worktree tool, and a
/// subagent (which may run in a worktree of its own).
const TOOLS: [&str; 5] = ["Bash", "PowerShell", "EnterWorktree", "Agent", "Task"];

/// Run for every hook event of an agent Ariekei started: notes the clone's worktrees before
/// its turn, and looks for new ones after a tool call that may have made one (`scan`, in the
/// background: it runs git).
pub fn after_event(app: &tauri::AppHandle, state: &AppState, event: &HookEvent) {
    let Some(id) = state
        .terminals
        .key_for(event.terminal.as_deref(), &event.session_id)
        .and_then(|key| key.parse::<i64>().ok())
    else {
        return;
    };
    let tool = event.tool_name.as_deref().unwrap_or_default();
    match event.hook_event_name.as_str() {
        "UserPromptSubmit" | "PreToolUse" => {
            let clone = state
                .store
                .item(id)
                .and_then(|item| state.store.repo(item.repo_id))
                .ok()
                .flatten()
                .map(|repo| repo.path);
            if let Some(clone) = clone
                && let Err(e) = state.agent_worktrees.baseline(&clone)
            {
                tracing::warn!("listing the worktrees of {}: {e}", clone.display());
            }
        }
        "PostToolUse" | "PostToolUseFailure" if TOOLS.contains(&tool) => {
            let command = event
                .tool_input
                .as_ref()
                .and_then(|i| i["command"].as_str())
                .map(str::to_owned);
            let app = app.clone();
            std::thread::spawn(move || {
                use tauri::Manager as _;
                if let Err(e) = scan(&app.state::<AppState>(), id, command.as_deref()) {
                    tracing::warn!("looking for the worktrees agent {id} made: {e}");
                }
            });
        }
        _ => {}
    }
}

/// After a tool call of item `id`'s agent (`command`: the shell command it ran, if it was one):
/// records the new worktrees it made, and forgets those that are gone.
pub fn scan(state: &AppState, id: i64, command: Option<&str>) -> Result<(), String> {
    let item = state.store.item(id)?;
    let repo = state
        .store
        .repo(item.repo_id)?
        .ok_or("unknown repository")?;
    let since = state.agent_worktrees.baseline(&repo.path)?;
    let listed = present(&repo.path)?;
    forget_gone(state, &repo, &listed)?;
    let items: HashSet<PathBuf> = state
        .store
        .items(repo.id)?
        .iter()
        .filter_map(|i| i.worktree_path.as_deref().map(canon))
        .collect();
    let own = item.worktree_path.as_deref().map(canon);
    let mut known = state.agent_worktrees.known.lock().unwrap();
    let Some(known) = known.get_mut(&canon(&repo.path)) else {
        return Ok(());
    };
    known.paths.retain(|p| listed.iter().any(|w| &w.path == p));
    let mut found = false;
    for w in &listed {
        if known.paths.contains(&w.path)
            || items.contains(&w.path)
            || !made_by(w, own.as_deref(), command)
        {
            continue;
        }
        let branch_created = w
            .branch
            .as_deref()
            .is_some_and(|b| made_since(&repo.path, b, since));
        state
            .store
            .add_agent_worktree(id, &w.path, w.branch.as_deref(), branch_created)?;
        known.paths.insert(w.path.clone());
        found = true;
    }
    if found {
        state.events.worktrees_changed(id);
    }
    Ok(())
}

/// Whether the agent working in `own` made worktree `w`: it is inside `own`, or the shell
/// command it ran (`command`) is about worktrees and names it.
fn made_by(w: &git::Worktree, own: Option<&Path>, command: Option<&str>) -> bool {
    if own.is_some_and(|own| w.path.starts_with(own)) {
        return true;
    }
    let Some(command) = command.filter(|c| c.contains("worktree")) else {
        return false;
    };
    // Not a name so short it is in many a command.
    let named = |name: &str| name.len() > 2 && command.contains(name);
    w.path
        .file_name()
        .is_some_and(|n| named(&n.to_string_lossy()))
        || w.branch.as_deref().is_some_and(named)
}

/// Whether `branch` was made after `since` (Unix seconds; one made in that very second may
/// have been there before, and is kept): its reflog starts then.
fn made_since(clone: &Path, branch: &str, since: i64) -> bool {
    let log = git_str(
        clone,
        &[
            "reflog",
            "show",
            "--date=unix",
            "--format=%gd %gs",
            &format!("refs/heads/{branch}"),
        ],
    )
    .unwrap_or_default();
    // Oldest last: `<branch>@{<time>} branch: Created from …`.
    log.lines()
        .last()
        .and_then(|first| first.split_once("} "))
        .is_some_and(|(at, what)| {
            what.starts_with("branch: Created")
                && at
                    .rsplit_once("@{")
                    .and_then(|(_, t)| t.parse::<i64>().ok())
                    .is_some_and(|t| t > since)
        })
}

/// Forgets the agents' worktrees of `repo` that are no longer among `listed` (removed by the
/// agent, e.g. Claude Code's own cleanup, or by the user).
fn forget_gone(state: &AppState, repo: &Repo, listed: &[git::Worktree]) -> Result<(), String> {
    for w in state.store.agent_worktrees(repo.id, None)? {
        if !listed.iter().any(|l| l.path == canon(&w.path)) {
            state.store.delete_agent_worktree(w.id)?;
            state.events.worktrees_changed(w.item_id);
        }
    }
    Ok(())
}

/// Item `id`'s agent's worktrees that are there and not an item's own (e.g. made a task).
fn live(state: &AppState, repo: &Repo, id: i64) -> Result<Vec<AgentWorktree>, String> {
    let listed = present(&repo.path)?;
    forget_gone(state, repo, &listed)?;
    let items: HashSet<PathBuf> = state
        .store
        .items(repo.id)?
        .iter()
        .filter_map(|i| i.worktree_path.as_deref().map(canon))
        .collect();
    Ok(state
        .store
        .agent_worktrees(repo.id, Some(id))?
        .into_iter()
        .filter(|w| !items.contains(&canon(&w.path)))
        .collect())
}

/// What removing worktree `w` (and its branch, if the agent made it) would lose.
fn losses_of(repo: &Repo, w: &AgentWorktree) -> Vec<String> {
    let branch = w.branch.as_deref();
    let deleted = branch.filter(|b| w.branch_created && git::branch_exists(&repo.path, b));
    lifecycle::losses(repo, Some(&w.path), branch, deleted)
}

fn folder(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// Why item `item`'s cleanup would lose something in the worktrees its agent made (`prs`: as
/// in `lifecycle::blockers`, fetched first).
pub fn blockers(
    state: &AppState,
    repo: &Repo,
    item: &WorkItem,
    prs: &[i64],
) -> Result<Vec<String>, String> {
    let worktrees = live(state, repo, item.id)?;
    if worktrees.is_empty() {
        return Ok(vec![]);
    }
    lifecycle::fetch_heads(repo, prs, &item.short_name());
    Ok(worktrees
        .iter()
        .flat_map(|w| {
            losses_of(repo, w)
                .into_iter()
                .map(|l| format!("The agent's worktree {}: {l}", folder(&w.path)))
        })
        .collect())
}

/// Removes the worktrees item `item`'s agent made, and their branches if it made those too;
/// `force`: even if that loses work. Says what it removed.
pub fn remove_all(
    state: &AppState,
    repo: &Repo,
    item: &WorkItem,
    force: bool,
) -> Result<Vec<String>, String> {
    let mut removed = vec![];
    for w in live(state, repo, item.id)? {
        removed.extend(remove(state, repo, &w, force)?);
    }
    Ok(removed)
}

/// Removes worktree `w` and its branch if the agent made it. Claude Code locks the worktrees
/// it makes ("claude session <name> (pid <N> …)") and leaves the lock when it exits: such a
/// lock is lifted once that Claude Code is gone; any other lock is the user's, and refuses.
fn remove(
    state: &AppState,
    repo: &Repo,
    w: &AgentWorktree,
    force: bool,
) -> Result<Vec<String>, String> {
    let lock = git::worktrees(&repo.path)?
        .into_iter()
        .find(|l| canon(&l.path) == canon(&w.path))
        .and_then(|l| l.locked);
    if let Some(reason) = lock {
        match claude_lock(&reason) {
            Some(pid) if running(pid) => {
                return Err(format!(
                    "Claude Code (process {pid}) still works in {}",
                    w.path.display()
                ));
            }
            Some(_) => {
                git(
                    &repo.path,
                    &["worktree", "unlock", &w.path.to_string_lossy()],
                )?;
            }
            None => {
                return Err(format!(
                    "{} is locked ({reason}): unlock it to remove it",
                    w.path.display()
                ));
            }
        }
    }
    lifecycle::remove_worktree(&repo.path, &w.path, force)?;
    let mut removed = vec![format!("the agent's worktree {}", w.path.display())];
    if let Some(branch) = w.branch.as_deref().filter(|_| w.branch_created)
        && git(&repo.path, &["branch", "-D", branch]).is_ok()
    {
        removed.push(format!("the branch {branch}"));
    }
    state.store.delete_agent_worktree(w.id)?;
    state.events.worktrees_changed(w.item_id);
    Ok(removed)
}

/// The process of Claude Code's lock reason "claude session <name> (pid <N> start <T>)".
fn claude_lock(reason: &str) -> Option<u32> {
    let rest = reason.strip_prefix("claude ")?;
    let pid = rest.split("(pid ").nth(1)?;
    pid.split(|c: char| !c.is_ascii_digit())
        .next()?
        .parse()
        .ok()
}

fn running(pid: u32) -> bool {
    crate::procs::table().is_ok_and(|t| t.iter().any(|p| p.pid == pid))
}

/// A worktree item `id`'s agent made, as the item shows it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Shown {
    pub id: i64,
    pub path: PathBuf,
    pub branch: Option<String>,
    /// The agent made the branch too: it goes with the worktree.
    pub branch_created: bool,
    /// What removing it would lose (`lifecycle::losses`; the PR heads are not fetched for it).
    pub losses: Vec<String>,
}

/// The worktrees item `id`'s agent made, oldest first.
pub fn list(state: &AppState, id: i64) -> Result<Vec<Shown>, String> {
    let item = state.store.item(id)?;
    let repo = state
        .store
        .repo(item.repo_id)?
        .ok_or("unknown repository")?;
    Ok(live(state, &repo, id)?
        .into_iter()
        .map(|w| Shown {
            losses: losses_of(&repo, &w),
            id: w.id,
            path: w.path,
            branch: w.branch,
            branch_created: w.branch_created,
        })
        .collect())
}

fn find(state: &AppState, id: i64, worktree: i64) -> Result<(Repo, AgentWorktree), String> {
    let item = state.store.item(id)?;
    let repo = state
        .store
        .repo(item.repo_id)?
        .ok_or("unknown repository")?;
    let w = live(state, &repo, id)?
        .into_iter()
        .find(|w| w.id == worktree)
        .ok_or("that worktree is gone")?;
    Ok((repo, w))
}

/// Removes worktree `worktree` of item `id`'s agent; `force`: even if that loses work (the
/// user saw what). Refused while the agent is in the middle of a turn: it may work there.
pub fn remove_one(state: &AppState, id: i64, worktree: i64, force: bool) -> Result<(), String> {
    if lifecycle::busy(state, id) {
        return Err("The agent is in the middle of a turn: remove it once it has finished".into());
    }
    let (repo, w) = find(state, id, worktree)?;
    if !force {
        let lost = losses_of(&repo, &w);
        if !lost.is_empty() {
            return Err(format!("Removing it would lose: {}", lost.join("; ")));
        }
    }
    remove(state, &repo, &w, force).map(|_| ())
}

/// Makes worktree `worktree` of item `id`'s agent a task of its own, on its branch; it is
/// cleaned up with the task as if Ariekei had made it. Returns the task.
pub fn make_task(state: &AppState, id: i64, worktree: i64) -> Result<i64, String> {
    let item = state.store.item(id)?;
    let (repo, w) = find(state, id, worktree)?;
    let branch = w
        .branch
        .clone()
        .ok_or("Its HEAD is detached: check out a branch there first")?;
    let notes = format!(
        "The agent of {} made this worktree ({}).",
        item.label(),
        folder(&w.path)
    );
    let task = crate::workitem::create_task(state, Some(repo.clone()), "", &notes, &branch)?;
    let made = state.store.item(task)?;
    if made
        .worktree_path
        .as_deref()
        .is_some_and(|p| lifecycle::same_dir(p, &w.path))
    {
        state
            .store
            .set_worktree(task, Some(&w.path), Some(&branch), true, w.branch_created)?;
        state.store.delete_agent_worktree(w.id)?;
    }
    state.events.worktrees_changed(id);
    state.events.inbox_changed(repo.id);
    Ok(task)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::Sent;
    use crate::testkit::{TestApp, commit, git as run};

    fn scan_after(
        app: &TestApp,
        id: i64,
        command: Option<&str>,
    ) -> Vec<(String, Option<String>, bool)> {
        scan(&app.state, id, command).unwrap();
        list(&app.state, id)
            .unwrap()
            .into_iter()
            .map(|w| (folder(&w.path), w.branch, w.branch_created))
            .collect()
    }

    #[test]
    fn the_worktrees_an_agent_makes_are_its_own() {
        let app = TestApp::new();
        let (id, dir) = app.pr_with_worktree(5, "feature");
        let (other, _) = app.pr_with_worktree(6, "other");
        // There before Ariekei looked: nobody's, though a command names it later.
        run(
            &app.clone,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "old-try",
                &app.path("proj_old").to_string_lossy(),
            ],
        );
        run(&app.clone, &["branch", "kept-branch"]);
        assert!(scan_after(&app, id, None).is_empty());
        // Branches made in the second Ariekei looked may have been there before.
        std::thread::sleep(std::time::Duration::from_millis(1100));

        // A command about worktrees that names the new one (its folder, or its branch).
        let add = |name: &str, args: &[&str]| {
            let path = app.path(name).to_string_lossy().into_owned();
            let mut all = vec!["worktree", "add", "-q"];
            all.extend(args);
            all.push(&path);
            run(&app.clone, &all);
        };
        add("proj_try", &["-b", "try-it"]);
        assert!(
            scan_after(&app, other, Some("git status")).is_empty(),
            "not named"
        );
        assert!(
            scan_after(&app, other, Some("ls ../proj_try")).is_empty(),
            "not about worktrees"
        );
        assert_eq!(
            scan_after(&app, id, Some("git worktree add -b try-it ../proj_try")),
            [("proj_try".to_owned(), Some("try-it".to_owned()), true)]
        );
        assert!(
            scan_after(&app, other, Some("git worktree add ../proj_try")).is_empty(),
            "already the first's"
        );
        assert!(
            scan_after(&app, id, Some("git worktree list | grep proj_old")).len() == 1,
            "nobody's stays so"
        );

        // Inside its own worktree (Claude Code's `EnterWorktree`, a subagent's), on a branch
        // that was there before: kept when the worktree goes.
        add("unused", &[]);
        let inside = dir.join(".claude/worktrees/probe");
        run(
            &app.clone,
            &[
                "worktree",
                "add",
                "-q",
                &inside.to_string_lossy(),
                "kept-branch",
            ],
        );
        let found = scan_after(&app, id, None);
        assert_eq!(
            found[1],
            ("probe".to_owned(), Some("kept-branch".to_owned()), false)
        );
        assert!(
            app.events
                .sent
                .lock()
                .unwrap()
                .contains(&Sent::WorktreesChanged(id))
        );

        // Removed by the agent: forgotten.
        run(
            &app.clone,
            &["worktree", "remove", &inside.to_string_lossy()],
        );
        assert_eq!(scan_after(&app, id, None).len(), 1);
    }

    #[test]
    fn cleanup_takes_the_agents_worktrees_unless_that_loses_work() {
        let app = TestApp::new();
        let (id, dir) = app.pr_with_worktree(5, "feature");
        app.state.agent_worktrees.baseline(&app.clone).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let path = app.path("proj_try");
        run(
            &app.clone,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "try-it",
                &path.to_string_lossy(),
            ],
        );
        // Claude Code's lock, left by a Claude Code that is gone.
        run(
            &app.clone,
            &[
                "worktree",
                "lock",
                "--reason",
                "claude session try (pid 999999999 start 1)",
                &path.to_string_lossy(),
            ],
        );
        scan(&app.state, id, Some("git worktree add ../proj_try")).unwrap();
        // As git and the cleanup spell it (`/private/var/…` on macOS), while it is there.
        let spelled = canon(&path);

        crate::testkit::write(&path.join("new.txt"), "x\n");
        let blocked = crate::lifecycle::clean_up(&app.state, id).unwrap();
        let crate::lifecycle::Cleanup::Blocked { blockers } = blocked else {
            panic!("cleaned up: {blocked:?}");
        };
        assert_eq!(
            blockers,
            ["The agent's worktree proj_try: 1 uncommitted change: new.txt"]
        );
        let shown = list(&app.state, id).unwrap();
        assert_eq!(shown[0].losses, ["1 uncommitted change: new.txt"]);
        assert!(remove_one(&app.state, id, shown[0].id, false).is_err());

        commit(&path, "try", "me");
        let blocked = crate::lifecycle::clean_up(&app.state, id).unwrap();
        assert!(
            matches!(&blocked, crate::lifecycle::Cleanup::Blocked { blockers } if blockers[0].contains("1 commit is not on GitHub")),
            "{blocked:?}"
        );

        run(&path, &["reset", "-q", "--hard", "HEAD~1"]);
        let cleaned = crate::lifecycle::clean_up(&app.state, id).unwrap();
        let crate::lifecycle::Cleanup::Cleaned { removed } = cleaned else {
            panic!("blocked: {cleaned:?}");
        };
        assert!(
            removed.contains(&format!("the agent's worktree {}", spelled.display())),
            "{removed:?}"
        );
        assert!(
            removed.contains(&"the branch try-it".to_owned()),
            "{removed:?}"
        );
        assert!(!path.exists() && !dir.exists());
        assert!(!crate::git::branch_exists(&app.clone, "try-it"));
    }

    #[test]
    fn a_worktree_the_agent_made_becomes_a_task() {
        let app = TestApp::new();
        let (id, _) = app.pr_with_worktree(5, "feature");
        app.state.agent_worktrees.baseline(&app.clone).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let path = app.path("proj_try");
        run(
            &app.clone,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "try-it",
                &path.to_string_lossy(),
            ],
        );
        let detached = app.path("proj_detached");
        run(
            &app.clone,
            &[
                "worktree",
                "add",
                "-q",
                "--detach",
                &detached.to_string_lossy(),
            ],
        );
        scan(&app.state, id, Some("git worktree add -b try-it ../proj_try && git worktree add --detach ../proj_detached")).unwrap();
        let shown = list(&app.state, id).unwrap();
        let on = |branch: Option<&str>| {
            shown
                .iter()
                .find(|w| w.branch.as_deref() == branch)
                .unwrap()
                .id
        };
        assert!(
            make_task(&app.state, id, on(None))
                .unwrap_err()
                .contains("detached")
        );

        let task = make_task(&app.state, id, on(Some("try-it"))).unwrap();
        let task = app.state.store.item(task).unwrap();
        assert_eq!(
            (
                task.title(),
                task.branch.as_deref(),
                task.worktree_created,
                task.branch_created
            ),
            ("try-it", Some("try-it"), true, true)
        );
        assert!(crate::lifecycle::same_dir(
            task.worktree_path.as_deref().unwrap(),
            &path
        ));
        assert_eq!(list(&app.state, id).unwrap().len(), 1, "the task's now");
    }

    #[test]
    fn claude_codes_locks_name_their_process() {
        assert_eq!(
            claude_lock("claude session probe (pid 2430869 start 57603076)"),
            Some(2430869)
        );
        assert_eq!(claude_lock("claude agent a1b2 (pid 12)"), Some(12));
        assert_eq!(claude_lock("on a USB disk"), None);
        assert!(running(std::process::id()));
    }
}
