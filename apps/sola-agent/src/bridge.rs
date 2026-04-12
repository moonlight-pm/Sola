use serde::{Deserialize, Serialize};
use webkit6::prelude::*;

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
pub enum Command {
    SendMessage { session_id: String, text: String },
    Cancel { session_id: String },
    NewSession { working_dir: String },
    ResumeSession { session_id: String },
    CloseSession { session_id: String },
    ListConversations,
    RenameConversation { session_id: String, name: String },
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
pub enum Event {
    MessageStart { session_id: String },
    MessageDelta { session_id: String, text: String },
    MessageEnd { session_id: String },
    ToolStart { session_id: String, tool_name: String, tool_input: String },
    ToolEnd { session_id: String, tool_name: String, result: String, is_error: bool },
    SessionState { session_id: String, status: String },
    ConversationsList { conversations: Vec<ConversationSummary> },
    SessionLoaded { session_id: String, messages: Vec<MessageView> },
    Error { session_id: Option<String>, message: String },
}

#[derive(Debug, Clone, Serialize)]
pub struct ConversationSummary {
    pub session_id: String,
    pub name: Option<String>,
    pub first_prompt: Option<String>,
    pub working_dir: String,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct MessageView {
    pub role: String,
    pub content: String,
}

/// Push an event to the WebView by calling window.sola.dispatch(json).
pub fn dispatch_event(webview: &webkit6::WebView, event: &Event) {
    let json = serde_json::to_string(event).unwrap_or_default();
    let escaped = json.replace('\\', "\\\\").replace('\'', "\\'");
    let script = format!("window.sola && window.sola.dispatch('{}')", escaped);
    webview.evaluate_javascript(&script, None, None, None::<&gtk4::gio::Cancellable>, |_| {});
}
