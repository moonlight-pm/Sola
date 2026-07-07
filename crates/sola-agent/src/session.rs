//! Transcript tree, JSONL persistence, branching, input reconstruction.
//!
//! Foundation defines the persisted node types (`Usage`, `Role`,
//! `Content`, `Node`). The `Session` struct and its methods land in the
//! session layer.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::event::NodeId;
use crate::provider::InputItem;
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

/// Sidebar metadata for one session, persisted in `sessions/index.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexEntry {
    pub id: String,
    pub title: String,
    pub project_root: PathBuf,
    pub updated: u64,
}

fn index_path() -> PathBuf {
    sessions_dir().join("index.json")
}

fn write_index(entries: &[IndexEntry]) -> std::io::Result<()> {
    let path = index_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(entries)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(path, json)
}

/// Read the session index, rebuilding it from the JSONL files if the index
/// file is missing or unparseable.
pub fn load_index() -> Vec<IndexEntry> {
    match std::fs::read_to_string(index_path()) {
        Ok(s) => match serde_json::from_str::<Vec<IndexEntry>>(&s) {
            Ok(entries) => entries,
            Err(_) => rebuild_index().unwrap_or_default(),
        },
        Err(_) => rebuild_index().unwrap_or_default(),
    }
}

/// Scan every `<id>.jsonl` under the sessions dir, derive an entry per file,
/// and rewrite `index.json`. Recovers id + derived title + file mtime;
/// `project_root` is not stored per-node and comes back empty.
pub fn rebuild_index() -> std::io::Result<Vec<IndexEntry>> {
    let dir = sessions_dir();
    std::fs::create_dir_all(&dir)?;
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(&dir)? {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let Ok(session) = Session::load(&path) else { continue };
        let updated = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        entries.push(IndexEntry {
            id: session.id,
            title: session.title,
            project_root: session.project_root,
            updated,
        });
    }
    write_index(&entries)?;
    Ok(entries)
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
        self.update_index();
        id
    }

    /// Upsert this session's entry into `index.json`.
    fn update_index(&self) {
        let mut entries = load_index();
        let entry = IndexEntry {
            id: self.id.clone(),
            title: self.title.clone(),
            project_root: self.project_root.clone(),
            updated: now_ms(),
        };
        match entries.iter_mut().find(|e| e.id == self.id) {
            Some(existing) => *existing = entry,
            None => entries.push(entry),
        }
        if let Err(e) = write_index(&entries) {
            tracing::error!(session = %self.id, error = %e, "failed to update session index");
        }
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

    /// Rebuild a session from its append-only JSONL file. `active_leaf` is the
    /// last node written; `title` is derived from the first user text node;
    /// `project_root` is not stored per-node, so it is left empty (the session
    /// index is the authoritative source for it during a live session).
    pub fn load(path: &Path) -> std::io::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let mut nodes = HashMap::new();
        let mut order = Vec::new();
        for line in content.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let node: Node = serde_json::from_str(line)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            order.push(node.id.clone());
            nodes.insert(node.id.clone(), node);
        }
        let id = path
            .file_stem()
            .and_then(|s| s.to_str())
            .map(str::to_string)
            .unwrap_or_default();
        let active_leaf = order.last().cloned();
        let title = order
            .iter()
            .filter_map(|nid| nodes.get(nid))
            .find_map(|n| match (&n.role, &n.content) {
                (Role::User, Content::Text(t)) => Some(derive_title(t)),
                _ => None,
            })
            .unwrap_or_default();
        Ok(Session {
            id,
            title,
            project_root: PathBuf::new(),
            nodes,
            order,
            active_leaf,
        })
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

    /// Move the active leaf back to an earlier node so the next `append` forks
    /// a new child off it, leaving the previous branch intact.
    pub fn branch_from(&mut self, parent: NodeId) {
        self.active_leaf = Some(parent);
    }

    /// Map the active branch (root..=leaf) to the provider's `InputItem`s, in
    /// order. Text nodes become role-tagged messages; function-call and
    /// function-call-output nodes pass their fields through unchanged.
    pub fn to_input(&self) -> Vec<InputItem> {
        self.path_to_leaf()
            .into_iter()
            .map(|node| match node.content {
                Content::Text(text) => InputItem::Message {
                    role: match node.role {
                        Role::User => "user".to_string(),
                        Role::Assistant => "assistant".to_string(),
                        Role::Tool => "user".to_string(),
                    },
                    text,
                },
                Content::FunctionCall { call_id, name, arguments } => {
                    InputItem::FunctionCall { call_id, name, arguments }
                }
                Content::FunctionCallOutput { call_id, output } => {
                    InputItem::FunctionCallOutput { call_id, output }
                }
            })
            .collect()
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

    #[test]
    fn reload_reconstructs_tree_and_leaf() {
        let (_g, _tmp) = temp_env();

        let (path, id, n1, n2) = {
            let mut s = Session::new(PathBuf::from("/tmp/project"));
            let n1 = s.append(Role::User, Content::Text("hi".into()), None, None);
            let n2 = s.append(
                Role::Assistant,
                Content::Text("hello".into()),
                Some("fugu".into()),
                None,
            );
            (s.path(), s.id.clone(), n1, n2)
        };

        let reloaded = Session::load(&path).expect("load session");

        assert_eq!(reloaded.id, id, "id recovered from filename");
        assert_eq!(reloaded.active_leaf.as_ref(), Some(&n2));
        assert_eq!(reloaded.nodes.len(), 2);
        assert_eq!(reloaded.nodes[&n2].parent_id.as_ref(), Some(&n1));
        assert!(reloaded.nodes[&n1].parent_id.is_none());

        let ids: Vec<NodeId> = reloaded.path_to_leaf().into_iter().map(|n| n.id).collect();
        assert_eq!(ids, vec![n1, n2]);
    }

    #[test]
    fn branch_from_forks_a_sibling_without_touching_old_branch() {
        let (_g, _tmp) = temp_env();

        let mut s = Session::new(PathBuf::from("/tmp/project"));
        let root = s.append(Role::User, Content::Text("root".into()), None, None);
        let old = s.append(Role::Assistant, Content::Text("old reply".into()), None, None);

        s.branch_from(root.clone());
        assert_eq!(s.active_leaf.as_ref(), Some(&root), "leaf moved back to the parent");

        let new = s.append(Role::Assistant, Content::Text("new reply".into()), None, None);

        // The new node is a second child of root; the old branch is untouched.
        assert_eq!(s.nodes[&new].parent_id.as_ref(), Some(&root));
        assert_eq!(s.nodes[&old].parent_id.as_ref(), Some(&root));
        match &s.nodes[&old].content {
            Content::Text(t) => assert_eq!(t, "old reply"),
            other => panic!("old branch content changed: {other:?}"),
        }

        let children = s
            .order
            .iter()
            .filter(|id| s.nodes.get(*id).and_then(|n| n.parent_id.as_ref()) == Some(&root))
            .count();
        assert_eq!(children, 2, "root has two children after branching");

        let leaf_ids: Vec<NodeId> = s.path_to_leaf().into_iter().map(|n| n.id).collect();
        assert_eq!(leaf_ids, vec![root, new], "active path is the new branch");
    }

    #[test]
    fn to_input_maps_each_content_variant() {
        let (_g, _tmp) = temp_env();

        let mut s = Session::new(PathBuf::from("/tmp/project"));
        s.append(Role::User, Content::Text("hello".into()), None, None);
        s.append(
            Role::Assistant,
            Content::FunctionCall {
                call_id: "c1".into(),
                name: "read".into(),
                arguments: "{\"path\":\"a.txt\"}".into(),
            },
            None,
            None,
        );
        s.append(
            Role::Tool,
            Content::FunctionCallOutput {
                call_id: "c1".into(),
                output: "file body".into(),
            },
            None,
            None,
        );

        let items = s.to_input();
        assert_eq!(items.len(), 3);

        match &items[0] {
            InputItem::Message { role, text } => {
                assert_eq!(role, "user");
                assert_eq!(text, "hello");
            }
            other => panic!("expected Message, got {other:?}"),
        }
        match &items[1] {
            InputItem::FunctionCall { call_id, name, arguments } => {
                assert_eq!(call_id, "c1");
                assert_eq!(name, "read");
                assert_eq!(arguments, "{\"path\":\"a.txt\"}");
            }
            other => panic!("expected FunctionCall, got {other:?}"),
        }
        match &items[2] {
            InputItem::FunctionCallOutput { call_id, output } => {
                assert_eq!(call_id, "c1");
                assert_eq!(output, "file body");
            }
            other => panic!("expected FunctionCallOutput, got {other:?}"),
        }
    }

    #[test]
    fn append_maintains_index_and_rebuild_recovers_it() {
        let (_g, _tmp) = temp_env();

        let mut s = Session::new(PathBuf::from("/home/joshua/proj"));
        s.append(Role::User, Content::Text("index me".into()), None, None);
        let id = s.id.clone();

        // append upserted an entry with live title + project_root
        let index = load_index();
        let entry = index.iter().find(|e| e.id == id).expect("append writes an index entry");
        assert_eq!(entry.title, "index me");
        assert_eq!(entry.project_root, PathBuf::from("/home/joshua/proj"));

        // wipe the index; load_index must rebuild it from the jsonl files.
        // (project_root is not stored per-node, so only id + derived title survive.)
        std::fs::remove_file(index_path()).unwrap();
        let rebuilt = load_index();
        let rebuilt_entry =
            rebuilt.iter().find(|e| e.id == id).expect("index rebuilt from files");
        assert_eq!(rebuilt_entry.title, "index me", "title recovered from first user node");
    }
}

