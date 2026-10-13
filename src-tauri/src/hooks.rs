//! Agent status from Claude Code hooks.
//!
//! Every `claude` that Ariekei starts gets `--settings <claude-hooks.json>`, which adds
//! `http` hooks (merged with the user's and the project's own hooks) that POST each event
//! to a localhost endpoint. Events carry the session id, which identifies the work item.

use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager as _};

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

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum AgentEvent {
    Status {
        key: String,
        status: AgentStatus,
        detail: Option<String>,
    },
}

pub fn emit_status(app: &AppHandle, key: &str, status: AgentStatus, detail: Option<String>) {
    let event = AgentEvent::Status {
        key: key.to_owned(),
        status,
        detail,
    };
    if let Err(e) = app.emit("agent-event", &event) {
        tracing::warn!("failed to emit agent event: {e}");
    }
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
    let tool = || event.tool_name.clone();
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
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
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
    if !accepted {
        return Ok(None);
    }
    Ok(serde_json::from_slice(&body).ok())
}

/// Routes a hook event to its work item and updates the item's status.
pub fn dispatch(app: &AppHandle, event: HookEvent) {
    // Events can arrive before the app state exists (startup race): drop those.
    let Some(state) = app.try_state::<crate::AppState>() else {
        return;
    };
    let Some(key) = state.terminals.key_for_session(&event.session_id) else {
        // Hooks of sessions Ariekei did not start (or already closed) are ignored.
        return;
    };
    if event.hook_event_name == "UserPromptSubmit"
        && let Ok(id) = key.parse()
        && let Err(e) = state
            .store
            .mark_session_started(id, event.transcript_path.as_deref())
    {
        tracing::warn!("failed to record session start: {e}");
    }
    if let Some((status, detail)) = status_for(&event) {
        state.terminals.report(&key, status, detail);
    }
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
        write!(s, "POST {path} HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
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
}
