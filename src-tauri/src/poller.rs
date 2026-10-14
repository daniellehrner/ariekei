//! Keeps the Inbox current: polls GitHub for the open PRs of every repo shown in a window
//! every minute (or right away when asked) and stores the result.

use crate::github::{self, PrSummary};
use crate::store::{PolledPr, Repo};
use serde::Serialize;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

/// Seconds between polls; `ARIEKEI_POLL_SECS` shortens it for tests.
fn interval() -> Duration {
    Duration::from_secs(
        std::env::var("ARIEKEI_POLL_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(60),
    )
}

pub struct Poller {
    wake: Sender<()>,
}

impl Poller {
    /// Starts polling; with `enabled` false (tests with fake repos) it never polls.
    pub fn start(app: AppHandle, enabled: bool) -> Self {
        let (wake, rx) = channel();
        if enabled {
            let _ = std::thread::Builder::new()
                .name("github-poller".into())
                .spawn(move || run(app, rx));
        }
        Self { wake }
    }

    /// Polls now instead of waiting for the next interval.
    pub fn refresh(&self) {
        let _ = self.wake.send(());
    }
}

/// A failed poll of one repo (`inbox-error`).
#[derive(Clone, Serialize)]
struct PollError {
    repo: i64,
    message: String,
}

fn run(app: AppHandle, wake: Receiver<()>) {
    loop {
        let state = app.state::<crate::AppState>();
        for id in state.windows.open_repos() {
            let Ok(Some(repo)) = state.store.repo(id) else {
                continue;
            };
            // `inbox-changed` and `inbox-error` carry the repo id: each window shows one.
            match poll(&app, &repo) {
                Ok(()) => {
                    let _ = app.emit("inbox-changed", repo.id);
                }
                Err(message) => {
                    tracing::warn!(
                        "GitHub poll of {}/{} failed: {message}",
                        repo.owner,
                        repo.name
                    );
                    let _ = app.emit(
                        "inbox-error",
                        PollError {
                            repo: repo.id,
                            message,
                        },
                    );
                }
            }
        }
        match wake.recv_timeout(interval()) {
            Ok(()) | Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        // Coalesce a burst of refresh requests into one poll.
        while wake.try_recv().is_ok() {}
    }
}

/// One poll of `repo`: the viewer's PRs, PRs requesting their review, and the PRs they
/// track (by number, so merged/closed ones get their final state).
pub fn poll(app: &AppHandle, repo: &Repo) -> Result<(), String> {
    let state = app.state::<crate::AppState>();
    let (o, n, me) = (&repo.owner, &repo.name, &repo.viewer);
    let mine = github::search_prs(o, n, me, &format!("author:{me}"))?;
    let requested = github::search_prs(o, n, me, &format!("review-requested:{me}"))?;
    let known: Vec<i64> = mine.iter().chain(&requested).map(|p| p.number).collect();
    let tracked: Vec<i64> = state
        .store
        .tracked_numbers(repo.id, "pr")?
        .into_iter()
        .filter(|t| !known.contains(t))
        .collect();
    let others = github::prs_by_number(o, n, me, &tracked)?;

    let mut prs: Vec<(PrSummary, &'static str)> = vec![];
    for pr in mine.into_iter().chain(requested).chain(others) {
        if prs.iter().any(|(p, _)| p.number == pr.number) {
            continue;
        }
        let role = role_of(&pr, me, &known);
        prs.push((pr, role));
    }

    // Issues: the ones assigned to the viewer, plus tracked ones (e.g. with a worktree).
    let assigned = github::assigned_issues(o, n, me)?;
    let assigned_numbers: Vec<i64> = assigned.iter().map(|i| i.number).collect();
    let tracked_issues: Vec<i64> = state
        .store
        .tracked_numbers(repo.id, "issue")?
        .into_iter()
        .filter(|t| !assigned_numbers.contains(t))
        .collect();
    let issues: Vec<_> = assigned
        .into_iter()
        .chain(github::issues_by_number(o, n, me, &tracked_issues)?)
        .collect();

    let before = state.store.items(repo.id)?;
    let polled: Vec<PolledPr> = prs
        .iter()
        .map(|(p, role)| PolledPr {
            number: p.number,
            kind: "pr",
            role,
            open: p.state == "OPEN",
            snapshot: serde_json::to_value(p).unwrap_or_default(),
        })
        .chain(issues.iter().map(|i| PolledPr {
            number: i.number,
            kind: "issue",
            role: if assigned_numbers.contains(&i.number) {
                "assignee"
            } else {
                "other"
            },
            open: i.state == "OPEN",
            snapshot: serde_json::to_value(i).unwrap_or_default(),
        }))
        .collect();
    state.store.sync_prs(repo.id, &polled)?;

    for item in state.store.items(repo.id)? {
        let old = before.iter().find(|b| b.id == item.id);
        if item.kind == "issue" {
            if old.is_none()
                && let Some(i) = issues.iter().find(|i| i.number == item.number)
            {
                state
                    .store
                    .set_last_seen(item.id, i.latest_activity.as_deref().unwrap_or(""))?;
            }
            continue;
        }
        let Some((pr, _)) = prs.iter().find(|(p, _)| p.number == item.number) else {
            continue;
        };
        match old.and_then(|o| serde_json::from_value::<PrSummary>(o.snapshot.clone()).ok()) {
            Some(previous) => crate::notify::pr_changed(app, &item.id.to_string(), &previous, pr),
            // First sight of this PR: what is there now counts as seen, so only later
            // activity shows up as new.
            None => state
                .store
                .set_last_seen(item.id, pr.latest_activity.as_deref().unwrap_or(""))?,
        }
    }
    crate::lifecycle::after_poll(&state, app, repo)?;
    Ok(())
}

/// Why a PR is in the Inbox. `requested` holds the numbers found by the author and
/// review-requested queries; a PR in there that the viewer did not write was requested.
pub fn role_of(pr: &PrSummary, viewer: &str, requested: &[i64]) -> &'static str {
    if pr.author.eq_ignore_ascii_case(viewer) {
        "author"
    } else if requested.contains(&pr.number) {
        "requested"
    } else {
        "other"
    }
}
