// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

//! One embedded terminal per work item, running the user's `claude` TUI unmodified.
//!
//! Output is kept in a bounded scrollback buffer so the UI can detach (switch work
//! items) and reattach without losing anything, and streamed live to the attached view.
//! Agent status comes from hooks (see `hooks.rs`) and is tracked here per terminal.
//! Terminals nobody looks at and whose agent is idle are closed after a while; the
//! conversation is resumed when the item is opened again.

use crate::events::Events;
use crate::hooks::AgentStatus;
use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};
use serde::Serialize;
use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::time::{Duration, Instant};
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
    /// A working agent whose terminal has been silent this long is not working (see
    /// `settle_quiet`).
    pub quiet_after: Duration,
    /// Claude Code's directory, where it keeps transcripts (`claude::config_dir`).
    pub claude_dir: Option<PathBuf>,
    /// Extra environment, for tests only (where the fake agent keeps its transcripts).
    pub env: Vec<(String, String)>,
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
    /// Names the terminal in its hook events (`hooks::TERMINAL_ENV`).
    token: String,
    /// The conversation it is in: the one it started with, until `/clear` or `/resume`.
    session_id: Mutex<String>,
    /// Why its last conversation ended (SessionEnd), until the next one shows up.
    ended: Mutex<Option<String>>,
    master: Mutex<Box<dyn MasterPty + Send>>,
    writer: Mutex<Box<dyn Write + Send>>,
    killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
    /// The agent's process, whose own processes (background shells) keep it open.
    pid: Option<u32>,
    output: Mutex<Output>,
    /// Latest status and when it was reported.
    status: Mutex<(AgentStatus, Instant)>,
    /// What the latest status is about (e.g. the command it asks to run).
    detail: Mutex<Option<String>>,
    /// When the agent last wrote to the terminal.
    last_output: Mutex<Instant>,
    /// The agent finished a turn since the user last opened the item's review.
    unreviewed: AtomicBool,
    /// How many prompts the agent took (UserPromptSubmit), and a signal for each.
    prompts: (Mutex<u64>, Condvar),
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
    events: Arc<dyn Events>,
    /// This manager, for the threads of its terminals.
    me: Weak<TerminalManager>,
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

/// Whether keys typed at a permission prompt answer it: Enter, Esc, Ctrl+C or an option's
/// number. Not arrow keys (they choose), nor what the terminal itself reports (focus,
/// mouse, cursor position): escape sequences, which never hold a CR or Ctrl+C byte.
fn answers_prompt(data: &[u8]) -> bool {
    data == b"\x1b"
        || matches!(data, [d] if d.is_ascii_digit())
        || data.iter().any(|b| matches!(b, b'\r' | 0x03))
}

/// Statuses in which the agent waits for the user, so an unwatched terminal may close.
fn idle(status: AgentStatus) -> bool {
    matches!(
        status,
        AgentStatus::Ready | AgentStatus::Finished | AgentStatus::Error
    )
}

impl TerminalManager {
    pub fn new(events: Arc<dyn Events>, launch: Result<Launch, String>) -> Arc<Self> {
        let manager = Arc::new_cyclic(|me| Self {
            events,
            me: me.clone(),
            launch,
            terms: Mutex::default(),
        });
        if let Ok(launch) = manager.launch.as_ref() {
            let (idle_after, quiet_after) = (launch.idle_after, launch.quiet_after);
            let weak = Arc::downgrade(&manager);
            let _ = std::thread::Builder::new()
                .name("terminal-reaper".into())
                .spawn(move || {
                    let mut idle_checked = Instant::now();
                    while let Some(manager) = weak.upgrade() {
                        manager.settle_quiet(quiet_after);
                        if idle_checked.elapsed() >= Duration::from_secs(30).min(idle_after) {
                            manager.close_idle(idle_after);
                            idle_checked = Instant::now();
                        }
                        drop(manager);
                        std::thread::sleep(Duration::from_secs(1));
                    }
                });
        }
        manager
    }

    /// The user typed `data` into the item's terminal. Keys that answer a permission prompt
    /// (or a question) mean the agent goes on: running what it asked for (until PostToolUse,
    /// which for a long command is long after), or, if the answer was No, ending its turn,
    /// which Claude Code reports with no hook at all (`settle_quiet` tells).
    pub fn typed(&self, key: &str, data: &[u8]) {
        let Ok(term) = self.get(key) else { return };
        if term.status.lock().unwrap().0 == AgentStatus::WaitingForPermission
            && answers_prompt(data)
        {
            let detail = term.detail.lock().unwrap().clone();
            self.report(key, AgentStatus::Working, detail);
        }
    }

    /// Agents that seem to work but whose terminal has been silent for `after` are not
    /// working: while it works, Claude Code redraws its spinner and timer all the time. A turn
    /// that the user interrupted (Esc, or No at a permission prompt) ends with no hook, so
    /// this is how its end shows.
    fn settle_quiet(&self, after: Duration) {
        let quiet: Vec<String> = self
            .terms
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, t)| {
                let (status, since) = *t.status.lock().unwrap();
                status == AgentStatus::Working
                    && since.elapsed() >= after
                    && t.last_output.lock().unwrap().elapsed() >= after
            })
            .map(|(k, _)| k.clone())
            .collect();
        for key in quiet {
            tracing::info!("agent {key} is quiet: its turn ended");
            self.report(&key, AgentStatus::Ready, None);
        }
    }

    /// The item whose terminal sent a hook event: the one `token` names, or (when the event
    /// does not name one) the one in conversation `session_id`.
    pub fn key_for(&self, token: Option<&str>, session_id: &str) -> Option<String> {
        let terms = self.terms.lock().unwrap();
        terms
            .iter()
            .find(|(_, t)| match token {
                Some(token) => t.token == token,
                None => *t.session_id.lock().unwrap() == session_id,
            })
            .map(|(k, _)| k.clone())
    }

    /// Notes that the item's terminal is in conversation `session_id`. When that is a new one
    /// (after `/clear` or `/resume`), returns why the one before ended, if known.
    pub fn follow(&self, key: &str, session_id: &str) -> Option<Option<String>> {
        let term = self.get(key).ok()?;
        let mut current = term.session_id.lock().unwrap();
        if *current == session_id {
            return None;
        }
        tracing::info!("agent {key} went on in session {session_id}");
        *current = session_id.to_owned();
        Some(term.ended.lock().unwrap().take())
    }

    /// The transcript Claude Code saved for `session_id`, if any (`claude::find_transcript`).
    pub fn find_transcript(&self, session_id: &str) -> Option<PathBuf> {
        let dir = self.launch.as_ref().ok()?.claude_dir.as_deref()?;
        crate::claude::find_transcript(dir, session_id)
    }

    /// The token that names the item's terminal in its hook events.
    #[cfg(test)]
    pub fn token(&self, key: &str) -> Option<String> {
        self.get(key).ok().map(|t| t.token.clone())
    }

    /// The conversation in the item's terminal ended, for `reason` (SessionEnd).
    pub fn session_ended(&self, key: &str, reason: Option<String>) {
        if let Ok(term) = self.get(key) {
            *term.ended.lock().unwrap() = reason;
        }
    }

    fn get(&self, key: &str) -> Result<Arc<Term>, String> {
        self.terms
            .lock()
            .unwrap()
            .get(key)
            .cloned()
            .ok_or_else(|| "the agent is not running; start it in its Terminal".to_owned())
    }

    /// Every running agent: its item key, process and status.
    pub fn agents(&self) -> Vec<(String, Option<u32>, AgentStatus)> {
        self.terms
            .lock()
            .unwrap()
            .iter()
            .map(|(k, t)| (k.clone(), t.pid, t.status.lock().unwrap().0))
            .collect()
    }

    /// The agent's status, or `None` when no terminal runs for the item.
    pub fn status(&self, key: &str) -> Option<AgentStatus> {
        self.get(key).ok().map(|t| t.status.lock().unwrap().0)
    }

    /// Records and broadcasts a status change for a running terminal; returns the status
    /// before, or `None` when no terminal runs for the item. (`AppState::report` also tells
    /// the user about the change.)
    pub fn report(
        &self,
        key: &str,
        status: AgentStatus,
        detail: Option<String>,
    ) -> Option<AgentStatus> {
        let term = self.get(key).ok()?;
        let previous =
            std::mem::replace(&mut *term.status.lock().unwrap(), (status, Instant::now())).0;
        term.detail.lock().unwrap().clone_from(&detail);
        if status == AgentStatus::Finished && previous != AgentStatus::Finished {
            term.unreviewed.store(true, Ordering::Relaxed);
        }
        self.events.agent_status(key, status, detail);
        Some(previous)
    }

    /// The agent finished and the user has not opened the item's review since.
    pub fn ready_for_review(&self, key: &str) -> bool {
        self.get(key).is_ok_and(|t| {
            t.unreviewed.load(Ordering::Relaxed)
                && t.status.lock().unwrap().0 == AgentStatus::Finished
        })
    }

    /// The user opened the item's review: what the agent did so far is seen.
    pub fn review_seen(&self, key: &str) {
        if let Ok(term) = self.get(key) {
            term.unreviewed.store(false, Ordering::Relaxed);
        }
    }

    /// Attaches `view` to the item's terminal, starting `claude` in `cwd` if it isn't
    /// running (`plan` is only evaluated then). The scrollback is replayed into `view`
    /// before live output continues. `label` names the item in a notice if its agent crashes.
    pub fn open(
        &self,
        key: &str,
        label: &str,
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
                session_id: term.session_id.lock().unwrap().clone(),
                resumed: false,
                reattached: true,
            });
        }

        let launch = self.launch.clone()?;
        let plan = plan()?;
        let pair = native_pty_system()
            .openpty(size)
            .map_err(|e| e.to_string())?;
        let (program, prefix) = crate::tools::program(&launch.claude);
        let mut cmd = CommandBuilder::new(program);
        cmd.args(prefix);
        cmd.args(claude_args(&launch, &plan));
        cmd.cwd(cwd);
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        for (k, v) in &launch.env {
            cmd.env(k, v);
        }
        for var in PARENT_SESSION_ENV {
            cmd.env_remove(var);
        }
        let token = uuid::Uuid::new_v4().simple().to_string();
        cmd.env(crate::hooks::TERMINAL_ENV, &token);
        let mut child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
        drop(pair.slave);
        tracing::info!(
            "agent {key} started (pid {:?}, {})",
            child.process_id(),
            if plan.resume {
                "resuming"
            } else {
                "new session"
            }
        );

        let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let term = Arc::new(Term {
            token,
            session_id: Mutex::new(plan.session_id.clone()),
            ended: Mutex::default(),
            writer: Mutex::new(pair.master.take_writer().map_err(|e| e.to_string())?),
            killer: Mutex::new(child.clone_killer()),
            pid: child.process_id(),
            master: Mutex::new(pair.master),
            output: Mutex::new(Output {
                scrollback: VecDeque::new(),
                view: Some(view),
            }),
            status: Mutex::new((AgentStatus::Starting, Instant::now())),
            detail: Mutex::default(),
            last_output: Mutex::new(Instant::now()),
            unreviewed: AtomicBool::new(false),
            prompts: Default::default(),
        });
        // Registered before output starts, so the first hook finds the item.
        self.terms
            .lock()
            .unwrap()
            .insert(key.to_owned(), term.clone());
        self.events.agent_status(key, AgentStatus::Starting, None);

        let (reader_term, reader_manager, reader_key) =
            (term.clone(), self.me.clone(), key.to_owned());
        std::thread::Builder::new()
            .name(format!("pty-read {key}"))
            .spawn(move || {
                let mut buf = [0u8; 16 * 1024];
                let mut first = true;
                let mut total = 0usize;
                while let Ok(n) = reader.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    total += n;
                    reader_term.output.lock().unwrap().push(&buf[..n]);
                    *reader_term.last_output.lock().unwrap() = Instant::now();
                    if std::mem::take(&mut first)
                        && let Some(manager) = reader_manager.upgrade()
                    {
                        // The TUI is up; hooks report everything after this.
                        manager.report(&reader_key, AgentStatus::Ready, None);
                    }
                }
                tracing::info!("agent {reader_key} output ended after {total} bytes");
            })
            .map_err(|e| e.to_string())?;

        let (wait_manager, wait_key, wait_term) = (self.me.clone(), key.to_owned(), term.clone());
        let label = label.to_owned();
        std::thread::Builder::new()
            .name(format!("pty-wait {key}"))
            .spawn(move || {
                let status = child.wait();
                tracing::info!("agent {wait_key} exited: {status:?}");
                let Some(manager) = wait_manager.upgrade() else {
                    return;
                };
                let mut terms = manager.terms.lock().unwrap();
                // Only forget the entry if it is still this process (not a restarted one, nor
                // one Ariekei closed: then it did not end by itself).
                let by_itself = terms
                    .get(&wait_key)
                    .is_some_and(|t| Arc::ptr_eq(t, &wait_term));
                if by_itself {
                    terms.remove(&wait_key);
                }
                drop(terms);
                let detail = status
                    .ok()
                    .filter(|s| !s.success())
                    .map(|s| format!("exit {}", s.exit_code()));
                manager
                    .events
                    .agent_status(&wait_key, AgentStatus::Stopped, detail.clone());
                if by_itself && let Some(detail) = detail {
                    crate::notify::agent_exited(&*manager.events, &wait_key, &label, &detail);
                }
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

    /// What the terminal of `key` showed so far (its scrollback), as text.
    #[cfg(test)]
    pub fn scrollback(&self, key: &str) -> String {
        self.get(key)
            .map(|t| {
                let out = t.output.lock().unwrap();
                String::from_utf8_lossy(&out.scrollback.iter().copied().collect::<Vec<u8>>())
                    .into_owned()
            })
            .unwrap_or_default()
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

    /// Types `text` into the agent's prompt as one paste and submits it. Unless the agent is
    /// in a turn (then Claude Code queues it for later), it must take the message within
    /// `wait` (UserPromptSubmit), or this fails, so what was sent can be kept: at a dialog
    /// (the trust prompt, a question) or in a menu Enter does something else instead.
    pub fn send_to_agent(&self, key: &str, text: &str, wait: Duration) -> Result<(), String> {
        let term = self.get(key)?;
        let status = term.status.lock().unwrap().0;
        if status == AgentStatus::WaitingForPermission {
            return Err("the agent is waiting for your answer; answer it in its Terminal".into());
        }
        let before = *term.prompts.0.lock().unwrap();
        self.write(key, format!("\x1b[200~{text}\x1b[201~").as_bytes())?;
        // Give the TUI a moment to finish handling the paste before Enter arrives.
        std::thread::sleep(Duration::from_millis(80));
        self.write(key, b"\r")?;
        if status == AgentStatus::Working {
            return Ok(());
        }
        let (prompts, taken) = &term.prompts;
        let (prompts, _) = taken
            .wait_timeout_while(prompts.lock().unwrap(), wait, |n| *n == before)
            .unwrap();
        if *prompts == before {
            return Err(
                "the agent did not take it; see its Terminal (a dialog may be open there)".into(),
            );
        }
        Ok(())
    }

    /// The agent in the item's terminal took a prompt (UserPromptSubmit).
    pub fn prompt_submitted(&self, key: &str) {
        if let Ok(term) = self.get(key) {
            *term.prompts.0.lock().unwrap() += 1;
            term.prompts.1.notify_all();
        }
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
    /// for longer than `after`, unless the agent still runs processes of its own (e.g.
    /// background shells): they would end with it, and resuming does not restart them.
    fn close_idle(&self, after: Duration) {
        let idle: Vec<(String, Option<u32>)> = self
            .terms
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, t)| {
                let (status, since) = *t.status.lock().unwrap();
                idle(status) && since.elapsed() >= after && t.output.lock().unwrap().view.is_none()
            })
            .map(|(k, t)| (k.clone(), t.pid))
            .collect();
        if idle.is_empty() {
            return;
        }
        let Ok(table) = crate::procs::table()
            .inspect_err(|e| tracing::warn!("idle terminals kept open: no process list: {e}"))
        else {
            return;
        };
        for (key, pid) in idle {
            let running = pid.map_or(vec![], |pid| crate::procs::descendants(&table, pid));
            if running.is_empty() {
                tracing::info!("closing idle terminal {key}");
                self.close(&key);
            } else {
                let names: Vec<String> = running
                    .iter()
                    .map(|p| format!("{} ({})", p.command, p.pid))
                    .collect();
                tracing::debug!(
                    "idle terminal {key} kept open, running {}",
                    names.join(", ")
                );
            }
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
            quiet_after: Duration::from_secs(30),
            claude_dir: None,
            env: vec![],
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
    fn answering_a_prompt_takes_enter_esc_ctrl_c_or_a_number() {
        for answer in [&b"\r"[..], b"\x1b", b"\x03", b"2", b"\x1b[B\r"] {
            assert!(answers_prompt(answer), "{answer:?}");
        }
        // Arrow keys, focus and mouse reports, a cursor position report, typing.
        for other in [
            &b"\x1b[B"[..],
            b"\x1b[I",
            b"\x1b[O",
            b"\x1b[<0;3;4M",
            b"\x1b[12;1R",
            b"no",
        ] {
            assert!(!answers_prompt(other), "{other:?}");
        }
    }

    mod with_app {
        use super::super::*;
        use crate::events::Sent;
        use crate::testkit::TestApp;
        use crate::workitem::create_task;

        #[test]
        fn an_agent_that_crashes_says_so_and_one_ariekei_closes_does_not() {
            let app = TestApp::new();
            let task = create_task(&app.state, Some(app.repo.clone()), "Crash", "", "").unwrap();
            let key = task.to_string();
            app.start_agent(task);
            app.state.terminals.close(&key);
            app.wait_for(|| {
                app.events
                    .take()
                    .contains(&Sent::Status(key.clone(), AgentStatus::Stopped))
            });
            assert!(app.events.notices().is_empty(), "closed by Ariekei");

            app.start_agent(task);
            app.state
                .terminals
                .write(&key, b"Exit with code 3\r")
                .unwrap();
            app.wait_for(|| app.state.terminals.status(&key).is_none());
            app.wait_for(|| !app.events.notices().is_empty());
            assert_eq!(
                app.events.notices(),
                [(
                    "error".to_owned(),
                    "T1 Crash".to_owned(),
                    "it exited (exit 3)".to_owned()
                )]
            );
            let err = app
                .state
                .terminals
                .send_to_agent(&key, "hello", Duration::ZERO)
                .unwrap_err();
            assert!(err.contains("not running"), "{err}");
        }

        #[test]
        fn a_message_counts_as_sent_once_the_agent_takes_it() {
            let app = TestApp::new();
            let task = create_task(&app.state, Some(app.repo.clone()), "Send", "", "").unwrap();
            app.start_agent(task);
            let key = task.to_string();
            let terms = &app.state.terminals;
            let wait = Duration::from_millis(300);

            // No UserPromptSubmit: e.g. a dialog took the Enter. (The test's hooks go nowhere.)
            let err = terms.send_to_agent(&key, "first", wait).unwrap_err();
            assert!(err.contains("did not take it"), "{err}");

            std::thread::scope(|s| {
                s.spawn(|| {
                    std::thread::sleep(Duration::from_millis(150));
                    terms.prompt_submitted(&key);
                });
                terms
                    .send_to_agent(&key, "second", Duration::from_secs(5))
                    .unwrap();
            });

            // In a turn Claude Code queues it for later; waiting, it would answer the prompt.
            terms.report(&key, AgentStatus::Working, None);
            terms.send_to_agent(&key, "third", wait).unwrap();
            terms.report(&key, AgentStatus::WaitingForPermission, None);
            let err = terms.send_to_agent(&key, "fourth", wait).unwrap_err();
            assert!(err.contains("waiting for your answer"), "{err}");
        }

        #[test]
        fn an_answered_or_interrupted_turn_is_not_left_waiting_or_working() {
            let app = TestApp::new();
            let task = create_task(&app.state, Some(app.repo.clone()), "Quiet", "", "").unwrap();
            app.start_agent(task);
            let key = task.to_string();
            let terms = &app.state.terminals;
            let status = || terms.status(&key).unwrap();
            terms.report(
                &key,
                AgentStatus::WaitingForPermission,
                Some("Bash ./gradlew test".into()),
            );
            terms.typed(&key, b"\x1b[B");
            assert_eq!(status(), AgentStatus::WaitingForPermission, "only choosing");
            // Yes: it runs the command, long before PostToolUse.
            terms.typed(&key, b"\r");
            assert_eq!(status(), AgentStatus::Working);
            assert_eq!(
                terms.get(&key).unwrap().detail.lock().unwrap().as_deref(),
                Some("Bash ./gradlew test")
            );
            terms.typed(&key, b"\r");
            assert_eq!(status(), AgentStatus::Working, "typing while it works");

            // Claude Code redraws while it works; an interrupted turn goes silent, no hook.
            let quiet = Duration::from_millis(300);
            terms.settle_quiet(quiet);
            assert_eq!(status(), AgentStatus::Working, "not quiet for long yet");
            std::thread::sleep(quiet + Duration::from_millis(100));
            *terms.get(&key).unwrap().last_output.lock().unwrap() = Instant::now();
            terms.settle_quiet(quiet);
            assert_eq!(status(), AgentStatus::Working, "it just wrote something");
            std::thread::sleep(quiet + Duration::from_millis(100));
            terms.settle_quiet(quiet);
            assert_eq!(status(), AgentStatus::Ready);
            assert!(
                app.events.notices().is_empty(),
                "the user ended it: nothing to tell"
            );
        }
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
