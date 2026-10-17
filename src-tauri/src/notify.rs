// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

//! Notifications for things that need the user: an agent waiting for permission or
//! finishing a turn, CI starting to fail, and new reviews or comments. Nothing is shown
//! for the item the user is looking at in a focused window. While an Ariekei window is
//! focused the notice appears in it (an OS notification is easy to miss with the app in
//! front, and some desktops don't show one then); otherwise it is an OS notification.

use crate::AppState;
use crate::github::PrSummary;
use crate::hooks::AgentStatus;

fn label(state: &AppState, key: &str) -> String {
    match key.parse().ok().and_then(|id| state.store.item(id).ok()) {
        Some(item) => item.label(),
        None => key.to_owned(),
    }
}

/// What, if anything, to say about an agent status change (`detail`: what it asks to run):
/// the notice's kind and its detail. The in-app notice shows the kind as its reason ("Agent
/// needs you") and the detail after it; an OS notification says it in a sentence (`sentence`).
fn agent_message(
    previous: AgentStatus,
    status: AgentStatus,
    detail: Option<&str>,
) -> Option<(&'static str, String)> {
    match (previous, status) {
        (_, AgentStatus::WaitingForPermission) => {
            Some(("needs-you", detail.unwrap_or_default().to_owned()))
        }
        (AgentStatus::Working, AgentStatus::Finished) => {
            Some(("finished", "the agent finished".to_owned()))
        }
        (_, AgentStatus::Error) => Some(("error", String::new())),
        _ => None,
    }
}

/// A notice as one sentence, for an OS notification: its kind's reason with the detail.
pub fn sentence(kind: &str, detail: &str) -> String {
    let with = |what: &str| {
        if detail.is_empty() {
            format!("{what}.")
        } else {
            format!("{what}: {detail}")
        }
    };
    match kind {
        "needs-you" => with("The agent is waiting for your permission"),
        "finished" => "The agent finished: ready for review.".to_owned(),
        "error" => "The agent stopped with an error.".to_owned(),
        "ci-failing" => with("CI is failing"),
        "activity" => "New reviews or comments.".to_owned(),
        _ => detail.to_owned(),
    }
}

pub fn agent_status(
    state: &AppState,
    key: &str,
    previous: AgentStatus,
    status: AgentStatus,
    detail: Option<&str>,
) {
    if let Some((kind, message)) = agent_message(previous, status, detail)
        && !state.events.watching(key)
    {
        state.events.notice(key, kind, &label(state, key), &message);
    }
}

/// A work item started or ended its life (an issue's PR found, cleaned up, cleanup blocked).
pub fn lifecycle(state: &AppState, key: &str, title: &str, message: &str) {
    if !state.events.watching(key) {
        state.events.notice(key, "lifecycle", title, message);
    }
}

/// What changed on GitHub between two snapshots of the same PR that the user should hear about.
fn pr_messages(before: &PrSummary, after: &PrSummary) -> Vec<(&'static str, String)> {
    let mut messages = vec![];
    let failing = |p: &PrSummary| matches!(p.ci.as_deref(), Some("FAILURE" | "ERROR"));
    if failing(after) && !failing(before) {
        messages.push(("ci-failing", after.failing_checks.join(", ")));
    }
    if after.latest_activity > before.latest_activity {
        messages.push(("activity", String::new()));
    }
    messages
}

pub fn pr_changed(state: &AppState, key: &str, before: &PrSummary, after: &PrSummary) {
    if state.events.watching(key) {
        return;
    }
    for (kind, message) in pr_messages(before, after) {
        state.events.notice(
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
            Some(("needs-you", "Bash ./gradlew test".to_owned()))
        );
        assert_eq!(
            sentence("needs-you", "Bash ./gradlew test"),
            "The agent is waiting for your permission: Bash ./gradlew test"
        );
        assert_eq!(
            sentence("needs-you", ""),
            "The agent is waiting for your permission."
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
            [("ci-failing", "test".to_owned())]
        );
        assert_eq!(sentence("ci-failing", "test"), "CI is failing: test");
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
            [("activity", String::new())]
        );
        assert_eq!(sentence("activity", ""), "New reviews or comments.");
        assert_eq!(sentence("lifecycle", "Cleaned up."), "Cleaned up.");
    }
}
