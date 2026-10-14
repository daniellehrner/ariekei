mod claude;
mod context;
#[cfg(feature = "e2e")]
mod e2e;
mod git;
mod github;
mod guide;
mod hooks;
mod inbox;
mod lifecycle;
mod notify;
mod poller;
mod review_order;
mod store;
mod terminal;
mod tools;
mod windows;
mod workitem;
mod worktree;

use portable_pty::PtySize;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{Manager, RunEvent, State};
use terminal::{Launch, Opened, TerminalManager};

pub struct AppState {
    terminals: Arc<TerminalManager>,
    store: store::Store,
    poller: poller::Poller,
    data_dir: PathBuf,
    /// The repo and work item each window shows (no notifications for an item while its
    /// window is focused).
    windows: windows::Windows,
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

/// The repo this window shows.
#[tauri::command]
fn repo(state: State<'_, AppState>, window: tauri::Window) -> Res<Option<store::Repo>> {
    windows::repo_of(&state, window.label())
}

/// A connected repo, and where it is shown.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct RepoEntry {
    #[serde(flatten)]
    repo: store::Repo,
    /// Shown in the asking window.
    here: bool,
    /// Shown in another window.
    elsewhere: bool,
}

#[tauri::command]
fn repos(state: State<'_, AppState>, window: tauri::Window) -> Res<Vec<RepoEntry>> {
    Ok(state
        .store
        .repos()?
        .into_iter()
        .map(|repo| {
            let shown_in = state.windows.window_of(repo.id);
            RepoEntry {
                here: shown_in.as_deref() == Some(window.label()),
                elsewhere: shown_in.is_some_and(|l| l != window.label()),
                repo,
            }
        })
        .collect())
}

/// What showing a repo did: `here` false means another window already shows it and got
/// the focus instead.
#[derive(serde::Serialize)]
struct Shown {
    repo: store::Repo,
    here: bool,
}

/// Connects the clone at `path` and shows it in this window.
#[tauri::command]
async fn connect_repo(app: tauri::AppHandle, window: tauri::Window, path: String) -> Res<Shown> {
    let repo = blocking(app.clone(), move |s| workitem::connect(s, &path)).await?;
    let here = windows::show(&app, window.label(), &repo)?;
    Ok(Shown { repo, here })
}

/// Shows a connected repo in this window.
#[tauri::command]
async fn switch_repo(app: tauri::AppHandle, window: tauri::Window, id: i64) -> Res<Shown> {
    let state = app.state::<AppState>();
    let repo = state.store.repo(id)?.ok_or("unknown repository")?;
    let here = windows::show(&app, window.label(), &repo)?;
    Ok(Shown { repo, here })
}

/// Opens a new window for repo `id`, or an empty one (Connect screen) without it.
#[tauri::command]
async fn open_window(app: tauri::AppHandle, id: Option<i64>) -> Res<()> {
    let repo = match id {
        Some(id) => Some(
            app.state::<AppState>()
                .store
                .repo(id)?
                .ok_or("unknown repository")?,
        ),
        None => None,
    };
    windows::open(&app, repo.as_ref())
}

#[tauri::command]
async fn inbox(app: tauri::AppHandle, window: tauri::Window) -> Res<Vec<inbox::Row>> {
    let label = window.label().to_owned();
    blocking(app, move |s| {
        workitem::inbox(s, windows::repo_of(s, &label)?)
    })
    .await
}

#[tauri::command]
fn refresh(state: State<'_, AppState>) {
    state.poller.refresh();
}

#[tauri::command]
fn mark_seen(state: State<'_, AppState>, id: i64) -> Res<()> {
    workitem::mark_seen(&state, id)
}

/// The item shown in this window; remembered (per repo) so the next launch opens it again.
#[tauri::command]
fn set_active_item(state: State<'_, AppState>, window: tauri::Window, id: Option<i64>) -> Res<()> {
    state
        .windows
        .set_item(window.label(), id.map(|i| i.to_string()));
    match (id, state.windows.repo_of(window.label())) {
        (Some(id), Some(repo)) => state
            .store
            .set_setting(&format!("selected_item.{repo}"), &id.to_string()),
        _ => Ok(()),
    }
}

/// UI preferences (e.g. whether the Inbox is collapsed), kept in the database.
#[tauri::command]
fn ui_setting(state: State<'_, AppState>, key: String) -> Res<Option<String>> {
    state.store.setting(&format!("ui.{key}"))
}

#[tauri::command]
fn set_ui_setting(state: State<'_, AppState>, key: String, value: String) -> Res<()> {
    state.store.set_setting(&format!("ui.{key}"), &value)
}

#[tauri::command]
fn selected_item(state: State<'_, AppState>, window: tauri::Window) -> Res<Option<i64>> {
    let Some(repo) = state.windows.repo_of(window.label()) else {
        return Ok(None);
    };
    // "selected_item" is from before windows had repos of their own.
    let saved = match state.store.setting(&format!("selected_item.{repo}"))? {
        Some(v) => Some(v),
        None => state.store.setting("selected_item")?,
    };
    Ok(saved
        .and_then(|v| v.parse().ok())
        .filter(|id| state.store.item(*id).is_ok_and(|i| i.repo_id == repo)))
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

// --- other people's PRs and the PR conversation ---

#[tauri::command]
async fn search_prs(
    app: tauri::AppHandle,
    window: tauri::Window,
    text: String,
) -> Res<Vec<workitem::SearchHit>> {
    let label = window.label().to_owned();
    blocking(app, move |s| {
        workitem::search(s, windows::repo_of(s, &label)?, &text)
    })
    .await
}

/// Adds a PR (e.g. a search result) to the Inbox; returns its work item id.
#[tauri::command]
async fn track_pr(app: tauri::AppHandle, window: tauri::Window, number: i64) -> Res<i64> {
    let label = window.label().to_owned();
    blocking(app, move |s| {
        workitem::track(s, windows::repo_of(s, &label)?, number)
    })
    .await
}

#[tauri::command]
async fn pr_conversation(app: tauri::AppHandle, id: i64) -> Res<github::Conversation> {
    blocking(app, move |s| workitem::conversation(s, id)).await
}

/// Fetches the PR head into the clone for reviewing without a worktree.
#[tauri::command]
async fn fetch_pr(app: tauri::AppHandle, id: i64) -> Res<String> {
    blocking(app, move |s| workitem::fetch_pr(s, id)).await
}

#[tauri::command]
async fn comment_ranges(
    app: tauri::AppHandle,
    id: i64,
    paths: Vec<String>,
) -> Res<workitem::CommentRanges> {
    blocking(app, move |s| workitem::comment_ranges(s, id, &paths)).await
}

#[tauri::command]
async fn submit_review(
    app: tauri::AppHandle,
    id: i64,
    event: String,
    body: String,
    comments: Vec<github::LineComment>,
) -> Res<String> {
    blocking(app, move |s| {
        workitem::submit_review(s, id, &event, &body, &comments)
    })
    .await
}

#[tauri::command]
async fn reply_comment(app: tauri::AppHandle, id: i64, comment_id: i64, body: String) -> Res<()> {
    blocking(app, move |s| {
        workitem::reply_comment(s, id, comment_id, &body)
    })
    .await
}

#[tauri::command]
async fn post_comment(app: tauri::AppHandle, id: i64, body: String) -> Res<()> {
    blocking(app, move |s| workitem::post_comment(s, id, &body)).await
}

#[tauri::command]
async fn resolve_thread(app: tauri::AppHandle, thread_id: String, resolved: bool) -> Res<()> {
    blocking(app, move |s| {
        workitem::resolve_thread(s, &thread_id, resolved)
    })
    .await
}

/// Cleans the item up now if nothing would be lost; otherwise says what blocks it.
#[tauri::command]
async fn mark_done(app: tauri::AppHandle, id: i64) -> Res<lifecycle::Cleanup> {
    blocking(app, move |s| workitem::mark_done(s, id)).await
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
    ignore_ws: bool,
) -> Res<Vec<workitem::ReviewFile>> {
    blocking(app, move |s| {
        workitem::review_files(s, id, &from, to.as_deref(), ignore_ws)
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
    ignore_ws: bool,
) -> Res<git::FileDiff> {
    blocking(app, move |s| {
        workitem::review_file(s, id, &from, to.as_deref(), &path, ignore_ws)
    })
    .await
}

/// Marks the file's current content reviewed (or unmarks it).
#[tauri::command]
async fn mark_file_reviewed(
    app: tauri::AppHandle,
    id: i64,
    path: String,
    reviewed: bool,
) -> Res<()> {
    blocking(app, move |s| {
        workitem::mark_file_reviewed(s, id, &path, reviewed)
    })
    .await
}

#[tauri::command]
async fn review_guide(app: tauri::AppHandle, id: i64) -> Res<Option<workitem::GuideView>> {
    blocking(app, move |s| workitem::review_guide(s, id)).await
}

/// Asks Claude for a review guide (takes a while: runs headless on the diff).
#[tauri::command]
async fn prepare_guide(app: tauri::AppHandle, id: i64) -> Res<workitem::GuideView> {
    blocking(app, move |s| workitem::prepare_guide(s, id)).await
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
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
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
                windows: windows::Windows::default(),
            });
            if let Err(e) = workitem::reconcile(&app.state::<AppState>()) {
                tracing::warn!("reconcile failed: {e}");
            }
            windows::restore(app.handle());
            #[cfg(feature = "e2e")]
            e2e::start(app.handle());
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Destroyed = event {
                windows::closed(window.app_handle(), window.label());
            }
        })
        .invoke_handler(tauri::generate_handler![
            repo,
            repos,
            connect_repo,
            switch_repo,
            open_window,
            inbox,
            refresh,
            mark_seen,
            set_active_item,
            selected_item,
            ui_setting,
            set_ui_setting,
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
            mark_file_reviewed,
            review_guide,
            prepare_guide,
            save_file,
            search_prs,
            track_pr,
            pr_conversation,
            fetch_pr,
            comment_ranges,
            submit_review,
            reply_comment,
            post_comment,
            resolve_thread,
            mark_done,
            self_rss_kib,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Ariekei");

    app.run(|app, event| {
        // Windows destroyed while quitting stay recorded, to come back on the next launch.
        if let RunEvent::ExitRequested { .. } = event
            && let Some(state) = app.try_state::<AppState>()
        {
            state.windows.exiting();
        }
        if let RunEvent::Exit = event
            && let Some(state) = app.try_state::<AppState>()
        {
            // Conversations stay resumable; only the processes end.
            state.terminals.close_all();
        }
    });
}
