//! Agent status from Claude Code hooks.
//!
//! Every `claude` that Ariekei starts gets `--settings <claude-hooks.json>`, which adds
//! `http` hooks (merged with the user's and the project's own hooks) that POST each event
//! to a localhost endpoint. Events carry the session id, which identifies the work item.
//! A task's agent opening a PR or an issue (`gh pr create`, …) links it to the task.

use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::{AppHandle, Manager as _};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentStatus {
    Starting,
    Ready,
    Working,
    WaitingForPermission,
    Finished,
    Error,
    Stopped,
}

/// The fields Ariekei uses from a hook payload; everything else is ignored.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct HookEvent {
    pub hook_event_name: String,
    pub session_id: String,
    #[serde(default)]
    pub transcript_path: Option<PathBuf>,
    #[serde(default)]
    pub notification_type: Option<String>,
    #[serde(default)]
    pub tool_name: Option<String>,
    #[serde(default)]
    pub tool_input: Option<serde_json::Value>,
    #[serde(default)]
    pub tool_response: Option<serde_json::Value>,
    #[serde(default)]
    pub error: Option<serde_json::Value>,
}

/// Events registered in the hooks settings file. Not SessionStart: it ignores `http`
/// hooks (only `command` hooks run), so `ready` comes from the terminal's first output.
const EVENTS: [&str; 8] = [
    "UserPromptSubmit",
    "PreToolUse",
    "PostToolUse",
    "PermissionRequest",
    "Notification",
    "Stop",
    "StopFailure",
    "SessionEnd",
];

/// The status an event implies, if any (e.g. most notification types change nothing).
pub fn status_for(event: &HookEvent) -> Option<(AgentStatus, Option<String>)> {
    use AgentStatus::*;
    let tool = || tool_detail(event);
    Some(match event.hook_event_name.as_str() {
        "UserPromptSubmit" => (Working, None),
        "PreToolUse" | "PostToolUse" => (Working, tool()),
        "PermissionRequest" => (WaitingForPermission, tool()),
        "Notification" => match event.notification_type.as_deref() {
            Some("permission_prompt") => (WaitingForPermission, None),
            Some("idle_prompt") => (Finished, None),
            _ => return None,
        },
        "Stop" => (Finished, None),
        "StopFailure" => (Error, event.error.as_ref().map(|e| e.to_string())),
        "SessionEnd" => (Stopped, None),
        _ => return None,
    })
}

/// The tool an event is about, with what it acts on: `Bash ./gradlew test`, `Edit src/a.rs`.
fn tool_detail(event: &HookEvent) -> Option<String> {
    let name = event.tool_name.clone()?;
    let input = event.tool_input.as_ref();
    let target = ["command", "file_path", "path", "pattern", "url"]
        .iter()
        .find_map(|k| input?.get(*k)?.as_str())
        .map(|t| t.lines().next().unwrap_or_default().trim().to_owned());
    Some(match target {
        Some(t) if !t.is_empty() => {
            let t = if t.chars().count() > 160 {
                format!("{}…", t.chars().take(160).collect::<String>())
            } else {
                t
            };
            format!("{name} {t}")
        }
        _ => name,
    })
}

/// The PRs and issues of `owner/name` that a finished tool call created, as ("pr" | "issue",
/// number): from what `gh pr create` / `gh issue create` print (their URL), or the response
/// of a GitHub tool that creates one (e.g. an MCP server's `create_pull_request`).
pub fn created_links(event: &HookEvent, owner: &str, name: &str) -> Vec<(&'static str, i64)> {
    if event.hook_event_name != "PostToolUse" {
        return vec![];
    }
    let tool = event
        .tool_name
        .as_deref()
        .unwrap_or_default()
        .to_lowercase();
    let (prs, issues) = if tool == "bash" {
        let command = event
            .tool_input
            .as_ref()
            .and_then(|i| i["command"].as_str())
            .unwrap_or_default()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        (
            command.contains("gh pr create"),
            command.contains("gh issue create"),
        )
    } else {
        // The tool's own name, not a longer one (`create_pull_request_review` creates no PR).
        (
            tool.ends_with("create_pull_request"),
            tool.ends_with("create_issue"),
        )
    };
    if !prs && !issues {
        return vec![];
    }
    let Some(response) = event.tool_response.as_ref() else {
        return vec![];
    };
    let text = response.to_string();
    let prefix = format!("https://github.com/{owner}/{name}/").to_lowercase();
    let mut found = vec![];
    let lower = text.to_lowercase();
    for (at, _) in lower.match_indices(&prefix) {
        let rest = &text[at + prefix.len()..];
        let (kind, rest) = if let Some(r) = rest.strip_prefix("pull/").filter(|_| prs) {
            ("pr", r)
        } else if let Some(r) = rest.strip_prefix("issues/").filter(|_| issues) {
            ("issue", r)
        } else {
            continue;
        };
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        if let Ok(number) = digits.parse()
            && !found.contains(&(kind, number))
        {
            found.push((kind, number));
        }
    }
    found
}

/// PRs and issues a task's agent opened on GitHub, to link to the task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskLinks {
    pub task: i64,
    pub repo: i64,
    /// ("pr" | "issue", number), see `created_links`.
    pub links: Vec<(&'static str, i64)>,
}

/// What the task behind terminal `key` just opened on GitHub, if anything.
fn links_of_task(state: &crate::AppState, key: &str, event: &HookEvent) -> Option<TaskLinks> {
    let task = state.store.item(key.parse().ok()?).ok()?;
    if !task.is_task() {
        return None;
    }
    let repo = state.store.repo(task.repo_id).ok()??;
    let links = created_links(event, &repo.owner, &repo.name);
    (!links.is_empty()).then_some(TaskLinks {
        task: task.id,
        repo: repo.id,
        links,
    })
}

/// Links what a task's agent opened on GitHub to the task (fetching each from GitHub), then
/// refreshes the Inbox.
pub fn link_to_task(state: &crate::AppState, links: &TaskLinks) {
    for (kind, number) in &links.links {
        if let Err(e) = crate::workitem::link_created(state, links.task, kind, *number) {
            tracing::warn!(
                "could not link {kind} #{number} to task {}: {e}",
                links.task
            );
        }
    }
    state.events.inbox_changed(links.repo);
}

/// Hooks settings for `claude --settings`: one http hook per event, posting to `url`.
fn settings_json(url: &str) -> serde_json::Value {
    let hook = serde_json::json!([{ "hooks": [{ "type": "http", "url": url, "timeout": 5 }] }]);
    let hooks: serde_json::Map<String, serde_json::Value> = EVENTS
        .iter()
        .map(|e| (e.to_string(), hook.clone()))
        .collect();
    serde_json::json!({ "hooks": hooks })
}

pub struct HookServer {
    pub settings_path: PathBuf,
}

impl HookServer {
    /// Binds a localhost port, writes the hooks settings file into `data_dir` and serves
    /// hook requests on a background thread. Each event goes to `on_event`.
    pub fn start(
        data_dir: &Path,
        on_event: impl Fn(HookEvent) + Send + 'static,
    ) -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        let token = uuid::Uuid::new_v4().simple().to_string();
        let path = format!("/hook/{token}");

        let settings_path = data_dir.join("claude-hooks.json");
        let settings = settings_json(&format!("http://127.0.0.1:{port}{path}"));
        std::fs::write(&settings_path, serde_json::to_vec_pretty(&settings)?)?;

        std::thread::Builder::new()
            .name("hook-server".into())
            .spawn(move || {
                for stream in listener.incoming().flatten() {
                    match handle(stream, &path) {
                        Ok(Some(event)) => on_event(event),
                        Ok(None) => {}
                        Err(e) => tracing::debug!("hook request failed: {e}"),
                    }
                }
            })?;
        Ok(Self { settings_path })
    }
}

/// Reads one HTTP request, answers it right away, and returns the parsed event.
/// Requests for any other path (wrong token) are rejected with 404.
fn handle(mut stream: TcpStream, expected_path: &str) -> std::io::Result<Option<HookEvent>> {
    // Generous for a loaded machine; Claude Code sends each event in one small request.
    stream.set_read_timeout(Some(Duration::from_secs(15)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    let mut parts = request_line.split_whitespace();
    let (method, path) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));

    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 || line == "\r\n" || line == "\n" {
            break;
        }
        if let Some((name, value)) = line.split_once(':')
            && name.trim().eq_ignore_ascii_case("content-length")
        {
            content_length = value.trim().parse().unwrap_or(0);
        }
    }
    // Hook payloads are small; refuse anything absurd rather than buffering it.
    let mut body = vec![0; content_length.min(1 << 20)];
    reader.read_exact(&mut body)?;

    let accepted = method == "POST" && path == expected_path;
    let response: &[u8] = if accepted {
        b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}"
    } else {
        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    };
    stream.write_all(response)?;
    // Close our side cleanly so the client reads the whole answer (Windows otherwise may
    // reset a connection closed right after the write).
    let _ = stream.shutdown(std::net::Shutdown::Write);
    if !accepted {
        return Ok(None);
    }
    Ok(serde_json::from_slice(&body).ok())
}

/// Routes a hook event to its work item and updates the item's status. What a task's agent
/// opened on GitHub is linked in the background (it fetches each from GitHub).
pub fn dispatch(app: &AppHandle, event: HookEvent) {
    // Events can arrive before the app state exists (startup race): drop those.
    let Some(state) = app.try_state::<crate::AppState>() else {
        return;
    };
    if let Some(links) = record(&state, &event) {
        let app = app.clone();
        std::thread::spawn(move || link_to_task(&app.state::<crate::AppState>(), &links));
    }
}

/// The part of `dispatch` that needs no app: records the session start and the status of
/// the event's terminal, and returns what a task's agent opened on GitHub, to be linked
/// (`link_to_task`).
pub fn record(state: &crate::AppState, event: &HookEvent) -> Option<TaskLinks> {
    // Hooks of sessions Ariekei did not start (or already closed) are ignored.
    let key = state.terminals.key_for_session(&event.session_id)?;
    if event.hook_event_name == "UserPromptSubmit"
        && let Ok(id) = key.parse()
        && let Err(e) = state
            .store
            .mark_session_started(id, event.transcript_path.as_deref())
    {
        tracing::warn!("failed to record session start: {e}");
    }
    let links = links_of_task(state, &key, event);
    if let Some((status, detail)) = status_for(event) {
        state.report(&key, status, detail);
    }
    links
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(name: &str, extra: serde_json::Value) -> HookEvent {
        let mut v = serde_json::json!({ "hook_event_name": name, "session_id": "s1", "cwd": "/w" });
        v.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn links_what_gh_and_github_tools_created_in_this_repo() {
        let bash = |command: &str, stdout: &str| {
            event(
                "PostToolUse",
                serde_json::json!({
                    "tool_name": "Bash",
                    "tool_input": { "command": command },
                    "tool_response": { "stdout": stdout, "stderr": "" }
                }),
            )
        };
        let pr = bash(
            "git push -u origin HEAD && gh  pr create --fill",
            "Creating pull request for task-1 into main in Owner/Proj\n\nhttps://github.com/owner/proj/pull/12\n",
        );
        assert_eq!(created_links(&pr, "owner", "proj"), [("pr", 12)]);
        let issue = bash(
            "gh issue create --title x --body 'see #3'",
            "https://github.com/owner/proj/issues/13\n",
        );
        assert_eq!(created_links(&issue, "owner", "proj"), [("issue", 13)]);
        // Another repo's, and URLs of a command that creates nothing.
        let elsewhere = bash("gh pr create", "https://github.com/other/proj/pull/5\n");
        assert!(created_links(&elsewhere, "owner", "proj").is_empty());
        let view = bash("gh pr view 12", "https://github.com/owner/proj/pull/12\n");
        assert!(created_links(&view, "owner", "proj").is_empty());
        let before = HookEvent {
            hook_event_name: "PreToolUse".into(),
            ..pr.clone()
        };
        assert!(created_links(&before, "owner", "proj").is_empty());
        let mcp = event(
            "PostToolUse",
            serde_json::json!({
                "tool_name": "mcp__github__create_issue",
                "tool_response": [{ "type": "text", "text": "{\"html_url\":\"https://github.com/owner/proj/issues/14\"}" }]
            }),
        );
        assert_eq!(created_links(&mcp, "owner", "proj"), [("issue", 14)]);
        // A review of an existing PR, or a comment on an issue, creates neither.
        for tool in [
            "mcp__github__create_pull_request_review",
            "mcp__github__create_issue_comment",
        ] {
            let other = HookEvent {
                tool_name: Some(tool.into()),
                ..mcp.clone()
            };
            assert!(created_links(&other, "owner", "proj").is_empty(), "{tool}");
        }
    }

    #[test]
    fn details_name_the_tool_and_its_target() {
        let ask = event(
            "PermissionRequest",
            serde_json::json!({ "tool_name": "Bash", "tool_input": { "command": "./gradlew test\necho done" } }),
        );
        assert_eq!(tool_detail(&ask).as_deref(), Some("Bash ./gradlew test"));
        let edit = event(
            "PreToolUse",
            serde_json::json!({ "tool_name": "Edit", "tool_input": { "file_path": "src/a.rs" } }),
        );
        assert_eq!(tool_detail(&edit).as_deref(), Some("Edit src/a.rs"));
        let bare = event("PreToolUse", serde_json::json!({ "tool_name": "Task" }));
        assert_eq!(tool_detail(&bare).as_deref(), Some("Task"));
    }

    #[test]
    fn maps_hook_events_to_status() {
        use AgentStatus::*;
        let none = serde_json::json!({});
        let cases = [
            (
                event("UserPromptSubmit", serde_json::json!({ "prompt": "hi" })),
                Some(Working),
            ),
            (
                event("PreToolUse", serde_json::json!({ "tool_name": "Bash" })),
                Some(Working),
            ),
            (
                event(
                    "PermissionRequest",
                    serde_json::json!({ "tool_name": "Write" }),
                ),
                Some(WaitingForPermission),
            ),
            (
                event(
                    "Notification",
                    serde_json::json!({ "notification_type": "permission_prompt" }),
                ),
                Some(WaitingForPermission),
            ),
            (
                event(
                    "Notification",
                    serde_json::json!({ "notification_type": "idle_prompt" }),
                ),
                Some(Finished),
            ),
            (
                event(
                    "Notification",
                    serde_json::json!({ "notification_type": "auth_success" }),
                ),
                None,
            ),
            (event("Stop", none.clone()), Some(Finished)),
            (
                event("StopFailure", serde_json::json!({ "error": "rate_limit" })),
                Some(Error),
            ),
            (event("SessionEnd", none.clone()), Some(Stopped)),
            (event("PreCompact", none), None),
        ];
        for (e, expected) in cases {
            assert_eq!(
                status_for(&e).map(|(s, _)| s),
                expected,
                "{}",
                e.hook_event_name
            );
        }
        let pre = event("PreToolUse", serde_json::json!({ "tool_name": "Bash" }));
        assert_eq!(status_for(&pre).unwrap().1.as_deref(), Some("Bash"));
    }

    fn post(port: u16, path: &str, body: &str) -> String {
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
        // One write, like a real HTTP client.
        let request = format!(
            "POST {path} HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        s.write_all(request.as_bytes()).unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).unwrap();
        out
    }

    #[test]
    fn server_accepts_only_the_token_path_and_writes_settings() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let server = HookServer::start(dir.path(), move |e| tx.send(e).unwrap()).unwrap();

        let settings: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&server.settings_path).unwrap()).unwrap();
        let url = settings["hooks"]["Stop"][0]["hooks"][0]["url"]
            .as_str()
            .unwrap()
            .to_owned();
        assert_eq!(settings["hooks"].as_object().unwrap().len(), EVENTS.len());
        let (port, path) = url
            .trim_start_matches("http://127.0.0.1:")
            .split_once('/')
            .unwrap();
        let (port, path) = (port.parse().unwrap(), format!("/{path}"));

        let body = r#"{"hook_event_name":"Stop","session_id":"abc"}"#;
        assert!(post(port, "/hook/wrong-token", body).starts_with("HTTP/1.1 404"));
        let ok = post(port, &path, body);
        assert!(ok.starts_with("HTTP/1.1 200") && ok.ends_with("{}"), "{ok}");

        let received = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(
            (
                received.hook_event_name.as_str(),
                received.session_id.as_str()
            ),
            ("Stop", "abc")
        );
        assert!(
            rx.try_recv().is_err(),
            "the rejected request must not produce an event"
        );
    }

    mod with_app {
        use super::super::*;
        use crate::events::Sent;
        use crate::testkit::TestApp;
        use crate::workitem::create_task;
        use serde_json::json;

        fn hook(session: &str, name: &str, extra: serde_json::Value) -> HookEvent {
            let mut v = json!({"hook_event_name": name, "session_id": session});
            v.as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            serde_json::from_value(v).unwrap()
        }

        #[test]
        fn hooks_of_a_tasks_agent_set_its_status_and_link_what_it_opened() {
            let app = TestApp::new();
            let task = create_task(&app.state, Some(app.repo.clone()), "Hooks", "").unwrap();
            app.start_agent(task);
            let key = task.to_string();
            let session = app.state.store.item(task).unwrap().session_id.unwrap();

            assert_eq!(
                record(
                    &app.state,
                    &hook(
                        &session,
                        "UserPromptSubmit",
                        json!({"transcript_path": "/t.jsonl"})
                    )
                ),
                None
            );
            assert_eq!(app.state.terminals.status(&key), Some(AgentStatus::Working));
            assert!(app.state.store.item(task).unwrap().session_started);
            record(
                &app.state,
                &hook(
                    &session,
                    "PermissionRequest",
                    json!({"tool_name": "Bash", "tool_input": {"command": "rm -r build"}}),
                ),
            );
            assert_eq!(
                app.state.terminals.status(&key),
                Some(AgentStatus::WaitingForPermission)
            );
            assert!(
                app.events
                    .notices()
                    .iter()
                    .any(|(kind, title, body)| kind == "needs-you"
                        && title == "T1 Hooks"
                        && body.contains("rm -r build"))
            );

            app.add_pr(json!({"number": 30, "title": "From the task", "head_ref": "main"}));
            let links = record(
                &app.state,
                &hook(
                    &session,
                    "PostToolUse",
                    json!({
                        "tool_name": "Bash",
                        "tool_input": {"command": "gh pr create --fill"},
                        "tool_response": {"stdout": "https://github.com/owner/proj/pull/30\n"}
                    }),
                ),
            )
            .expect("a PR to link");
            assert_eq!(
                links,
                TaskLinks {
                    task,
                    repo: app.repo.id,
                    links: vec![("pr", 30)]
                }
            );
            link_to_task(&app.state, &links);
            let pr = app.item(30);
            assert_eq!((pr.task_id, pr.tracked), (Some(task), true));
            assert!(app.events.take().contains(&Sent::InboxChanged(app.repo.id)));

            // Hooks of sessions Ariekei did not start change nothing.
            assert_eq!(
                record(&app.state, &hook("someone-else", "Stop", json!({}))),
                None
            );
            assert_eq!(
                app.state.terminals.status(&key),
                Some(AgentStatus::Working),
                "PostToolUse said working"
            );
        }

        #[test]
        fn a_pr_agent_opening_a_pr_links_nothing() {
            let app = TestApp::new();
            let (id, _) = app.pr_with_worktree(10, "feature/x");
            app.start_agent(id);
            let session = app.state.store.item(id).unwrap().session_id.unwrap();
            app.add_pr(json!({"number": 31, "title": "Other", "head_ref": "main"}));
            let links = record(
                &app.state,
                &hook(
                    &session,
                    "PostToolUse",
                    json!({
                        "tool_name": "Bash",
                        "tool_input": {"command": "gh pr create --fill"},
                        "tool_response": {"stdout": "https://github.com/owner/proj/pull/31\n"}
                    }),
                ),
            );
            assert_eq!(links, None, "only a task's agent links what it opens");
        }
    }
}
