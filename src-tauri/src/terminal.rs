//! One embedded terminal per work item, running the user's `claude` TUI unmodified.
//!
//! Output is kept in a bounded scrollback buffer so the UI can detach (switch work
//! items) and reattach without losing anything, and streamed live to the attached view.
//! Agent status does not come from here but from hooks (see `hooks.rs`).

use crate::hooks::{AgentStatus, emit_status};
use crate::sessions::{Session, SessionStore};
use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};
use serde::Serialize;
use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::AppHandle;
use tauri::ipc::{Channel, InvokeResponseBody};

const SCROLLBACK_BYTES: usize = 2 << 20;

/// Markers a running Claude Code puts into its children's environment. If Ariekei was
/// started from inside a Claude Code session, they would make our `claude` think it is a
/// nested child session (which, for example, turns transcript saving off).
const PARENT_SESSION_ENV: [&str; 10] = [
    "CLAUDECODE",
    "CLAUDE_CODE_CHILD_SESSION",
    "CLAUDE_CODE_SESSION_ID",
    "CLAUDE_CODE_SESSION_ATTENDED",
    "CLAUDE_CODE_ENTRYPOINT",
    "CLAUDE_CODE_EXECPATH",
    "CLAUDE_CODE_MESSAGING_SOCKET",
    "CLAUDE_CODE_MESSAGING_TOKEN",
    "CLAUDE_PID",
    "CLAUDE_EFFORT",
];

#[derive(Debug, Clone)]
pub struct Launch {
    pub claude: PathBuf,
    pub hooks_settings: PathBuf,
    /// Extra CLI arguments, for tests only (`ARIEKEI_CLAUDE_ARGS`).
    pub extra_args: Vec<String>,
}

#[derive(Default)]
struct Output {
    scrollback: VecDeque<u8>,
    view: Option<Channel<InvokeResponseBody>>,
}

impl Output {
    fn push(&mut self, bytes: &[u8]) {
        self.scrollback.extend(bytes);
        let excess = self.scrollback.len().saturating_sub(SCROLLBACK_BYTES);
        self.scrollback.drain(..excess);
        if let Some(view) = &self.view
            && view.send(InvokeResponseBody::Raw(bytes.to_vec())).is_err()
        {
            self.view = None;
        }
    }
}

struct Term {
    session_id: String,
    master: Mutex<Box<dyn MasterPty + Send>>,
    writer: Mutex<Box<dyn Write + Send>>,
    killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
    output: Mutex<Output>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Opened {
    pub key: String,
    pub session_id: String,
    /// True when an existing conversation was resumed rather than started.
    pub resumed: bool,
    /// True when the terminal was already running and the view just reattached.
    pub reattached: bool,
}

pub struct TerminalManager {
    app: AppHandle,
    /// Why terminals cannot start (e.g. `claude` not found), reported on open.
    launch: Result<Launch, String>,
    terms: Mutex<HashMap<String, Arc<Term>>>,
}

/// Canonical form of a worktree path, used as the work item key.
pub fn worktree_key(path: &str) -> Result<String, String> {
    let dir = std::fs::canonicalize(path).map_err(|e| format!("{path}: {e}"))?;
    if !dir.join(".git").exists() {
        return Err(format!("{} is not a git worktree", dir.display()));
    }
    Ok(dir.to_string_lossy().into_owned())
}

/// Arguments for `claude`: resume the item's conversation if it has one, else start a
/// new one under a session id we choose (so hook events can be routed to the item).
fn claude_args(launch: &Launch, session: &Session, resume: bool) -> Vec<String> {
    let mut args = vec![
        // Inherit the user's config, but none of their MCP servers.
        "--strict-mcp-config".to_owned(),
        // `--resume` does not restore `--settings`, so pass the hooks every time.
        "--settings".to_owned(),
        launch.hooks_settings.to_string_lossy().into_owned(),
    ];
    args.push(if resume { "--resume" } else { "--session-id" }.to_owned());
    args.push(session.session_id.clone());
    args.extend(launch.extra_args.iter().cloned());
    args
}

/// Whether a stored session has a conversation that `--resume` can load.
fn resumable(session: &Session) -> bool {
    session.started && session.transcript_path.as_deref().is_none_or(Path::exists)
}

impl TerminalManager {
    pub fn new(app: AppHandle, launch: Result<Launch, String>) -> Self {
        Self {
            app,
            launch,
            terms: Mutex::default(),
        }
    }

    pub fn key_for_session(&self, session_id: &str) -> Option<String> {
        let terms = self.terms.lock().unwrap();
        terms
            .iter()
            .find(|(_, t)| t.session_id == session_id)
            .map(|(k, _)| k.clone())
    }

    fn get(&self, key: &str) -> Result<Arc<Term>, String> {
        self.terms
            .lock()
            .unwrap()
            .get(key)
            .cloned()
            .ok_or_else(|| format!("no terminal for {key}"))
    }

    /// Attaches `view` to the item's terminal, starting `claude` if it isn't running.
    /// The scrollback is replayed into `view` before live output continues.
    pub fn open(
        &self,
        sessions: &SessionStore,
        key: String,
        size: PtySize,
        view: Channel<InvokeResponseBody>,
    ) -> Result<Opened, String> {
        if let Some(term) = self.terms.lock().unwrap().get(&key).cloned() {
            let mut out = term.output.lock().unwrap();
            let replay: Vec<u8> = out.scrollback.iter().copied().collect();
            view.send(InvokeResponseBody::Raw(replay))
                .map_err(|e| e.to_string())?;
            out.view = Some(view);
            drop(out);
            let _ = term.master.lock().unwrap().resize(size);
            return Ok(Opened {
                key,
                session_id: term.session_id.clone(),
                resumed: false,
                reattached: true,
            });
        }

        let launch = self.launch.clone()?;
        let stored = sessions.get(&key);
        let resume = stored.as_ref().is_some_and(resumable);
        let session = match stored {
            Some(s) if resume || !s.started => s,
            // Started but its transcript is gone: begin a fresh conversation.
            _ => Session::new(uuid::Uuid::new_v4().to_string()),
        };
        sessions.put(&key, &session)?;

        let pair = native_pty_system()
            .openpty(size)
            .map_err(|e| e.to_string())?;
        let mut cmd = CommandBuilder::new(&launch.claude);
        cmd.args(claude_args(&launch, &session, resume));
        cmd.cwd(&key);
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        for var in PARENT_SESSION_ENV {
            cmd.env_remove(var);
        }
        let mut child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let term = Arc::new(Term {
            session_id: session.session_id.clone(),
            writer: Mutex::new(pair.master.take_writer().map_err(|e| e.to_string())?),
            killer: Mutex::new(child.clone_killer()),
            master: Mutex::new(pair.master),
            output: Mutex::new(Output {
                scrollback: VecDeque::new(),
                view: Some(view),
            }),
        });
        // Registered before output starts, so the first hook (SessionStart) finds the item.
        self.terms.lock().unwrap().insert(key.clone(), term.clone());
        emit_status(&self.app, &key, AgentStatus::Starting, None);

        let (reader_term, reader_app, reader_key) = (term.clone(), self.app.clone(), key.clone());
        std::thread::Builder::new()
            .name(format!("pty-read {}", session.session_id))
            .spawn(move || {
                let mut buf = [0u8; 16 * 1024];
                let mut first = true;
                while let Ok(n) = reader.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    reader_term.output.lock().unwrap().push(&buf[..n]);
                    if std::mem::take(&mut first) {
                        // The TUI is up; hooks report everything after this.
                        emit_status(&reader_app, &reader_key, AgentStatus::Ready, None);
                    }
                }
            })
            .map_err(|e| e.to_string())?;

        let (app, wait_key, wait_term) = (self.app.clone(), key.clone(), term.clone());
        std::thread::Builder::new()
            .name(format!("pty-wait {}", session.session_id))
            .spawn(move || {
                let status = child.wait();
                use tauri::Manager as _;
                let state = app.state::<crate::AppState>();
                let mut terms = state.terminals.terms.lock().unwrap();
                // Only forget the entry if it is still this process (not a restarted one).
                if terms
                    .get(&wait_key)
                    .is_some_and(|t| Arc::ptr_eq(t, &wait_term))
                {
                    terms.remove(&wait_key);
                }
                drop(terms);
                let detail = status
                    .ok()
                    .filter(|s| !s.success())
                    .map(|s| format!("exit {}", s.exit_code()));
                emit_status(&app, &wait_key, AgentStatus::Stopped, detail);
            })
            .map_err(|e| e.to_string())?;

        Ok(Opened {
            key,
            session_id: session.session_id,
            resumed: resume,
            reattached: false,
        })
    }

    /// Stops streaming to the current view; the terminal keeps running and buffering.
    pub fn detach(&self, key: &str) {
        if let Ok(term) = self.get(key) {
            term.output.lock().unwrap().view = None;
        }
    }

    pub fn write(&self, key: &str, data: &[u8]) -> Result<(), String> {
        let term = self.get(key)?;
        let mut writer = term.writer.lock().unwrap();
        writer
            .write_all(data)
            .and_then(|_| writer.flush())
            .map_err(|e| e.to_string())
    }

    pub fn resize(&self, key: &str, size: PtySize) -> Result<(), String> {
        self.get(key)?
            .master
            .lock()
            .unwrap()
            .resize(size)
            .map_err(|e| e.to_string())
    }

    /// Types `text` into the agent's prompt as one paste and submits it.
    pub fn send_to_agent(&self, key: &str, text: &str) -> Result<(), String> {
        self.write(key, format!("\x1b[200~{text}\x1b[201~").as_bytes())?;
        // Give the TUI a moment to finish handling the paste before Enter arrives.
        std::thread::sleep(Duration::from_millis(80));
        self.write(key, b"\r")
    }

    /// Ends the item's `claude` process; its conversation stays resumable.
    pub fn close(&self, key: &str) {
        let term = self.terms.lock().unwrap().remove(key);
        if let Some(term) = term {
            let _ = term.killer.lock().unwrap().kill();
        }
    }

    /// Ends every terminal, e.g. when the app quits.
    pub fn close_all(&self) {
        let keys: Vec<String> = self.terms.lock().unwrap().keys().cloned().collect();
        for key in keys {
            self.close(&key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn launch() -> Launch {
        Launch {
            claude: "claude".into(),
            hooks_settings: "/data/hooks.json".into(),
            extra_args: vec![],
        }
    }

    #[test]
    fn new_sessions_get_our_session_id_and_resumed_ones_resume() {
        let s = Session::new("id-1".into());
        let fresh = claude_args(&launch(), &s, false);
        assert_eq!(
            fresh,
            [
                "--strict-mcp-config",
                "--settings",
                "/data/hooks.json",
                "--session-id",
                "id-1"
            ]
        );
        let resumed = claude_args(&launch(), &s, true);
        assert_eq!(&resumed[3..], ["--resume", "id-1"]);
    }

    #[test]
    fn only_started_sessions_with_a_transcript_are_resumable() {
        let dir = tempfile::tempdir().unwrap();
        let transcript = dir.path().join("t.jsonl");
        let mut s = Session::new("id".into());
        assert!(!resumable(&s), "never prompted: nothing to resume");
        s.started = true;
        assert!(resumable(&s), "started, transcript path not known yet");
        s.transcript_path = Some(transcript.clone());
        assert!(!resumable(&s), "transcript deleted");
        std::fs::write(&transcript, "{}").unwrap();
        assert!(resumable(&s));
    }

    #[test]
    fn scrollback_is_bounded() {
        let mut out = Output::default();
        out.push(&vec![b'a'; SCROLLBACK_BYTES]);
        out.push(b"tail");
        assert_eq!(out.scrollback.len(), SCROLLBACK_BYTES);
        assert!(out.scrollback.iter().rev().take(4).eq(b"liat".iter()));
    }
}
