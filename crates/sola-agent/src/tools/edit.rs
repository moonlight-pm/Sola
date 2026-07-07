use serde_json::{json, Value};

use super::{error_result, resolve, ToolCtx, ToolDetail, ToolResult};

pub fn schema() -> Value {
    json!({
        "type": "function",
        "name": "edit",
        "description": "Replace an exact, unique string in a file with new text. Fails if 'old' is absent or occurs more than once; include surrounding context to make it unique.",
        "parameters": {
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "File path, absolute or relative to the project root." },
                "old": { "type": "string", "description": "Exact text to find. Must occur exactly once." },
                "new": { "type": "string", "description": "Replacement text." }
            },
            "required": ["path", "old", "new"],
            "additionalProperties": false
        },
        "strict": true
    })
}

pub fn run(args: &Value, ctx: &ToolCtx) -> ToolResult {
    let path = match args.get("path").and_then(Value::as_str) {
        Some(p) => p,
        None => return error_result("edit: missing required 'path' argument"),
    };
    let old = match args.get("old").and_then(Value::as_str) {
        Some(o) => o,
        None => return error_result("edit: missing required 'old' argument"),
    };
    let new = match args.get("new").and_then(Value::as_str) {
        Some(n) => n,
        None => return error_result("edit: missing required 'new' argument"),
    };
    let full = resolve(ctx, path);
    let before = match std::fs::read_to_string(&full) {
        Ok(c) => c,
        Err(e) => return error_result(format!("edit: cannot read {}: {e}", full.display())),
    };
    if old.is_empty() {
        return error_result(format!("edit: 'old' must be a non-empty string for {path}"));
    }
    let count = before.matches(old).count();
    if count == 0 {
        return error_result(format!("edit: 'old' string not found in {path}"));
    }
    if count > 1 {
        return error_result(format!(
            "edit: 'old' string is ambiguous in {path} ({count} matches); provide more surrounding context"
        ));
    }
    let after = before.replacen(old, new, 1);
    if let Err(e) = std::fs::write(&full, &after) {
        return error_result(format!("edit: cannot write {}: {e}", full.display()));
    }
    tracing::debug!(path, "edit tool applied");
    ToolResult {
        model_text: format!("Edited {path}"),
        ui_detail: ToolDetail::Diff {
            path: path.to_string(),
            before,
            after,
        },
    }
}

#[cfg(test)]
mod tests {
    use crate::tools::{ToolCtx, ToolDetail};
    use serde_json::json;
    use std::fs;

    #[test]
    fn edit_replaces_exact_match() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("f.txt"), "hello world").unwrap();
        let ctx = ToolCtx { project_root: dir.path().to_path_buf() };
        let res = super::run(&json!({ "path": "f.txt", "old": "world", "new": "there" }), &ctx);

        assert_eq!(fs::read_to_string(dir.path().join("f.txt")).unwrap(), "hello there");
        match res.ui_detail {
            ToolDetail::Diff { before, after, .. } => {
                assert_eq!(before, "hello world");
                assert_eq!(after, "hello there");
            }
            other => panic!("expected Diff, got {other:?}"),
        }
    }

    #[test]
    fn edit_errors_when_old_absent() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("f.txt"), "hello world").unwrap();
        let ctx = ToolCtx { project_root: dir.path().to_path_buf() };
        let res = super::run(&json!({ "path": "f.txt", "old": "missing", "new": "x" }), &ctx);

        assert!(res.model_text.contains("not found"));
        assert!(matches!(res.ui_detail, ToolDetail::Text(_)));
        assert_eq!(fs::read_to_string(dir.path().join("f.txt")).unwrap(), "hello world");
    }

    #[test]
    fn edit_errors_when_old_ambiguous() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("f.txt"), "a a").unwrap();
        let ctx = ToolCtx { project_root: dir.path().to_path_buf() };
        let res = super::run(&json!({ "path": "f.txt", "old": "a", "new": "b" }), &ctx);

        assert!(res.model_text.contains("ambiguous"));
        assert!(matches!(res.ui_detail, ToolDetail::Text(_)));
        assert_eq!(fs::read_to_string(dir.path().join("f.txt")).unwrap(), "a a");
    }
}
