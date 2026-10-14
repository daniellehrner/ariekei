//! Notifications for things that need the user: an agent waiting for permission or
//! finishing a turn, CI starting to fail, and new reviews or comments. Nothing is shown
//! for the item the user is looking at in a focused window. While an Ariekei window is
//! focused the notice appears in it (an OS notification is easy to miss with the app in
//! front, and some desktops don't show one then); otherwise it is an OS notification.

use crate::github::PrSummary;
use crate::hooks::AgentStatus;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;

fn watching(app: &AppHandle, key: &str) -> bool {
    app.state::<crate::AppState>()
        .windows
        .showing(key)
        .iter()
        .filter_map(|label| app.get_webview_window(label))
        .any(|w| w.is_focused().unwrap_or(false))
}

/// An in-app notice (the `notice` event). `kind`: needs-you, finished, error, ci-failing,
/// activity or lifecycle.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Notice<'a> {
    item: Option<i64>,
    kind: &'a str,
    title: &'a str,
    body: &'a str,
}

fn tell(app: &AppHandle, key: &str, kind: &str, title: &str, body: &str) {
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

/// What, if anything, to say about an agent status change (`detail`: what it asks to run),
/// with the notice's kind.
fn agent_message(
    previous: AgentStatus,
    status: AgentStatus,
    detail: Option<&str>,
) -> Option<(&'static str, String)> {
    match (previous, status) {
        (_, AgentStatus::WaitingForPermission) => Some((
            "needs-you",
            match detail {
                Some(d) => format!("The agent is waiting for your permission: {d}"),
                None => "The agent is waiting for your permission.".to_owned(),
            },
        )),
        (AgentStatus::Working, AgentStatus::Finished) => {
            Some(("finished", "The agent finished.".to_owned()))
        }
        (_, AgentStatus::Error) => Some(("error", "The agent stopped with an error.".to_owned())),
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
    if let Some((kind, message)) = agent_message(previous, status, detail)
        && !watching(app, key)
    {
        tell(app, key, kind, &label(app, key), &message);
    }
}

/// A work item started or ended its life (draft PR opened, cleaned up, cleanup blocked).
pub fn lifecycle(app: &AppHandle, key: &str, title: &str, message: &str) {
    if !watching(app, key) {
        tell(app, key, "lifecycle", title, message);
    }
}

/// What changed on GitHub between two snapshots of the same PR that the user should hear about.
fn pr_messages(before: &PrSummary, after: &PrSummary) -> Vec<(&'static str, String)> {
    let mut messages = vec![];
    let failing = |p: &PrSummary| matches!(p.ci.as_deref(), Some("FAILURE" | "ERROR"));
    if failing(after) && !failing(before) {
        messages.push((
            "ci-failing",
            format!("CI is failing: {}", after.failing_checks.join(", ")),
        ));
    }
    if after.latest_activity > before.latest_activity {
        messages.push(("activity", "New reviews or comments.".to_owned()));
    }
    messages
}

pub fn pr_changed(app: &AppHandle, key: &str, before: &PrSummary, after: &PrSummary) {
    if watching(app, key) {
        return;
    }
    for (kind, message) in pr_messages(before, after) {
        tell(
            app,
            key,
            kind,
            &format!("#{} {}", after.number, after.title),
            &message,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_notifications() {
        use AgentStatus::*;
        assert_eq!(
            agent_message(Working, WaitingForPermission, Some("Bash ./gradlew test")),
            Some((
                "needs-you",
                "The agent is waiting for your permission: Bash ./gradlew test".to_owned()
            ))
        );
        assert_eq!(
            agent_message(Working, Finished, None).unwrap().0,
            "finished"
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
        assert_eq!(
            pr_messages(&base, &failing),
            [("ci-failing", "CI is failing: test".to_owned())]
        );
        assert!(
            pr_messages(&failing, &failing).is_empty(),
            "only the transition is news"
        );
        let commented = PrSummary {
            latest_activity: Some("2026-10-02T00:00:00Z".into()),
            ..base.clone()
        };
        assert_eq!(
            pr_messages(&base, &commented),
            [("activity", "New reviews or comments.".to_owned())]
        );
    }
}
