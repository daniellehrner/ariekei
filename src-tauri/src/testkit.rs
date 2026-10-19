// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

//! Test kit for the logic behind the commands: an `AppState` without a Tauri app (events
//! are recorded), a "GitHub" repo with a clone of it, GitHub itself played by
//! `e2e/fake_gh.py`, and agents by `e2e/fake_claude.py` (as in the e2e scenarios).

use crate::AppState;
use crate::events::Recorded;
use crate::git::git_str;
use crate::inbox::Row;
use crate::store::{Repo, WorkItem};
use crate::terminal::Launch;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tempfile::TempDir;

/// Where the clone's `origin` says it lives; `insteadOf` maps it to the local "GitHub" repo.
pub const GITHUB: &str = "git@github.com:owner/proj.git";

fn e2e(file: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../e2e")
        .join(file)
}

/// Runs git in `dir` without the user's settings getting in the way (signing, line endings).
pub fn git(dir: &Path, args: &[&str]) -> String {
    let mut all = vec![
        "-c",
        "commit.gpgsign=false",
        "-c",
        "core.autocrlf=false",
        "-c",
        "init.defaultBranch=main",
    ];
    all.extend(args);
    git_str(dir, &all).unwrap_or_else(|e| panic!("git {args:?} in {}: {e}", dir.display()))
}

/// Commits everything in `dir` as `who` (the fake GitHub takes the name as their login).
pub fn commit(dir: &Path, message: &str, who: &str) {
    git(dir, &["add", "-A"]);
    git(
        dir,
        &[
            "-c",
            &format!("user.name={who}"),
            "-c",
            &format!("user.email={who}@example.com"),
            "commit",
            "-qm",
            message,
        ],
    );
}

pub fn write(path: &Path, text: &str) {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).unwrap();
    }
    std::fs::write(path, text).unwrap();
}

/// An `AppState` connected to a clone of a local "GitHub" repo (`owner/proj`, viewer `me`).
pub struct TestApp {
    pub root: TempDir,
    pub state: AppState,
    pub events: Arc<Recorded>,
    pub repo: Repo,
    /// The "GitHub" repo (non-bare; pushes to other branches than main are accepted).
    pub upstream: PathBuf,
    pub clone: PathBuf,
    gh_dir: PathBuf,
}

impl TestApp {
    pub fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let upstream = root.path().join("github-proj");
        let clone = root.path().join("proj");
        std::fs::create_dir_all(&upstream).unwrap();
        git(&upstream, &["init", "-q", "-b", "main"]);
        git(
            &upstream,
            &["config", "receive.denyCurrentBranch", "ignore"],
        );
        write(&upstream.join("a.txt"), "one\ntwo\nthree\n");
        commit(&upstream, "init", "me");
        git(
            root.path(),
            &[
                "clone",
                "-q",
                &upstream.to_string_lossy(),
                &clone.to_string_lossy(),
            ],
        );
        git(
            &clone,
            &[
                "config",
                &format!("url.{}.insteadOf", upstream.to_string_lossy()),
                GITHUB,
            ],
        );
        git(&clone, &["remote", "set-url", "origin", GITHUB]);

        let gh_dir = root.path().join("gh");
        std::fs::create_dir_all(&gh_dir).unwrap();
        std::fs::write(
            gh_dir.join("data.json"),
            json!({"viewer": "me", "upstream": upstream, "prs": [], "issues": []}).to_string(),
        )
        .unwrap();
        crate::github::use_fake(Some(&gh_dir));

        let data_dir = root.path().join("data");
        std::fs::create_dir_all(&data_dir).unwrap();
        let hooks = data_dir.join("claude-hooks.json");
        // Nothing listens there: the fake agent's hook calls fail quietly. Tests report
        // statuses themselves.
        std::fs::write(
            &hooks,
            json!({"hooks": {"Stop": [{"hooks": [{"type": "http", "url": "http://127.0.0.1:9/hooks"}]}]}})
                .to_string(),
        )
        .unwrap();
        let launch = Launch {
            claude: e2e("fake_claude.py"),
            hooks_settings: hooks,
            extra_args: vec![],
            idle_after: Duration::from_secs(3600),
            quiet_after: Duration::from_secs(3600),
            env: vec![(
                "FAKE_CLAUDE_DIR".into(),
                root.path().join("claude").to_string_lossy().into_owned(),
            )],
        };
        let store = crate::store::Store::in_memory();
        let repo = store
            .add_repo(&clone, "owner", "proj", "origin", "me")
            .unwrap();
        let events = Arc::new(Recorded::default());
        let state = AppState::new(
            store,
            data_dir,
            events.clone(),
            Ok(launch),
            crate::poller::Poller::idle(),
        );
        Self {
            root,
            state,
            events,
            repo,
            upstream,
            clone,
            gh_dir,
        }
    }

    pub fn path(&self, name: &str) -> PathBuf {
        self.root.path().join(name)
    }

    // --- the fake GitHub ---

    pub fn data(&self) -> Value {
        serde_json::from_str(&std::fs::read_to_string(self.gh_dir.join("data.json")).unwrap())
            .unwrap()
    }

    pub fn set_data(&self, data: &Value) {
        std::fs::write(self.gh_dir.join("data.json"), data.to_string()).unwrap();
    }

    /// Adds a PR (`number`, `title`, `head_ref` at least; author `me`, base `main`, open).
    pub fn add_pr(&self, pr: Value) {
        let mut pr = pr;
        for (k, v) in [("author", "me"), ("base_ref", "main"), ("state", "OPEN")] {
            pr.as_object_mut()
                .unwrap()
                .entry(k)
                .or_insert(Value::from(v));
        }
        let mut data = self.data();
        data["prs"].as_array_mut().unwrap().push(pr);
        self.set_data(&data);
    }

    /// Adds an issue (`number`, `title` at least; by `carol`).
    pub fn add_issue(&self, issue: Value) {
        let mut issue = issue;
        issue
            .as_object_mut()
            .unwrap()
            .entry("author")
            .or_insert(Value::from("carol"));
        let mut data = self.data();
        data["issues"].as_array_mut().unwrap().push(issue);
        self.set_data(&data);
    }

    /// Changes fields of PR or issue `number` (`kind`: "prs" or "issues").
    pub fn set(&self, kind: &str, number: i64, fields: Value) {
        let mut data = self.data();
        for item in data[kind].as_array_mut().unwrap() {
            if item["number"] == number {
                for (k, v) in fields.as_object().unwrap() {
                    item[k] = v.clone();
                }
            }
        }
        self.set_data(&data);
    }

    /// Removes PR or issue `number` from GitHub (deleted, transferred).
    pub fn remove(&self, kind: &str, number: i64) {
        let mut data = self.data();
        data[kind]
            .as_array_mut()
            .unwrap()
            .retain(|i| i["number"] != number);
        self.set_data(&data);
    }

    fn jsonl(&self, file: &str) -> Vec<Value> {
        std::fs::read_to_string(self.gh_dir.join(file))
            .unwrap_or_default()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    /// What was sent to GitHub (REST posts, GraphQL mutations).
    pub fn writes(&self) -> Vec<Value> {
        self.jsonl("writes.jsonl")
    }

    /// The calls made to GitHub since the last `take_calls` (see `log_call` in fake_gh.py).
    pub fn take_calls(&self) -> Vec<Value> {
        let calls = self.jsonl("calls.jsonl");
        let _ = std::fs::remove_file(self.gh_dir.join("calls.jsonl"));
        calls
    }

    /// While set, GitHub's rate limit is used up.
    pub fn rate_limited(&self, on: bool) {
        let flag = self.gh_dir.join("rate-limited");
        if on {
            std::fs::write(flag, "").unwrap();
        } else {
            let _ = std::fs::remove_file(flag);
        }
    }

    /// Commits `files` (path, content) on `branch` of the "GitHub" repo as `who`; the branch
    /// starts from main if it is new.
    pub fn push_to(&self, branch: &str, files: &[(&str, &str)], who: &str) {
        let up = &self.upstream;
        if git_str(
            up,
            &[
                "rev-parse",
                "--verify",
                "-q",
                &format!("refs/heads/{branch}"),
            ],
        )
        .is_err()
        {
            git(up, &["branch", branch, "main"]);
        }
        git(up, &["checkout", "-q", branch]);
        for (path, content) in files {
            write(&up.join(path), content);
        }
        commit(up, &format!("{who} on {branch}"), who);
        git(up, &["checkout", "-q", "main"]);
    }

    /// A fork `<owner>/proj` of the "GitHub" repo with a commit of `owner` on `branch`
    /// (changing c.txt), reached from the clone like GitHub's (through insteadOf).
    pub fn fork(&self, owner: &str, branch: &str) -> PathBuf {
        let fork = self.root.path().join(format!("github-{owner}-proj"));
        git(
            self.root.path(),
            &[
                "clone",
                "-q",
                &self.upstream.to_string_lossy(),
                &fork.to_string_lossy(),
            ],
        );
        git(&fork, &["checkout", "-q", "-b", branch]);
        write(&fork.join("c.txt"), &format!("{owner}'s work\n"));
        commit(&fork, &format!("{owner} on {branch}"), owner);
        git(&fork, &["checkout", "-q", "--detach"]);
        git(
            &self.clone,
            &[
                "config",
                &format!("url.{}.insteadOf", fork.to_string_lossy()),
                &format!("git@github.com:{owner}/proj.git"),
            ],
        );
        fork
    }

    /// A PR of `me` from branch `branch` (with one commit of `me` changing b.txt), polled into
    /// the Inbox, opened and given a worktree; returns its item id and the worktree.
    pub fn pr_with_worktree(&self, number: i64, branch: &str) -> (i64, PathBuf) {
        self.push_to(branch, &[("b.txt", "mine\n")], "me");
        self.add_pr(json!({"number": number, "title": format!("PR {number}"), "head_ref": branch}));
        self.poll().unwrap();
        let id = self.item(number).id;
        // Opened first, as in the UI (which tracks it: polled by number from then on).
        crate::workitem::mark_seen(&self.state, id).unwrap();
        let prepared = crate::workitem::prepare(&self.state, id).unwrap();
        (id, prepared.path)
    }

    // --- the app ---

    /// One poll of the repo (`full`: all details).
    pub fn poll(&self) -> Result<(), String> {
        crate::poller::poll(&self.state, &self.repo, false)
    }

    pub fn items(&self) -> Vec<WorkItem> {
        self.state.store.items(self.repo.id).unwrap()
    }

    pub fn item(&self, number: i64) -> WorkItem {
        self.items()
            .into_iter()
            .find(|i| i.number == number)
            .unwrap_or_else(|| panic!("no item #{number}"))
    }

    pub fn rows(&self) -> Vec<Row> {
        crate::workitem::inbox(&self.state, Some(self.repo.clone())).unwrap()
    }

    pub fn row(&self, number: i64) -> Option<Row> {
        self.rows().into_iter().find(|r| r.number == number)
    }

    /// Starts item `id`'s agent (the fake one) in its worktree, as opening its terminal does.
    pub fn start_agent(&self, id: i64) {
        let item = self.state.store.item(id).unwrap();
        let cwd = item
            .worktree_path
            .clone()
            .expect("the item has no worktree");
        let view = tauri::ipc::Channel::new(|_| Ok(()));
        self.state
            .terminals
            .open(
                &id.to_string(),
                &item.label(),
                &cwd,
                || crate::workitem::session_plan(&self.state, &self.state.data_dir, id),
                portable_pty::PtySize::default(),
                view,
            )
            .unwrap();
        self.wait_for(|| {
            self.state
                .terminals
                .status(&id.to_string())
                .is_some_and(|s| s != crate::hooks::AgentStatus::Starting)
        });
    }

    /// Waits (up to 20 s) until `done` holds.
    pub fn wait_for(&self, done: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while !done() {
            assert!(Instant::now() < deadline, "timed out");
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

impl Drop for TestApp {
    fn drop(&mut self) {
        self.state.terminals.close_all();
        crate::github::use_fake(None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::Sent;
    use crate::hooks::AgentStatus;

    #[test]
    fn the_kit_polls_the_fake_github_and_runs_fake_agents() {
        let app = TestApp::new();
        app.add_pr(json!({"number": 7, "title": "Seven", "head_ref": "main"}));
        app.poll().unwrap();
        let row = app.row(7).expect("PR #7 is in the Inbox");
        assert_eq!((row.title.as_str(), row.role.as_str()), ("Seven", "author"));
        assert!(app.take_calls().iter().any(|c| c["search"] == true));

        let id = crate::workitem::create_task(&app.state, Some(app.repo.clone()), "Look", "", "")
            .unwrap();
        app.start_agent(id);
        let key = id.to_string();
        assert_eq!(app.state.terminals.status(&key), Some(AgentStatus::Ready));
        let sent = app.events.take();
        assert!(sent.contains(&Sent::Status(key.clone(), AgentStatus::Starting)));
        assert!(sent.contains(&Sent::Status(key, AgentStatus::Ready)));
        assert!(app.path("proj_task1").is_dir());
    }
}
