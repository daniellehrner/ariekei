mod claude;
mod context;
mod git;
mod github;
mod hooks;
mod inbox;
mod notify;
mod poller;
mod store;
mod terminal;
mod workitem;
mod worktree;

use portable_pty::PtySize;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{Manager, RunEvent, State};
use terminal::{Launch, Opened, TerminalManager};

pub struct AppState {
    terminals: Arc<TerminalManager>,
    store: store::Store,
    poller: poller::Poller,
    data_dir: PathBuf,
    /// The work item shown in the UI (no notifications for it while the window is focused).
    active_item: Mutex<Option<String>>,
}

type Res<T> = Result<T, String>;

/// Runs blocking work (git, gh) off the async runtime's threads.
async fn blocking<T: Send + 'static>(
    app: tauri::AppHandle,
    f: impl FnOnce(&AppState) -> Res<T> + Send + 'static,
) -> Res<T> {
    tauri::async_runtime::spawn_blocking(move || f(&app.state::<AppState>()))
        .await
        .map_err(|e| e.to_string())?
}

fn pty_size(cols: u16, rows: u16) -> PtySize {
    PtySize {
        cols: cols.max(2),
        rows: rows.max(2),
        ..PtySize::default()
    }
}

// --- repo and inbox ---

#[tauri::command]
fn repo(state: State<'_, AppState>) -> Res<Option<store::Repo>> {
    state.store.active_repo()
}

#[tauri::command]
async fn connect_repo(app: tauri::AppHandle, path: String) -> Res<store::Repo> {
    blocking(app, move |s| {
        let repo = workitem::connect(s, &path)?;
        s.poller.refresh();
        Ok(repo)
    })
    .await
}

#[tauri::command]
async fn inbox(app: tauri::AppHandle) -> Res<Vec<inbox::Row>> {
    blocking(app, workitem::inbox).await
}

#[tauri::command]
fn refresh(state: State<'_, AppState>) {
    state.poller.refresh();
}

#[tauri::command]
fn mark_seen(state: State<'_, AppState>, id: i64) -> Res<()> {
    workitem::mark_seen(&state, id)
}

#[tauri::command]
fn set_active_item(state: State<'_, AppState>, id: Option<i64>) {
    *state.active_item.lock().unwrap() = id.map(|i| i.to_string());
}

#[tauri::command]
fn ui_state(state: State<'_, AppState>, id: i64) -> Res<Option<serde_json::Value>> {
    Ok(state.store.item(id)?.ui_state)
}

#[tauri::command]
fn set_ui_state(state: State<'_, AppState>, id: i64, value: serde_json::Value) -> Res<()> {
    state.store.set_ui_state(id, &value)
}

// --- work item: worktree and agent ---

#[tauri::command]
async fn prepare_item(app: tauri::AppHandle, id: i64) -> Res<worktree::Prepared> {
    blocking(app, move |s| workitem::prepare(s, id)).await
}

/// Attaches the view to the item's terminal, starting (or resuming) its `claude`.
#[tauri::command]
async fn term_open(
    app: tauri::AppHandle,
    id: i64,
    cols: u16,
    rows: u16,
    output: Channel<InvokeResponseBody>,
) -> Res<Opened> {
    blocking(app, move |s| {
        let cwd = s
            .store
            .item(id)?
            .worktree_path
            .ok_or("the worktree is not ready")?;
        s.terminals.open(
            &id.to_string(),
            &cwd,
            || workitem::session_plan(s, &s.data_dir, id),
            pty_size(cols, rows),
            output,
        )
    })
    .await
}

#[tauri::command]
fn term_detach(state: State<'_, AppState>, id: i64, view: u32) {
    state.terminals.detach(&id.to_string(), view)
}

#[tauri::command]
fn term_write(state: State<'_, AppState>, id: i64, data: String) -> Res<()> {
    state.terminals.write(&id.to_string(), data.as_bytes())
}

#[tauri::command]
fn term_resize(state: State<'_, AppState>, id: i64, cols: u16, rows: u16) -> Res<()> {
    state
        .terminals
        .resize(&id.to_string(), pty_size(cols, rows))
}

#[tauri::command]
fn term_close(state: State<'_, AppState>, id: i64) {
    state.terminals.close(&id.to_string())
}

/// Sends `text` to the item's agent as one message (e.g. review feedback).
#[tauri::command]
async fn agent_send(app: tauri::AppHandle, id: i64, text: String) -> Res<()> {
    blocking(app, move |s| {
        s.terminals.send_to_agent(&id.to_string(), &text)
    })
    .await
}

/// Sends the PR's current review threads, review summaries and failing checks to the agent.
#[tauri::command]
async fn send_pr_feedback(app: tauri::AppHandle, id: i64) -> Res<()> {
    blocking(app, move |s| {
        let message = workitem::comments_message(s, id)?;
        s.terminals.send_to_agent(&id.to_string(), &message)
    })
    .await
}

// --- review ---

#[tauri::command]
async fn review_state(app: tauri::AppHandle, id: i64) -> Res<workitem::ReviewState> {
    blocking(app, move |s| workitem::review_state(s, id)).await
}

#[tauri::command]
async fn mark_reviewed(app: tauri::AppHandle, id: i64) -> Res<String> {
    blocking(app, move |s| workitem::mark_reviewed(s, id)).await
}

/// Changed files from `from` to `to` (a commit), or to the working tree when `to` is null.
#[tauri::command]
async fn review_files(
    app: tauri::AppHandle,
    id: i64,
    from: String,
    to: Option<String>,
) -> Res<Vec<git::ChangedFile>> {
    blocking(app, move |s| {
        workitem::review_files(s, id, &from, to.as_deref())
    })
    .await
}

#[tauri::command]
async fn review_file(
    app: tauri::AppHandle,
    id: i64,
    from: String,
    to: Option<String>,
    path: String,
) -> Res<git::FileDiff> {
    blocking(app, move |s| {
        workitem::review_file(s, id, &from, to.as_deref(), &path)
    })
    .await
}

/// Saves a file edited in the review view.
#[tauri::command]
async fn save_file(app: tauri::AppHandle, id: i64, path: String, content: String) -> Res<()> {
    blocking(app, move |s| workitem::save_file(s, id, &path, &content)).await
}

/// Resident memory of the app process in KiB (Linux only).
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
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            // ARIEKEI_DATA_DIR keeps test runs away from the real database and state.
            let data_dir = match std::env::var_os("ARIEKEI_DATA_DIR") {
                Some(dir) => PathBuf::from(dir),
                None => app.path().app_data_dir()?,
            };
            std::fs::create_dir_all(&data_dir)?;

            let handle = app.handle().clone();
            let hook_server =
                hooks::HookServer::start(&data_dir, move |e| hooks::dispatch(&handle, e))?;
            let env_secs = |name: &str, default: u64| {
                std::env::var(name)
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(default)
            };
            let launch = claude::find().map(|claude| Launch {
                claude,
                hooks_settings: hook_server.settings_path.clone(),
                extra_args: std::env::var("ARIEKEI_CLAUDE_ARGS")
                    .map(|a| a.split_whitespace().map(str::to_owned).collect())
                    .unwrap_or_default(),
                idle_after: Duration::from_secs(env_secs("ARIEKEI_IDLE_SECS", 15 * 60)),
            });
            match &launch {
                Ok(l) => tracing::info!("claude launch: {l:?}"),
                Err(e) => tracing::warn!("agents unavailable: {e}"),
            }
            let store = store::Store::open(&data_dir.join("ariekei.db"))?;
            app.manage(AppState {
                terminals: TerminalManager::new(app.handle().clone(), launch),
                store,
                poller: poller::Poller::start(
                    app.handle().clone(),
                    std::env::var_os("ARIEKEI_NO_POLL").is_none(),
                ),
                data_dir,
                active_item: Mutex::new(None),
            });
            if let Err(e) = workitem::reconcile(&app.state::<AppState>()) {
                tracing::warn!("reconcile failed: {e}");
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            repo,
            connect_repo,
            inbox,
            refresh,
            mark_seen,
            set_active_item,
            ui_state,
            set_ui_state,
            prepare_item,
            term_open,
            term_detach,
            term_write,
            term_resize,
            term_close,
            agent_send,
            send_pr_feedback,
            review_state,
            mark_reviewed,
            review_files,
            review_file,
            save_file,
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
