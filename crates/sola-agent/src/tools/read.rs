use serde_json::{json, Value};

use super::{error_result, resolve, ToolCtx, ToolDetail, ToolResult};

pub fn schema() -> Value {
    json!({
        "type": "function",
        "name": "read",
        "description": "Read a file's contents. Optionally restrict to an inclusive 1-based line range [start, end].",
        "parameters": {
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "File path, absolute or relative to the project root." },
                "start": { "type": ["integer", "null"], "description": "First line to read (1-based, inclusive). Null for the whole file." },
                "end": { "type": ["integer", "null"], "description": "Last line to read (1-based, inclusive). Null for the whole file." }
            },
            "required": ["path", "start", "end"],
            "additionalProperties": false
        },
        "strict": true
    })
}

pub fn run(args: &Value, ctx: &ToolCtx) -> ToolResult {
    let path = match args.get("path").and_then(Value::as_str) {
        Some(p) => p,
        None => return error_result("read: missing required 'path' argument"),
    };
    let full = resolve(ctx, path);
    let contents = match std::fs::read_to_string(&full) {
        Ok(c) => c,
        Err(e) => return error_result(format!("read: cannot read {}: {e}", full.display())),
    };
    let start = args.get("start").and_then(Value::as_u64);
    let end = args.get("end").and_then(Value::as_u64);
    let text = match (start, end) {
        // Normalize line endings the same way the ranged branch does, so a
        // whole-file read and a full-range read agree on output.
        (None, None) => contents.lines().collect::<Vec<&str>>().join("\n"),
        _ => {
            let lines: Vec<&str> = contents.lines().collect();
            let total = lines.len() as u64;
            let s = start.unwrap_or(1).max(1);
            let e = end.unwrap_or(total).min(total);
            if s > e || total == 0 {
                return error_result(format!(
                    "read: empty range [{s}, {e}] for {} ({total} lines)",
                    full.display()
                ));
            }
            lines[(s as usize - 1)..(e as usize)].join("\n")
        }
    };
    ToolResult {
        model_text: text.clone(),
        ui_detail: ToolDetail::Text(text),
    }
}

#[cfg(test)]
mod tests {
    use crate::tools::{ToolCtx, ToolDetail};
    use serde_json::json;
    use std::fs;

    #[test]
    fn read_whole_file_returns_all_lines() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("f.txt"), "l1\nl2\nl3\n").unwrap();
        let ctx = ToolCtx { project_root: dir.path().to_path_buf() };
        let res = super::run(&json!({ "path": "f.txt", "start": null, "end": null }), &ctx);
        assert_eq!(res.model_text, "l1\nl2\nl3");
        assert!(matches!(res.ui_detail, ToolDetail::Text(_)));
    }

    #[test]
    fn read_honors_inclusive_range() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("f.txt"), "l1\nl2\nl3\nl4\nl5\n").unwrap();
        let ctx = ToolCtx { project_root: dir.path().to_path_buf() };
        let res = super::run(&json!({ "path": "f.txt", "start": 2, "end": 4 }), &ctx);
        assert_eq!(res.model_text, "l2\nl3\nl4");
    }

    #[test]
    fn read_schema_is_strict_function() {
        let s = super::schema();
        assert_eq!(s["type"], "function");
        assert_eq!(s["name"], "read");
        assert_eq!(s["strict"], true);
    }
}
