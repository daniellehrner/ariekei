// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

//! Review drafts: the line comments of a review until it is submitted to GitHub, the user's
//! (made in the Review tab) and those the item's agent drafts with Ariekei's tools (`mcp`).
//! The Review tab shows them all at their lines, where the user edits them, sends one back
//! to the agent, and submits them.

use crate::AppState;
use crate::store::{DraftAnchor, ReviewDraft, ThreadRef};
use serde_json::{Value, json};
use std::path::Path;

pub fn list(state: &AppState, id: i64) -> Result<Vec<ReviewDraft>, String> {
    state.store.drafts(id)
}

/// The user's new draft on item `id`.
pub fn add(state: &AppState, id: i64, at: &DraftAnchor, text: &str) -> Result<i64, String> {
    let draft = state.store.add_draft(id, at, text, "user", None)?;
    state.events.drafts_changed(id);
    Ok(draft)
}

/// The user changed the text of a draft.
pub fn edit(state: &AppState, id: i64, draft: i64, text: &str) -> Result<(), String> {
    if !state.store.edit_draft(id, draft, text, true)? {
        return Err("the draft is gone".into());
    }
    state.events.drafts_changed(id);
    Ok(())
}

pub fn delete(state: &AppState, id: i64, drafts: &[i64]) -> Result<(), String> {
    state.store.delete_drafts(id, drafts)?;
    state.events.drafts_changed(id);
    Ok(())
}

// --- the agent's tools ---------------------------------------------------------------

/// What every agent is told about the tools (MCP `instructions`): a resumed conversation does
/// not get the item's context again, the tools are all it knows.
pub const INSTRUCTIONS: &str = "Ariekei is the review workbench the user runs you in. When the \
user reviews a pull request someone else wrote and you draft review comments for them, add \
each one with add_review_comment instead of only listing it in your reply, and an answer in a \
thread someone already started with reply_to_review_thread: the user reads, edits and submits \
the drafts to GitHub from Ariekei's Review tab. Never post review comments or replies to \
GitHub yourself.";

/// The tools (MCP `tools/list`).
pub fn tools() -> Value {
    let lines = "Line numbers are those of the file as committed at the worktree's HEAD (the PR's \
new version): `line` is the last line commented on, `start_line` the first one when it is \
several.";
    json!([
        {
            "name": "add_review_comment",
            "description": format!("Drafts a line comment of the review the user is writing of \
    this pull request; Ariekei shows it at those lines, where the user edits it and submits it to \
    GitHub with the rest. Write it as it should appear on GitHub, one point per comment. {lines} \
    GitHub takes line comments only on lines of the diff and the 3 lines around them; the answer \
    says when a draft is not on one. To answer in an existing thread instead, use \
    reply_to_review_thread."),
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "The file, relative to the repository's root" },
                    "line": { "type": "integer", "minimum": 1 },
                    "start_line": { "type": "integer", "minimum": 1 },
                    "body": { "type": "string", "description": "The comment (GitHub Markdown)" },
                },
                "required": ["path", "line", "body"],
            },
        },
        {
            "name": "list_review_threads",
            "description": "Lists the PR's review threads on GitHub as they are now, resolved ones \
    too: each one's id (for reply_to_review_thread), its lines, and its comments.",
            "inputSchema": { "type": "object", "properties": {} },
        },
        {
            "name": "reply_to_review_thread",
            "description": "Drafts a reply in an existing review thread of this pull request, \
    e.g. to answer another reviewer or join their point; it is submitted with the user's review, \
    in that thread. The thread's id is in your context's list of unresolved threads, or from \
    list_review_threads.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "thread": { "type": "string", "description": "The thread's id (PRRT_…)" },
                    "body": { "type": "string", "description": "The reply (GitHub Markdown)" },
                },
                "required": ["thread", "body"],
            },
        },
        {
            "name": "list_review_comments",
            "description": "Lists the drafts of the review, the user's and yours, replies \
    included, with their ids.",
            "inputSchema": { "type": "object", "properties": {} },
        },
        {
            "name": "update_review_comment",
            "description": format!("Changes a draft of the review (yours or the user's, e.g. \
    when they ask you to): its text, its lines, or both; a reply stays in its thread. {lines}"),
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "integer" },
                    "body": { "type": "string" },
                    "path": { "type": "string", "description": "Another file (with `line`)" },
                    "line": { "type": "integer", "minimum": 1 },
                    "start_line": { "type": "integer", "minimum": 1 },
                },
                "required": ["id"],
            },
        },
        {
            "name": "delete_review_comment",
            "description": "Deletes a draft of the review.",
            "inputSchema": {
                "type": "object",
                "properties": { "id": { "type": "integer" } },
                "required": ["id"],
            },
        },
    ])
}

/// Runs tool `name` with `args` for the agent in terminal `terminal`: the answer for the agent,
/// or why it failed (also for the agent).
pub fn call(
    state: &AppState,
    terminal: Option<&str>,
    name: &str,
    args: &Value,
) -> Result<String, String> {
    let id = terminal
        .and_then(|t| state.terminals.key_for(Some(t), ""))
        .and_then(|key| key.parse::<i64>().ok())
        .ok_or("Ariekei does not know this agent: its tools work only in agents it started")?;
    let item = state.store.item(id)?;
    let dir = item
        .worktree_path
        .clone()
        .filter(|p| p.exists() && item.kind == "pr")
        .ok_or("Review comments can be drafted only in a pull request's worktree")?;
    let answer = match name {
        "add_review_comment" => {
            let at = anchor(&dir, &string(args, "path")?, args)?;
            let text = string(args, "body")?;
            let draft = state.store.add_draft(id, &at, &text, "agent", None)?;
            format!(
                "Drafted #{draft} on {}.{}",
                place(&at),
                note(state, id, &at)
            )
        }
        "list_review_threads" => {
            let threads = threads(state, id)?;
            if threads.is_empty() {
                return Ok("The pull request has no review threads.".into());
            }
            return Ok(threads
                .iter()
                .map(|t| {
                    let mut text = format!("Thread {} on {}", t.id, thread_place(t));
                    if t.resolved {
                        text.push_str(" (resolved)");
                    }
                    if t.outdated {
                        text.push_str(" (outdated: the code changed since)");
                    }
                    for c in &t.comments {
                        text.push_str(&format!("\n{}: {}", c.author, c.body.trim()));
                    }
                    text
                })
                .collect::<Vec<_>>()
                .join("\n\n"));
        }
        "reply_to_review_thread" => {
            let wanted = string(args, "thread")?;
            let text = string(args, "body")?;
            let t = threads(state, id)?
                .into_iter()
                .find(|t| t.id == wanted.trim())
                .ok_or(format!(
                    "The pull request has no review thread {wanted}: list_review_threads lists them"
                ))?;
            let first = t.comments.first();
            let reply_to = ThreadRef {
                thread: t.id.clone(),
                author: first.map(|c| c.author.clone()).unwrap_or_default(),
                text: first
                    .map(|c| crate::context::truncate(c.body.trim(), 300).to_owned())
                    .unwrap_or_default(),
            };
            let at = thread_anchor(state, id, &dir, &t);
            let draft = state
                .store
                .add_draft(id, &at, &text, "agent", Some(&reply_to))?;
            format!(
                "Drafted #{draft}, a reply in {}'s thread on {}.",
                reply_to.author,
                thread_place(&t)
            )
        }
        "list_review_comments" => {
            let drafts = state.store.drafts(id)?;
            if drafts.is_empty() {
                return Ok("The review has no drafts.".into());
            }
            let by = |d: &ReviewDraft| match (d.author.as_str(), d.edited) {
                ("agent", false) => "you",
                ("agent", true) => "you, edited by the user",
                _ => "the user",
            };
            return Ok(drafts
                .iter()
                .map(|d| match &d.reply_to {
                    Some(r) => format!(
                        "#{} by {}, a reply in {}'s thread {} on {}:\n{}",
                        d.id,
                        by(d),
                        r.author,
                        r.thread,
                        place(&d.anchor),
                        d.text
                    ),
                    None => format!(
                        "#{} by {} on {}:\n{}",
                        d.id,
                        by(d),
                        place(&d.anchor),
                        d.text
                    ),
                })
                .collect::<Vec<_>>()
                .join("\n\n"));
        }
        "update_review_comment" => {
            let draft = int(args, "id")?.ok_or("`id` is missing")?.into();
            let current = state
                .store
                .drafts(id)?
                .into_iter()
                .find(|d| d.id == draft)
                .ok_or(format!("There is no draft #{draft}"))?;
            let mut answer = format!("Changed #{draft}.");
            let moves = ["line", "path", "start_line"]
                .iter()
                .any(|k| args.get(k).is_some());
            if moves && current.reply_to.is_some() {
                return Err(format!(
                    "#{draft} is a reply in a thread: it stays there, only its body changes"
                ));
            }
            if args.get("line").is_some() {
                let path = match args.get("path") {
                    Some(_) => string(args, "path")?,
                    None => current.anchor.path,
                };
                let at = anchor(&dir, &path, args)?;
                state.store.move_draft(id, draft, &at)?;
                answer = format!("Moved #{draft} to {}.{}", place(&at), note(state, id, &at));
            } else if args.get("path").is_some() || args.get("start_line").is_some() {
                return Err("Give `line` too, to move the draft".into());
            }
            if args.get("body").is_some() {
                state
                    .store
                    .edit_draft(id, draft, &string(args, "body")?, false)?;
            }
            answer
        }
        "delete_review_comment" => {
            let draft = int(args, "id")?.ok_or("`id` is missing")?.into();
            if state.store.delete_drafts(id, &[draft])? == 0 {
                return Err(format!("There is no draft #{draft}"));
            }
            format!("Deleted #{draft}.")
        }
        _ => return Err(format!("There is no tool {name}")),
    };
    state.events.drafts_changed(id);
    Ok(answer)
}

fn string(args: &Value, key: &str) -> Result<String, String> {
    match args.get(key) {
        Some(Value::String(s)) if !s.trim().is_empty() => Ok(s.clone()),
        Some(_) => Err(format!("`{key}` must be a text")),
        None => Err(format!("`{key}` is missing")),
    }
}

fn int(args: &Value, key: &str) -> Result<Option<u32>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .filter(|&n| n > 0)
            .map(Some)
            .ok_or(format!("`{key}` must be a line number")),
    }
}

/// `path:3` or `path:3–5`.
fn place(at: &DraftAnchor) -> String {
    if at.from_line == at.to_line {
        format!("{}:{}", at.path, at.to_line)
    } else {
        format!("{}:{}–{}", at.path, at.from_line, at.to_line)
    }
}

/// The lines `args` name (`line`, `start_line`) of `path` as committed at the HEAD of `dir`.
fn anchor(dir: &Path, path: &str, args: &Value) -> Result<DraftAnchor, String> {
    let to = int(args, "line")?.ok_or("`line` is missing")?;
    let from = int(args, "start_line")?.unwrap_or(to);
    let head = crate::git::head(dir)?;
    let path = path.trim_start_matches("./");
    let content = crate::git::git(dir, &["show", &format!("{head}:{path}")]).map_err(|_| {
        format!(
            "{path} is not in the worktree's HEAD ({}): give its path from the repository's root, as committed",
            &head[..8]
        )
    })?;
    let content = String::from_utf8_lossy(&content);
    let lines: Vec<&str> = content.lines().collect();
    if from > to || to as usize > lines.len() {
        return Err(format!(
            "{path} has {} lines at HEAD: there are no lines {from}–{to}",
            lines.len()
        ));
    }
    Ok(DraftAnchor {
        path: path.to_owned(),
        from_line: from,
        to_line: to,
        code: lines[from as usize - 1..to as usize].join("\n"),
        rev: Some(head),
    })
}

/// The PR's review threads on GitHub now.
fn threads(state: &AppState, id: i64) -> Result<Vec<crate::github::Thread>, String> {
    let (item, repo) = crate::workitem::item_repo(state, id)?;
    Ok(crate::github::pr_detail(&repo.owner, &repo.name, item.number)?.threads)
}

/// `path:12`, or the path of a thread on no line.
fn thread_place(t: &crate::github::Thread) -> String {
    match t.line {
        Some(line) => format!("{}:{line}", t.path),
        None => t.path.clone(),
    }
}

/// Where a reply shows: its thread's line, in the PR's head on GitHub (an outdated thread's
/// line is in an older version: no commit is known for it).
fn thread_anchor(state: &AppState, id: i64, dir: &Path, t: &crate::github::Thread) -> DraftAnchor {
    let line = t.line.and_then(|l| u32::try_from(l).ok()).unwrap_or(0);
    let rev = (!t.outdated)
        .then(|| {
            state
                .store
                .item(id)
                .and_then(|i| crate::workitem::summary(&i))
        })
        .and_then(Result::ok)
        .map(|s| s.head_sha)
        .filter(|sha| !sha.is_empty());
    let code = rev
        .as_ref()
        .filter(|_| line > 0)
        .and_then(|rev| crate::git::git(dir, &["show", &format!("{rev}:{}", t.path)]).ok())
        .and_then(|content| {
            String::from_utf8_lossy(&content)
                .lines()
                .nth(line as usize - 1)
                .map(str::to_owned)
        })
        .unwrap_or_default();
    DraftAnchor {
        path: t.path.clone(),
        from_line: line,
        to_line: line,
        code,
        rev,
    }
}

/// Why a draft would not become a line comment on GitHub (empty when it would): GitHub takes
/// those on the PR's head only, and only on lines of its diff (see `workitem::comment_ranges`).
fn note(state: &AppState, id: i64, at: &DraftAnchor) -> String {
    let rev = at.rev.as_deref().unwrap_or_default();
    let pr_head = state
        .store
        .item(id)
        .and_then(|item| crate::workitem::summary(&item))
        .map(|s| s.head_sha)
        .unwrap_or_default();
    if rev != pr_head {
        return format!(
            " Note: the worktree's HEAD ({}) is not the PR's head on GitHub, which line comments \
             must be on: unless that changes before the user submits, this one goes into the \
             review's text.",
            &rev[..rev.len().min(8)]
        );
    }
    let ranges = crate::workitem::comment_ranges(state, id, std::slice::from_ref(&at.path), rev);
    let on_diff = ranges.is_ok_and(|r| {
        r.iter()
            .flat_map(|(_, ranges)| ranges)
            .any(|&(first, last)| first <= at.from_line && at.to_line <= last)
    });
    if on_diff {
        String::new()
    } else {
        " Note: these lines are not all in one hunk of the PR's diff (changed lines and the 3 \
         around them), so GitHub would not take it as a line comment: it goes into the \
         review's text unless you move it."
            .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::{TestApp, commit, write};

    #[test]
    fn the_agent_drafts_comments_at_lines_of_its_head() {
        let app = TestApp::new();
        let (id, wt) = app.pr_with_worktree(5, "feature");
        write(&wt.join("c.txt"), "one\ntwo\nthree\n");
        commit(&wt, "Add c", "me");
        crate::testkit::git(&wt, &["push", "-q", "origin", "feature"]);
        let head = crate::git::head(&wt).unwrap();
        app.poll().unwrap();
        // Not an agent Ariekei started.
        assert!(
            call(
                &app.state,
                Some("nobody"),
                "list_review_comments",
                &json!({})
            )
            .is_err()
        );
        app.start_agent(id);
        let token = app.state.terminals.token(&id.to_string()).unwrap();
        let call = |name, args| call(&app.state, Some(&token), name, &args);
        app.events.take();

        let answer = call(
            "add_review_comment",
            json!({"path": "c.txt", "start_line": 2, "line": 3, "body": "Why three?"}),
        )
        .unwrap();
        assert!(
            answer.starts_with("Drafted #") && answer.ends_with("c.txt:2–3."),
            "{answer}"
        );
        let drafts = app.state.store.drafts(id).unwrap();
        let d = &drafts[0];
        assert_eq!(
            (
                d.anchor.code.as_str(),
                d.anchor.rev.as_deref(),
                d.author.as_str()
            ),
            ("two\nthree", Some(head.as_str()), "agent")
        );
        assert!(
            app.events
                .take()
                .contains(&crate::events::Sent::DraftsChanged(id))
        );

        // Lines that are not there, or a file that is not, are refused.
        assert!(
            call(
                "add_review_comment",
                json!({"path": "c.txt", "line": 4, "body": "x"})
            )
            .is_err()
        );
        assert!(
            call(
                "add_review_comment",
                json!({"path": "nope.txt", "line": 1, "body": "x"})
            )
            .is_err()
        );

        // Text changes and moves; a move to lines GitHub would not take is said.
        let draft = d.id;
        call(
            "update_review_comment",
            json!({"id": draft, "body": "Why the third?"}),
        )
        .unwrap();
        let moved = call(
            "update_review_comment",
            json!({"id": draft, "path": "b.txt", "line": 1}),
        )
        .unwrap();
        assert!(
            moved.starts_with(&format!("Moved #{draft} to b.txt:1.")),
            "{moved}"
        );
        let d = &app.state.store.drafts(id).unwrap()[0];
        assert_eq!(
            (d.anchor.path.as_str(), d.text.as_str(), d.edited),
            ("b.txt", "Why the third?", false)
        );
        let listed = call("list_review_comments", json!({})).unwrap();
        assert_eq!(
            listed,
            format!("#{draft} by you on b.txt:1:\nWhy the third?")
        );

        call("delete_review_comment", json!({"id": draft})).unwrap();
        assert!(app.state.store.drafts(id).unwrap().is_empty());
        assert!(call("delete_review_comment", json!({"id": draft})).is_err());
    }

    #[test]
    fn the_agent_replies_in_threads_and_they_go_with_the_review() {
        let app = TestApp::new();
        let (id, _) = app.pr_with_worktree(5, "feature");
        let mut data = app.data();
        data["prs"][0]["threads"] = json!([
            {"id": "PRRT_1", "path": "b.txt", "line": 1, "comment_id": 101, "author": "bob",
             "body": "Why mine?", "html": "<p>Why mine?</p>",
             "replies": [{"comment_id": 102, "author": "carol", "html": "<p>Same</p>"}]},
        ]);
        app.set_data(&data);
        app.start_agent(id);
        let token = app.state.terminals.token(&id.to_string()).unwrap();
        let call = |name, args| call(&app.state, Some(&token), name, &args);

        let listed = call("list_review_threads", json!({})).unwrap();
        assert!(
            listed.starts_with("Thread PRRT_1 on b.txt:1\nbob: Why mine?"),
            "{listed}"
        );
        assert!(
            call(
                "reply_to_review_thread",
                json!({"thread": "PRRT_9", "body": "x"})
            )
            .unwrap_err()
            .contains("no review thread PRRT_9")
        );
        let answer = call(
            "reply_to_review_thread",
            json!({"thread": "PRRT_1", "body": "It is the default."}),
        )
        .unwrap();
        assert!(
            answer.ends_with("a reply in bob's thread on b.txt:1."),
            "{answer}"
        );
        let d = &app.state.store.drafts(id).unwrap()[0];
        let head = crate::workitem::summary(&app.item(5)).unwrap().head_sha;
        assert_eq!(
            (
                d.anchor.to_line,
                d.anchor.code.as_str(),
                d.anchor.rev.as_deref()
            ),
            (1, "mine", Some(head.as_str()))
        );
        assert_eq!(
            d.reply_to,
            Some(ThreadRef {
                thread: "PRRT_1".into(),
                author: "bob".into(),
                text: "Why mine?".into()
            })
        );
        // It stays in its thread; its text changes.
        assert!(
            call(
                "update_review_comment",
                json!({"id": d.id, "line": 1, "body": "x"})
            )
            .is_err()
        );
        call(
            "update_review_comment",
            json!({"id": d.id, "body": "It is the default here."}),
        )
        .unwrap();
        assert!(
            call("list_review_comments", json!({}))
                .unwrap()
                .contains("a reply in bob's thread PRRT_1 on b.txt:1:\nIt is the default here.")
        );

        // Submitted: a pending review, the reply in it, then the review's event.
        let reply = |thread: &str| crate::github::ThreadReply {
            thread: thread.into(),
            body: "It is the default here.".into(),
        };
        let submit = |replies: &[crate::github::ThreadReply]| {
            crate::workitem::submit_review(&app.state, id, "APPROVE", "", &[], replies, &head)
        };
        let before = app.writes().len();
        let url = submit(&[reply("PRRT_1")]).unwrap();
        assert!(url.ends_with("/reviews/7/events"), "{url}");
        let writes = app.writes().split_off(before);
        assert_eq!(writes[0]["post"], "repos/owner/proj/pulls/5/reviews");
        assert_eq!(
            (
                writes[0]["body"].get("event"),
                &writes[0]["body"]["commit_id"]
            ),
            (None, &json!(head))
        );
        assert_eq!(writes[1]["graphql"], "addPullRequestReviewThreadReply");
        assert_eq!(
            (&writes[1]["vars"]["review"], &writes[1]["vars"]["thread"]),
            (&json!("PRR_7"), &json!("PRRT_1"))
        );
        assert_eq!(
            (&writes[2]["post"], &writes[2]["body"]),
            (
                &json!("repos/owner/proj/pulls/5/reviews/7/events"),
                &json!({"event": "APPROVE"})
            )
        );

        // A thread GitHub no longer has: the pending review goes again, nothing is posted.
        let before = app.writes().len();
        assert!(submit(&[reply("PRRT_9")]).is_err());
        let writes = app.writes().split_off(before);
        assert_eq!(
            writes.last().unwrap()["delete"],
            "repos/owner/proj/pulls/5/reviews/7"
        );
        assert!(
            writes
                .iter()
                .all(|w| !w["post"].as_str().is_some_and(|p| p.ends_with("/events")))
        );
    }

    #[test]
    fn a_draft_github_would_not_take_as_a_line_comment_is_said() {
        let app = TestApp::new();
        let lines: String = (1..=20).map(|n| format!("line {n}\n")).collect();
        app.push_to("main", &[("long.txt", &lines)], "me");
        let (id, wt) = app.pr_with_worktree(5, "feature");
        write(
            &wt.join("long.txt"),
            &lines.replace("line 20\n", "line twenty\n"),
        );
        commit(&wt, "Change its end", "me");
        crate::testkit::git(&wt, &["push", "-q", "origin", "feature"]);
        app.poll().unwrap();
        app.start_agent(id);
        let token = app.state.terminals.token(&id.to_string()).unwrap();
        let add = |line: u32| {
            call(
                &app.state,
                Some(&token),
                "add_review_comment",
                &json!({"path": "long.txt", "line": line, "body": "x"}),
            )
            .unwrap()
        };
        // GitHub's hunk: the changed line and the 3 before it.
        assert!(!add(17).contains("Note"));
        assert!(add(2).contains("not all in one hunk of the PR's diff"));

        // Not on the PR's head.
        write(&wt.join("long.txt"), "local\n");
        commit(&wt, "Local only", "me");
        assert!(add(1).contains("is not the PR's head on GitHub"));
    }
}
