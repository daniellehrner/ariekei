//! Spike probe: drive the Claude ACP adapter headlessly against a git worktree.
//!
//! Checks: spawn + initialize, capabilities (load/resume), a permission round-trip,
//! and whether a session survives killing the agent process (`session/load`).
//!
//! Usage: cargo run --example acp_probe -- <node> <adapter-index.js> <worktree-dir>

use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::schema::v1::{
    ContentBlock, InitializeRequest, LoadSessionRequest, NewSessionRequest, PermissionOptionKind,
    PromptRequest, RequestPermissionOutcome, RequestPermissionRequest, RequestPermissionResponse,
    SelectedPermissionOutcome, SessionId, SessionNotification, SetSessionModeRequest, TextContent,
};
use agent_client_protocol::{AcpAgent, AcpAgentConfig, Agent, ConnectionTo};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

fn agent(node: &str, script: &str) -> AcpAgent {
    AcpAgent::new(AcpAgentConfig::new(node).arg(script)).with_debug(|line, dir| {
        if matches!(dir, agent_client_protocol::LineDirection::Stderr) {
            eprintln!("  [agent stderr] {line}");
        }
    })
}

fn update_kind(n: &SessionNotification) -> String {
    serde_json::to_value(&n.update)
        .ok()
        .and_then(|v| {
            v.get("sessionUpdate")
                .and_then(|s| s.as_str())
                .map(str::to_owned)
        })
        .unwrap_or_else(|| "?".into())
}

async fn run(
    node: &str,
    script: &str,
    cwd: PathBuf,
    load: Option<SessionId>,
    prompt: &str,
) -> Result<SessionId, agent_client_protocol::Error> {
    let notifications = Arc::new(AtomicUsize::new(0));
    let seen = notifications.clone();
    let prompt = prompt.to_owned();
    let started = Instant::now();

    agent_client_protocol::Client
        .builder()
        .on_receive_notification(
            async move |n: SessionNotification, _cx| {
                seen.fetch_add(1, Ordering::Relaxed);
                let kind = update_kind(&n);
                if kind != "agent_message_chunk" && kind != "agent_thought_chunk" {
                    println!("  <- update: {kind}");
                }
                Ok(())
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_request(
            async move |req: RequestPermissionRequest, responder, _cx| {
                println!(
                    "  <- permission request: {:?} options={:?}",
                    req.tool_call.fields.title,
                    req.options
                        .iter()
                        .map(|o| (&o.name, &o.kind))
                        .collect::<Vec<_>>()
                );
                let allow = req
                    .options
                    .iter()
                    .find(|o| matches!(o.kind, PermissionOptionKind::AllowOnce))
                    .map(|o| o.option_id.clone());
                let outcome = match allow {
                    Some(id) => {
                        RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(id))
                    }
                    None => RequestPermissionOutcome::Cancelled,
                };
                responder.respond(RequestPermissionResponse::new(outcome))
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_with(agent(node, script), async move |cx: ConnectionTo<Agent>| {
            let init = cx
                .send_request(InitializeRequest::new(ProtocolVersion::V1))
                .block_task()
                .await?;
            println!(
                "  initialized in {:?}: {:?}",
                started.elapsed(),
                init.agent_info
            );
            println!(
                "  capabilities: load_session={} session_capabilities={}",
                init.agent_capabilities.load_session,
                serde_json::to_string(&init.agent_capabilities.session_capabilities).unwrap()
            );

            let session_id = match load {
                Some(id) => {
                    let t = Instant::now();
                    cx.send_request(LoadSessionRequest::new(id.clone(), cwd.clone()))
                        .block_task()
                        .await?;
                    println!(
                        "  session/load ok in {:?}, replayed {} notifications",
                        t.elapsed(),
                        notifications.load(Ordering::Relaxed)
                    );
                    id
                }
                None => {
                    let s = cx
                        .send_request(NewSessionRequest::new(cwd.clone()))
                        .block_task()
                        .await?;
                    println!("  new session {}", s.session_id);
                    if let Some(modes) = &s.modes {
                        println!(
                            "  modes: current={} available={:?}",
                            modes.current_mode_id,
                            modes
                                .available_modes
                                .iter()
                                .map(|m| m.id.to_string())
                                .collect::<Vec<_>>()
                        );
                    }
                    // Don't inherit the user's global permission mode (e.g. "auto"):
                    // the IDE decides, so permission requests reach the client.
                    cx.send_request(SetSessionModeRequest::new(s.session_id.clone(), "default"))
                        .block_task()
                        .await?;
                    println!("  mode set to default");
                    s.session_id
                }
            };

            println!("  -> prompt: {prompt}");
            let t = Instant::now();
            let resp = cx
                .send_request(PromptRequest::new(
                    session_id.clone(),
                    vec![ContentBlock::Text(TextContent::new(prompt))],
                ))
                .block_task()
                .await?;
            println!(
                "  turn finished in {:?}: {:?}",
                t.elapsed(),
                resp.stop_reason
            );
            Ok(session_id)
        })
        .await
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let [_, node, script, cwd] = args.as_slice() else {
        return Err("usage: acp_probe <node> <adapter-index.js> <worktree-dir>".into());
    };
    let cwd = std::fs::canonicalize(cwd)?;

    println!("== run 1: new session, write a file (expects a permission request)");
    let id = run(
        node,
        script,
        cwd.clone(),
        None,
        "Create a file named hello.txt containing exactly the word hi. Do nothing else.",
    )
    .await?;
    println!("  hello.txt exists: {}", cwd.join("hello.txt").exists());

    println!("== run 2: agent process was killed; reload session {id}");
    run(
        node,
        script,
        cwd,
        Some(id),
        "Reply with only the name of the file you created earlier in this conversation.",
    )
    .await?;
    Ok(())
}
