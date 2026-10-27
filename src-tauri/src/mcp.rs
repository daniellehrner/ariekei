// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

//! Ariekei's tools for its agents: a small MCP server on localhost (Streamable HTTP, answered
//! with plain JSON, no event streams). Every `claude` Ariekei starts gets
//! `--mcp-config <claude-mcp.json>`, which names the agent's terminal in a header as the hooks
//! do (`hooks::TERMINAL_ENV`), so a call finds its work item. The tools are `drafts`'.

use crate::hooks::{TERMINAL_ENV, TERMINAL_HEADER, read_request, respond};
use serde_json::{Value, json};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The server's name for Claude Code: its tools are `mcp__ariekei__<tool>`.
pub const SERVER: &str = "ariekei";

/// Runs a tool: (terminal, tool, arguments) to the answer for the agent, or why it failed.
type Call = dyn Fn(Option<&str>, &str, &Value) -> Result<String, String> + Send + Sync;

pub struct McpServer {
    pub config_path: PathBuf,
}

impl McpServer {
    /// Binds a localhost port, writes the MCP config file into `data_dir` and serves requests,
    /// each connection on a thread of its own (a tool may run git). `tools` is the
    /// `tools/list` answer, `instructions` what the agent is told about them.
    pub fn start(
        data_dir: &Path,
        tools: Value,
        instructions: &'static str,
        call: impl Fn(Option<&str>, &str, &Value) -> Result<String, String> + Send + Sync + 'static,
    ) -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        let path = format!("/mcp/{}", uuid::Uuid::new_v4().simple());

        let config_path = data_dir.join("claude-mcp.json");
        let config = json!({ "mcpServers": { SERVER: {
            "type": "http",
            "url": format!("http://127.0.0.1:{port}{path}"),
            "headers": { TERMINAL_HEADER: format!("${{{TERMINAL_ENV}}}") },
        } } });
        std::fs::write(&config_path, serde_json::to_vec_pretty(&config)?)?;

        let server = Arc::new(Server {
            path,
            tools,
            instructions,
            call: Box::new(call),
        });
        std::thread::Builder::new()
            .name("mcp-server".into())
            .spawn(move || {
                for stream in listener.incoming().flatten() {
                    let server = server.clone();
                    std::thread::spawn(move || {
                        if let Err(e) = server.handle(stream) {
                            tracing::debug!("MCP request failed: {e}");
                        }
                    });
                }
            })?;
        Ok(Self { config_path })
    }
}

struct Server {
    path: String,
    tools: Value,
    instructions: &'static str,
    call: Box<Call>,
}

impl Server {
    fn handle(&self, stream: TcpStream) -> std::io::Result<()> {
        let request = read_request(&stream)?;
        if request.path != self.path {
            return respond(&stream, "404 Not Found", None);
        }
        // GET would open an event stream for messages from the server: there are none.
        if request.method != "POST" {
            return respond(&stream, "405 Method Not Allowed", None);
        }
        let Ok(message) = serde_json::from_slice::<Value>(&request.body) else {
            let error = rpc_error(Value::Null, -32700, "Parse error");
            return respond(
                &stream,
                "400 Bad Request",
                Some(&serde_json::to_vec(&error)?),
            );
        };
        match self.answer(&message, request.terminal.as_deref()) {
            Some(answer) => respond(&stream, "200 OK", Some(&serde_json::to_vec(&answer)?)),
            // A notification or a response: taken, nothing to answer.
            None => respond(&stream, "202 Accepted", None),
        }
    }

    /// The JSON-RPC answer to `message` from the agent in terminal `terminal`; `None` for a
    /// notification.
    fn answer(&self, message: &Value, terminal: Option<&str>) -> Option<Value> {
        let id = message.get("id")?.clone();
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        let result = match message["method"].as_str().unwrap_or_default() {
            "initialize" => json!({
                // Its own version: the little used here is the same in all of them.
                "protocolVersion": params["protocolVersion"].as_str().unwrap_or("2025-06-18"),
                "capabilities": { "tools": {} },
                "serverInfo": { "name": SERVER, "version": env!("CARGO_PKG_VERSION") },
                "instructions": self.instructions,
            }),
            "ping" => json!({}),
            "tools/list" => json!({ "tools": self.tools }),
            "tools/call" => {
                let name = params["name"].as_str().unwrap_or_default();
                let args = params.get("arguments").cloned().unwrap_or(json!({}));
                let (text, failed) = match (self.call)(terminal, name, &args) {
                    Ok(text) => (text, false),
                    Err(e) => (e, true),
                };
                json!({ "content": [{ "type": "text", "text": text }], "isError": failed })
            }
            _ => return Some(rpc_error(id, -32601, "Method not found")),
        };
        Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
    }
}

fn rpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::time::Duration;

    fn request(port: u16, method: &str, path: &str, body: &str) -> String {
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
        let request = format!(
            "{method} {path} HTTP/1.1\r\nHost: x\r\nX-Ariekei-Terminal: t1\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        s.write_all(request.as_bytes()).unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).unwrap();
        out
    }

    fn body(response: &str) -> Value {
        serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap()
    }

    #[test]
    fn serves_the_tools_to_the_agents_it_names() {
        let dir = tempfile::tempdir().unwrap();
        let server = McpServer::start(
            dir.path(),
            json!([{ "name": "echo" }]),
            "Use echo.",
            |terminal, name, args| match name {
                "echo" => Ok(format!("{} from {}", args["text"], terminal.unwrap_or("?"))),
                _ => Err(format!("no tool {name}")),
            },
        )
        .unwrap();
        let config: Value =
            serde_json::from_slice(&std::fs::read(&server.config_path).unwrap()).unwrap();
        let ariekei = &config["mcpServers"]["ariekei"];
        assert_eq!(
            ariekei["headers"]["X-Ariekei-Terminal"],
            "${ARIEKEI_TERMINAL}"
        );
        let url = ariekei["url"]
            .as_str()
            .unwrap()
            .trim_start_matches("http://127.0.0.1:");
        let (port, path) = url.split_once('/').unwrap();
        let (port, path) = (port.parse().unwrap(), format!("/{path}"));
        let post = |message: Value| request(port, "POST", &path, &message.to_string());

        let init = post(json!({"jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {"protocolVersion": "2025-11-25", "capabilities": {}}}));
        assert!(init.starts_with("HTTP/1.1 200"), "{init}");
        let result = &body(&init)["result"];
        assert_eq!(
            (
                result["protocolVersion"].as_str(),
                result["instructions"].as_str()
            ),
            (Some("2025-11-25"), Some("Use echo."))
        );

        let note = post(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
        assert!(note.starts_with("HTTP/1.1 202"), "{note}");
        assert_eq!(
            body(&post(
                json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})
            ))["result"]["tools"][0]["name"],
            "echo"
        );

        let call = |id: i64, name: &str| {
            body(&post(
                json!({"jsonrpc": "2.0", "id": id, "method": "tools/call",
                "params": {"name": name, "arguments": {"text": "hi"}}}),
            ))
        };
        let ok = call(3, "echo");
        assert_eq!(
            (
                ok["id"].as_i64(),
                &ok["result"]["content"][0]["text"],
                &ok["result"]["isError"]
            ),
            (Some(3), &json!("\"hi\" from t1"), &json!(false))
        );
        let failed = call(4, "nope");
        assert_eq!(
            (
                &failed["result"]["content"][0]["text"],
                &failed["result"]["isError"]
            ),
            (&json!("no tool nope"), &json!(true))
        );

        // A method it does not know (e.g. a newer protocol's discovery) is an error the client
        // falls back from.
        let unknown = body(&post(
            json!({"jsonrpc": "2.0", "id": "d", "method": "server/discover"}),
        ));
        assert_eq!(unknown["error"]["code"], -32601);
        assert!(request(port, "GET", &path, "").starts_with("HTTP/1.1 405"));
        assert!(request(port, "POST", "/mcp/wrong", "{}").starts_with("HTTP/1.1 404"));
    }
}
