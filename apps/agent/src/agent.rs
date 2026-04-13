use crate::session::{SessionManager, SessionStatus};
use anyhow::Result;
use serde_json::json;
use std::path::PathBuf;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;

pub async fn run_session_message(
    session_id: String,
    text: String,
    working_dir: PathBuf,
    session_mgr: Arc<SessionManager>,
    event_tx: std::sync::mpsc::Sender<String>,
    bus_tools: Vec<Box<dyn claurst_tools::Tool>>,
    cancel_token: tokio_util::sync::CancellationToken,
) {
    session_mgr
        .set_status(&session_id, SessionStatus::Running)
        .await;
    send_event(&event_tx, json!({
        "event": "session_state", "session_id": session_id, "status": "running"
    }));

    match run_agent_loop(
        &session_id, &text, &working_dir, &session_mgr, &event_tx,
        bus_tools, cancel_token,
    )
    .await
    {
        Ok(()) => {
            session_mgr.set_status(&session_id, SessionStatus::Idle).await;
            send_event(&event_tx, json!({
                "event": "session_state", "session_id": session_id, "status": "idle"
            }));
        }
        Err(e) => {
            let msg = format!("{:#}", e);
            tracing::error!("Agent error for session {}: {}", session_id, msg);
            session_mgr
                .set_status(&session_id, SessionStatus::Error(msg.clone()))
                .await;
            send_event(&event_tx, json!({
                "event": "error", "session_id": session_id, "message": msg
            }));
        }
    }
}

fn send_event(tx: &std::sync::mpsc::Sender<String>, value: serde_json::Value) {
    let _ = tx.send(value.to_string());
}

async fn run_agent_loop(
    session_id: &str,
    text: &str,
    working_dir: &PathBuf,
    session_mgr: &SessionManager,
    event_tx: &std::sync::mpsc::Sender<String>,
    bus_tools: Vec<Box<dyn claurst_tools::Tool>>,
    cancel_token: tokio_util::sync::CancellationToken,
) -> Result<()> {
    // Route through Junction proxy (handles OAuth attestation)
    let api_key = std::env::var("JUNCTION_API_KEY")
        .unwrap_or_else(|_| "a626d79941d9c85a4f7015b3b69805f0df659406f1701ccd3aa07bb7123f50a3".into());
    let api_base = std::env::var("JUNCTION_URL")
        .unwrap_or_else(|_| "https://junction.moonlight.pm".into());

    let client_config = claurst_api::client::ClientConfig {
        api_key,
        api_base,
        ..Default::default()
    };
    let client = claurst_api::AnthropicClient::new(client_config)?;

    let mut tools: Vec<Box<dyn claurst_tools::Tool>> = claurst_tools::all_tools();
    tools.extend(bus_tools);

    let cost_tracker = claurst_core::cost::CostTracker::new();

    let tool_ctx = claurst_tools::ToolContext {
        working_dir: working_dir.clone(),
        permission_mode: claurst_core::config::PermissionMode::BypassPermissions,
        permission_handler: Arc::new(claurst_core::permissions::AutoPermissionHandler {
            mode: claurst_core::config::PermissionMode::BypassPermissions,
        }),
        cost_tracker: cost_tracker.clone(),
        session_id: session_id.to_string(),
        file_history: Arc::new(parking_lot::Mutex::new(
            claurst_core::file_history::FileHistory::new(),
        )),
        current_turn: Arc::new(AtomicUsize::new(0)),
        non_interactive: true,
        mcp_manager: None,
        config: claurst_core::config::Config::default(),
        managed_agent_config: None,
        completion_notifier: None,
    };

    let config = claurst_query::QueryConfig {
        model: "claude-opus-4-6".into(),
        max_tokens: 16384,
        max_turns: 30,
        system_prompt: Some(build_system_prompt(working_dir)),
        ..Default::default()
    };

    let user_msg = claurst_core::types::Message::user(text);
    let mut messages = {
        let sessions = session_mgr.sessions.read().await;
        let session = sessions.get(session_id).unwrap();
        let mut msgs = session.messages.clone();
        msgs.push(user_msg);
        msgs
    };

    let (query_event_tx, mut query_event_rx) =
        tokio::sync::mpsc::unbounded_channel::<claurst_query::QueryEvent>();

    let event_tx_clone = event_tx.clone();
    let sid = session_id.to_string();
    let event_forwarder = tokio::spawn(async move {
        while let Some(qe) = query_event_rx.recv().await {
            for evt in translate_query_event(&sid, qe) {
                let _ = event_tx_clone.send(evt.to_string());
            }
        }
    });

    let outcome = claurst_query::run_query_loop(
        &client, &mut messages, &tools, &tool_ctx, &config,
        cost_tracker, Some(query_event_tx), cancel_token, None,
    )
    .await;

    let _ = event_forwarder.await;

    {
        let mut sessions = session_mgr.sessions.write().await;
        if let Some(session) = sessions.get_mut(session_id) {
            session.messages = messages;
        }
    }

    match outcome {
        claurst_query::QueryOutcome::EndTurn { .. } => Ok(()),
        claurst_query::QueryOutcome::Cancelled => Ok(()),
        claurst_query::QueryOutcome::Error(e) => Err(anyhow::anyhow!("{}", e)),
        claurst_query::QueryOutcome::MaxTokens { .. } => Ok(()),
        claurst_query::QueryOutcome::BudgetExceeded { cost_usd, limit_usd } => {
            Err(anyhow::anyhow!("Budget exceeded: ${:.2} / ${:.2}", cost_usd, limit_usd))
        }
    }
}

fn translate_query_event(session_id: &str, event: claurst_query::QueryEvent) -> Vec<serde_json::Value> {
    match event {
        claurst_query::QueryEvent::Stream(stream_event) => {
            translate_stream_event(session_id, stream_event)
        }
        claurst_query::QueryEvent::ToolStart { tool_name, input_json, .. } => {
            vec![json!({
                "event": "tool_start", "session_id": session_id,
                "tool_name": tool_name, "tool_input": input_json
            })]
        }
        claurst_query::QueryEvent::ToolEnd { tool_name, result, is_error, .. } => {
            vec![json!({
                "event": "tool_end", "session_id": session_id,
                "tool_name": tool_name, "result": result, "is_error": is_error
            })]
        }
        _ => vec![],
    }
}

fn translate_stream_event(session_id: &str, event: claurst_api::streaming::AnthropicStreamEvent) -> Vec<serde_json::Value> {
    match event {
        claurst_api::streaming::AnthropicStreamEvent::MessageStart { .. } => {
            vec![json!({"event": "message_start", "session_id": session_id})]
        }
        claurst_api::streaming::AnthropicStreamEvent::ContentBlockDelta { delta, .. } => {
            match delta {
                claurst_api::streaming::ContentDelta::TextDelta { text } => {
                    vec![json!({"event": "message_delta", "session_id": session_id, "text": text})]
                }
                _ => vec![],
            }
        }
        claurst_api::streaming::AnthropicStreamEvent::MessageStop => {
            vec![json!({"event": "message_end", "session_id": session_id})]
        }
        _ => vec![],
    }
}

fn build_system_prompt(working_dir: &PathBuf) -> String {
    format!(
        "You are a coding assistant running inside Sola, a Wayland desktop environment. \
         Your working directory is: {}\n\n\
         You have access to standard coding tools (bash, file read/write/edit, grep, glob, web search) \
         plus Sola-specific tools (raise_app, launch_app, list_apps) for interacting with the desktop.\n\n\
         Be concise and direct. When editing code, show the changes clearly.",
        working_dir.display()
    )
}
