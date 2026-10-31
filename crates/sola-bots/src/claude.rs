//! A Claude bot's turn: one `claude -p` per message, on the bot's session.
//!
//! Claude Code runs on its own login (the subscription): an
//! `ANTHROPIC_API_KEY` in the daemon's environment would switch it to the
//! API, so it is never passed on, nor the variables of a Claude Code
//! session the daemon happens to run under. The first turn names the
//! session (`--session-id`); later turns `--resume` it. Text streams as
//! `stream_event` deltas (`--include-partial-messages`).

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde_json::Value;
use tracing::warn;

use crate::foundation;
use crate::paths;

/// One turn.
pub struct Turn<'a> {
    pub home: &'a Path,
    /// Empty for Claude Code's default.
    pub model: &'a str,
    pub session: &'a str,
    /// Resume `session`; otherwise start it.
    pub resume: bool,
    pub prompt: &'a str,
}

/// Run `turn`, calling `on_text` with each piece of the reply.
pub fn run(
    turn: &Turn<'_>,
    cancel: impl Fn() -> bool,
    mut on_text: impl FnMut(&str),
) -> Result<(), String> {
    let bin = paths::claude_bin();
    let mut cmd = Command::new(&bin);
    cmd.args([
        "-p",
        "--output-format",
        "stream-json",
        "--verbose",
        "--include-partial-messages",
        "--permission-mode",
        "bypassPermissions",
        // No MCP servers, as a Grok bot has none (not the account's
        // claude.ai connectors either).
        "--strict-mcp-config",
        "--append-system-prompt",
        foundation::SEAT_RULES,
    ]);
    if !turn.model.is_empty() {
        cmd.args(["--model", turn.model]);
    }
    cmd.args([if turn.resume { "--resume" } else { "--session-id" }, turn.session]);
    // Variadic: last, so it takes no flag after it.
    cmd.arg("--disallowed-tools");
    cmd.args(foundation::SEAT_DENY.iter().map(|p| format!("Bash({p}:*)")));
    for (key, _) in std::env::vars_os() {
        let key = key.to_string_lossy();
        if key == "ANTHROPIC_API_KEY"
            || key == "CLAUDECODE"
            || key == "CLAUDE_PID"
            || key == "CLAUDE_EFFORT"
            || key.starts_with("CLAUDE_CODE_")
        {
            cmd.env_remove(key.as_ref());
        }
    }
    cmd.env("SOLA_BOT", "1")
        .current_dir(turn.home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("spawn claude ({}) failed: {e}", bin.display()))?;

    // The prompt goes in on stdin: a message can outgrow an argument.
    if let Some(mut stdin) = child.stdin.take() {
        let prompt = turn.prompt.to_string();
        thread::Builder::new()
            .name("bots-claude-in".into())
            .spawn(move || {
                let _ = stdin.write_all(prompt.as_bytes());
            })
            .map_err(|e| e.to_string())?;
    }
    let stderr_tail = Arc::new(Mutex::new(String::new()));
    let mut stderr_done = None;
    if let Some(stderr) = child.stderr.take() {
        let tail = Arc::clone(&stderr_tail);
        let reader = thread::Builder::new()
            .name("bots-claude-err".into())
            .spawn(move || {
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    if line.trim().is_empty() {
                        continue;
                    }
                    warn!("claude stderr: {line}");
                    if let Ok(mut t) = tail.lock() {
                        *t = line;
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        stderr_done = Some(reader);
    }
    let (tx, rx) = mpsc::channel::<Value>();
    let stdout = child.stdout.take().ok_or("claude: no stdout")?;
    thread::Builder::new()
        .name("bots-claude-out".into())
        .spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                match serde_json::from_str::<Value>(&line) {
                    Ok(v) => {
                        if tx.send(v).is_err() {
                            break;
                        }
                    }
                    Err(_) if line.trim().is_empty() => {}
                    Err(e) => warn!("claude: skip non-JSON line ({e}): {line}"),
                }
            }
        })
        .map_err(|e| e.to_string())?;

    let mut said = false;
    let outcome: Result<(), Option<String>> = loop {
        if cancel() {
            let _ = child.kill();
            let _ = child.wait();
            return Err("cancelled".to_string());
        }
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(event) => match read_event(&event) {
                Event::TextStarts if said => on_text("\n\n"),
                Event::Text(text) => {
                    said = true;
                    on_text(&text);
                }
                Event::Done(result) => break result,
                Event::TextStarts | Event::Other => {}
            },
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break Err(None),
        }
    };
    let _ = child.wait();
    // A failure without words in its result has them on stderr ("No
    // conversation found with session ID…").
    outcome.map_err(|why| {
        if let Some(reader) = stderr_done {
            let _ = reader.join();
        }
        let tail = stderr_tail.lock().map(|t| t.clone()).unwrap_or_default();
        match why {
            Some(why) => format!("claude: {why}"),
            None if !tail.is_empty() => format!("claude: {tail}"),
            None => "claude stopped without replying".to_string(),
        }
    })
}

#[derive(Debug, PartialEq)]
enum Event {
    /// A new text block: after earlier text, a paragraph break.
    TextStarts,
    Text(String),
    /// The turn's end; a failure's words when its result has any.
    Done(Result<(), Option<String>>),
    Other,
}

fn read_event(v: &Value) -> Event {
    match v.get("type").and_then(Value::as_str) {
        Some("stream_event") => {
            let ev = &v["event"];
            match ev.get("type").and_then(Value::as_str) {
                Some("content_block_start")
                    if ev.pointer("/content_block/type").and_then(Value::as_str) == Some("text") =>
                {
                    Event::TextStarts
                }
                Some("content_block_delta")
                    if ev.pointer("/delta/type").and_then(Value::as_str) == Some("text_delta") =>
                {
                    match ev.pointer("/delta/text").and_then(Value::as_str) {
                        Some(t) if !t.is_empty() => Event::Text(t.to_string()),
                        _ => Event::Other,
                    }
                }
                _ => Event::Other,
            }
        }
        Some("result") => {
            if v.get("is_error").and_then(Value::as_bool) == Some(true) {
                let why = v
                    .get("result")
                    .and_then(Value::as_str)
                    .filter(|s| !s.trim().is_empty())
                    .map(str::to_string);
                Event::Done(Err(why))
            } else {
                Event::Done(Ok(()))
            }
        }
        _ => Event::Other,
    }
}

/// Whether a resume failed because Claude Code has no such session (its
/// files went): start a new one instead.
pub fn session_missing(err: &str) -> bool {
    err.contains("No conversation found")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_text_deltas_and_the_result() {
        let delta = json!({"type":"stream_event","event":{"type":"content_block_delta","index":1,
            "delta":{"type":"text_delta","text":"hi there"}}});
        assert_eq!(read_event(&delta), Event::Text("hi there".into()));
        let thinking = json!({"type":"stream_event","event":{"type":"content_block_delta","index":0,
            "delta":{"type":"thinking_delta","thinking":"hmm"}}});
        assert_eq!(read_event(&thinking), Event::Other);
        let start = json!({"type":"stream_event","event":{"type":"content_block_start","index":1,
            "content_block":{"type":"text","text":""}}});
        assert_eq!(read_event(&start), Event::TextStarts);
        let ok = json!({"type":"result","subtype":"success","is_error":false,"result":"hi"});
        assert_eq!(read_event(&ok), Event::Done(Ok(())));
        let bad = json!({"type":"result","subtype":"error_during_execution","is_error":true});
        assert_eq!(read_event(&bad), Event::Done(Err(None)));
        let said = json!({"type":"result","is_error":true,"result":"API Error: 529"});
        assert_eq!(read_event(&said), Event::Done(Err(Some("API Error: 529".into()))));
    }
}
