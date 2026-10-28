// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

mod agent_worktrees;
mod claude;
mod confine;
mod context;
mod drafts;
#[cfg(feature = "e2e")]
mod e2e;
mod events;
mod git;
mod github;
mod hooks;
mod inbox;
mod instance;
mod lifecycle;
mod mcp;
mod notify;
mod poller;
mod procs;
mod quit;
mod review_order;
mod store;
mod terminal;
#[cfg(test)]
mod testkit;
mod tools;
mod windows;
mod workitem;
mod worktree;

use portable_pty::PtySize;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{Emitter, Manager, RunEvent, State};
use terminal::{Launch, Opened, TerminalManager};

pub struct AppState {
    terminals: Arc<TerminalManager>,
    store: store::Store,
    poller: poller::Poller,
    data_dir: PathBuf,
    /// The repo and work item each window shows (no notifications for an item while its
    /// window is focused).
    windows: windows::Windows,
    /// What the backend tells the windows and the desktop.
    events: Arc<dyn events::Events>,
    /// The user agreed to quit although it stops agents' work (`quit`).
    quit_confirmed: std::sync::atomic::AtomicBool,
    /// The worktrees there were before the agents made theirs (`agent_worktrees`).
    agent_worktrees: agent_worktrees::Watch,
}

impl AppState {
    fn new(
        store: store::Store,
        data_dir: PathBuf,
        events: Arc<dyn events::Events>,
        launch: Result<Launch, String>,
        poller: poller::Poller,
    ) -> Self {
        Self {
            terminals: TerminalManager::new(events.clone(), launch),
            store,
            poller,
            data_dir,
            windows: windows::Windows::default(),
            events,
            quit_confirmed: Default::default(),
            agent_worktrees: Default::default(),
        }
    }

    /// Records an agent's new status (see `TerminalManager::report`) and tells the user
    /// about it when it needs them; the status goes out first, so an in-app notice about it
    /// finds the status it is about.
    fn report(&self, key: &str, status: hooks::AgentStatus, detail: Option<String>) {
        let shown = detail.clone();
        if let Some(previous) = self.terminals.report(key, status, detail)
            && previous != status
        {
            notify::agent_status(self, key, previous, status, shown.as_deref());
        }
    }
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

/// The worktrees Ariekei made for repo `id` that removing it leaves on disk.
#[tauri::command]
async fn repo_leftovers(app: tauri::AppHandle, id: i64) -> Res<Vec<String>> {
    blocking(app, move |s| lifecycle::repo_leftovers(s, id)).await
}

/// Quits although that stops agents' work: the user confirmed it (`confirm-quit`).
#[tauri::command]
fn quit_app(app: tauri::AppHandle) {
    quit::quit_now(&app)
}

/// Removes repo `id` from Ariekei (`lifecycle::remove_repo`). Not while another window shows
/// it; this window, if it showed it, goes back to the Connect screen.
#[tauri::command]
async fn remove_repo(app: tauri::AppHandle, window: tauri::Window, id: i64) -> Res<()> {
    let label = window.label().to_owned();
    let shown_in = app.state::<AppState>().windows.window_of(id);
    if shown_in.as_ref().is_some_and(|l| *l != label) {
        return Err("it is open in another window; close that window first".into());
    }
    blocking(app.clone(), move |s| lifecycle::remove_repo(s, id)).await?;
    if shown_in.is_some() {
        windows::show_none(&app, &label);
    }
    Ok(())
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

/// Shows work item `id` in its repo's window, opening or focusing that window (a notice
/// can be about an item of another repo).
#[tauri::command]
async fn open_item(app: tauri::AppHandle, id: i64) -> Res<()> {
    let state = app.state::<AppState>();
    let item = state.store.item(id)?;
    let repo = state
        .store
        .repo(item.repo_id)?
        .ok_or("unknown repository")?;
    // A window opened for it selects the item from this setting; an open one is told.
    state
        .store
        .set_setting(&format!("selected_item.{}", repo.id), &id.to_string())?;
    windows::open(&app, Some(&repo))?;
    if let Some(label) = state.windows.window_of(repo.id) {
        app.emit_to(label.as_str(), "select-item", id)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
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
fn mark_seen(state: State<'_, AppState>, id: i64) -> Res<workitem::Seen> {
    workitem::mark_seen(&state, id)
}

/// The user opened the review of item `id`: its agent is no longer "ready for review".
#[tauri::command]
fn review_seen(state: State<'_, AppState>, id: i64) {
    state.terminals.review_seen(&id.to_string());
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
        let item = s.store.item(id)?;
        let cwd = item
            .worktree_path
            .clone()
            .ok_or("the worktree is not ready")?;
        s.terminals.open(
            &id.to_string(),
            &item.label(),
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
    let key = id.to_string();
    state.terminals.typed(&key, data.as_bytes());
    state.terminals.write(&key, data.as_bytes())
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

/// How long an idle agent has to accept a message sent to it (`send_to_agent`).
const AGENT_TAKES: Duration = Duration::from_secs(10);

/// Sends `text` to the item's agent as one message (e.g. review feedback).
#[tauri::command]
async fn agent_send(app: tauri::AppHandle, id: i64, text: String) -> Res<()> {
    blocking(app, move |s| {
        workitem::ensure_on_branch(s, id)?;
        s.terminals
            .send_to_agent(&id.to_string(), &text, AGENT_TAKES)
    })
    .await
}

/// Sends the PR's current review threads, review summaries and failing checks to the agent.
#[tauri::command]
async fn send_pr_feedback(app: tauri::AppHandle, id: i64) -> Res<()> {
    blocking(app, move |s| {
        workitem::ensure_on_branch(s, id)?;
        let message = workitem::comments_message(s, id)?;
        s.terminals
            .send_to_agent(&id.to_string(), &message, AGENT_TAKES)
    })
    .await
}

// --- browsing open PRs and issues, the Inbox's own lists, the PR conversation ---

/// Open PRs or issues (`kind`: "pr" or "issue") matching `text`.
#[tauri::command]
async fn search_items(
    app: tauri::AppHandle,
    window: tauri::Window,
    kind: String,
    text: String,
) -> Res<Vec<workitem::SearchHit>> {
    let label = window.label().to_owned();
    blocking(app, move |s| {
        workitem::search(s, windows::repo_of(s, &label)?, &kind, &text)
    })
    .await
}

/// Adds a PR or issue (e.g. a search result) to the Inbox; returns its work item id.
#[tauri::command]
async fn track_item(
    app: tauri::AppHandle,
    window: tauri::Window,
    kind: String,
    number: i64,
) -> Res<i64> {
    let label = window.label().to_owned();
    blocking(app, move |s| {
        workitem::track(s, windows::repo_of(s, &label)?, &kind, number)
    })
    .await
}

/// Takes a PR or issue the user added out of the Inbox again.
#[tauri::command]
fn untrack(state: State<'_, AppState>, id: i64) -> Res<()> {
    workitem::untrack(&state, id)
}

#[tauri::command]
fn set_starred(state: State<'_, AppState>, id: i64, starred: bool) -> Res<()> {
    workitem::set_starred(&state, id, starred)
}

/// Starts a task (no PR or issue yet) with its worktree, on a new branch or the existing
/// `branch` (e.g. `owner:branch` on a fork); returns its work item id.
#[tauri::command]
async fn create_task(
    app: tauri::AppHandle,
    window: tauri::Window,
    title: String,
    notes: String,
    branch: String,
) -> Res<i64> {
    let label = window.label().to_owned();
    blocking(app, move |s| {
        workitem::create_task(s, windows::repo_of(s, &label)?, &title, &notes, &branch)
    })
    .await
}

#[tauri::command]
async fn pr_conversation(app: tauri::AppHandle, id: i64) -> Res<github::Conversation> {
    blocking(app, move |s| workitem::conversation(s, id)).await
}

/// The conversation as it was last loaded, shown while `pr_conversation` loads.
#[tauri::command]
fn cached_conversation(state: State<'_, AppState>, id: i64) -> Res<Option<serde_json::Value>> {
    workitem::cached_conversation(&state, id)
}

#[tauri::command]
async fn comment_ranges(
    app: tauri::AppHandle,
    id: i64,
    paths: Vec<String>,
    commit: String,
) -> Res<workitem::CommentRanges> {
    blocking(app, move |s| {
        workitem::comment_ranges(s, id, &paths, &commit)
    })
    .await
}

/// The review's drafts, the user's and its agent's (`drafts`).
#[tauri::command]
fn review_drafts(state: State<'_, AppState>, id: i64) -> Res<Vec<store::ReviewDraft>> {
    drafts::list(&state, id)
}

#[tauri::command]
fn add_draft(
    state: State<'_, AppState>,
    id: i64,
    at: store::DraftAnchor,
    text: String,
) -> Res<i64> {
    drafts::add(&state, id, &at, &text)
}

#[tauri::command]
fn edit_draft(state: State<'_, AppState>, id: i64, draft: i64, text: String) -> Res<()> {
    drafts::edit(&state, id, draft, &text)
}

#[tauri::command]
fn delete_drafts(state: State<'_, AppState>, id: i64, drafts: Vec<i64>) -> Res<()> {
    drafts::delete(&state, id, &drafts)
}

/// The worktrees item `id`'s agent made itself (`agent_worktrees`).
#[tauri::command]
async fn agent_worktrees(app: tauri::AppHandle, id: i64) -> Res<Vec<agent_worktrees::Shown>> {
    blocking(app, move |s| agent_worktrees::list(s, id)).await
}

/// Removes one; `force`: although that loses work (the user saw what).
#[tauri::command]
async fn remove_agent_worktree(
    app: tauri::AppHandle,
    id: i64,
    worktree: i64,
    force: bool,
) -> Res<()> {
    blocking(app, move |s| {
        agent_worktrees::remove_one(s, id, worktree, force)
    })
    .await
}

/// Makes one a task of its own; returns the task.
#[tauri::command]
async fn agent_worktree_task(app: tauri::AppHandle, id: i64, worktree: i64) -> Res<i64> {
    blocking(app, move |s| agent_worktrees::make_task(s, id, worktree)).await
}

#[tauri::command]
async fn submit_review(
    app: tauri::AppHandle,
    id: i64,
    event: String,
    body: String,
    comments: Vec<github::LineComment>,
    commit: String,
) -> Res<String> {
    blocking(app, move |s| {
        workitem::submit_review(s, id, &event, &body, &comments, &commit)
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

/// Keeps the finished item until Done instead of cleaning it up automatically.
#[tauri::command]
fn keep_item(state: State<'_, AppState>, id: i64) -> Res<()> {
    state.store.set_kept(id, true)
}

/// What discarding the task would lose (uncommitted changes, commits not on GitHub).
#[tauri::command]
async fn discard_losses(app: tauri::AppHandle, id: i64) -> Res<Vec<String>> {
    blocking(app, move |s| lifecycle::discard_losses(s, id)).await
}

/// Deletes the task with its worktree and branch, losing what `discard_losses` listed.
#[tauri::command]
async fn discard_task(app: tauri::AppHandle, id: i64) -> Res<lifecycle::Cleanup> {
    blocking(app, move |s| lifecycle::discard(s, id)).await
}

// --- review ---

#[tauri::command]
async fn review_state(
    app: tauri::AppHandle,
    id: i64,
    refresh: Option<bool>,
) -> Res<workitem::ReviewState> {
    blocking(app, move |s| {
        workitem::review_state(s, id, refresh.unwrap_or(false))
    })
    .await
}

/// Puts the agent's worktree back on its branch after someone switched it.
#[tauri::command]
async fn switch_back(app: tauri::AppHandle, id: i64) -> Res<()> {
    blocking(app, move |s| workitem::switch_back(s, id)).await
}

/// Makes the branch a task's worktree was switched to the task's branch.
#[tauri::command]
async fn follow_branch(app: tauri::AppHandle, id: i64) -> Res<()> {
    blocking(app, move |s| workitem::follow_branch(s, id)).await
}

/// Moves the item's branch to its worktree's detached HEAD.
#[tauri::command]
async fn move_branch_here(app: tauri::AppHandle, id: i64) -> Res<()> {
    blocking(app, move |s| workitem::move_branch_here(s, id)).await
}

/// Brings the PR's branch up to GitHub's head of it; replaces it only with `rewrite` (see
/// `workitem::update_branch`): `false` if the fetch found the history rewritten.
#[tauri::command]
async fn update_branch(app: tauri::AppHandle, id: i64, rewrite: bool) -> Res<bool> {
    blocking(app, move |s| workitem::update_branch(s, id, rewrite)).await
}

/// Fetches what the review compares if GitHub may have newer; true if that moved it.
#[tauri::command]
async fn review_fetch(app: tauri::AppHandle, id: i64, force: Option<bool>) -> Res<bool> {
    blocking(app, move |s| {
        workitem::review_fetch(s, id, force.unwrap_or(false))
    })
    .await
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

/// Saves a file edited in the review view, unless it changed on disk since it was `loaded`.
#[tauri::command]
async fn save_file(
    app: tauri::AppHandle,
    id: i64,
    path: String,
    content: String,
    loaded: String,
) -> Res<()> {
    blocking(app, move |s| {
        workitem::save_file(s, id, &path, &content, &loaded)
    })
    .await
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
        .plugin(tauri_plugin_clipboard_manager::init())
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
            // One Ariekei per data directory (see `instance`): a second one shows the first.
            let handle = app.handle().clone();
            let show = move || {
                let windows = handle.webview_windows();
                if let Some(window) = windows.get("main").or_else(|| windows.values().next()) {
                    let _ = window.unminimize();
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            };
            let Some(instance) = instance::claim(&data_dir, show)? else {
                tracing::info!(
                    "Ariekei runs already on {}: it was asked to show its window",
                    data_dir.display()
                );
                std::process::exit(0);
            };
            app.manage(instance);

            let handle = app.handle().clone();
            let hook_server =
                hooks::HookServer::start(&data_dir, move |e| hooks::dispatch(&handle, e))?;
            // Not fatal: the agents work without Ariekei's tools.
            let handle = app.handle().clone();
            let mcp_server = mcp::McpServer::start(
                &data_dir,
                drafts::tools(),
                drafts::INSTRUCTIONS,
                move |terminal, tool, args| {
                    let state = handle
                        .try_state::<AppState>()
                        .ok_or("Ariekei is still starting")?;
                    drafts::call(&state, terminal, tool, args)
                },
            )
            .inspect_err(|e| tracing::warn!("the agents' tools are unavailable: {e}"))
            .ok();
            let env_secs = |name: &str, default: u64| {
                std::env::var(name)
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(default)
            };
            let launch = claude::find().map(|claude| Launch {
                claude,
                hooks_settings: hook_server.settings_path.clone(),
                mcp_config: mcp_server.map(|s| s.config_path),
                extra_args: std::env::var("ARIEKEI_CLAUDE_ARGS")
                    .map(|a| a.split_whitespace().map(str::to_owned).collect())
                    .unwrap_or_default(),
                idle_after: Duration::from_secs(env_secs("ARIEKEI_IDLE_SECS", 45 * 60)),
                quiet_after: Duration::from_secs(env_secs("ARIEKEI_QUIET_SECS", 30)),
                claude_dir: claude::config_dir(),
                env: vec![],
            });
            match &launch {
                Ok(l) => tracing::info!("claude launch: {l:?}"),
                Err(e) => tracing::warn!("agents unavailable: {e}"),
            }
            let store = store::Store::open(&data_dir.join("ariekei.db"))?;
            app.manage(AppState::new(
                store,
                data_dir,
                Arc::new(events::TauriEvents(app.handle().clone())),
                launch,
                poller::Poller::start(
                    app.handle().clone(),
                    std::env::var_os("ARIEKEI_NO_POLL").is_none(),
                ),
            ));
            if let Err(e) = workitem::reconcile(&app.state::<AppState>()) {
                tracing::warn!("reconcile failed: {e}");
            }
            if let Err(e) = workitem::unlink_from_tasks(&app.state::<AppState>()) {
                tracing::warn!("unlinking the tasks' PRs and issues failed: {e}");
            }
            // The worktrees there are now are nobody's: the agents' are those made later.
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                let state = handle.state::<AppState>();
                for repo in state.store.repos().unwrap_or_default() {
                    if let Err(e) = state.agent_worktrees.baseline(&repo.path) {
                        tracing::warn!("listing the worktrees of {}: {e}", repo.path.display());
                    }
                }
            });
            windows::restore(app.handle());
            #[cfg(feature = "e2e")]
            e2e::start(app.handle());
            Ok(())
        })
        .on_window_event(|window, event| match event {
            // Closing the last window quits: asked first if that stops agents' work.
            tauri::WindowEvent::CloseRequested { api, .. }
                if window.app_handle().webview_windows().len() == 1
                    && quit::ask_first(window.app_handle()) =>
            {
                api.prevent_close();
                quit::check(window.app_handle().clone(), window.label().to_owned());
            }
            tauri::WindowEvent::Destroyed => windows::closed(window.app_handle(), window.label()),
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            repo,
            repos,
            connect_repo,
            switch_repo,
            repo_leftovers,
            remove_repo,
            quit_app,
            open_window,
            open_item,
            inbox,
            refresh,
            mark_seen,
            review_seen,
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
            review_fetch,
            switch_back,
            follow_branch,
            move_branch_here,
            update_branch,
            review_files,
            review_file,
            save_file,
            search_items,
            track_item,
            untrack,
            set_starred,
            create_task,
            pr_conversation,
            cached_conversation,
            comment_ranges,
            review_drafts,
            add_draft,
            edit_draft,
            delete_drafts,
            agent_worktrees,
            remove_agent_worktree,
            agent_worktree_task,
            submit_review,
            reply_comment,
            post_comment,
            resolve_thread,
            mark_done,
            keep_item,
            discard_losses,
            discard_task,
            self_rss_kib,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Ariekei");

    app.run(|app, event| {
        if let RunEvent::ExitRequested { api, .. } = &event
            && let Some(state) = app.try_state::<AppState>()
        {
            // Quit from the app menu (Cmd+Q): asked first in the focused window, if that
            // stops agents' work, as closing the last window is.
            let windows = app.webview_windows();
            let asking = windows
                .values()
                .find(|w| w.is_focused().unwrap_or(false))
                .or_else(|| windows.values().next())
                .map(|w| w.label().to_owned());
            if let Some(label) = asking.filter(|_| quit::ask_first(app)) {
                api.prevent_exit();
                quit::check(app.clone(), label);
            } else {
                // Windows destroyed while quitting stay recorded, to come back next launch.
                state.windows.exiting();
            }
        }
        if let RunEvent::Exit = event
            && let Some(state) = app.try_state::<AppState>()
        {
            // Conversations stay resumable; only the processes end.
            state.terminals.close_all();
        }
    });
}
