//! The Inbox: one row per work item, ordered by what needs the user's attention.

use crate::hooks::AgentStatus;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    pub id: i64,
    pub number: i64,
    pub title: String,
    pub author: String,
    /// "author", "requested" or "other" (see `store`).
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
    pub worktree: Option<String>,
    /// The worktree has commits or uncommitted changes the user has not marked reviewed.
    pub unreviewed: bool,
    /// Why the row is where it is (for the UI), most urgent first.
    pub attention: Option<&'static str>,
}

/// Rank of the row's most urgent reason; lower is more urgent.
fn attention(row: &Row) -> (u8, Option<&'static str>) {
    use AgentStatus::*;
    if !row.open {
        return (7, None);
    }
    if row.agent == Some(WaitingForPermission) {
        return (0, Some("agent needs you"));
    }
    if matches!(row.ci.as_deref(), Some("FAILURE" | "ERROR")) {
        return (1, Some("CI failing"));
    }
    if row.new_activity {
        return (2, Some("new reviews or comments"));
    }
    if row.unreviewed && matches!(row.agent, Some(Finished | Ready | Error) | None) {
        return (3, Some("changes to review"));
    }
    if row.agent == Some(Working) {
        return (4, Some("agent working"));
    }
    (5, None)
}

/// Fills in `attention` and sorts: most urgent first, then most recently updated.
pub fn order(mut rows: Vec<Row>) -> Vec<Row> {
    for row in &mut rows {
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
            worktree: None,
            unreviewed: false,
            attention: None,
        }
    }

    #[test]
    fn orders_by_urgency_then_recency() {
        let quiet_old = row(1, "2026-01-01");
        let quiet_new = row(2, "2026-02-01");
        let working = Row {
            agent: Some(AgentStatus::Working),
            ..row(3, "2026-01-01")
        };
        let to_review = Row {
            agent: Some(AgentStatus::Finished),
            unreviewed: true,
            ..row(4, "2026-01-01")
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

        let ordered = order(vec![
            quiet_old, closed, quiet_new, working, to_review, activity, ci, needs_you,
        ]);
        let numbers: Vec<i64> = ordered.iter().map(|r| r.number).collect();
        assert_eq!(numbers, [7, 6, 5, 4, 3, 2, 1, 8]);
        assert_eq!(ordered[0].attention, Some("agent needs you"));
        assert_eq!(ordered[5].attention, None);
    }

    #[test]
    fn unreviewed_changes_wait_until_the_agent_is_done() {
        let busy = Row {
            agent: Some(AgentStatus::Working),
            unreviewed: true,
            ..row(1, "x")
        };
        assert_eq!(order(vec![busy])[0].attention, Some("agent working"));
    }
}
