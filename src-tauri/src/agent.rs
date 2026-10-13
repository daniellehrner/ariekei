//! Agent sessions over ACP: one adapter process per work item.
//!
//! Each session runs as its own task holding the ACP connection. The UI talks to it
//! through commands (prompt, cancel) and receives everything back as `agent-event`s.
//! Permission requests are parked until the UI answers them.

use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::schema::v1::{
    CancelNotification, ContentBlock, InitializeRequest, LoadSessionRequest, Meta,
    NewSessionRequest, PromptRequest, RequestPermissionOutcome, RequestPermissionRequest,
    RequestPermissionResponse, SelectedPermissionOutcome, SessionId, SessionNotification,
    TextContent,
};
use agent_client_protocol::{AcpAgent, AcpAgentConfig, Agent, ConnectionTo};
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager as _};
use tokio::sync::{mpsc, oneshot};

/// Adapter options for every session (`_meta.claudeCode.options`). The user's Claude
/// settings, permission mode, CLAUDE.md and skills apply unchanged; only MCP servers
/// are not inherited: `strictMcpConfig` limits them to the ones the IDE passes (none yet).
/// Note this also skips a repo's `.mcp.json`; forwarding those would bypass Claude
/// Code's approval prompt for project MCP servers.
fn session_meta() -> Meta {
    let meta = serde_json::json!({ "claudeCode": { "options": { "strictMcpConfig": true } } });
    match meta {
        serde_json::Value::Object(map) => map,
        _ => unreachable!("literal object"),
    }
}

#[derive(Debug, Clone, Serialize)]
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
#[serde(rename_all = "camelCase")]
pub struct PermissionOptionView {
    id: String,
    name: String,
    kind: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum AgentEvent {
    #[serde(rename_all = "camelCase")]
    Status {
        key: String,
        status: AgentStatus,
        detail: Option<String>,
    },
    /// A raw ACP `SessionUpdate`, passed through to the UI unchanged.
    Update {
        key: String,
        update: serde_json::Value,
    },
    #[serde(rename_all = "camelCase")]
    Permission {
        key: String,
        request_id: String,
        title: Option<String>,
        options: Vec<PermissionOptionView>,
    },
    #[serde(rename_all = "camelCase")]
    PermissionResolved { key: String, request_id: String },
}

enum Command {
    Prompt(String),
    Cancel,
}

struct SessionHandle {
    tx: mpsc::UnboundedSender<Command>,
}

#[derive(Debug, Clone)]
pub struct AgentLaunch {
    pub node: PathBuf,
    pub adapter: PathBuf,
}

pub struct AgentManager {
    app: AppHandle,
    /// Why agents cannot run (e.g. no Node 22+), reported when one is started.
    launch: Result<AgentLaunch, String>,
    sessions: Mutex<HashMap<String, SessionHandle>>,
    pending: Arc<Mutex<HashMap<String, oneshot::Sender<Option<String>>>>>,
    next_request: Arc<AtomicU64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Started {
    pub key: String,
    pub session_id: String,
    pub loaded: bool,
}

impl AgentManager {
    pub fn new(app: AppHandle, launch: Result<AgentLaunch, String>) -> Self {
        Self {
            app,
            launch,
            sessions: Mutex::default(),
            pending: Arc::default(),
            next_request: Arc::new(AtomicU64::new(1)),
        }
    }

    fn emit(app: &AppHandle, event: AgentEvent) {
        if let Err(e) = app.emit("agent-event", &event) {
            tracing::warn!("failed to emit agent event: {e}");
        }
    }

    fn status(app: &AppHandle, key: &str, status: AgentStatus, detail: Option<String>) {
        Self::emit(
            app,
            AgentEvent::Status {
                key: key.to_owned(),
                status,
                detail,
            },
        );
    }

    /// Starts (or loads, when `resume` is given) the session for `key` in `cwd`.
    /// If loading fails, a new session is created instead.
    pub async fn start(
        &self,
        key: String,
        cwd: PathBuf,
        resume: Option<String>,
    ) -> Result<Started, String> {
        let launch = self.launch.clone()?;
        if self.sessions.lock().unwrap().contains_key(&key) {
            return Err(format!("agent for {key} is already running"));
        }
        let (tx, rx) = mpsc::unbounded_channel();
        let (started_tx, started_rx) = oneshot::channel();
        self.sessions
            .lock()
            .unwrap()
            .insert(key.clone(), SessionHandle { tx });

        Self::status(&self.app, &key, AgentStatus::Starting, None);
        let task = SessionTask {
            app: self.app.clone(),
            key: key.clone(),
            cwd,
            resume,
            pending: self.pending.clone(),
            next_request: self.next_request.clone(),
        };
        let app = self.app.clone();
        tokio::spawn(async move {
            let result = task.run(launch, rx, started_tx).await;
            let (status, detail) = match result {
                Ok(()) => (AgentStatus::Stopped, None),
                Err(e) => (AgentStatus::Error, Some(e.to_string())),
            };
            app.state::<crate::AppState>()
                .agents
                .sessions
                .lock()
                .unwrap()
                .remove(&key);
            Self::status(&app, &key, status, detail);
        });

        started_rx
            .await
            .map_err(|_| "agent exited during startup".to_owned())?
    }

    fn send(&self, key: &str, command: Command) -> Result<(), String> {
        let sessions = self.sessions.lock().unwrap();
        let handle = sessions
            .get(key)
            .ok_or_else(|| format!("no agent running for {key}"))?;
        handle
            .tx
            .send(command)
            .map_err(|_| "agent has stopped".to_owned())
    }

    pub fn prompt(&self, key: &str, text: String) -> Result<(), String> {
        self.send(key, Command::Prompt(text))
    }

    pub fn cancel(&self, key: &str) -> Result<(), String> {
        self.send(key, Command::Cancel)
    }

    /// Dropping the handle closes the command channel; the session task then ends
    /// and the connection's guard kills the adapter's process group.
    pub fn stop(&self, key: &str) {
        self.sessions.lock().unwrap().remove(key);
    }

    pub fn answer_permission(
        &self,
        request_id: &str,
        option_id: Option<String>,
    ) -> Result<(), String> {
        let sender = self
            .pending
            .lock()
            .unwrap()
            .remove(request_id)
            .ok_or_else(|| format!("unknown permission request {request_id}"))?;
        sender
            .send(option_id)
            .map_err(|_| "agent no longer waiting".to_owned())
    }
}

struct SessionTask {
    app: AppHandle,
    key: String,
    cwd: PathBuf,
    resume: Option<String>,
    pending: Arc<Mutex<HashMap<String, oneshot::Sender<Option<String>>>>>,
    next_request: Arc<AtomicU64>,
}

impl SessionTask {
    async fn run(
        self,
        launch: AgentLaunch,
        mut rx: mpsc::UnboundedReceiver<Command>,
        started_tx: oneshot::Sender<Result<Started, String>>,
    ) -> Result<(), agent_client_protocol::Error> {
        let agent =
            AcpAgent::new(AcpAgentConfig::new(&launch.node).arg(launch.adapter.to_string_lossy()))
                .with_debug(|line, dir| {
                    if matches!(dir, agent_client_protocol::LineDirection::Stderr) {
                        tracing::debug!(target: "agent", "{line}");
                    }
                });

        let SessionTask {
            app,
            key,
            cwd,
            resume,
            pending,
            next_request,
        } = self;
        let (app_n, key_n) = (app.clone(), key.clone());
        let (app_p, key_p) = (app.clone(), key.clone());

        agent_client_protocol::Client
            .builder()
            .on_receive_notification(
                async move |n: SessionNotification, _cx| {
                    let update = serde_json::to_value(&n.update).unwrap_or_default();
                    AgentManager::emit(
                        &app_n,
                        AgentEvent::Update {
                            key: key_n.clone(),
                            update,
                        },
                    );
                    Ok(())
                },
                agent_client_protocol::on_receive_notification!(),
            )
            .on_receive_request(
                async move |req: RequestPermissionRequest, responder, cx| {
                    let request_id = next_request.fetch_add(1, Ordering::Relaxed).to_string();
                    let (tx, rx) = oneshot::channel();
                    pending.lock().unwrap().insert(request_id.clone(), tx);
                    AgentManager::emit(
                        &app_p,
                        AgentEvent::Permission {
                            key: key_p.clone(),
                            request_id: request_id.clone(),
                            title: req.tool_call.fields.title.clone(),
                            options: req
                                .options
                                .iter()
                                .map(|o| PermissionOptionView {
                                    id: o.option_id.to_string(),
                                    name: o.name.clone(),
                                    kind: serde_json::to_value(o.kind).unwrap_or_default(),
                                })
                                .collect(),
                        },
                    );
                    AgentManager::status(&app_p, &key_p, AgentStatus::WaitingForPermission, None);

                    // Waiting for the user must not block the connection's event loop.
                    let (app, key) = (app_p.clone(), key_p.clone());
                    cx.spawn(async move {
                        let outcome = match rx.await {
                            Ok(Some(id)) => RequestPermissionOutcome::Selected(
                                SelectedPermissionOutcome::new(id),
                            ),
                            _ => RequestPermissionOutcome::Cancelled,
                        };
                        AgentManager::emit(
                            &app,
                            AgentEvent::PermissionResolved {
                                key: key.clone(),
                                request_id,
                            },
                        );
                        AgentManager::status(&app, &key, AgentStatus::Working, None);
                        responder.respond(RequestPermissionResponse::new(outcome))
                    })
                },
                agent_client_protocol::on_receive_request!(),
            )
            .connect_with(agent, async move |cx: ConnectionTo<Agent>| {
                let opened = open_session(&cx, &cwd, resume).await;
                let (session_id, loaded) = match opened {
                    Ok(v) => v,
                    Err(e) => {
                        let _ = started_tx.send(Err(e.to_string()));
                        return Err(e);
                    }
                };
                let _ = started_tx.send(Ok(Started {
                    key: key.clone(),
                    session_id: session_id.to_string(),
                    loaded,
                }));
                AgentManager::status(&app, &key, AgentStatus::Ready, None);

                while let Some(command) = rx.recv().await {
                    match command {
                        Command::Prompt(text) => {
                            AgentManager::status(&app, &key, AgentStatus::Working, None);
                            let request = PromptRequest::new(
                                session_id.clone(),
                                vec![ContentBlock::Text(TextContent::new(text))],
                            );
                            let (app, key, cx2) = (app.clone(), key.clone(), cx.clone());
                            cx.spawn(async move {
                                let (status, detail) =
                                    match cx2.send_request(request).block_task().await {
                                        Ok(r) => (
                                            AgentStatus::Finished,
                                            Some(format!("{:?}", r.stop_reason)),
                                        ),
                                        Err(e) => (AgentStatus::Error, Some(e.to_string())),
                                    };
                                AgentManager::status(&app, &key, status, detail);
                                Ok(())
                            })?;
                        }
                        Command::Cancel => {
                            cx.send_notification(CancelNotification::new(session_id.clone()))?;
                        }
                    }
                }
                Ok(())
            })
            .await
    }
}

async fn open_session(
    cx: &ConnectionTo<Agent>,
    cwd: &Path,
    resume: Option<String>,
) -> Result<(SessionId, bool), agent_client_protocol::Error> {
    let init = cx
        .send_request(InitializeRequest::new(ProtocolVersion::V1))
        .block_task()
        .await?;

    let mut loaded = None;
    if let Some(id) = resume.filter(|_| init.agent_capabilities.load_session) {
        let id = SessionId::from(id);
        match cx
            .send_request(LoadSessionRequest::new(id.clone(), cwd).meta(session_meta()))
            .block_task()
            .await
        {
            Ok(_) => loaded = Some(id),
            Err(e) => tracing::warn!("session/load failed, starting a new session: {e}"),
        }
    }
    let (session_id, was_loaded) = match loaded {
        Some(id) => (id, true),
        None => {
            let s = cx
                .send_request(NewSessionRequest::new(cwd).meta(session_meta()))
                .block_task()
                .await?;
            (s.session_id, false)
        }
    };
    Ok((session_id, was_loaded))
}
