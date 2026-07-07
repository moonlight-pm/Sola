//! Transcript tree, JSONL persistence, branching, input reconstruction.
//!
//! Foundation defines the persisted node types (`Usage`, `Role`,
//! `Content`, `Node`). The `Session` struct and its methods land in the
//! session layer.

use std::collections::HashMap;
use std::path::PathBuf;

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

/// In-memory view of a transcript tree plus its on-disk JSONL location.
pub struct Session {
    pub id: String,
    pub title: String,
    pub project_root: PathBuf,
    nodes: HashMap<NodeId, Node>,
    order: Vec<NodeId>,
    pub active_leaf: Option<NodeId>,
}

/// `<config>/sola/agent/sessions`, honoring `$XDG_CONFIG_HOME`.
fn sessions_dir() -> PathBuf {
    sola_core::config::sola_config_dir().join("agent").join("sessions")
}

/// Milliseconds since the Unix epoch, for `Node::ts`.
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Derive a one-line, <=60-char title from the first user message text.
fn derive_title(text: &str) -> String {
    let first = text.trim().lines().next().unwrap_or("").trim();
    if first.chars().count() > 60 {
        let head: String = first.chars().take(57).collect();
        format!("{head}...")
    } else {
        first.to_string()
    }
}

impl Session {
    /// A fresh, empty session with a random v4 id and no nodes.
    pub fn new(project_root: PathBuf) -> Self {
        Session {
            id: uuid::Uuid::new_v4().to_string(),
            title: String::new(),
            project_root,
            nodes: HashMap::new(),
            order: Vec::new(),
            active_leaf: None,
        }
    }

    /// `~/.config/sola/agent/sessions/<id>.jsonl`.
    pub fn path(&self) -> PathBuf {
        sessions_dir().join(format!("{}.jsonl", self.id))
    }

    /// Append a node as a child of the current leaf, advance the leaf, and
    /// persist it as one JSONL line. Returns the new node's id.
    pub fn append(
        &mut self,
        role: Role,
        content: Content,
        model: Option<String>,
        usage: Option<Usage>,
    ) -> NodeId {
        let id = uuid::Uuid::new_v4().to_string();
        let node = Node {
            id: id.clone(),
            parent_id: self.active_leaf.clone(),
            role,
            content,
            model,
            usage,
            ts: now_ms(),
        };
        if self.title.is_empty() {
            if let (Role::User, Content::Text(t)) = (&node.role, &node.content) {
                self.title = derive_title(t);
            }
        }
        if let Err(e) = self.write_node_line(&node) {
            tracing::error!(session = %self.id, node = %id, error = %e,
                "failed to persist transcript node");
        }
        self.order.push(id.clone());
        self.nodes.insert(id.clone(), node);
        self.active_leaf = Some(id.clone());
        id
    }

    /// Serialize one node and append it as a line to the session file.
    fn write_node_line(&self, node: &Node) -> std::io::Result<()> {
        use std::io::Write;
        let path = self.path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let line = serde_json::to_string(node)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        writeln!(file, "{line}")?;
        Ok(())
    }

    /// The chain of nodes from the root down to (and including) the active leaf.
    pub fn path_to_leaf(&self) -> Vec<Node> {
        let mut chain = Vec::new();
        let mut cursor = self.active_leaf.clone();
        while let Some(id) = cursor {
            match self.nodes.get(&id) {
                Some(node) => {
                    cursor = node.parent_id.clone();
                    chain.push(node.clone());
                }
                None => break,
            }
        }
        chain.reverse();
        chain
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Serializes `$XDG_CONFIG_HOME` mutation so the fs tests don't race.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Point `$XDG_CONFIG_HOME` at a fresh tempdir for the test's duration.
    /// The returned guard + TempDir must be kept alive by the caller.
    fn temp_env() -> (std::sync::MutexGuard<'static, ()>, tempfile::TempDir) {
        let guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        // SAFETY: guarded by ENV_LOCK; no other thread reads the env here.
        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", tmp.path());
        }
        (guard, tmp)
    }

    #[test]
    fn new_session_has_unique_id_and_scoped_path() {
        let (_g, tmp) = temp_env();

        let a = Session::new(PathBuf::from("/home/joshua/project"));
        let b = Session::new(PathBuf::from("/home/joshua/project"));

        assert_ne!(a.id, b.id, "each session gets a distinct id");
        assert!(a.active_leaf.is_none(), "a fresh session has no leaf");

        let want = format!("{}.jsonl", a.id);
        let path = a.path();
        assert_eq!(path.file_name().and_then(|s| s.to_str()), Some(want.as_str()));
        assert!(
            path.starts_with(tmp.path()),
            "path {path:?} should live under the temp config root {:?}",
            tmp.path()
        );
        assert!(path.ends_with(format!("sola/agent/sessions/{}.jsonl", a.id)));
    }

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

    #[test]
    fn append_builds_a_linear_path_to_leaf() {
        let (_g, _tmp) = temp_env();

        let mut s = Session::new(PathBuf::from("/tmp/project"));
        let n1 = s.append(Role::User, Content::Text("first".into()), None, None);
        let n2 = s.append(
            Role::Assistant,
            Content::Text("second".into()),
            Some("fugu".into()),
            Some(Usage { input_tokens: 3, output_tokens: 5 }),
        );

        assert_eq!(s.active_leaf.as_ref(), Some(&n2));
        assert_eq!(s.nodes[&n2].parent_id.as_ref(), Some(&n1));
        assert!(s.nodes[&n1].parent_id.is_none());

        let path: Vec<NodeId> = s.path_to_leaf().into_iter().map(|n| n.id).collect();
        assert_eq!(path, vec![n1, n2], "path_to_leaf is root..=leaf");
    }
}

