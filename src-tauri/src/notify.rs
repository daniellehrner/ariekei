//! OS notifications for things that need the user: an agent waiting for permission or
//! finishing a turn, CI starting to fail, and new reviews or comments. Nothing is shown
//! for the item the user is looking at in a focused window.

use crate::github::PrSummary;
use crate::hooks::AgentStatus;
use tauri::{AppHandle, Manager};
use tauri_plugin_notification::NotificationExt;

fn watching(app: &AppHandle, key: &str) -> bool {
    app.state::<crate::AppState>()
        .windows
        .showing(key)
        .iter()
        .filter_map(|label| app.get_webview_window(label))
        .any(|w| w.is_focused().unwrap_or(false))
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

/// What, if anything, to say about an agent status change (`detail`: what it asks to run).
fn agent_message(
    previous: AgentStatus,
    status: AgentStatus,
    detail: Option<&str>,
) -> Option<String> {
    match (previous, status) {
        (_, AgentStatus::WaitingForPermission) => Some(match detail {
            Some(d) => format!("The agent is waiting for your permission: {d}"),
            None => "The agent is waiting for your permission.".to_owned(),
        }),
        (AgentStatus::Working, AgentStatus::Finished) => Some("The agent finished.".to_owned()),
        (_, AgentStatus::Error) => Some("The agent stopped with an error.".to_owned()),
        _ => None,
    }
}

pub fn agent_status(
    app: &AppHandle,
    key: &str,
    previous: AgentStatus,
    status: AgentStatus,
    detail: Option<&str>,
) {
    if let Some(message) = agent_message(previous, status, detail)
        && !watching(app, key)
    {
        show(app, &label(app, key), &message);
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
        assert_eq!(
            agent_message(Working, WaitingForPermission, Some("Bash ./gradlew test")).as_deref(),
            Some("The agent is waiting for your permission: Bash ./gradlew test")
        );
        assert!(agent_message(Working, WaitingForPermission, None).is_some());
        assert!(agent_message(Working, Finished, None).is_some());
        assert!(
            agent_message(Ready, Finished, None).is_none(),
            "an idle prompt at startup is not news"
        );
        assert!(agent_message(Starting, Ready, None).is_none());
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
