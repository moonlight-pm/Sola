use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize)]
pub struct McpConfigFile {
    #[serde(rename = "mcpServers", default)]
    pub mcp_servers: HashMap<String, McpServerEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct McpServerEntry {
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    pub url: Option<String>,
}

pub fn discover(working_dir: &Path) -> HashMap<String, McpServerEntry> {
    let mut merged = HashMap::new();

    // Global config (lowest priority)
    if let Some(home) = std::env::var("HOME").ok() {
        let global_path = PathBuf::from(home).join(".claude/.mcp.json");
        if let Some(config) = load_mcp_file(&global_path) {
            merged.extend(config.mcp_servers);
        }
    }

    // Walk up directory tree (closer to project wins)
    let mut ancestors: Vec<PathBuf> = Vec::new();
    let mut current = working_dir.to_path_buf();
    loop {
        ancestors.push(current.clone());
        if !current.pop() {
            break;
        }
    }

    // Process from root toward project (so project-local overrides parent)
    for dir in ancestors.into_iter().rev() {
        let mcp_path = dir.join(".mcp.json");
        if let Some(config) = load_mcp_file(&mcp_path) {
            merged.extend(config.mcp_servers);
        }
    }

    merged
}

fn load_mcp_file(path: &Path) -> Option<McpConfigFile> {
    let contents = std::fs::read_to_string(path).ok()?;
    match serde_json::from_str(&contents) {
        Ok(config) => {
            tracing::info!("Loaded MCP config from {}", path.display());
            Some(config)
        }
        Err(e) => {
            tracing::warn!("Failed to parse {}: {}", path.display(), e);
            None
        }
    }
}
