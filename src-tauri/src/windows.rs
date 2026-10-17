// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

//! Windows and the repos they show. Each window shows one repo, or the Connect screen
//! while it has none. A repo is open in at most one window (picking it elsewhere focuses
//! that window), so two views never drive the same agent terminal. The windows open at
//! quit come back on the next launch.

use crate::AppState;
use crate::store::{Repo, Store};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

const OPEN_SETTING: &str = "open_windows";

#[derive(Debug, Clone, Default, PartialEq)]
struct View {
    repo: Option<i64>,
    /// The work item shown (its terminal key).
    item: Option<String>,
}

#[derive(Default)]
pub struct Windows {
    /// In the order the windows were opened; "main" first.
    views: Mutex<Vec<(String, View)>>,
    next: AtomicU32,
    exiting: AtomicBool,
}

impl Windows {
    fn with<T>(&self, label: &str, f: impl FnOnce(&mut View) -> T) -> T {
        let mut views = self.views.lock().unwrap();
        if let Some((_, view)) = views.iter_mut().find(|(l, _)| l == label) {
            return f(view);
        }
        views.push((label.to_owned(), View::default()));
        f(&mut views.last_mut().unwrap().1)
    }

    pub fn repo_of(&self, label: &str) -> Option<i64> {
        self.with(label, |v| v.repo)
    }

    /// The window showing `repo`, if any.
    pub fn window_of(&self, repo: i64) -> Option<String> {
        self.views
            .lock()
            .unwrap()
            .iter()
            .find(|(_, v)| v.repo == Some(repo))
            .map(|(l, _)| l.clone())
    }

    fn bind(&self, label: &str, repo: i64) {
        self.with(label, |v| {
            if v.repo != Some(repo) {
                *v = View {
                    repo: Some(repo),
                    item: None,
                }
            }
        });
    }

    /// Repos shown in some window: the ones the poller keeps current.
    pub fn open_repos(&self) -> Vec<i64> {
        self.views
            .lock()
            .unwrap()
            .iter()
            .filter_map(|(_, v)| v.repo)
            .collect()
    }

    pub fn set_item(&self, label: &str, item: Option<String>) {
        self.with(label, |v| v.item = item);
    }

    /// Windows showing work item `key`.
    pub fn showing(&self, key: &str) -> Vec<String> {
        self.views
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, v)| v.item.as_deref() == Some(key))
            .map(|(l, _)| l.clone())
            .collect()
    }

    fn next_label(&self) -> String {
        format!("w{}", self.next.fetch_add(1, Ordering::Relaxed) + 1)
    }

    /// A window closed. The last one stays recorded (closing it quits the app, and it
    /// should come back); so do all of them when the app quits.
    fn closed(&self, label: &str) -> bool {
        let mut views = self.views.lock().unwrap();
        if self.exiting.load(Ordering::Relaxed) || views.len() <= 1 {
            return false;
        }
        views.retain(|(l, _)| l != label);
        true
    }

    pub fn exiting(&self) {
        self.exiting.store(true, Ordering::Relaxed);
    }

    fn persist(&self, store: &Store) {
        let repos = self.open_repos();
        if let Err(e) = store.set_setting(OPEN_SETTING, &serde_json::json!(repos).to_string()) {
            tracing::warn!("could not save the open windows: {e}");
        }
    }
}

fn title(repo: Option<&Repo>) -> String {
    match repo {
        Some(r) => format!("{}/{} — Ariekei", r.owner, r.name),
        None => "Ariekei".to_owned(),
    }
}

/// The repo window `label` shows.
pub fn repo_of(state: &AppState, label: &str) -> Result<Option<Repo>, String> {
    match state.windows.repo_of(label) {
        Some(id) => state.store.repo(id),
        None => Ok(None),
    }
}

/// Shows `repo` in window `label`, unless another window already shows it: then that one
/// is focused (and `label` closed if it was still empty). Returns whether `label` shows it.
pub fn show(app: &AppHandle, label: &str, repo: &Repo) -> Result<bool, String> {
    let state = app.state::<AppState>();
    if let Some(other) = state.windows.window_of(repo.id).filter(|o| o != label) {
        focus(app, &other);
        if state.windows.repo_of(label).is_none()
            && let Some(window) = app.get_webview_window(label)
        {
            let _ = window.close();
        }
        return Ok(false);
    }
    state.windows.bind(label, repo.id);
    state.windows.persist(&state.store);
    // The last repo used: the main window's fallback when nothing else is recorded.
    state
        .store
        .set_setting("active_repo", &repo.id.to_string())?;
    if let Some(window) = app.get_webview_window(label) {
        let _ = window.set_title(&title(Some(repo)));
    }
    state.poller.refresh();
    Ok(true)
}

fn focus(app: &AppHandle, label: &str) {
    if let Some(window) = app.get_webview_window(label) {
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// Opens a window for `repo` (or an empty one with the Connect screen), or focuses the
/// window that already shows it.
pub fn open(app: &AppHandle, repo: Option<&Repo>) -> Result<(), String> {
    let state = app.state::<AppState>();
    if let Some(other) = repo.and_then(|r| state.windows.window_of(r.id)) {
        focus(app, &other);
        return Ok(());
    }
    let label = state.windows.next_label();
    if let Some(r) = repo {
        state.windows.bind(&label, r.id);
    } else {
        state.windows.with(&label, |_| ());
    }
    WebviewWindowBuilder::new(app, &label, WebviewUrl::default())
        .title(title(repo))
        .inner_size(1400.0, 900.0)
        .build()
        .map_err(|e| e.to_string())?;
    state.windows.persist(&state.store);
    state.poller.refresh();
    Ok(())
}

/// At launch: the main window gets the first repo that was open, the others get windows
/// of their own.
pub fn restore(app: &AppHandle) {
    let state = app.state::<AppState>();
    let recorded: Vec<i64> = state
        .store
        .setting(OPEN_SETTING)
        .ok()
        .flatten()
        .and_then(|v| serde_json::from_str(&v).ok())
        .unwrap_or_default();
    let last_used = || {
        state
            .store
            .setting("active_repo")
            .ok()
            .flatten()
            .and_then(|v| v.parse().ok())
    };
    let ids = if recorded.is_empty() {
        last_used().into_iter().collect()
    } else {
        recorded
    };
    let repos: Vec<Repo> = ids
        .into_iter()
        .filter_map(|id| state.store.repo(id).ok().flatten())
        .collect();
    let mut repos = repos.iter();
    if let Some(first) = repos.next() {
        state.windows.bind("main", first.id);
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.set_title(&title(Some(first)));
        }
    }
    for repo in repos {
        if let Err(e) = open(app, Some(repo)) {
            tracing::warn!(
                "could not reopen the window of {}/{}: {e}",
                repo.owner,
                repo.name
            );
        }
    }
    state.poller.refresh();
}

/// A window was destroyed.
pub fn closed(app: &AppHandle, label: &str) {
    let state = app.state::<AppState>();
    if state.windows.closed(label) {
        state.windows.persist(&state.store);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_window_per_repo_and_closing_keeps_the_last() {
        let w = Windows::default();
        w.bind("main", 1);
        let second = w.next_label();
        w.bind(&second, 2);
        assert_eq!(w.window_of(2).as_deref(), Some(second.as_str()));
        assert_eq!(w.open_repos(), [1, 2]);

        w.set_item("main", Some("7".into()));
        assert_eq!(w.showing("7"), ["main"]);
        w.bind("main", 3);
        assert!(w.showing("7").is_empty(), "switching repo forgets the item");

        assert!(w.closed(&second));
        assert_eq!(w.open_repos(), [3]);
        assert!(!w.closed("main"), "the last window stays recorded");
        assert_eq!(w.open_repos(), [3]);

        let third = w.next_label();
        w.bind(&third, 4);
        w.exiting();
        assert!(!w.closed(&third), "quitting keeps every window");
        assert_eq!(w.open_repos(), [3, 4]);
    }
}
