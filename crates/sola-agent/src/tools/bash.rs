use std::process::Command;

use serde_json::{json, Value};

use super::{error_result, ToolCtx, ToolDetail, ToolResult};

pub fn schema() -> Value {
    json!({
        "type": "function",
        "name": "bash",
        "description": "Run a shell command with `sh -c` in the project root. Stdout, stderr, and the exit code are captured and returned; a nonzero exit is reported, not an error.",
        "parameters": {
            "type": "object",
            "properties": {
                "command": { "type": "string", "description": "The shell command line to execute." }
            },
            "required": ["command"],
            "additionalProperties": false
        },
        "strict": true
    })
}

pub fn run(args: &Value, ctx: &ToolCtx) -> ToolResult {
    let command = match args.get("command").and_then(Value::as_str) {
        Some(c) => c,
        None => return error_result("bash: missing required 'command' argument"),
    };
    // Stdout/stderr are captured in full (never redirected to /dev/null).
    let output = match Command::new("sh")
        .arg("-c")
        .arg(command)
        .current_dir(&ctx.project_root)
        .output()
    {
        Ok(o) => o,
        Err(e) => return error_result(format!("bash: failed to spawn `sh -c`: {e}")),
    };
    let code = output.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    tracing::debug!(command, code, "bash tool executed");

    let mut model_text = format!("exit code: {code}\n");
    if !stdout.is_empty() {
        model_text.push_str("stdout:\n");
        model_text.push_str(&stdout);
        if !stdout.ends_with('\n') {
            model_text.push('\n');
        }
    }
    if !stderr.is_empty() {
        model_text.push_str("stderr:\n");
        model_text.push_str(&stderr);
        if !stderr.ends_with('\n') {
            model_text.push('\n');
        }
    }

    ToolResult {
        model_text,
        ui_detail: ToolDetail::Bash { code, stdout, stderr },
    }
}

#[cfg(test)]
mod tests {
    use crate::tools::{ToolCtx, ToolDetail};
    use serde_json::json;

    #[test]
    fn bash_captures_stdout_and_zero_exit() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ToolCtx { project_root: dir.path().to_path_buf() };
        let res = super::run(&json!({ "command": "echo hi" }), &ctx);
        match res.ui_detail {
            ToolDetail::Bash { code, stdout, stderr } => {
                assert_eq!(code, 0);
                assert_eq!(stdout.trim(), "hi");
                assert_eq!(stderr, "");
            }
            other => panic!("expected Bash, got {other:?}"),
        }
    }

    #[test]
    fn bash_nonzero_exit_returns_code_not_error() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ToolCtx { project_root: dir.path().to_path_buf() };
        let res = super::run(&json!({ "command": "exit 3" }), &ctx);
        assert!(matches!(res.ui_detail, ToolDetail::Bash { code: 3, .. }));
    }
}
