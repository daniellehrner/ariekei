mod claude;
mod git;
mod hooks;
mod sessions;
mod terminal;

use portable_pty::PtySize;
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{Manager, RunEvent, State};
use terminal::{Launch, Opened, TerminalManager};

pub struct AppState {
    terminals: TerminalManager,
    sessions: sessions::SessionStore,
}

fn pty_size(cols: u16, rows: u16) -> PtySize {
    PtySize {
        cols: cols.max(2),
        rows: rows.max(2),
        ..PtySize::default()
    }
}

/// Canonical key for a worktree path; fails if it is not a git worktree.
#[tauri::command]
fn worktree_key(path: String) -> Result<String, String> {
    terminal::worktree_key(&path)
}

/// Attaches the view to the work item's terminal, starting (or resuming) `claude`.
#[tauri::command]
fn term_open(
    state: State<'_, AppState>,
    cwd: String,
    cols: u16,
    rows: u16,
    output: Channel<InvokeResponseBody>,
) -> Result<Opened, String> {
    let key = terminal::worktree_key(&cwd)?;
    state
        .terminals
        .open(&state.sessions, key, pty_size(cols, rows), output)
}

#[tauri::command]
fn term_detach(state: State<'_, AppState>, key: String) {
    state.terminals.detach(&key)
}

#[tauri::command]
fn term_write(state: State<'_, AppState>, key: String, data: String) -> Result<(), String> {
    state.terminals.write(&key, data.as_bytes())
}

#[tauri::command]
fn term_resize(
    state: State<'_, AppState>,
    key: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    state.terminals.resize(&key, pty_size(cols, rows))
}

#[tauri::command]
fn term_close(state: State<'_, AppState>, key: String) {
    state.terminals.close(&key)
}

/// Sends `text` to the agent as one message (e.g. review feedback).
#[tauri::command]
async fn agent_send(state: State<'_, AppState>, key: String, text: String) -> Result<(), String> {
    state.terminals.send_to_agent(&key, &text)
}

#[tauri::command]
fn diff_files(cwd: String, base: String) -> Result<Vec<git::ChangedFile>, String> {
    git::changed_files(cwd.as_ref(), &base)
}

#[tauri::command]
fn diff_file(cwd: String, base: String, path: String) -> Result<git::FileVersions, String> {
    git::file_versions(cwd.as_ref(), &base, &path)
}

/// Resident memory of the app process in KiB (Linux only; spike measurement).
#[tauri::command]
fn self_rss_kib() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    status
        .lines()
        .find_map(|l| l.strip_prefix("VmRSS:"))
        .and_then(|v| v.trim().trim_end_matches("kB").trim().parse().ok())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let app = tauri::Builder::default()
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;

            let handle = app.handle().clone();
            let hook_server =
                hooks::HookServer::start(&data_dir, move |e| hooks::dispatch(&handle, e))?;
            let launch = claude::find().map(|claude| Launch {
                claude,
                hooks_settings: hook_server.settings_path.clone(),
                extra_args: std::env::var("ARIEKEI_CLAUDE_ARGS")
                    .map(|a| a.split_whitespace().map(str::to_owned).collect())
                    .unwrap_or_default(),
            });
            match &launch {
                Ok(l) => tracing::info!("claude launch: {l:?}"),
                Err(e) => tracing::warn!("agents unavailable: {e}"),
            }
            app.manage(AppState {
                terminals: TerminalManager::new(app.handle().clone(), launch),
                sessions: sessions::SessionStore::new(data_dir.join("sessions.json")),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            worktree_key,
            term_open,
            term_detach,
            term_write,
            term_resize,
            term_close,
            agent_send,
            diff_files,
            diff_file,
            self_rss_kib,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Ariekei");

    app.run(|app, event| {
        if let RunEvent::Exit = event
            && let Some(state) = app.try_state::<AppState>()
        {
            // Conversations stay resumable; only the processes end.
            state.terminals.close_all();
        }
    });
}
