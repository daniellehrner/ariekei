// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

//! The Inbox: one row per work item, ordered by what needs the user's attention.

use crate::hooks::AgentStatus;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    pub id: i64,
    pub number: i64,
    /// "pr", "issue" or "task" (a task's number is negative: task `T1` is -1).
    pub kind: String,
    /// For a PR that started as an issue: the issue's number.
    pub issue_number: Option<i64>,
    pub title: String,
    pub author: String,
    /// "assignee", "author", "requested", "reviewed" or "other" (see `poller::role_of`).
    pub role: String,
    pub url: String,
    pub is_draft: bool,
    pub open: bool,
    pub updated_at: String,
    pub ci: Option<String>,
    pub failing_checks: Vec<String>,
    pub review_decision: Option<String>,
    pub unresolved_threads: i64,
    /// Reviews or comments by others since the user last looked at the item.
    pub new_activity: bool,
    pub agent: Option<AgentStatus>,
    /// The agent finished since the user last opened the item's review.
    pub ready_for_review: bool,
    pub worktree: Option<String>,
    /// The item's local branch, once it has a worktree.
    pub branch: Option<String>,
    /// The branch its worktree is on instead (`""`: a detached HEAD), when someone switched
    /// it; the agent gets nothing until it is back (`workitem::moved_to`).
    pub moved_to: Option<String>,
    /// Its worktree was there before (e.g. the user's own checkout): cleanup keeps it.
    pub adopted: bool,
    /// Why the row is where it is (for the UI), most urgent first.
    pub attention: Option<&'static str>,
    /// Why the finished item was not cleaned up, if it was not.
    pub cleanup_blockers: Option<Vec<String>>,
    /// For a PR or issue a task's agent opened: the task.
    pub task_id: Option<i64>,
    /// Whose agent works on this item: its own (it has a worktree), or its task's.
    pub agent_item: Option<i64>,
    /// A task's notes (what to look into).
    pub notes: Option<String>,
    /// A task whose PRs and issues are all closed or merged (it has some).
    pub outcomes_closed: bool,
    /// The user starred it (it heads the Inbox).
    pub starred: bool,
    /// The user added it and can take it out again (see `workitem::removable`).
    pub removable: bool,
}

/// Rank of the row's most urgent reason; lower is more urgent.
fn attention(row: &Row) -> (u8, Option<&'static str>) {
    use AgentStatus::*;
    // Also on a finished item: its cleanup waits for the agent's turn.
    if row.agent == Some(WaitingForPermission) {
        return (0, Some("agent needs you"));
    }
    if !row.open && row.cleanup_blockers.is_none() {
        return (7, None);
    }
    // Only a finished item waits for its cleanup; an open one whose Done was refused shows
    // the reasons on the item, and its other states still count.
    if !row.open && row.cleanup_blockers.is_some() {
        return (1, Some("finished, cleanup blocked"));
    }
    if row.outcomes_closed {
        return (2, Some("all its PRs and issues are closed"));
    }
    if matches!(row.ci.as_deref(), Some("FAILURE" | "ERROR")) {
        return (1, Some("CI failing"));
    }
    if row.new_activity {
        return (2, Some("new reviews or comments"));
    }
    if row.ready_for_review && row.agent == Some(Finished) {
        return (3, Some("ready for review"));
    }
    if row.agent == Some(Working) {
        return (4, Some("agent working"));
    }
    (5, None)
}

/// Fills in `attention` and sorts: most urgent first, then most recently updated.
pub fn order(mut rows: Vec<Row>) -> Vec<Row> {
    let closed: Vec<i64> = rows
        .iter()
        .filter(|t| t.kind == "task")
        .filter(|t| {
            let mut links = rows.iter().filter(|r| r.task_id == Some(t.id)).peekable();
            links.peek().is_some() && links.all(|r| !r.open)
        })
        .map(|t| t.id)
        .collect();
    for row in &mut rows {
        row.outcomes_closed = closed.contains(&row.id);
        row.attention = attention(row).1;
    }
    rows.sort_by(|a, b| {
        attention(a)
            .0
            .cmp(&attention(b).0)
            .then_with(|| b.updated_at.cmp(&a.updated_at))
    });
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(number: i64, updated: &str) -> Row {
        Row {
            id: number,
            number,
            kind: "pr".into(),
            issue_number: None,
            title: String::new(),
            author: String::new(),
            role: "author".into(),
            url: String::new(),
            is_draft: false,
            open: true,
            updated_at: updated.into(),
            ci: Some("SUCCESS".into()),
            failing_checks: vec![],
            review_decision: None,
            unresolved_threads: 0,
            new_activity: false,
            agent: None,
            ready_for_review: false,
            worktree: None,
            branch: None,
            moved_to: None,
            adopted: false,
            attention: None,
            cleanup_blockers: None,
            task_id: None,
            agent_item: None,
            notes: None,
            outcomes_closed: false,
            starred: false,
            removable: false,
        }
    }

    #[test]
    fn only_a_finished_item_waits_for_its_cleanup() {
        let blockers = Some(vec!["1 uncommitted change: a".to_owned()]);
        let finished = Row {
            open: false,
            cleanup_blockers: blockers.clone(),
            ..row(1, "2026-01-01")
        };
        // Done was refused while it was open, and it has new comments since.
        let open = Row {
            cleanup_blockers: blockers,
            new_activity: true,
            ..row(2, "2026-01-01")
        };
        let rows = order(vec![open, finished]);
        assert_eq!(
            rows.iter().map(|r| (r.id, r.attention)).collect::<Vec<_>>(),
            [
                (1, Some("finished, cleanup blocked")),
                (2, Some("new reviews or comments"))
            ]
        );
    }

    #[test]
    fn a_task_whose_prs_and_issues_are_all_closed_says_so() {
        let task = Row {
            kind: "task".into(),
            ..row(-1, "2026-01-01")
        };
        let merged = Row {
            open: false,
            task_id: Some(-1),
            ..row(10, "2026-01-01")
        };
        let open = Row {
            task_id: Some(-1),
            ..row(11, "2026-01-01")
        };
        let without_links = Row {
            kind: "task".into(),
            ..row(-2, "2026-01-01")
        };
        let rows = order(vec![
            task.clone(),
            merged.clone(),
            open,
            without_links.clone(),
        ]);
        assert!(!rows.iter().any(|r| r.outcomes_closed));
        let rows = order(vec![task, merged, without_links]);
        let attention = |id| rows.iter().find(|r| r.id == id).unwrap().attention;
        assert_eq!(attention(-1), Some("all its PRs and issues are closed"));
        assert_eq!(attention(-2), None);
    }

    #[test]
    fn orders_by_urgency_then_recency() {
        let quiet_old = row(1, "2026-01-01");
        let quiet_new = row(2, "2026-02-01");
        let working = Row {
            agent: Some(AgentStatus::Working),
            ..row(3, "2026-01-01")
        };
        let activity = Row {
            new_activity: true,
            ..row(5, "2026-01-01")
        };
        let ci = Row {
            ci: Some("FAILURE".into()),
            ..row(6, "2026-01-01")
        };
        let needs_you = Row {
            agent: Some(AgentStatus::WaitingForPermission),
            ci: Some("FAILURE".into()),
            ..row(7, "2026-01-01")
        };
        let closed = Row {
            open: false,
            new_activity: true,
            ..row(8, "2026-03-01")
        };
        let finished = Row {
            agent: Some(AgentStatus::Finished),
            ready_for_review: true,
            ..row(9, "2026-01-01")
        };
        // Reviewed since it finished: nothing to say.
        let seen = Row {
            agent: Some(AgentStatus::Finished),
            ..row(10, "2026-01-02")
        };

        let ordered = order(vec![
            quiet_old, closed, quiet_new, working, activity, ci, needs_you, finished, seen,
        ]);
        let numbers: Vec<i64> = ordered.iter().map(|r| r.number).collect();
        assert_eq!(numbers, [7, 6, 5, 9, 3, 2, 10, 1, 8]);
        assert_eq!(ordered[0].attention, Some("agent needs you"));
        assert_eq!(ordered[3].attention, Some("ready for review"));
        assert_eq!(ordered[5].attention, None);
    }
}
