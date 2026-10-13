//! One embedded terminal per work item, running the user's `claude` TUI unmodified.
//!
//! Output is kept in a bounded scrollback buffer so the UI can detach (switch work
//! items) and reattach without losing anything, and streamed live to the attached view.
//! Agent status comes from hooks (see `hooks.rs`) and is tracked here per terminal.
//! Terminals nobody looks at and whose agent is idle are closed after a while; the
//! conversation is resumed when the item is opened again.

use crate::hooks::{AgentStatus, emit_status};
use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};
use serde::Serialize;
use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
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
    /// Close detached, idle terminals after this long.
    pub idle_after: Duration,
}

/// How to start `claude` for a work item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionPlan {
    pub session_id: String,
    /// Resume the existing conversation instead of starting one with this id.
    pub resume: bool,
    /// Appended to the system prompt (the work item's context), if any.
    pub context_file: Option<PathBuf>,
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
    /// Latest status and when it was reported.
    status: Mutex<(AgentStatus, Instant)>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Opened {
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

/// Arguments for `claude`: resume the item's conversation or start one under a session
/// id we choose (so hook events can be routed to the item).
fn claude_args(launch: &Launch, plan: &SessionPlan) -> Vec<String> {
    let mut args = vec![
        // Inherit the user's config, but none of their MCP servers.
        "--strict-mcp-config".to_owned(),
        // `--resume` does not restore `--settings`, so pass the hooks every time.
        "--settings".to_owned(),
        launch.hooks_settings.to_string_lossy().into_owned(),
    ];
    args.push(
        if plan.resume {
            "--resume"
        } else {
            "--session-id"
        }
        .to_owned(),
    );
    args.push(plan.session_id.clone());
    if let Some(file) = &plan.context_file {
        args.push("--append-system-prompt-file".to_owned());
        args.push(file.to_string_lossy().into_owned());
    }
    args.extend(launch.extra_args.iter().cloned());
    args
}

/// Whether a session can be resumed: it was prompted (Claude Code saves a transcript
/// only then) and its transcript, if known, still exists.
pub fn resumable(started: bool, transcript: Option<&Path>) -> bool {
    started && transcript.is_none_or(Path::exists)
}

/// Statuses in which the agent waits for the user, so an unwatched terminal may close.
fn idle(status: AgentStatus) -> bool {
    matches!(
        status,
        AgentStatus::Ready | AgentStatus::Finished | AgentStatus::Error
    )
}

impl TerminalManager {
    pub fn new(app: AppHandle, launch: Result<Launch, String>) -> Arc<Self> {
        let manager = Arc::new(Self {
            app,
            launch,
            terms: Mutex::default(),
        });
        if let Ok(idle_after) = manager.launch.as_ref().map(|l| l.idle_after) {
            let weak = Arc::downgrade(&manager);
            let _ = std::thread::Builder::new()
                .name("terminal-reaper".into())
                .spawn(move || {
                    while let Some(manager) = weak.upgrade() {
                        manager.close_idle(idle_after);
                        drop(manager);
                        std::thread::sleep(Duration::from_secs(30).min(idle_after));
                    }
                });
        }
        manager
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

    /// The agent's status, or `None` when no terminal runs for the item.
    pub fn status(&self, key: &str) -> Option<AgentStatus> {
        self.get(key).ok().map(|t| t.status.lock().unwrap().0)
    }

    /// Records and broadcasts a status change for a running terminal.
    pub fn report(&self, key: &str, status: AgentStatus, detail: Option<String>) {
        let Ok(term) = self.get(key) else { return };
        let previous =
            std::mem::replace(&mut *term.status.lock().unwrap(), (status, Instant::now())).0;
        emit_status(&self.app, key, status, detail);
        if previous != status {
            crate::notify::agent_status(&self.app, key, previous, status);
        }
    }

    /// Attaches `view` to the item's terminal, starting `claude` in `cwd` if it isn't
    /// running (`plan` is only evaluated then). The scrollback is replayed into `view`
    /// before live output continues.
    pub fn open(
        &self,
        key: &str,
        cwd: &Path,
        plan: impl FnOnce() -> Result<SessionPlan, String>,
        size: PtySize,
        view: Channel<InvokeResponseBody>,
    ) -> Result<Opened, String> {
        if let Some(term) = self.terms.lock().unwrap().get(key).cloned() {
            let mut out = term.output.lock().unwrap();
            let replay: Vec<u8> = out.scrollback.iter().copied().collect();
            view.send(InvokeResponseBody::Raw(replay))
                .map_err(|e| e.to_string())?;
            out.view = Some(view);
            drop(out);
            let _ = term.master.lock().unwrap().resize(size);
            return Ok(Opened {
                session_id: term.session_id.clone(),
                resumed: false,
                reattached: true,
            });
        }

        let launch = self.launch.clone()?;
        let plan = plan()?;
        let pair = native_pty_system()
            .openpty(size)
            .map_err(|e| e.to_string())?;
        let mut cmd = CommandBuilder::new(&launch.claude);
        cmd.args(claude_args(&launch, &plan));
        cmd.cwd(cwd);
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        for var in PARENT_SESSION_ENV {
            cmd.env_remove(var);
        }
        let mut child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let term = Arc::new(Term {
            session_id: plan.session_id.clone(),
            writer: Mutex::new(pair.master.take_writer().map_err(|e| e.to_string())?),
            killer: Mutex::new(child.clone_killer()),
            master: Mutex::new(pair.master),
            output: Mutex::new(Output {
                scrollback: VecDeque::new(),
                view: Some(view),
            }),
            status: Mutex::new((AgentStatus::Starting, Instant::now())),
        });
        // Registered before output starts, so the first hook finds the item.
        self.terms
            .lock()
            .unwrap()
            .insert(key.to_owned(), term.clone());
        emit_status(&self.app, key, AgentStatus::Starting, None);

        let (reader_term, reader_app, reader_key) =
            (term.clone(), self.app.clone(), key.to_owned());
        std::thread::Builder::new()
            .name(format!("pty-read {key}"))
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
                        use tauri::Manager as _;
                        reader_app.state::<crate::AppState>().terminals.report(
                            &reader_key,
                            AgentStatus::Ready,
                            None,
                        );
                    }
                }
            })
            .map_err(|e| e.to_string())?;

        let (app, wait_key, wait_term) = (self.app.clone(), key.to_owned(), term.clone());
        std::thread::Builder::new()
            .name(format!("pty-wait {key}"))
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
            session_id: plan.session_id,
            resumed: plan.resume,
            reattached: false,
        })
    }

    /// Stops streaming to view `view` (a channel id); the terminal keeps running and
    /// buffering. A newer view that attached in the meantime stays attached.
    pub fn detach(&self, key: &str, view: u32) {
        if let Ok(term) = self.get(key) {
            let mut out = term.output.lock().unwrap();
            if out.view.as_ref().is_some_and(|v| v.id() == view) {
                out.view = None;
            }
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

    /// Closes terminals that nobody is viewing and whose agent has waited for the user
    /// for longer than `after`.
    fn close_idle(&self, after: Duration) {
        let idle_keys: Vec<String> = self
            .terms
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, t)| {
                let (status, since) = *t.status.lock().unwrap();
                idle(status) && since.elapsed() >= after && t.output.lock().unwrap().view.is_none()
            })
            .map(|(k, _)| k.clone())
            .collect();
        for key in idle_keys {
            tracing::info!("closing idle terminal {key}");
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
            idle_after: Duration::from_secs(900),
        }
    }

    #[test]
    fn new_sessions_get_our_session_id_and_resumed_ones_resume() {
        let mut plan = SessionPlan {
            session_id: "id-1".into(),
            resume: false,
            context_file: Some("/ctx.md".into()),
        };
        let fresh = claude_args(&launch(), &plan);
        assert_eq!(
            fresh,
            [
                "--strict-mcp-config",
                "--settings",
                "/data/hooks.json",
                "--session-id",
                "id-1",
                "--append-system-prompt-file",
                "/ctx.md"
            ]
        );
        plan.resume = true;
        plan.context_file = None;
        assert_eq!(&claude_args(&launch(), &plan)[3..], ["--resume", "id-1"]);
    }

    #[test]
    fn only_started_sessions_with_a_transcript_are_resumable() {
        let dir = tempfile::tempdir().unwrap();
        let transcript = dir.path().join("t.jsonl");
        assert!(!resumable(false, None), "never prompted: nothing to resume");
        assert!(
            resumable(true, None),
            "started, transcript path not known yet"
        );
        assert!(!resumable(true, Some(&transcript)), "transcript deleted");
        std::fs::write(&transcript, "{}").unwrap();
        assert!(resumable(true, Some(&transcript)));
    }

    #[test]
    fn only_waiting_agents_count_as_idle() {
        assert!(idle(AgentStatus::Finished) && idle(AgentStatus::Ready));
        assert!(!idle(AgentStatus::Working) && !idle(AgentStatus::WaitingForPermission));
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
