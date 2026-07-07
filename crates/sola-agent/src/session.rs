//! Transcript tree, JSONL persistence, branching, input reconstruction.
//!
//! Foundation defines the persisted node types (`Usage`, `Role`,
//! `Content`, `Node`). The `Session` struct and its methods land in the
//! session layer.

use crate::event::NodeId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Role {
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Content {
    Text(String),
    FunctionCall { call_id: String, name: String, arguments: String }, // arguments = raw JSON string
    FunctionCallOutput { call_id: String, output: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    pub parent_id: Option<NodeId>,
    pub role: Role,
    pub content: Content,
    pub model: Option<String>,
    pub usage: Option<Usage>,
    pub ts: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_json_round_trips() {
        let node = Node {
            id: "11111111-1111-4111-8111-111111111111".to_string(),
            parent_id: Some("00000000-0000-4000-8000-000000000000".to_string()),
            role: Role::Assistant,
            content: Content::FunctionCall {
                call_id: "call_1".to_string(),
                name: "read".to_string(),
                arguments: r#"{"path":"a.txt"}"#.to_string(),
            },
            model: Some("fugu".to_string()),
            usage: Some(Usage { input_tokens: 12, output_tokens: 7 }),
            ts: 1_725_000_000_000,
        };

        let json = serde_json::to_string(&node).unwrap();
        let back: Node = serde_json::from_str(&json).unwrap();

        assert_eq!(back.id, node.id);
        assert_eq!(back.parent_id, node.parent_id);
        assert_eq!(back.ts, node.ts);
        assert_eq!(back.model.as_deref(), Some("fugu"));
        assert!(matches!(back.role, Role::Assistant));
        assert_eq!(back.usage.map(|u| (u.input_tokens, u.output_tokens)), Some((12, 7)));
        match back.content {
            Content::FunctionCall { call_id, name, arguments } => {
                assert_eq!(call_id, "call_1");
                assert_eq!(name, "read");
                assert_eq!(arguments, r#"{"path":"a.txt"}"#);
            }
            other => panic!("wrong content variant: {other:?}"),
        }
    }
}
