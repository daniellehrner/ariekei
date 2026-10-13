mod agent;
mod git;
mod node;
mod sessions;

use agent::{AgentLaunch, AgentManager, Started};
use std::path::PathBuf;
use tauri::{Manager, State};

pub struct AppState {
    agents: AgentManager,
    sessions: sessions::SessionStore,
}

/// Node 22+ (detected) and the pinned ACP adapter. The adapter path is overridable
/// via env and defaults to this checkout's node_modules until packaging (M1+).
fn agent_launch() -> Result<AgentLaunch, String> {
    let node = node::find()?;
    let adapter = std::env::var_os("ARIEKEI_ACP_ADAPTER")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../node_modules/@agentclientprotocol/claude-agent-acp/dist/index.js")
        });
    Ok(AgentLaunch { node, adapter })
}

/// Starts the agent for a worktree, loading its previous session if there is one.
#[tauri::command]
async fn agent_start(state: State<'_, AppState>, cwd: String) -> Result<Started, String> {
    let cwd = std::fs::canonicalize(&cwd).map_err(|e| format!("{cwd}: {e}"))?;
    let key = cwd.to_string_lossy().into_owned();
    let resume = state.sessions.get(&key);
    let started = state.agents.start(key.clone(), cwd, resume).await?;
    state.sessions.put(&key, &started.session_id)?;
    Ok(started)
}

#[tauri::command]
fn agent_prompt(state: State<'_, AppState>, key: String, text: String) -> Result<(), String> {
    state.agents.prompt(&key, text)
}

#[tauri::command]
fn agent_cancel(state: State<'_, AppState>, key: String) -> Result<(), String> {
    state.agents.cancel(&key)
}

#[tauri::command]
fn agent_stop(state: State<'_, AppState>, key: String) {
    state.agents.stop(&key)
}

#[tauri::command]
fn agent_permission(
    state: State<'_, AppState>,
    request_id: String,
    option_id: Option<String>,
) -> Result<(), String> {
    state.agents.answer_permission(&request_id, option_id)
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

    tauri::Builder::default()
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let launch = agent_launch();
            match &launch {
                Ok(l) => tracing::info!("agent launch: {l:?}"),
                Err(e) => tracing::warn!("agents unavailable: {e}"),
            }
            app.manage(AppState {
                agents: AgentManager::new(app.handle().clone(), launch),
                sessions: sessions::SessionStore::new(data_dir.join("sessions.json")),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            agent_start,
            agent_prompt,
            agent_cancel,
            agent_stop,
            agent_permission,
            diff_files,
            diff_file,
            self_rss_kib,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Ariekei");
}
