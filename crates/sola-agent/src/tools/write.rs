use serde_json::{json, Value};

use super::{error_result, resolve, ToolCtx, ToolDetail, ToolResult};

pub fn schema() -> Value {
    json!({
        "type": "function",
        "name": "write",
        "description": "Create or overwrite a file with the given contents. Parent directories are created as needed.",
        "parameters": {
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "File path, absolute or relative to the project root." },
                "content": { "type": "string", "description": "The full new file contents." }
            },
            "required": ["path", "content"],
            "additionalProperties": false
        },
        "strict": true
    })
}

pub fn run(args: &Value, ctx: &ToolCtx) -> ToolResult {
    let path = match args.get("path").and_then(Value::as_str) {
        Some(p) => p,
        None => return error_result("write: missing required 'path' argument"),
    };
    let content = match args.get("content").and_then(Value::as_str) {
        Some(c) => c,
        None => return error_result("write: missing required 'content' argument"),
    };
    let full = resolve(ctx, path);
    let before = std::fs::read_to_string(&full).unwrap_or_default();
    if let Some(parent) = full.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            return error_result(format!("write: cannot create {}: {e}", parent.display()));
        }
    }
    if let Err(e) = std::fs::write(&full, content) {
        return error_result(format!("write: cannot write {}: {e}", full.display()));
    }
    tracing::debug!(path, bytes = content.len(), "write tool");
    ToolResult {
        model_text: format!("Wrote {} bytes to {path}", content.len()),
        ui_detail: ToolDetail::Diff {
            path: path.to_string(),
            before,
            after: content.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use crate::tools::{ToolCtx, ToolDetail};
    use serde_json::json;
    use std::fs;

    #[test]
    fn write_creates_file_and_reports_diff() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ToolCtx { project_root: dir.path().to_path_buf() };
        let res = super::run(&json!({ "path": "new.txt", "content": "hello\n" }), &ctx);

        let on_disk = fs::read_to_string(dir.path().join("new.txt")).unwrap();
        assert_eq!(on_disk, "hello\n");

        match res.ui_detail {
            ToolDetail::Diff { path, before, after } => {
                assert_eq!(path, "new.txt");
                assert_eq!(before, "");
                assert_eq!(after, "hello\n");
            }
            other => panic!("expected Diff, got {other:?}"),
        }
    }

    #[test]
    fn write_overwrites_file_and_shows_old_to_new_diff() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("existing.txt"), "old content\n").unwrap();
        let ctx = ToolCtx { project_root: dir.path().to_path_buf() };
        let res = super::run(&json!({ "path": "existing.txt", "content": "new content\n" }), &ctx);

        let on_disk = fs::read_to_string(dir.path().join("existing.txt")).unwrap();
        assert_eq!(on_disk, "new content\n");

        match res.ui_detail {
            ToolDetail::Diff { path, before, after } => {
                assert_eq!(path, "existing.txt");
                assert_eq!(before, "old content\n");
                assert_eq!(after, "new content\n");
            }
            other => panic!("expected Diff, got {other:?}"),
        }
    }

    #[test]
    fn write_creates_parent_directories() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ToolCtx { project_root: dir.path().to_path_buf() };
        let res = super::run(&json!({ "path": "dir1/dir2/file.txt", "content": "nested\n" }), &ctx);

        let on_disk = fs::read_to_string(dir.path().join("dir1/dir2/file.txt")).unwrap();
        assert_eq!(on_disk, "nested\n");

        match res.ui_detail {
            ToolDetail::Diff { path, before, after } => {
                assert_eq!(path, "dir1/dir2/file.txt");
                assert_eq!(before, "");
                assert_eq!(after, "nested\n");
            }
            other => panic!("expected Diff, got {other:?}"),
        }
    }

    #[test]
    fn write_schema_is_strict_function() {
        let s = super::schema();
        assert_eq!(s["type"], "function");
        assert_eq!(s["name"], "write");
        assert_eq!(s["strict"], true);
    }
}
