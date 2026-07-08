//! Local tools the agent can call. Each returns a split `ToolResult`:
//! `model_text` is what the model sees; `ui_detail` is the richer structured
//! view. Kept in many small files, one per tool.

use std::path::{Path, PathBuf};

use serde_json::Value;

pub mod bash;
pub mod edit;
pub mod read;
pub mod search;
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

/// The full set of function tools advertised to the Responses API this turn.
pub fn tool_schemas() -> Vec<Value> {
    vec![read::schema(), write::schema(), edit::schema(), bash::schema(), search::schema()]
}

/// Route a model tool call to its implementation. Unknown names return an error
/// result (never panic) so the loop can feed it back to the model. `on_chunk`
/// is a live-output sink; only `bash` streams through it today, other tools
/// run to completion synchronously and ignore it.
pub fn dispatch(name: &str, args: &Value, ctx: &ToolCtx, on_chunk: &mut dyn FnMut(&str)) -> ToolResult {
    match name {
        "read" => read::run(args, ctx),
        "write" => write::run(args, ctx),
        "edit" => edit::run(args, ctx),
        "bash" => bash::run(args, ctx, on_chunk),
        "search" => search::run(args, ctx),
        other => error_result(format!("dispatch: unknown tool '{other}'")),
    }
}

#[cfg(test)]
mod tests {
    use super::{dispatch, tool_schemas, ToolCtx, ToolDetail};
    use serde_json::json;

    #[test]
    fn tool_schemas_lists_five_strict_functions() {
        let schemas = tool_schemas();
        assert_eq!(schemas.len(), 5);
        let names: Vec<&str> = schemas.iter().map(|s| s["name"].as_str().unwrap()).collect();
        assert_eq!(names, vec!["read", "write", "edit", "bash", "search"]);
        for s in &schemas {
            assert_eq!(s["type"], "function");
            assert_eq!(s["strict"], true);
            assert!(s["parameters"].is_object());
        }
    }

    #[test]
    fn dispatch_routes_to_bash() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ToolCtx { project_root: dir.path().to_path_buf() };
        let res = dispatch("bash", &json!({ "command": "echo hi" }), &ctx, &mut |_| {});
        assert!(matches!(res.ui_detail, ToolDetail::Bash { code: 0, .. }));
    }

    #[test]
    fn dispatch_unknown_tool_is_error_text() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ToolCtx { project_root: dir.path().to_path_buf() };
        let res = dispatch("nope", &json!({}), &ctx, &mut |_| {});
        assert!(res.model_text.contains("unknown tool"));
        assert!(matches!(res.ui_detail, ToolDetail::Text(_)));
    }
}
