//! Tool definitions and execution.
//!
//! Each tool has a name, description, input schema, and an async execute function.
//! Modeled after Pi's pluggable tool design.

use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::path::Path;

use crate::api::ToolDef;

/// Execute a tool by name with the given input, in the given working directory.
pub async fn execute_tool(name: &str, input: &Value, cwd: &Path) -> (String, bool) {
    let result = match name {
        "Bash" => tool_bash(input, cwd).await,
        "Read" => tool_read(input, cwd).await,
        "Write" => tool_write(input, cwd).await,
        "Edit" => tool_edit(input, cwd).await,
        "Glob" => tool_glob(input, cwd).await,
        "Grep" => tool_grep(input, cwd).await,
        "WebSearch" => tool_websearch(input).await,
        _ => Err(anyhow::anyhow!("Unknown tool: {}", name)),
    };

    match result {
        Ok(output) => (output, false),
        Err(e) => (format!("Error: {:#}", e), true),
    }
}

/// Return tool definitions for the API.
pub fn tool_definitions() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "Bash".into(),
            description: "Execute a bash command and return its output. Use for running programs, installing packages, or any shell operation.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "The bash command to execute" },
                    "timeout": { "type": "integer", "description": "Timeout in milliseconds (default 120000)" }
                },
                "required": ["command"]
            }),
        },
        ToolDef {
            name: "Read".into(),
            description: "Read a file from the filesystem. Returns the file contents with line numbers.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "file_path": { "type": "string", "description": "Absolute path to the file to read" },
                    "offset": { "type": "integer", "description": "Line number to start reading from (0-based)" },
                    "limit": { "type": "integer", "description": "Maximum number of lines to read" }
                },
                "required": ["file_path"]
            }),
        },
        ToolDef {
            name: "Write".into(),
            description: "Write content to a file, creating it if it doesn't exist or overwriting if it does.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "file_path": { "type": "string", "description": "Absolute path to the file to write" },
                    "content": { "type": "string", "description": "The content to write" }
                },
                "required": ["file_path", "content"]
            }),
        },
        ToolDef {
            name: "Edit".into(),
            description: "Replace exact string matches in a file. The old_string must match exactly.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "file_path": { "type": "string", "description": "Absolute path to the file to edit" },
                    "old_string": { "type": "string", "description": "The exact text to find and replace" },
                    "new_string": { "type": "string", "description": "The replacement text" }
                },
                "required": ["file_path", "old_string", "new_string"]
            }),
        },
        ToolDef {
            name: "Glob".into(),
            description: "Find files matching a glob pattern.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "Glob pattern (e.g. '**/*.rs', 'src/**/*.ts')" },
                    "path": { "type": "string", "description": "Directory to search in (default: working directory)" }
                },
                "required": ["pattern"]
            }),
        },
        ToolDef {
            name: "Grep".into(),
            description: "Search file contents using a regex pattern.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "Regex pattern to search for" },
                    "path": { "type": "string", "description": "File or directory to search in (default: working directory)" },
                    "glob": { "type": "string", "description": "Glob filter for file names (e.g. '*.rs')" }
                },
                "required": ["pattern"]
            }),
        },
        ToolDef {
            name: "WebSearch".into(),
            description: "Search the web for information.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search query" }
                },
                "required": ["query"]
            }),
        },
    ]
}

// ── Tool Implementations ─────────────────────────────────────────────────────

async fn tool_bash(input: &Value, cwd: &Path) -> Result<String> {
    let command = input.get("command")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("command is required"))?;
    let timeout_ms = input.get("timeout")
        .and_then(|v| v.as_u64())
        .unwrap_or(120_000);

    let output = tokio::time::timeout(
        std::time::Duration::from_millis(timeout_ms),
        tokio::process::Command::new("bash")
            .arg("-c")
            .arg(command)
            .current_dir(cwd)
            .output(),
    )
    .await
    .map_err(|_| anyhow::anyhow!("Command timed out after {}ms", timeout_ms))?
    .map_err(|e| anyhow::anyhow!("Failed to execute command: {}", e))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    let mut result = String::new();
    if !stdout.is_empty() {
        result.push_str(&stdout);
    }
    if !stderr.is_empty() {
        if !result.is_empty() { result.push('\n'); }
        result.push_str("STDERR:\n");
        result.push_str(&stderr);
    }
    if result.is_empty() {
        result = format!("(exit code {})", output.status.code().unwrap_or(-1));
    }

    // Truncate very long output
    if result.len() > 100_000 {
        result.truncate(100_000);
        result.push_str("\n... (truncated)");
    }

    Ok(result)
}

async fn tool_read(input: &Value, cwd: &Path) -> Result<String> {
    let file_path = input.get("file_path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("file_path is required"))?;
    let offset = input.get("offset").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    let limit = input.get("limit").and_then(|v| v.as_u64()).unwrap_or(2000) as usize;

    let path = resolve_path(file_path, cwd);
    let content = tokio::fs::read_to_string(&path).await
        .with_context(|| format!("Failed to read {}", path.display()))?;

    let lines: Vec<&str> = content.lines().collect();
    let end = (offset + limit).min(lines.len());
    let selected: Vec<String> = lines[offset..end]
        .iter()
        .enumerate()
        .map(|(i, line)| format!("{}\t{}", offset + i + 1, line))
        .collect();

    Ok(selected.join("\n"))
}

async fn tool_write(input: &Value, cwd: &Path) -> Result<String> {
    let file_path = input.get("file_path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("file_path is required"))?;
    let content = input.get("content")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("content is required"))?;

    let path = resolve_path(file_path, cwd);
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(&path, content).await
        .with_context(|| format!("Failed to write {}", path.display()))?;

    Ok(format!("Wrote {} bytes to {}", content.len(), path.display()))
}

async fn tool_edit(input: &Value, cwd: &Path) -> Result<String> {
    let file_path = input.get("file_path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("file_path is required"))?;
    let old_string = input.get("old_string")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("old_string is required"))?;
    let new_string = input.get("new_string")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("new_string is required"))?;

    let path = resolve_path(file_path, cwd);
    let content = tokio::fs::read_to_string(&path).await
        .with_context(|| format!("Failed to read {}", path.display()))?;

    let count = content.matches(old_string).count();
    if count == 0 {
        anyhow::bail!("old_string not found in {}", path.display());
    }
    if count > 1 {
        anyhow::bail!("old_string matches {} times in {} — must be unique", count, path.display());
    }

    let new_content = content.replacen(old_string, new_string, 1);
    tokio::fs::write(&path, &new_content).await?;

    Ok(format!("Edited {}", path.display()))
}

async fn tool_glob(input: &Value, cwd: &Path) -> Result<String> {
    let pattern = input.get("pattern")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("pattern is required"))?;
    let search_dir = input.get("path")
        .and_then(|v| v.as_str())
        .map(|p| resolve_path(p, cwd))
        .unwrap_or_else(|| cwd.to_path_buf());

    let output = tokio::process::Command::new("find")
        .arg(&search_dir)
        .arg("-path")
        .arg(format!("{}/{}", search_dir.display(), pattern))
        .arg("-type")
        .arg("f")
        .output()
        .await?;

    // Fallback: use shell glob via bash
    if output.stdout.is_empty() {
        let output = tokio::process::Command::new("bash")
            .arg("-c")
            .arg(format!("shopt -s globstar nullglob; cd '{}' && printf '%s\\n' {}", search_dir.display(), pattern))
            .output()
            .await?;
        return Ok(String::from_utf8_lossy(&output.stdout).to_string());
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

async fn tool_grep(input: &Value, cwd: &Path) -> Result<String> {
    let pattern = input.get("pattern")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("pattern is required"))?;
    let search_path = input.get("path")
        .and_then(|v| v.as_str())
        .map(|p| resolve_path(p, cwd))
        .unwrap_or_else(|| cwd.to_path_buf());
    let glob_filter = input.get("glob").and_then(|v| v.as_str());

    let mut cmd = tokio::process::Command::new("rg");
    cmd.arg("--no-heading")
        .arg("--line-number")
        .arg("--color=never")
        .arg("--max-count=50");

    if let Some(g) = glob_filter {
        cmd.arg("--glob").arg(g);
    }

    cmd.arg(pattern).arg(&search_path);

    let output = cmd.output().await?;
    let result = String::from_utf8_lossy(&output.stdout).to_string();

    if result.is_empty() {
        Ok("No matches found".to_string())
    } else {
        Ok(result)
    }
}

async fn tool_websearch(input: &Value) -> Result<String> {
    let query = input.get("query")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("query is required"))?;

    // Use ddgr (DuckDuckGo CLI) or curl-based search
    let output = tokio::process::Command::new("bash")
        .arg("-c")
        .arg(format!(
            "curl -sL 'https://html.duckduckgo.com/html/?q={}' | \
             sed -n 's/.*class=\"result__a\"[^>]*>\\(.*\\)<\\/a>.*/\\1/p' | \
             head -10",
            urlencoding(query)
        ))
        .output()
        .await?;

    let result = String::from_utf8_lossy(&output.stdout).to_string();
    if result.trim().is_empty() {
        Ok(format!("No results found for: {}", query))
    } else {
        Ok(result)
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn resolve_path(path: &str, cwd: &Path) -> std::path::PathBuf {
    let p = std::path::PathBuf::from(path);
    if p.is_absolute() { p } else { cwd.join(p) }
}

fn urlencoding(s: &str) -> String {
    s.chars().map(|c| match c {
        ' ' => '+'.to_string(),
        c if c.is_alphanumeric() || "-_.~".contains(c) => c.to_string(),
        c => format!("%{:02X}", c as u32),
    }).collect()
}
