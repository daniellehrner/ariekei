// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

//! Remote control for the end-to-end tests (cargo feature `e2e`, never in release builds).
//!
//! With `ARIEKEI_E2E_PORT` set, the app serves a small HTTP API on 127.0.0.1 that runs
//! JavaScript in its windows and returns the result, lists and resizes the windows, and quits
//! the app the way a user would. `e2e/wd.py` drives the app through it on Linux, macOS and
//! Windows alike (WebDriver for Tauri's webviews does not exist on macOS).
//!
//!   GET  /status                              → "ok"
//!   GET  /windows                             → ["main", "w1", …]
//!   POST /eval    {window, script, args, timeout_ms} → the script's result (a function body)
//!   POST /resize  {window, width, height}     → {}
//!   POST /quit                                → {}

use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Listener, LogicalSize, Manager};

const RESULT_EVENT: &str = "e2e-result";

#[derive(Default)]
struct Bridge {
    next: AtomicU64,
    /// Evaluations waiting for their result event, by id.
    waiting: Mutex<HashMap<u64, Sender<(bool, String)>>>,
}

#[derive(Deserialize)]
struct Outcome {
    id: u64,
    ok: bool,
    /// The result as JSON when ok, else the error.
    value: String,
}

#[derive(Deserialize)]
struct EvalRequest {
    window: String,
    script: String,
    #[serde(default)]
    args: Vec<Value>,
    /// How long to wait for the result (default 60 s).
    #[serde(default)]
    timeout_ms: Option<u64>,
}

#[derive(Deserialize)]
struct ResizeRequest {
    window: String,
    width: f64,
    height: f64,
}

/// Starts the bridge when `ARIEKEI_E2E_PORT` is set.
pub fn start(app: &AppHandle) {
    let Some(port) = std::env::var("ARIEKEI_E2E_PORT")
        .ok()
        .and_then(|p| p.parse::<u16>().ok())
    else {
        return;
    };
    let listener = match TcpListener::bind(("127.0.0.1", port)) {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("e2e bridge: cannot listen on {port}: {e}");
            return;
        }
    };
    let bridge = Arc::new(Bridge::default());
    let results = bridge.clone();
    app.listen_any(RESULT_EVENT, move |event| {
        let Ok(outcome) = serde_json::from_str::<Outcome>(event.payload()) else {
            return;
        };
        if let Some(tx) = results.waiting.lock().unwrap().remove(&outcome.id) {
            let _ = tx.send((outcome.ok, outcome.value));
        }
    });
    tracing::info!("e2e bridge on 127.0.0.1:{port}");
    let app = app.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let (app, bridge) = (app.clone(), bridge.clone());
            std::thread::spawn(move || {
                if let Err(e) = serve(stream, &app, &bridge) {
                    tracing::warn!("e2e bridge: {e}");
                }
            });
        }
    });
}

fn serve(mut stream: TcpStream, app: &AppHandle, bridge: &Bridge) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    let mut parts = request_line.split_whitespace();
    let (method, path) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    let mut length = 0;
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header)? == 0 || header.trim().is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':')
            && name.eq_ignore_ascii_case("content-length")
        {
            length = value.trim().parse().unwrap_or(0);
        }
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;

    let (status, reply) = match handle(method, path, &body, app, bridge) {
        Ok(v) => ("200 OK", v),
        Err(e) => ("500 Internal Server Error", json!({ "error": e })),
    };
    let text = reply.to_string();
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
        text.len()
    );
    stream.write_all(response.as_bytes())?;
    let _ = stream.shutdown(std::net::Shutdown::Write);
    Ok(())
}

fn handle(
    method: &str,
    path: &str,
    body: &[u8],
    app: &AppHandle,
    bridge: &Bridge,
) -> Result<Value, String> {
    match (method, path) {
        ("GET", "/status") => Ok(json!("ok")),
        ("GET", "/windows") => {
            let mut labels: Vec<String> = app.webview_windows().into_keys().collect();
            labels.sort_by_key(|l| (l != "main", l.clone()));
            Ok(json!(labels))
        }
        ("POST", "/eval") => {
            let req: EvalRequest = parse(body, "eval")?;
            eval(app, bridge, &req)
        }
        ("POST", "/resize") => {
            let req: ResizeRequest = parse(body, "resize")?;
            let window = app
                .get_webview_window(&req.window)
                .ok_or("no such window")?;
            window
                .set_size(LogicalSize::new(req.width, req.height))
                .map_err(|e| e.to_string())?;
            Ok(json!({}))
        }
        ("POST", "/quit") => {
            // As if the user quit: windows open now come back on the next launch.
            let app = app.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(100));
                crate::quit::quit_now(&app);
            });
            Ok(json!({}))
        }
        _ => Err(format!("unknown endpoint {method} {path}")),
    }
}

fn parse<T: serde::de::DeserializeOwned>(body: &[u8], what: &str) -> Result<T, String> {
    serde_json::from_slice(body).map_err(|e| format!("bad {what} request: {e}"))
}

/// Runs `script` (a function body; `arguments` are `args`, `await` allowed) in a window and
/// waits for its result, which comes back as an event.
fn eval(app: &AppHandle, bridge: &Bridge, req: &EvalRequest) -> Result<Value, String> {
    let window = app
        .get_webview_window(&req.window)
        .ok_or("no such window")?;
    let id = bridge.next.fetch_add(1, Ordering::Relaxed);
    let (tx, rx) = channel();
    bridge.waiting.lock().unwrap().insert(id, tx);
    let args = serde_json::to_string(&req.args).map_err(|e| e.to_string())?;
    let js = format!(
        r#"(async () => {{
  const send = (ok, value) => window.__TAURI_INTERNALS__.invoke("plugin:event|emit", {{ event: "{RESULT_EVENT}", payload: {{ id: {id}, ok, value }} }});
  try {{
    const r = await (async function () {{ {script}
    }}).apply(null, {args});
    send(true, JSON.stringify(r === undefined ? null : r, (k, v) => (v instanceof Element ? {{ element: true }} : v)));
  }} catch (e) {{
    send(false, String((e && e.stack) || e));
  }}
}})();"#,
        script = req.script,
    );
    let outcome = window.eval(&js).map_err(|e| e.to_string()).and_then(|()| {
        rx.recv_timeout(Duration::from_millis(req.timeout_ms.unwrap_or(60_000)))
            .map_err(|_| "no result in time (page not loaded?)".to_owned())
    });
    bridge.waiting.lock().unwrap().remove(&id);
    let (ok, value) = outcome?;
    if ok {
        serde_json::from_str(&value).map_err(|e| e.to_string())
    } else {
        Err(value)
    }
}
