//! OS notifications for things that need the user: an agent waiting for permission or
//! finishing a turn, CI starting to fail, and new reviews or comments. Nothing is shown
//! for the item the user is looking at in a focused window.

use crate::github::PrSummary;
use crate::hooks::AgentStatus;
use tauri::{AppHandle, Manager};
use tauri_plugin_notification::NotificationExt;

fn watching(app: &AppHandle, key: &str) -> bool {
    let focused = app
        .get_webview_window("main")
        .and_then(|w| w.is_focused().ok())
        .unwrap_or(false);
    let active = app
        .state::<crate::AppState>()
        .active_item
        .lock()
        .unwrap()
        .clone();
    focused && active.as_deref() == Some(key)
}

fn show(app: &AppHandle, title: &str, body: &str) {
    if let Err(e) = app.notification().builder().title(title).body(body).show() {
        tracing::warn!("notification failed: {e}");
    }
}

fn label(app: &AppHandle, key: &str) -> String {
    let item = key
        .parse()
        .ok()
        .and_then(|id| app.state::<crate::AppState>().store.item(id).ok());
    match item {
        Some(item) => format!(
            "#{} {}",
            item.number,
            item.snapshot["title"].as_str().unwrap_or_default()
        ),
        None => key.to_owned(),
    }
}

/// What, if anything, to say about an agent status change.
fn agent_message(previous: AgentStatus, status: AgentStatus) -> Option<&'static str> {
    match (previous, status) {
        (_, AgentStatus::WaitingForPermission) => Some("The agent is waiting for your permission."),
        (AgentStatus::Working, AgentStatus::Finished) => Some("The agent finished."),
        (_, AgentStatus::Error) => Some("The agent stopped with an error."),
        _ => None,
    }
}

pub fn agent_status(app: &AppHandle, key: &str, previous: AgentStatus, status: AgentStatus) {
    if let Some(message) = agent_message(previous, status)
        && !watching(app, key)
    {
        show(app, &label(app, key), message);
    }
}

/// A work item started or ended its life (draft PR opened, cleaned up, cleanup blocked).
pub fn lifecycle(app: &AppHandle, key: &str, title: &str, message: &str) {
    if !watching(app, key) {
        show(app, title, message);
    }
}

/// What changed on GitHub between two snapshots of the same PR that the user should hear about.
fn pr_messages(before: &PrSummary, after: &PrSummary) -> Vec<String> {
    let mut messages = vec![];
    let failing = |p: &PrSummary| matches!(p.ci.as_deref(), Some("FAILURE" | "ERROR"));
    if failing(after) && !failing(before) {
        messages.push(format!(
            "CI is failing: {}",
            after.failing_checks.join(", ")
        ));
    }
    if after.latest_activity > before.latest_activity {
        messages.push("New reviews or comments.".to_owned());
    }
    messages
}

pub fn pr_changed(app: &AppHandle, key: &str, before: &PrSummary, after: &PrSummary) {
    if watching(app, key) {
        return;
    }
    for message in pr_messages(before, after) {
        show(app, &format!("#{} {}", after.number, after.title), &message);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_notifications() {
        use AgentStatus::*;
        assert!(agent_message(Working, WaitingForPermission).is_some());
        assert!(agent_message(Working, Finished).is_some());
        assert!(
            agent_message(Ready, Finished).is_none(),
            "an idle prompt at startup is not news"
        );
        assert!(agent_message(Starting, Ready).is_none());
    }

    #[test]
    fn pr_notifications() {
        let base: PrSummary = serde_json::from_value(serde_json::json!({
            "number": 1, "title": "t", "url": "", "isDraft": false, "updatedAt": "", "headRef": "", "headSha": "",
            "headOwner": "", "headRepo": "", "headSshUrl": "", "headHttpsUrl": "", "baseRef": "",
            "reviewDecision": null, "ci": "SUCCESS", "failingChecks": [], "unresolvedThreads": 0,
            "latestActivity": "2026-10-01T00:00:00Z", "reviews": []
        }))
        .unwrap();
        assert!(pr_messages(&base, &base).is_empty());
        let failing = PrSummary {
            ci: Some("FAILURE".into()),
            failing_checks: vec!["test".into()],
            ..base.clone()
        };
        assert_eq!(pr_messages(&base, &failing), ["CI is failing: test"]);
        assert!(
            pr_messages(&failing, &failing).is_empty(),
            "only the transition is news"
        );
        let commented = PrSummary {
            latest_activity: Some("2026-10-02T00:00:00Z".into()),
            ..base.clone()
        };
        assert_eq!(pr_messages(&base, &commented), ["New reviews or comments."]);
    }
}
