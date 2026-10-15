//! What the backend tells the windows and the desktop: agent status, Inbox refreshes and
//! poll errors, and notices. The app sends them through Tauri (`TauriEvents`); tests record
//! them, so the logic behind them runs without a Tauri app.

use crate::hooks::AgentStatus;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;

pub trait Events: Send + Sync {
    /// An agent's status changed (the `agent-event` event).
    fn agent_status(&self, key: &str, status: AgentStatus, detail: Option<String>);
    /// The Inbox of `repo` has new data (`inbox-changed`).
    fn inbox_changed(&self, repo: i64);
    /// Polling `repo` failed (`inbox-error`).
    fn inbox_error(&self, repo: i64, message: &str);
    /// Tells the user something about item `key`: in the focused window, or as an OS
    /// notification. `kind`: needs-you, finished, error, ci-failing, activity or lifecycle.
    fn notice(&self, key: &str, kind: &str, title: &str, body: &str);
    /// Whether item `key` is on screen in a focused window (then nothing needs telling).
    fn watching(&self, key: &str) -> bool;
}

pub struct TauriEvents(pub AppHandle);

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum AgentEvent {
    Status {
        key: String,
        status: AgentStatus,
        detail: Option<String>,
    },
}

/// An in-app notice (the `notice` event).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Notice<'a> {
    item: Option<i64>,
    kind: &'a str,
    title: &'a str,
    body: &'a str,
}

/// A failed poll of one repo (`inbox-error`).
#[derive(Clone, Serialize)]
struct PollError<'a> {
    repo: i64,
    message: &'a str,
}

impl Events for TauriEvents {
    fn agent_status(&self, key: &str, status: AgentStatus, detail: Option<String>) {
        let event = AgentEvent::Status {
            key: key.to_owned(),
            status,
            detail,
        };
        if let Err(e) = self.0.emit("agent-event", &event) {
            tracing::warn!("failed to emit agent event: {e}");
        }
    }

    // `inbox-changed` and `inbox-error` carry the repo id: each window shows one.
    fn inbox_changed(&self, repo: i64) {
        let _ = self.0.emit("inbox-changed", repo);
    }

    fn inbox_error(&self, repo: i64, message: &str) {
        let _ = self.0.emit("inbox-error", PollError { repo, message });
    }

    // While an Ariekei window is focused the notice appears in it (an OS notification is
    // easy to miss with the app in front, and some desktops don't show one then).
    fn notice(&self, key: &str, kind: &str, title: &str, body: &str) {
        let app = &self.0;
        let focused = app
            .webview_windows()
            .into_iter()
            .find(|(_, w)| w.is_focused().unwrap_or(false));
        if let Some((label, _)) = focused {
            let notice = Notice {
                item: key.parse().ok(),
                kind,
                title,
                body,
            };
            if let Err(e) = app.emit_to(label.as_str(), "notice", notice) {
                tracing::warn!("notice failed: {e}");
            }
        } else if let Err(e) = app.notification().builder().title(title).body(body).show() {
            tracing::warn!("notification failed: {e}");
        }
    }

    fn watching(&self, key: &str) -> bool {
        let app = &self.0;
        app.state::<crate::AppState>()
            .windows
            .showing(key)
            .iter()
            .filter_map(|label| app.get_webview_window(label))
            .any(|w| w.is_focused().unwrap_or(false))
    }
}
