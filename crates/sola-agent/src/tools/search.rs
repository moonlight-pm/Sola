use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use super::{error_result, resolve, ToolCtx, ToolDetail, ToolResult};

pub fn schema() -> Value {
    json!({
        "type": "function",
        "name": "search",
        "description": "Read-only lookups under the project. mode=ls lists a directory; mode=find lists files whose name contains 'query'; mode=grep lists lines containing 'query'.",
        "parameters": {
            "type": "object",
            "properties": {
                "mode": { "type": "string", "enum": ["ls", "find", "grep"], "description": "Which lookup to perform." },
                "path": { "type": "string", "description": "Directory to search under, absolute or relative to the project root." },
                "query": { "type": ["string", "null"], "description": "Substring to match. Required for find and grep; ignored for ls." }
            },
            "required": ["mode", "path", "query"],
            "additionalProperties": false
        },
        "strict": true
    })
}

pub fn run(args: &Value, ctx: &ToolCtx) -> ToolResult {
    let mode = match args.get("mode").and_then(Value::as_str) {
        Some(m) => m,
        None => return error_result("search: missing required 'mode' argument"),
    };
    let path = args.get("path").and_then(Value::as_str).unwrap_or(".");
    let query = args.get("query").and_then(Value::as_str).unwrap_or("");
    let root = resolve(ctx, path);

    let result = match mode {
        "ls" => ls(&root),
        "find" => find(&root, query),
        "grep" => grep(&root, query),
        other => return error_result(format!("search: unknown mode '{other}' (want ls|find|grep)")),
    };
    match result {
        Ok(text) => ToolResult {
            model_text: text.clone(),
            ui_detail: ToolDetail::Text(text),
        },
        Err(e) => error_result(format!("search: {e}")),
    }
}

fn ls(root: &Path) -> std::io::Result<String> {
    let mut entries: Vec<String> = Vec::new();
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if entry.file_type()?.is_dir() {
            entries.push(format!("{name}/"));
        } else {
            entries.push(name);
        }
    }
    entries.sort();
    Ok(entries.join("\n"))
}

fn walk(root: &Path, out: &mut Vec<PathBuf>) {
    let rd = match std::fs::read_dir(root) {
        Ok(rd) => rd,
        Err(_) => return,
    };
    for entry in rd.flatten() {
        let p = entry.path();
        match entry.file_type() {
            Ok(ft) if ft.is_dir() => walk(&p, out),
            Ok(_) => out.push(p),
            Err(_) => {}
        }
    }
}

fn find(root: &Path, query: &str) -> std::io::Result<String> {
    let mut files = Vec::new();
    walk(root, &mut files);
    let mut hits: Vec<String> = files
        .iter()
        .filter(|p| {
            p.file_name()
                .map(|n| n.to_string_lossy().contains(query))
                .unwrap_or(false)
        })
        .map(|p| p.display().to_string())
        .collect();
    hits.sort();
    Ok(hits.join("\n"))
}

fn grep(root: &Path, query: &str) -> std::io::Result<String> {
    let mut files = Vec::new();
    walk(root, &mut files);
    files.sort();
    let mut hits: Vec<String> = Vec::new();
    for file in files {
        let contents = match std::fs::read_to_string(&file) {
            Ok(c) => c,
            Err(_) => continue,
        };
        for (i, line) in contents.lines().enumerate() {
            if line.contains(query) {
                hits.push(format!("{}:{}: {}", file.display(), i + 1, line));
            }
        }
    }
    Ok(hits.join("\n"))
}

#[cfg(test)]
mod tests {
    use crate::tools::ToolCtx;
    use serde_json::json;
    use std::fs;

    fn fixture() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("sub/needle.txt"), "haystack\nfind the needle here\n").unwrap();
        fs::write(dir.path().join("top.txt"), "nothing\n").unwrap();
        dir
    }

    #[test]
    fn search_grep_finds_matching_line() {
        let dir = fixture();
        let ctx = ToolCtx { project_root: dir.path().to_path_buf() };
        let res = super::run(&json!({ "mode": "grep", "path": ".", "query": "needle" }), &ctx);
        assert!(res.model_text.contains("needle.txt"));
        assert!(res.model_text.contains("find the needle here"));
    }

    #[test]
    fn search_find_matches_filename() {
        let dir = fixture();
        let ctx = ToolCtx { project_root: dir.path().to_path_buf() };
        let res = super::run(&json!({ "mode": "find", "path": ".", "query": "needle" }), &ctx);
        assert!(res.model_text.contains("needle.txt"));
        assert!(!res.model_text.contains("top.txt"));
    }

    #[test]
    fn search_ls_lists_directory() {
        let dir = fixture();
        let ctx = ToolCtx { project_root: dir.path().to_path_buf() };
        let res = super::run(&json!({ "mode": "ls", "path": ".", "query": null }), &ctx);
        assert!(res.model_text.contains("sub/"));
        assert!(res.model_text.contains("top.txt"));
    }
}
