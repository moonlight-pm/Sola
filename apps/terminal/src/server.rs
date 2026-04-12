use std::sync::Arc;

use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio::sync::{broadcast, mpsc};
use tokio_tungstenite::tungstenite::Message;
use tracing::{debug, info, warn};

use crate::pty::PtyEvent;
use crate::state::{TabEntry, TerminalState};

/// Events forwarded from the glib bus polling to the WS server.
pub enum BusEvent {
    NewTab,
}

/// Embedded frontend assets (built by vite, included at compile time).
struct Assets {
    html: String,
    js: &'static str,
    css: &'static str,
}

/// Start the HTTP + WebSocket server. Binds to 127.0.0.1:0 (ephemeral port).
/// Serves frontend assets on GET and handles WebSocket upgrades.
/// Returns the bound port.
pub async fn start(
    state: Arc<TerminalState>,
    html_template: String,
    mut bus_rx: mpsc::UnboundedReceiver<BusEvent>,
) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind server");
    let port = listener.local_addr().unwrap().port();
    info!("server listening on 127.0.0.1:{port}");

    let (bus_tx, _) = broadcast::channel::<String>(64);
    let bus_broadcast = bus_tx.clone();

    tokio::spawn(async move {
        while let Some(event) = bus_rx.recv().await {
            let msg = match event {
                BusEvent::NewTab => json!({ "event": "new_tab" }).to_string(),
            };
            let _ = bus_broadcast.send(msg);
        }
    });

    let assets = Arc::new(Assets {
        html: html_template.replace("__WS_PORT__", &port.to_string()),
        js: include_str!("../web/dist/app.js"),
        css: include_str!("../web/dist/app.css"),
    });

    tokio::spawn(async move {
        loop {
            let (stream, addr) = match listener.accept().await {
                Ok(conn) => conn,
                Err(e) => {
                    warn!("TCP accept failed: {e}");
                    continue;
                }
            };

            let state = state.clone();
            let bus_rx = bus_tx.subscribe();
            let assets = assets.clone();

            tokio::spawn(async move {
                let mut buf = [0u8; 512];
                let n = match stream.peek(&mut buf).await {
                    Ok(n) => n,
                    Err(e) => {
                        debug!("peek failed from {addr}: {e}");
                        return;
                    }
                };
                let req = String::from_utf8_lossy(&buf[..n]);

                if req.to_ascii_lowercase().contains("upgrade: websocket") {
                    handle_connection(state, stream, bus_rx).await;
                } else if req.starts_with("GET /app.js") {
                    serve_asset(stream, "application/javascript", assets.js).await;
                } else if req.starts_with("GET /app.css") {
                    serve_asset(stream, "text/css", assets.css).await;
                } else if req.starts_with("GET") {
                    serve_asset(stream, "text/html; charset=utf-8", &assets.html).await;
                } else {
                    handle_connection(state, stream, bus_rx).await;
                }
            });
        }
    });

    port
}

/// Serve a static asset as an HTTP response.
/// Reads and discards the full HTTP request before responding.
async fn serve_asset(mut stream: tokio::net::TcpStream, content_type: &str, body: &str) {
    info!("serving {content_type} ({} bytes)", body.len());
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    // Read and discard the HTTP request (the peek didn't consume it).
    // Read until we see \r\n\r\n (end of headers).
    let mut req_buf = vec![0u8; 4096];
    let _ = stream.read(&mut req_buf).await;

    let header = format!(
        "HTTP/1.1 200 OK\r\n\
         Content-Type: {content_type}\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\
         \r\n",
        body.len(),
    );
    if stream.write_all(header.as_bytes()).await.is_err() {
        return;
    }
    if stream.write_all(body.as_bytes()).await.is_err() {
        return;
    }
    let _ = stream.flush().await;
    let _ = stream.shutdown().await;
}

async fn handle_connection(
    state: Arc<TerminalState>,
    stream: tokio::net::TcpStream,
    mut bus_rx: broadcast::Receiver<String>,
) {
    let ws = match tokio_tungstenite::accept_async(stream).await {
        Ok(ws) => ws,
        Err(e) => {
            warn!("WebSocket handshake failed: {e}");
            return;
        }
    };

    let (mut ws_sink, mut ws_stream) = ws.split();

    // Per-client channel for responses and PTY events
    let (client_tx, mut client_rx) = mpsc::unbounded_channel::<String>();

    // Send task: multiplex client_rx and bus broadcast
    let send_task = tokio::spawn(async move {
        loop {
            tokio::select! {
                msg = client_rx.recv() => {
                    match msg {
                        Some(text) => {
                            if ws_sink.send(Message::Text(text.into())).await.is_err() {
                                break;
                            }
                        }
                        None => break,
                    }
                }
                msg = bus_rx.recv() => {
                    match msg {
                        Ok(text) => {
                            if ws_sink.send(Message::Text(text.into())).await.is_err() {
                                break;
                            }
                        }
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            warn!("Bus broadcast lagged, dropped {n} messages");
                        }
                        Err(broadcast::error::RecvError::Closed) => break,
                    }
                }
            }
        }
    });

    // Recv loop: parse commands and dispatch
    while let Some(msg) = ws_stream.next().await {
        let msg = match msg {
            Ok(Message::Text(text)) => text.to_string(),
            Ok(Message::Close(_)) => break,
            Ok(_) => continue,
            Err(e) => {
                debug!("WebSocket recv error: {e}");
                break;
            }
        };

        let parsed: Value = match serde_json::from_str(&msg) {
            Ok(v) => v,
            Err(e) => {
                warn!("Invalid JSON from client: {e}");
                continue;
            }
        };

        let id = parsed.get("id").and_then(|v| v.as_u64());
        let cmd = parsed.get("cmd").and_then(|v| v.as_str()).unwrap_or("");
        let args = parsed.get("args").cloned().unwrap_or(json!({}));

        let result = dispatch(&state, cmd, &args, &client_tx).await;

        if let Some(id) = id {
            let response = json!({ "id": id, "result": result });
            let _ = client_tx.send(response.to_string());
        }
    }

    send_task.abort();
    debug!("WebSocket connection closed");
}

async fn dispatch(
    state: &Arc<TerminalState>,
    cmd: &str,
    args: &Value,
    client_tx: &mpsc::UnboundedSender<String>,
) -> Value {
    match cmd {
        "spawn_pty" => cmd_spawn_pty(state, args, client_tx).await,
        "write_pty" => cmd_write_pty(state, args).await,
        "resize_pty" => cmd_resize_pty(state, args).await,
        "close_pty" => cmd_close_pty(state, args).await,
        "reconnect_pty" => cmd_reconnect_pty(state, args).await,
        "rename_tab" => cmd_rename_tab(state, args).await,
        "update_cwd" => cmd_update_cwd(state, args).await,
        "reorder_tabs" => cmd_reorder_tabs(state, args).await,
        _ => json!({ "error": format!("unknown command: {cmd}") }),
    }
}

async fn cmd_spawn_pty(
    state: &Arc<TerminalState>,
    args: &Value,
    client_tx: &mpsc::UnboundedSender<String>,
) -> Value {
    let cols = args.get("cols").and_then(|v| v.as_u64()).unwrap_or(80) as u16;
    let rows = args.get("rows").and_then(|v| v.as_u64()).unwrap_or(24) as u16;
    let tmux_session = args
        .get("tmuxSession")
        .and_then(|v| v.as_str())
        .map(String::from);
    let cwd = args
        .get("cwd")
        .and_then(|v| v.as_str())
        .map(String::from);

    let pty_id = uuid::Uuid::new_v4().to_string();
    let (event_tx, event_rx) = mpsc::unbounded_channel::<PtyEvent>();

    let tmux_session_name = {
        let mut mgr = state.pty_manager.lock().await;
        match mgr.spawn_pty(
            pty_id.clone(),
            cols,
            rows,
            tmux_session,
            cwd.clone(),
            event_tx,
        ) {
            Ok(name) => name,
            Err(e) => return json!({ "error": e }),
        }
    };

    // Look up custom title if reattaching (keyed by tmux session name)
    let title = state
        .custom_titles
        .read()
        .await
        .get(&tmux_session_name)
        .cloned();

    // Add tab entry
    {
        let mut tabs = state.tabs.write().await;
        tabs.push(TabEntry {
            pty_id: pty_id.clone(),
            tmux_session: tmux_session_name.clone(),
            custom_title: title.clone(),
            cwd,
        });
    }

    // Start PTY event forwarding for this client
    let tx = client_tx.clone();
    tokio::spawn(forward_pty_events(pty_id.clone(), event_rx, tx));

    state.persist_to_disk().await;

    json!({
        "pty_id": pty_id,
        "tmux_session": tmux_session_name,
        "title": title,
    })
}

async fn cmd_write_pty(state: &Arc<TerminalState>, args: &Value) -> Value {
    let pty_id = match args.get("pty_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return json!({ "error": "missing pty_id" }),
    };
    let data_b64 = match args.get("data").and_then(|v| v.as_str()) {
        Some(d) => d,
        None => return json!({ "error": "missing data" }),
    };
    let data = match base64::engine::general_purpose::STANDARD.decode(data_b64) {
        Ok(d) => d,
        Err(e) => return json!({ "error": format!("base64 decode failed: {e}") }),
    };

    let mgr = state.pty_manager.lock().await;
    match mgr.write_pty(pty_id, &data) {
        Ok(()) => json!("ok"),
        Err(e) => json!({ "error": e }),
    }
}

async fn cmd_resize_pty(state: &Arc<TerminalState>, args: &Value) -> Value {
    let pty_id = match args.get("pty_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return json!({ "error": "missing pty_id" }),
    };
    let cols = match args.get("cols").and_then(|v| v.as_u64()) {
        Some(c) => c as u16,
        None => return json!({ "error": "missing cols" }),
    };
    let rows = match args.get("rows").and_then(|v| v.as_u64()) {
        Some(r) => r as u16,
        None => return json!({ "error": "missing rows" }),
    };

    let mgr = state.pty_manager.lock().await;
    match mgr.resize_pty(pty_id, cols, rows) {
        Ok(()) => {}
        Err(e) => return json!({ "error": e }),
    }
    match mgr.sigwinch_pty(pty_id) {
        Ok(()) => json!("ok"),
        Err(e) => json!({ "error": e }),
    }
}

async fn cmd_close_pty(state: &Arc<TerminalState>, args: &Value) -> Value {
    let pty_id = match args.get("pty_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return json!({ "error": "missing pty_id" }),
    };

    {
        let mut mgr = state.pty_manager.lock().await;
        if let Err(e) = mgr.close_pty(pty_id) {
            return json!({ "error": e });
        }
    }

    {
        let mut tabs = state.tabs.write().await;
        tabs.retain(|t| t.pty_id != pty_id);
    }

    state.persist_to_disk().await;

    json!("ok")
}

async fn cmd_reconnect_pty(state: &Arc<TerminalState>, args: &Value) -> Value {
    let pty_id = match args.get("pty_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return json!({ "error": "missing pty_id" }),
    };

    let mgr = state.pty_manager.lock().await;
    match mgr.reconnect_pty(pty_id) {
        Ok(scrollback) => json!({ "scrollback": scrollback }),
        Err(e) => json!({ "error": e }),
    }
}

async fn cmd_rename_tab(state: &Arc<TerminalState>, args: &Value) -> Value {
    let tmux_session = match args.get("tmux_session").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => return json!({ "error": "missing tmux_session" }),
    };
    let title = match args.get("title").and_then(|v| v.as_str()) {
        Some(t) => t.to_string(),
        None => return json!({ "error": "missing title" }),
    };

    // Verify the session exists
    {
        let tabs = state.tabs.read().await;
        if !tabs.iter().any(|t| t.tmux_session == tmux_session) {
            return json!({ "error": format!("no tab for session: {tmux_session}") });
        }
    }

    state
        .custom_titles
        .write()
        .await
        .insert(tmux_session.to_string(), title);

    state.persist_to_disk().await;

    json!("ok")
}

async fn cmd_update_cwd(state: &Arc<TerminalState>, args: &Value) -> Value {
    let pty_id = match args.get("pty_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return json!({ "error": "missing pty_id" }),
    };
    let cwd = match args.get("cwd").and_then(|v| v.as_str()) {
        Some(c) => c.to_string(),
        None => return json!({ "error": "missing cwd" }),
    };

    {
        let mut tabs = state.tabs.write().await;
        if let Some(tab) = tabs.iter_mut().find(|t| t.pty_id == pty_id) {
            tab.cwd = Some(cwd);
        }
    }

    state.persist_to_disk().await;

    json!("ok")
}

async fn cmd_reorder_tabs(state: &Arc<TerminalState>, args: &Value) -> Value {
    let pty_ids: Vec<String> = match args.get("pty_ids").and_then(|v| v.as_array()) {
        Some(arr) => arr
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect(),
        None => return json!({ "error": "missing pty_ids" }),
    };

    {
        let mut tabs = state.tabs.write().await;
        let mut reordered = Vec::with_capacity(pty_ids.len());
        for id in &pty_ids {
            if let Some(tab) = tabs.iter().find(|t| &t.pty_id == id).cloned() {
                reordered.push(tab);
            }
        }
        *tabs = reordered;
    }

    state.persist_to_disk().await;

    json!("ok")
}

async fn forward_pty_events(
    _pty_id: String,
    mut event_rx: mpsc::UnboundedReceiver<PtyEvent>,
    tx: mpsc::UnboundedSender<String>,
) {
    let b64 = base64::engine::general_purpose::STANDARD;

    while let Some(event) = event_rx.recv().await {
        let msg = match event {
            PtyEvent::Data { pty_id, data } => {
                json!({
                    "event": "pty:data",
                    "pty_id": pty_id,
                    "data": b64.encode(&data),
                })
            }
            PtyEvent::Scrollback { pty_id, data } => {
                json!({
                    "event": "pty:scrollback",
                    "pty_id": pty_id,
                    "data": b64.encode(&data),
                })
            }
            PtyEvent::Exit { pty_id } => {
                let msg = json!({
                    "event": "pty:exit",
                    "pty_id": pty_id,
                });
                let _ = tx.send(msg.to_string());
                break;
            }
        };
        if tx.send(msg.to_string()).is_err() {
            break;
        }
    }
}
