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

/// Added to the context of a PR someone else wrote (see `drafts`).
pub const REVIEWING: &str = "\n## Reviewing\n\nThe user is reviewing this pull request, which \
someone else wrote. When you draft review comments for them, add each one with Ariekei's \
add_review_comment tool instead of only listing it in your reply: they edit and submit the \
drafts from Ariekei's Review tab.\n";

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
         asks); Ariekei adds what they print to its Inbox as items of their own, and a pull \
         request opened for the branch by anyone. Wait for the user's instructions before \
         changing anything."
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

/// Longest message of a task's conversation in `render_origin`; the whole is at most
/// `MAX_ORIGIN`, the oldest messages (but the first) left out.
const MAX_MESSAGE: usize = 3_000;
const MAX_ORIGIN: usize = 40_000;

/// What the user said and the agent answered in Claude Code transcript `path` (JSON lines),
/// oldest first: `(true, text)` for the user. Tool calls and their results, thinking,
/// subagents and Claude Code's own notes (`<task-notification>`, commands) are left out.
pub fn conversation(path: &std::path::Path) -> Vec<(bool, String)> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return vec![];
    };
    let said = |text: &str| {
        let text = text.trim();
        (!text.is_empty()
            && !text.starts_with('<')
            && !text.starts_with("Another Claude session sent a message"))
        .then(|| text.to_owned())
    };
    let mut out = vec![];
    for line in text.lines() {
        let Ok(entry) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let user = match entry["type"].as_str() {
            Some("user") => true,
            Some("assistant") => false,
            _ => continue,
        };
        if entry["isMeta"] == true || entry["isSidechain"] == true {
            continue;
        }
        let content = &entry["message"]["content"];
        let texts: Vec<String> = match content.as_str() {
            Some(text) => said(text).into_iter().collect(),
            None => content
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or_default()
                .iter()
                .filter(|b| b["type"] == "text")
                .filter_map(|b| said(b["text"].as_str().unwrap_or_default()))
                .collect(),
        };
        out.extend(texts.into_iter().map(|t| (user, t)));
    }
    out
}

/// How a PR or issue (`kind`: "pull request" or "issue") that the agent of task `name` opened
/// came about, for the agent that later works on it: the task and its `conversation` up to
/// then.
pub fn render_origin(
    kind: &str,
    name: &str,
    title: &str,
    notes: &str,
    branch: &str,
    conversation: &[(bool, String)],
) -> String {
    let mut md = String::new();
    let _ = writeln!(
        md,
        "## How this came about\n\nThe agent of task {name} (\"{title}\", on `{branch}`) opened \
         this {kind} in Ariekei. You start a new conversation: below is what the user asked \
         that agent and what it answered up to then (its tool calls left out)."
    );
    let notes = notes.trim();
    if !notes.is_empty() {
        let _ = writeln!(md, "\nThe task's notes:\n\n{}", truncate(notes, MAX_BODY));
    }
    let said: Vec<String> = conversation
        .iter()
        .map(|(user, text)| {
            let cut = if text.chars().count() > MAX_MESSAGE {
                " …"
            } else {
                ""
            };
            format!(
                "\n**{}:** {}{cut}\n",
                if *user { "User" } else { "Agent" },
                truncate(text, MAX_MESSAGE)
            )
        })
        .collect();
    // The first message (what the task was about), then the latest that fit.
    let mut latest = vec![];
    let mut size = said.first().map_or(0, String::len);
    for message in said.iter().skip(1).rev() {
        if size + message.len() > MAX_ORIGIN {
            break;
        }
        size += message.len();
        latest.push(message);
    }
    if let Some(first) = said.first() {
        md.push_str(first);
    }
    if latest.len() + 1 < said.len() {
        let _ = writeln!(md, "\n(… earlier messages left out …)");
    }
    for message in latest.into_iter().rev() {
        md.push_str(message);
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

    #[test]
    fn the_origin_tells_what_was_said_without_the_tools() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.jsonl");
        let lines = [
            r#"{"type":"user","message":{"content":"Speed up keccak"}}"#,
            r#"{"type":"user","isMeta":true,"message":{"content":"Caveat: local command"}}"#,
            r#"{"type":"assistant","message":{"content":[{"type":"thinking","thinking":"hm"},{"type":"tool_use","name":"Bash"}]}}"#,
            r#"{"type":"user","message":{"content":[{"type":"tool_result","content":"build ok"}]}}"#,
            r#"{"type":"assistant","isSidechain":true,"message":{"content":[{"type":"text","text":"subagent"}]}}"#,
            r#"{"type":"user","message":{"content":"<task-notification>done</task-notification>"}}"#,
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Cloning the digest costs a fifth."}]}}"#,
            r#"{"type":"user","message":{"content":[{"type":"text","text":"open a PR"}]}}"#,
            "not json",
        ];
        std::fs::write(&path, lines.join("\n")).unwrap();
        let said = conversation(&path);
        assert_eq!(
            said,
            [
                (true, "Speed up keccak".to_owned()),
                (false, "Cloning the digest costs a fifth.".to_owned()),
                (true, "open a PR".to_owned())
            ]
        );
        let md = render_origin(
            "pull request",
            "T6",
            "Turbo",
            "Look at EVM v2.",
            "task-6",
            &said,
        );
        assert!(md.starts_with("## How this came about"), "{md}");
        assert!(md.contains("task T6 (\"Turbo\", on `task-6`)") && md.contains("Look at EVM v2."));
        assert!(md.contains("**User:** Speed up keccak") && md.contains("**Agent:** Cloning"));
        assert!(!md.contains("left out …"));
        assert!(conversation(&dir.path().join("gone.jsonl")).is_empty());

        // Too long: the first message and the latest that fit.
        let long: Vec<(bool, String)> = (0..40)
            .map(|i| {
                (
                    i % 2 == 0,
                    format!("message {i} {}", "x".repeat(MAX_MESSAGE + 5)),
                )
            })
            .collect();
        let md = render_origin("issue", "T6", "Turbo", "", "task-6", &long);
        assert!(
            md.contains("message 0 ") && md.contains("message 39 ") && !md.contains("message 1 ")
        );
        assert!(md.contains("(… earlier messages left out …)") && md.contains("x …"));
        assert!(md.len() < MAX_ORIGIN + 2_000);
    }
}
