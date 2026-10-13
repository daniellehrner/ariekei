//! The PR context given to the agent via `--append-system-prompt-file`. Claude Code
//! records the system prompt on a conversation's first request and reuses it on resume,
//! so this describes the PR as it was when the conversation started.

use crate::github::PrDetail;
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
         Ariekei IDE. The user reviews your changes there. This context was captured when the \
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
