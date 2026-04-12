use crate::auth::AuthManager;
use crate::bridge::Event;
use crate::session::{SessionManager, SessionStatus};
use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use tokio::sync::RwLock;

pub async fn run_session_message(
    session_id: String,
    text: String,
    working_dir: PathBuf,
    auth: Arc<RwLock<AuthManager>>,
    session_mgr: Arc<SessionManager>,
    event_tx: Arc<std::sync::mpsc::Sender<Event>>,
    bus_tools: Vec<Box<dyn claurst_tools::Tool>>,
    cancel_token: tokio_util::sync::CancellationToken,
) {
    session_mgr
        .set_status(&session_id, SessionStatus::Running)
        .await;
    let _ = event_tx.send(Event::SessionState {
        session_id: session_id.clone(),
        status: "running".into(),
    });

    match run_agent_loop(
        &session_id,
        &text,
        &working_dir,
        auth,
        &session_mgr,
        &event_tx,
        bus_tools,
        cancel_token,
    )
    .await
    {
        Ok(()) => {
            session_mgr
                .set_status(&session_id, SessionStatus::Idle)
                .await;
            let _ = event_tx.send(Event::SessionState {
                session_id,
                status: "idle".into(),
            });
        }
        Err(e) => {
            let msg = format!("{:#}", e);
            tracing::error!("Agent error for session {}: {}", session_id, msg);
            session_mgr
                .set_status(&session_id, SessionStatus::Error(msg.clone()))
                .await;
            let _ = event_tx.send(Event::Error {
                session_id: Some(session_id),
                message: msg,
            });
        }
    }
}

async fn run_agent_loop(
    session_id: &str,
    text: &str,
    working_dir: &PathBuf,
    auth: Arc<RwLock<AuthManager>>,
    session_mgr: &SessionManager,
    event_tx: &Arc<std::sync::mpsc::Sender<Event>>,
    bus_tools: Vec<Box<dyn claurst_tools::Tool>>,
    cancel_token: tokio_util::sync::CancellationToken,
) -> Result<()> {
    // Ensure token is valid
    auth.write().await.ensure_valid().await?;

    // Create API client with current token
    let token = auth.read().await.access_token().to_string();
    let client_config = claurst_api::client::ClientConfig {
        api_key: token,
        use_bearer_auth: true,
        ..Default::default()
    };
    let client = claurst_api::AnthropicClient::new(client_config)?;

    // Get tools: claurst built-in + bus tools
    let mut tools: Vec<Box<dyn claurst_tools::Tool>> = claurst_tools::all_tools();
    tools.extend(bus_tools);

    // CostTracker::new() returns Arc<CostTracker>
    let cost_tracker = claurst_core::cost::CostTracker::new();

    // Build tool context
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

    // Build query config
    let config = claurst_query::QueryConfig {
        model: "claude-sonnet-4-6-20250514".into(),
        max_tokens: 16384,
        max_turns: 30,
        system_prompt: Some(build_system_prompt(working_dir)),
        ..Default::default()
    };

    // Build messages
    let user_msg = claurst_core::types::Message::user(text);
    let mut messages = {
        let sessions = session_mgr.sessions.read().await;
        let session = sessions.get(session_id).unwrap();
        let mut msgs = session.messages.clone();
        msgs.push(user_msg);
        msgs
    };

    // Set up event channel for streaming
    let (query_event_tx, mut query_event_rx) =
        tokio::sync::mpsc::unbounded_channel::<claurst_query::QueryEvent>();

    // Forward query events to bridge events
    let event_tx_clone = event_tx.clone();
    let sid = session_id.to_string();
    let event_forwarder = tokio::spawn(async move {
        while let Some(qe) = query_event_rx.recv().await {
            for evt in translate_query_event(&sid, qe) {
                let _ = event_tx_clone.send(evt);
            }
        }
    });

    // Run the agent loop
    let outcome = claurst_query::run_query_loop(
        &client,
        &mut messages,
        &tools,
        &tool_ctx,
        &config,
        cost_tracker,
        Some(query_event_tx),
        cancel_token,
        None,
    )
    .await;

    let _ = event_forwarder.await;

    // Save messages back to session
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
        claurst_query::QueryOutcome::BudgetExceeded {
            cost_usd,
            limit_usd,
        } => Err(anyhow::anyhow!(
            "Budget exceeded: ${:.2} / ${:.2}",
            cost_usd,
            limit_usd
        )),
    }
}

fn translate_query_event(session_id: &str, event: claurst_query::QueryEvent) -> Vec<Event> {
    match event {
        claurst_query::QueryEvent::Stream(stream_event) => {
            translate_stream_event(session_id, stream_event)
        }
        claurst_query::QueryEvent::ToolStart {
            tool_name,
            input_json,
            ..
        } => vec![Event::ToolStart {
            session_id: session_id.into(),
            tool_name,
            tool_input: input_json,
        }],
        claurst_query::QueryEvent::ToolEnd {
            tool_name,
            result,
            is_error,
            ..
        } => vec![Event::ToolEnd {
            session_id: session_id.into(),
            tool_name,
            result,
            is_error,
        }],
        _ => vec![],
    }
}

fn translate_stream_event(
    session_id: &str,
    event: claurst_api::streaming::AnthropicStreamEvent,
) -> Vec<Event> {
    match event {
        claurst_api::streaming::AnthropicStreamEvent::MessageStart { .. } => {
            vec![Event::MessageStart {
                session_id: session_id.into(),
            }]
        }
        claurst_api::streaming::AnthropicStreamEvent::ContentBlockDelta { delta, .. } => {
            match delta {
                claurst_api::streaming::ContentDelta::TextDelta { text } => {
                    vec![Event::MessageDelta {
                        session_id: session_id.into(),
                        text,
                    }]
                }
                _ => vec![],
            }
        }
        claurst_api::streaming::AnthropicStreamEvent::MessageStop => {
            vec![Event::MessageEnd {
                session_id: session_id.into(),
            }]
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
