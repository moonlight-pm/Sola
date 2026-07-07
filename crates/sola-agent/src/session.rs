//! Transcript tree, JSONL persistence, branching, input reconstruction.
//!
//! Foundation defines the persisted node types (`Usage`, `Role`,
//! `Content`, `Node`). The `Session` struct and its methods land in the
//! session layer.

use crate::event::NodeId;

#[derive(Debug, Clone, Copy)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

#[derive(Debug, Clone)]
pub enum Role {
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone)]
pub enum Content {
    Text(String),
    FunctionCall { call_id: String, name: String, arguments: String }, // arguments = raw JSON string
    FunctionCallOutput { call_id: String, output: String },
}

#[derive(Debug, Clone)]
pub struct Node {
    pub id: NodeId,
    pub parent_id: Option<NodeId>,
    pub role: Role,
    pub content: Content,
    pub model: Option<String>,
    pub usage: Option<Usage>,
    pub ts: u64,
}
