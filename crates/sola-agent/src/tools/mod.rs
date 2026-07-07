#![allow(dead_code)]
//! Local tools the agent can call. Each returns a split `ToolResult`:
//! `model_text` is what the model sees; `ui_detail` is the richer structured
//! view. Kept in many small files, one per tool.

use std::path::{Path, PathBuf};

pub mod edit;
pub mod read;
pub mod write;

/// Per-conversation execution context. `project_root` scopes `bash` and
/// resolves relative tool paths.
#[derive(Debug, Clone)]
pub struct ToolCtx {
    pub project_root: PathBuf,
}

/// Resolve a tool path argument against the session's project root. Absolute
/// paths pass through; relative paths join onto the root.
pub(crate) fn resolve(ctx: &ToolCtx, path: &str) -> PathBuf {
    let p = Path::new(path);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        ctx.project_root.join(p)
    }
}

/// Build a uniform error result: the message is both what the model reads back
/// and a `Text` UI detail. Tools never panic on bad input or I/O failure.
pub(crate) fn error_result(msg: impl Into<String>) -> ToolResult {
    let msg = msg.into();
    ToolResult { model_text: msg.clone(), ui_detail: ToolDetail::Text(msg) }
}

#[derive(Debug, Clone)]
pub struct ToolResult {
    pub model_text: String,
    pub ui_detail: ToolDetail,
}

#[derive(Debug, Clone)]
pub enum ToolDetail {
    Text(String),
    Diff { path: String, before: String, after: String },
    Bash { code: i32, stdout: String, stderr: String },
}
