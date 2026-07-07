//! Agent event / command types + the iced <-> worker bridge.
//!
//! Foundation defines the message enums and the `NodeId` alias. The
//! channel statics, `init_channels`, `agent_subscription`, `agent_send`,
//! `emit`, and `take_cmd_rx` are added in the bridge layer.

use crate::session::Usage;
use crate::tools::ToolResult;

pub type NodeId = String; // uuid v4 string

#[derive(Debug, Clone)]
pub enum AgentEvent {
    Delta { node_id: NodeId, text: String },
    Reasoning { text: String },
    ToolStart { call_id: String, tool: String, args: serde_json::Value },
    ToolOutput { call_id: String, chunk: String },
    ToolEnd { call_id: String, result: ToolResult },
    ApprovalRequest { call_id: String, tool: String, preview: String },
    TurnEnd { usage: Usage },
    Error { message: String },
}

#[derive(Debug, Clone)]
pub enum AgentCmd {
    Send { text: String, branch_from: Option<NodeId> },
    Approve { call_id: String, remember: bool },
    Deny { call_id: String, reason: Option<String> },
    Abort,
    SetModel { model: String, effort: String },
}
