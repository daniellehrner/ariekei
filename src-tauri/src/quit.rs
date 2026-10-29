// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

//! Quitting ends every agent, and with it what it runs in the background (dev servers,
//! watchers): resuming a conversation does not start those again. So quitting asks first
//! when it would stop work, naming the agents, where they work and what they run.

use crate::AppState;
use crate::hooks::AgentStatus;
use crate::procs;
use serde::Serialize;
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Emitter, Manager};

/// An agent whose work quitting would stop.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Running {
    pub item: i64,
    /// The item as the Inbox names it (`T1 …`, `#42 …`).
    pub name: String,
    /// `owner/name` of its repo.
    pub repo: String,
    pub worktree: Option<String>,
    /// In the middle of a turn (working, or waiting for permission).
    pub busy: bool,
    /// What it started that still runs (e.g. its background shells).
    pub processes: Vec<Started>,
}

#[derive(Debug, Serialize)]
pub struct Started {
    pub pid: u32,
    pub command: String,
}

/// Whether any agent runs at all (cheap: no process list); only then can quitting stop work.
pub fn any_agents(state: &AppState) -> bool {
    !state.terminals.agents().is_empty()
}

/// The agents that are in a turn or run processes of their own.
pub fn running(state: &AppState) -> Vec<Running> {
    let agents = state.terminals.agents();
    if agents.is_empty() {
        return vec![];
    }
    let table = procs::table()
        .inspect_err(|e| tracing::warn!("no process list: {e}"))
        .unwrap_or_default();
    agents
        .into_iter()
        .filter_map(|(key, pid, status)| {
            let busy = matches!(
                status,
                AgentStatus::Working | AgentStatus::WaitingForPermission
            );
            let processes: Vec<Started> = pid
                .map(|pid| procs::started_by(&table, pid))
                .unwrap_or_default()
                .into_iter()
                .map(|p| Started {
                    pid: p.pid,
                    command: procs::shown_command(&p.command),
                })
                .collect();
            if !busy && processes.is_empty() {
                return None;
            }
            let id: i64 = key.parse().ok()?;
            let item = state.store.item(id).ok()?;
            let repo = state.store.repo(item.repo_id).ok().flatten()?;
            Some(Running {
                item: id,
                name: item.label(),
                repo: format!("{}/{}", repo.owner, repo.name),
                worktree: item.worktree_path.map(|p| p.display().to_string()),
                busy,
                processes,
            })
        })
        .collect()
}

/// Whether quitting needs a look first: agents run, and the user has not confirmed yet.
pub fn ask_first(app: &AppHandle) -> bool {
    let state = app.state::<AppState>();
    !state.quit_confirmed.load(Ordering::Relaxed) && any_agents(&state)
}

/// Looks for work quitting would stop and asks about it in window `label` (`confirm-quit`),
/// or quits when there is none. Off the event loop: the process list can take a moment.
pub fn check(app: AppHandle, label: String) {
    std::thread::spawn(move || {
        let running = running(&app.state::<AppState>());
        if running.is_empty() {
            quit_now(&app);
        } else if let Err(e) = app.emit_to(label.as_str(), "confirm-quit", &running) {
            tracing::warn!("could not ask before quitting: {e}");
        }
    });
}

/// Quits without asking (again).
pub fn quit_now(app: &AppHandle) {
    app.state::<AppState>()
        .quit_confirmed
        .store(true, Ordering::Relaxed);
    crate::crashlog::quitting("Quit, with nothing running or after the user confirmed");
    app.exit(0);
}
