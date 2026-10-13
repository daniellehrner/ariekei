//! Keeps the Inbox current: polls GitHub for the active repo's open PRs every minute
//! (or right away when asked) and stores the result.

use crate::github::{self, PrSummary};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

const INTERVAL: Duration = Duration::from_secs(60);

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

fn run(app: AppHandle, wake: Receiver<()>) {
    loop {
        let result = poll(&app);
        match &result {
            Ok(()) => {
                let _ = app.emit("inbox-changed", ());
            }
            Err(e) => {
                tracing::warn!("GitHub poll failed: {e}");
                let _ = app.emit("inbox-error", e);
            }
        }
        match wake.recv_timeout(INTERVAL) {
            Ok(()) | Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        // Coalesce a burst of refresh requests into one poll.
        while wake.try_recv().is_ok() {}
    }
}

/// One poll of the active repo. Public so a command can run it synchronously.
pub fn poll(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<crate::AppState>();
    let Some(repo) = state.store.active_repo()? else {
        return Ok(());
    };
    let prs = github::open_prs(&repo.owner, &repo.name, &repo.viewer)?;

    let before = state.store.items(repo.id)?;
    let snapshots: Vec<(i64, serde_json::Value)> = prs
        .iter()
        .map(|p| (p.number, serde_json::to_value(p).unwrap_or_default()))
        .collect();
    state.store.sync_open_prs(repo.id, &snapshots)?;

    for item in state.store.items(repo.id)? {
        let Some(pr) = prs.iter().find(|p| p.number == item.number) else {
            continue;
        };
        let old = before.iter().find(|b| b.id == item.id);
        match old.and_then(|o| serde_json::from_value::<PrSummary>(o.snapshot.clone()).ok()) {
            Some(previous) => crate::notify::pr_changed(app, &item.id.to_string(), &previous, pr),
            // First sight of this PR: what is there now counts as seen, so only later
            // activity shows up as new.
            None => state
                .store
                .set_last_seen(item.id, pr.latest_activity.as_deref().unwrap_or(""))?,
        }
    }
    Ok(())
}
