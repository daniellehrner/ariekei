// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

//! The PR context given to the agent via `--append-system-prompt-file`. Claude Code
//! records the system prompt on a conversation's first request and reuses it on resume,
//! so this describes the PR as it was when the conversation started.

use crate::github::PrDetail;
use crate::worktree::BranchRef;
use std::fmt::Write as _;

const MAX_BODY: usize = 6_000;

fn truncate(s: &str, max: usize) -> &str {
    match s.char_indices().nth(max) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}

/// Markdown for the agent. `logs` holds `(check name, log tail)` for failing checks.
pub fn render(number: i64, pr: &PrDetail, logs: &[(String, String)]) -> String {
    let mut md = String::new();
    let _ = writeln!(md, "# Work item: pull request #{number}: {}", pr.title);
    let _ = writeln!(md);
    let _ = writeln!(
        md,
        "You are working in a git worktree of this pull request ({} → {}), opened from the \
         Ariekei review workbench. The user reviews your changes there. This context was captured when the \
         conversation started; newer activity may arrive as messages. Wait for the user's \
         instructions before changing anything.",
        pr.head_ref, pr.base_ref
    );
    let _ = writeln!(md, "\nURL: {}", pr.url);

    let body = pr.body.trim();
    if !body.is_empty() {
        let _ = writeln!(md, "\n## Description\n\n{}", truncate(body, MAX_BODY));
        if body.chars().count() > MAX_BODY {
            let _ = writeln!(md, "\n(description truncated)");
        }
    }

    if !pr.unresolved.is_empty() {
        let _ = writeln!(
            md,
            "\n## Unresolved review threads ({})",
            pr.unresolved.len()
        );
        for t in &pr.unresolved {
            let location = match t.line {
                Some(l) => format!("{}:{l}", t.path),
                None => t.path.clone(),
            };
            let outdated = if t.outdated {
                " (outdated: the code changed since)"
            } else {
                ""
            };
            let _ = writeln!(md, "\n### {location}{outdated}");
            for c in &t.comments {
                let _ = writeln!(md, "\n**{}:** {}", c.author, c.body.trim());
            }
        }
    }

    if !pr.reviews.is_empty() {
        let _ = writeln!(md, "\n## Review summaries");
        for (author, state, body) in &pr.reviews {
            let _ = writeln!(
                md,
                "\n**{author}** ({}): {}",
                state.to_lowercase().replace('_', " "),
                body.trim()
            );
        }
    }

    if !pr.failing.is_empty() {
        let _ = writeln!(md, "\n## Failing checks");
        for check in &pr.failing {
            let summary = if check.summary.is_empty() {
                String::new()
            } else {
                format!(": {}", check.summary)
            };
            let _ = writeln!(md, "\n### {}{summary}\n\n{}", check.name, check.url);
            if let Some((_, log)) = logs.iter().find(|(name, _)| name == &check.name) {
                let _ = writeln!(md, "\nEnd of the job log:\n\n```\n{}\n```", log.trim_end());
            }
        }
    }
    md
}

/// Markdown for the agent working on an issue (`comments` are `(author, body)`).
pub fn render_issue(
    number: i64,
    title: &str,
    url: &str,
    body: &str,
    comments: &[(String, String)],
    branch: &str,
    base: &str,
) -> String {
    let mut md = String::new();
    let _ = writeln!(md, "# Work item: issue #{number}: {title}");
    let _ = writeln!(md);
    let _ = writeln!(
        md,
        "You are working on this issue in a git worktree opened from the Ariekei review workbench, on the new \
         branch `{branch}` (from `{base}`). The user reviews your changes there. `git push` is set \
         up for the branch. Ariekei does not open a pull request: when you are asked to, open it \
         yourself, and Ariekei follows it in place of the issue. This context was captured when the conversation started. Wait for the \
         user's instructions before changing anything."
    );
    let _ = writeln!(md, "\nURL: {url}");
    let body = body.trim();
    if !body.is_empty() {
        let _ = writeln!(md, "\n## Description\n\n{}", truncate(body, MAX_BODY));
    }
    if !comments.is_empty() {
        let _ = writeln!(md, "\n## Comments");
        for (author, text) in comments {
            let _ = writeln!(md, "\n**{author}:** {}", truncate(text.trim(), MAX_BODY));
        }
    }
    md
}

/// Markdown for the agent of a task (no PR or issue yet). `notes`: what the user wants
/// looked into; `pushed`: the existing branch `head` is on GitHub (else only in the clone).
pub fn render_task(
    name: &str,
    title: &str,
    notes: &str,
    branch: &str,
    base: &str,
    head: Option<&BranchRef>,
    pushed: bool,
) -> String {
    let mut md = String::new();
    let _ = writeln!(md, "# Work item: task {name}: {title}");
    let _ = writeln!(md);
    let on = match head {
        Some(h) if !pushed => format!(
            "on `{branch}`, an existing branch that is only local: it is not on GitHub yet. \
             Push it only when the user asks (a plain `git push` creates it as `{}` of \
             `{}/{}`); the review compares it with `{base}`",
            h.branch, h.owner, h.repo
        ),
        Some(h) => format!(
            "on `{branch}`, the existing branch `{}` of `{}/{}` (a plain `git push` updates \
             it; the review compares it with `{base}`)",
            h.branch, h.owner, h.repo
        ),
        None => format!("on the new branch `{branch}` (from `{base}`)"),
    };
    let _ = writeln!(
        md,
        "You are working on this task in a git worktree opened from the Ariekei review \
         workbench, {on}. The user reviews your changes there. The task has no pull request \
         or issue yet: it may end as one or more pull requests, issues, or nothing. Open pull \
         requests with `gh pr create` and issues with `gh issue create` (only when the user \
         asks); Ariekei links what they print to this task, and a pull request opened for \
         the branch by anyone. Wait for the user's instructions before changing anything."
    );
    let notes = notes.trim();
    if !notes.is_empty() {
        let _ = writeln!(
            md,
            "\n## What to look into\n\n{}",
            truncate(notes, MAX_BODY)
        );
    }
    md
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::{FailingCheck, Thread, ThreadComment};

    #[test]
    fn renders_threads_reviews_and_failing_checks() {
        let pr = PrDetail {
            title: "Notifier".into(),
            body: "Adds a notifier.".into(),
            url: "https://github.com/o/r/pull/7".into(),
            head_ref: "feat".into(),
            base_ref: "main".into(),
            unresolved: vec![Thread {
                path: "src/A.java".into(),
                line: Some(12),
                outdated: true,
                comments: vec![ThreadComment {
                    author: "alice".into(),
                    body: "Null check?".into(),
                }],
            }],
            reviews: vec![(
                "bob".into(),
                "CHANGES_REQUESTED".into(),
                "Split this up".into(),
            )],
            failing: vec![FailingCheck {
                name: "unit-tests".into(),
                url: "U".into(),
                job_id: Some(1),
                summary: "2 failed".into(),
            }],
        };
        let md = render(
            7,
            &pr,
            &[("unit-tests".into(), "FooTest > bar FAILED\n".into())],
        );
        assert!(md.starts_with("# Work item: pull request #7: Notifier"));
        assert!(md.contains("Wait for the user's instructions"));
        assert!(md.contains(
            "### src/A.java:12 (outdated: the code changed since)\n\n**alice:** Null check?"
        ));
        assert!(md.contains("**bob** (changes requested): Split this up"));
        assert!(md.contains(
            "### unit-tests: 2 failed\n\nU\n\nEnd of the job log:\n\n```\nFooTest > bar FAILED\n```"
        ));
    }

    #[test]
    fn renders_an_issue_with_its_branch_and_comments() {
        let md = render_issue(
            7,
            "Crash",
            "U",
            "It crashes.",
            &[("bob".into(), "Me too".into())],
            "issue-7-crash",
            "main",
        );
        assert!(md.starts_with("# Work item: issue #7: Crash"));
        assert!(md.contains("branch `issue-7-crash` (from `main`)"));
        assert!(md.contains("## Description\n\nIt crashes."));
        assert!(md.contains("**bob:** Me too"));
    }

    #[test]
    fn renders_a_task_with_its_notes() {
        let md = render_task(
            "T2",
            "Flaky sync",
            "Peers drop.",
            "task-2-flaky-sync",
            "main",
            None,
            false,
        );
        assert!(md.starts_with("# Work item: task T2: Flaky sync"));
        assert!(md.contains("branch `task-2-flaky-sync` (from `main`)"));
        assert!(md.contains("`gh pr create`"));
        assert!(md.contains("## What to look into\n\nPeers drop."));
        assert!(!render_task("T2", "x", " ", "b", "main", None, false).contains("## What"));
    }

    #[test]
    fn a_task_on_an_existing_branch_names_it() {
        let head = BranchRef {
            owner: "friend".into(),
            repo: "besu".into(),
            branch: "feat/evm".into(),
        };
        let md = render_task("T3", "feat/evm", "", "feat/evm", "main", Some(&head), true);
        assert!(
            md.contains(
                "the existing branch `feat/evm` of `friend/besu` (a plain `git push` updates it"
            ),
            "{md}"
        );
        let md = render_task("T3", "feat/evm", "", "feat/evm", "main", Some(&head), false);
        assert!(
            md.contains("it is not on GitHub yet. Push it only when the user asks (a plain `git push` creates it as `feat/evm` of `friend/besu`)"),
            "{md}"
        );
    }

    #[test]
    fn omits_empty_sections_and_truncates_long_descriptions() {
        let pr = PrDetail {
            title: "T".into(),
            body: "x".repeat(MAX_BODY + 10),
            ..Default::default()
        };
        let md = render(1, &pr, &[]);
        assert!(!md.contains("## Unresolved") && !md.contains("## Failing"));
        assert!(md.contains("(description truncated)"));
    }
}
