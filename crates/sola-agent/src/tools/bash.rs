use std::io::Read;
use std::process::{Command, Stdio};

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

/// Runs `sh -c <command>` in the project root. Stdout is streamed to
/// `on_chunk` as it arrives (so a long command doesn't leave the transcript
/// looking frozen) while still being captured in full for the final
/// `ToolResult`, exactly as a synchronous `Command::output()` would build it.
/// Stderr is drained on a dedicated thread concurrently with stdout so a
/// chatty stderr can't fill its pipe buffer and deadlock the child (the
/// classic two-pipe hazard — see `std::process::Child` docs).
pub fn run(args: &Value, ctx: &ToolCtx, on_chunk: &mut dyn FnMut(&str)) -> ToolResult {
    let command = match args.get("command").and_then(Value::as_str) {
        Some(c) => c,
        None => return error_result("bash: missing required 'command' argument"),
    };
    // Stdout/stderr are captured in full (never redirected to /dev/null).
    let mut child = match Command::new("sh")
        .arg("-c")
        .arg(command)
        .current_dir(&ctx.project_root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => return error_result(format!("bash: failed to spawn `sh -c`: {e}")),
    };

    let mut stderr_pipe = child.stderr.take().expect("stderr piped above");
    let stderr_handle = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stderr_pipe.read_to_end(&mut buf);
        buf
    });

    let mut stdout_pipe = child.stdout.take().expect("stdout piped above");
    let mut raw_stdout = Vec::new();
    let mut read_buf = [0u8; 4096];
    loop {
        match stdout_pipe.read(&mut read_buf) {
            Ok(0) => break,
            Ok(n) => {
                on_chunk(&String::from_utf8_lossy(&read_buf[..n]));
                raw_stdout.extend_from_slice(&read_buf[..n]);
            }
            Err(_) => break,
        }
    }
    drop(stdout_pipe);

    let raw_stderr = stderr_handle.join().unwrap_or_default();
    let status = match child.wait() {
        Ok(s) => s,
        Err(e) => return error_result(format!("bash: failed to wait on `sh -c`: {e}")),
    };
    let code = status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&raw_stdout).into_owned();
    let stderr = String::from_utf8_lossy(&raw_stderr).into_owned();
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
        let res = super::run(&json!({ "command": "echo hi" }), &ctx, &mut |_| {});
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
        let res = super::run(&json!({ "command": "exit 3" }), &ctx, &mut |_| {});
        assert!(matches!(res.ui_detail, ToolDetail::Bash { code: 3, .. }));
    }

    #[test]
    fn bash_streams_stdout_chunks_to_on_chunk() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ToolCtx { project_root: dir.path().to_path_buf() };
        let mut seen = String::new();
        let res = super::run(
            &json!({ "command": "echo hi" }),
            &ctx,
            &mut |chunk| seen.push_str(chunk),
        );
        assert!(seen.contains("hi"), "on_chunk should have seen streamed stdout: {seen:?}");
        match res.ui_detail {
            ToolDetail::Bash { stdout, .. } => assert_eq!(stdout.trim(), "hi"),
            other => panic!("expected Bash, got {other:?}"),
        }
    }
}
