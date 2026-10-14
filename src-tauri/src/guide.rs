//! The agent review guide: Claude reads the change and returns where to start, what each
//! part is for, and whether the description's claims are backed by code. Research on agent
//! PRs found descriptions that claim changes never made; reviewers given a short focus
//! find more than ones given checklists.

use serde::{Deserialize, Serialize};
use std::io::Write as _;
use std::path::Path;
use std::process::Stdio;

/// Diff text sent to Claude at most (the rest is summarized by `--stat`).
const MAX_DIFF_CHARS: usize = 150_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Group {
    pub title: String,
    pub intent: String,
    /// "high", "medium" or "low".
    pub risk: String,
    pub files: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claim {
    pub claim: String,
    /// "implemented", "partial" or "missing".
    pub status: String,
    pub files: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Guide {
    pub focus: String,
    pub groups: Vec<Group>,
    pub claims: Vec<Claim>,
    /// Changed files no claim explains.
    pub unexplained: Vec<String>,
}

pub fn schema() -> serde_json::Value {
    let strings = serde_json::json!({ "type": "array", "items": { "type": "string" } });
    serde_json::json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["focus", "groups", "claims", "unexplained"],
        "properties": {
            "focus": { "type": "string", "description": "At most two sentences: what the reviewer should look at most carefully, and why." },
            "groups": {
                "type": "array",
                "description": "The changed files grouped by concern, the riskiest group first. Every changed file in exactly one group.",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["title", "intent", "risk", "files"],
                    "properties": {
                        "title": { "type": "string" },
                        "intent": { "type": "string", "description": "One line: what these changes do and why." },
                        "risk": { "type": "string", "enum": ["high", "medium", "low"] },
                        "files": strings
                    }
                }
            },
            "claims": {
                "type": "array",
                "description": "Each change the description or commit messages say was made, checked against the diff.",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["claim", "status", "files"],
                    "properties": {
                        "claim": { "type": "string" },
                        "status": { "type": "string", "enum": ["implemented", "partial", "missing"] },
                        "files": strings
                    }
                }
            },
            "unexplained": { "type": "array", "items": { "type": "string" }, "description": "Changed files that no claim explains." }
        }
    })
}

pub struct Input<'a> {
    pub title: &'a str,
    pub description: &'a str,
    pub commits: &'a [String],
    pub stat: &'a str,
    pub diff: &'a str,
}

pub fn prompt(input: &Input) -> String {
    let truncated = input.diff.len() > MAX_DIFF_CHARS;
    let diff = if truncated {
        let mut end = MAX_DIFF_CHARS;
        while !input.diff.is_char_boundary(end) {
            end -= 1;
        }
        &input.diff[..end]
    } else {
        input.diff
    };
    let commits = if input.commits.is_empty() {
        "(none)".to_owned()
    } else {
        input
            .commits
            .iter()
            .map(|c| format!("- {c}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    format!(
        "You are preparing a code review guide for a human reviewer. Most of this change was \
         written by a coding agent. Do not review style. Read the change (you may read files of \
         the repository for context) and return:\n\
         - focus: at most two sentences on what deserves the most careful look and why;\n\
         - groups: every changed file in exactly one group of related changes, the riskiest \
           group first (security, data loss, concurrency, public API, removed or weakened \
           tests are high risk), each with a one-line intent;\n\
         - claims: each change the title, description or commit messages say was made, with \
           whether the diff implements it (implemented / partial / missing) and where;\n\
         - unexplained: changed files that no claim explains.\n\
         Be brief and specific. Use the file paths exactly as in the diff.\n\n\
         # Title\n{title}\n\n# Description\n{description}\n\n# Commit messages\n{commits}\n\n\
         # Files changed\n{stat}\n\n# Diff{note}\n{diff}\n",
        title = input.title,
        description = if input.description.trim().is_empty() {
            "(none)"
        } else {
            input.description.trim()
        },
        stat = input.stat,
        note = if truncated {
            " (truncated; read the files for the rest)"
        } else {
            ""
        },
    )
}

/// Parses `claude -p --output-format json` output into a guide.
pub fn parse(output: &[u8]) -> Result<Guide, String> {
    let v: serde_json::Value =
        serde_json::from_slice(output).map_err(|e| format!("Claude returned no JSON: {e}"))?;
    if v["is_error"].as_bool().unwrap_or(false) || v["subtype"] != "success" {
        let reason = v["result"]
            .as_str()
            .or(v["subtype"].as_str())
            .unwrap_or("unknown error");
        return Err(format!("Claude could not write the guide: {reason}"));
    }
    serde_json::from_value(v["structured_output"].clone())
        .map_err(|e| format!("the guide did not match its schema: {e}"))
}

/// Runs the user's `claude` headless in `cwd` with read-only tools and returns the guide.
pub fn run(claude: &Path, cwd: &Path, input: &Input) -> Result<Guide, String> {
    let schema = schema().to_string();
    let mut child = crate::tools::command(claude)
        .current_dir(cwd)
        .args([
            "-p",
            "--output-format",
            "json",
            "--json-schema",
            &schema,
            "--no-session-persistence",
            "--strict-mcp-config",
            "--tools",
            "Read,Grep,Glob",
        ])
        .env_remove("CLAUDECODE")
        .env_remove("CLAUDE_CODE_CHILD_SESSION")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to run claude: {e}"))?;
    // The prompt goes in on stdin: it can be far longer than a command line allows.
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(prompt(input).as_bytes())
            .map_err(|e| e.to_string())?;
    }
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    if !out.status.success() && out.stdout.is_empty() {
        return Err(format!(
            "claude failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    parse(&out.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input<'a>(diff: &'a str, commits: &'a [String]) -> Input<'a> {
        Input {
            title: "Fix the crash",
            description: "Closes #5. Also adds a cache.",
            commits,
            stat: " a.rs | 2 +-",
            diff,
        }
    }

    #[test]
    fn prompt_has_the_change_and_truncates_long_diffs() {
        let commits = vec!["Fix the crash on start".to_owned()];
        let p = prompt(&input("diff --git a/a.rs b/a.rs\n+fix", &commits));
        assert!(
            p.contains("# Title\nFix the crash")
                && p.contains("- Fix the crash on start")
                && p.contains("+fix")
        );
        assert!(!p.contains("truncated"));
        let long = "é".repeat(MAX_DIFF_CHARS);
        let p = prompt(&input(&long, &[]));
        assert!(
            p.contains("(truncated; read the files for the rest)")
                && p.contains("# Commit messages\n(none)")
        );
    }

    #[test]
    fn parses_structured_output_and_reports_failures() {
        let ok = serde_json::json!({
            "type": "result", "subtype": "success", "is_error": false,
            "structured_output": {
                "focus": "Check the cache invalidation.",
                "groups": [{ "title": "Cache", "intent": "Adds a cache", "risk": "high", "files": ["a.rs"] }],
                "claims": [{ "claim": "Fixes the crash", "status": "missing", "files": [] }],
                "unexplained": ["b.rs"]
            }
        });
        let g = parse(ok.to_string().as_bytes()).unwrap();
        assert_eq!(
            (
                g.groups[0].risk.as_str(),
                g.claims[0].status.as_str(),
                g.unexplained.len()
            ),
            ("high", "missing", 1)
        );

        let failed = serde_json::json!({ "type": "result", "subtype": "error_max_turns", "is_error": true, "result": "out of turns" });
        assert!(
            parse(failed.to_string().as_bytes())
                .unwrap_err()
                .contains("out of turns")
        );
        assert!(parse(b"not json").unwrap_err().contains("no JSON"));
        let wrong = serde_json::json!({ "subtype": "success", "is_error": false, "structured_output": { "focus": 1 } });
        assert!(
            parse(wrong.to_string().as_bytes())
                .unwrap_err()
                .contains("schema")
        );
    }
}
