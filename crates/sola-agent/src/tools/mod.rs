//! Tool registry + result types. Individual tools (read/write/edit/
//! bash/search) and the `tool_schemas`/`dispatch`/`ToolCtx` items land
//! in the tools layer.

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
