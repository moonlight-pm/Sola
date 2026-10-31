//! A Gab AI bot's turn: Gab's chat API (OpenAI-shaped), streamed.
//!
//! The API keeps no conversation, so each turn sends the dialog so far.
//! The key is `GAB_API_KEY`, or `~/.config/sola/bots/gab.key` (Gab AI →
//! Settings → API); requests need Gab's Plus plan.

use std::io::{BufRead, BufReader};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::Duration;

use serde_json::{Value, json};

use crate::paths;

pub const ENDPOINT: &str = "https://gab.ai/v1/chat/completions";
/// Gab's own model.
pub const DEFAULT_MODEL: &str = "arya";
/// The most dialog a turn sends back, newest kept.
const HISTORY_CHARS: usize = 60_000;

/// One message of the conversation sent.
pub struct Message<'a> {
    /// `system`, `user`, or `assistant`.
    pub role: &'a str,
    pub content: &'a str,
}

pub fn key() -> Result<String, String> {
    if let Ok(k) = std::env::var("GAB_API_KEY") {
        if !k.trim().is_empty() {
            return Ok(k.trim().to_string());
        }
    }
    let path = paths::gab_key_path();
    std::fs::read_to_string(&path)
        .ok()
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty())
        .ok_or_else(|| {
            format!(
                "Gab AI needs an API key: put it in {} (Gab AI → Settings → API)",
                path.display()
            )
        })
}

/// Run a turn over `messages` (system first, the new message last),
/// calling `on_text` with each piece of the reply.
pub fn run(
    model: &str,
    messages: &[Message<'_>],
    cancel: impl Fn() -> bool,
    mut on_text: impl FnMut(&str),
) -> Result<(), String> {
    let key = key()?;
    let model = if model.is_empty() { DEFAULT_MODEL } else { model };
    let body = json!({
        "model": model,
        "stream": true,
        "messages": trimmed(messages)
            .iter()
            .map(|m| json!({ "role": m.role, "content": m.content }))
            .collect::<Vec<_>>(),
    })
    .to_string();

    // The request blocks; it runs on its own thread so Stop is heard.
    let (tx, rx) = mpsc::channel::<Result<Option<String>, String>>();
    thread::Builder::new()
        .name("bots-gab".into())
        .spawn(move || {
            if let Err(e) = request(&key, &body, &tx) {
                let _ = tx.send(Err(e));
            }
        })
        .map_err(|e| e.to_string())?;
    loop {
        if cancel() {
            return Err("cancelled".into());
        }
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(Ok(Some(text))) => on_text(&text),
            Ok(Ok(None)) => return Ok(()),
            Ok(Err(e)) => return Err(e),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return Ok(()),
        }
    }
}

/// Send the request; pass each piece of text on, then `None` at the end.
fn request(
    key: &str,
    body: &str,
    tx: &mpsc::Sender<Result<Option<String>, String>>,
) -> Result<(), String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_recv_response(Some(Duration::from_secs(90)))
        .http_status_as_error(false)
        .user_agent("Sola Bots")
        .build()
        .into();
    let mut response = agent
        .post(ENDPOINT)
        .header("Authorization", &format!("Bearer {key}"))
        .header("Content-Type", "application/json")
        .header("Accept", "text/event-stream")
        .send(body)
        .map_err(|e| format!("gab.ai: {e}"))?;
    let status = response.status();
    let streamed = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("text/event-stream"));
    if !status.is_success() || !streamed {
        let text = response
            .body_mut()
            .read_to_string()
            .map_err(|e| format!("gab.ai: {e}"))?;
        if !status.is_success() {
            return Err(format!("gab.ai ({status}): {}", error_text(&text)));
        }
        // A whole reply instead of a stream.
        let v: Value = serde_json::from_str(&text).map_err(|e| format!("gab.ai: {e}"))?;
        if let Some(t) = v.pointer("/choices/0/message/content").and_then(Value::as_str) {
            let _ = tx.send(Ok(Some(t.to_string())));
        }
        let _ = tx.send(Ok(None));
        return Ok(());
    }
    let reader = BufReader::new(response.into_body().into_reader());
    for line in reader.lines() {
        let line = line.map_err(|e| format!("gab.ai: {e}"))?;
        match read_line(&line) {
            Line::Text(t) => {
                if tx.send(Ok(Some(t))).is_err() {
                    return Ok(());
                }
            }
            Line::Done => break,
            Line::Error(e) => return Err(format!("gab.ai: {e}")),
            Line::Other => {}
        }
    }
    let _ = tx.send(Ok(None));
    Ok(())
}

#[derive(Debug, PartialEq)]
enum Line {
    Text(String),
    Done,
    Error(String),
    Other,
}

fn read_line(line: &str) -> Line {
    let Some(data) = line.strip_prefix("data:").map(str::trim) else {
        return Line::Other;
    };
    if data == "[DONE]" {
        return Line::Done;
    }
    let Ok(v) = serde_json::from_str::<Value>(data) else {
        return Line::Other;
    };
    if v.get("error").is_some() {
        return Line::Error(error_text(data));
    }
    match v.pointer("/choices/0/delta/content").and_then(Value::as_str) {
        Some(t) if !t.is_empty() => Line::Text(t.to_string()),
        _ => Line::Other,
    }
}

/// An OpenAI-shaped error's message, or the body as it came.
fn error_text(body: &str) -> String {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| {
            v.pointer("/error/message")
                .or_else(|| v.get("error"))
                .or_else(|| v.get("message"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| body.trim().chars().take(300).collect())
}

/// The system message and the newest messages that fit [`HISTORY_CHARS`].
fn trimmed<'a, 'b>(messages: &'b [Message<'a>]) -> Vec<&'b Message<'a>> {
    let Some((system, rest)) = messages.split_first() else {
        return Vec::new();
    };
    let mut kept: Vec<&Message<'_>> = Vec::new();
    let mut size = 0;
    for m in rest.iter().rev() {
        size += m.content.len();
        if size > HISTORY_CHARS && !kept.is_empty() {
            break;
        }
        kept.push(m);
    }
    kept.push(system);
    kept.reverse();
    kept
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_stream_lines() {
        assert_eq!(
            read_line(r#"data: {"choices":[{"delta":{"content":"Hi"}}]}"#),
            Line::Text("Hi".into())
        );
        assert_eq!(read_line("data: [DONE]"), Line::Done);
        assert_eq!(read_line(": keepalive"), Line::Other);
        assert_eq!(
            read_line(r#"data: {"error":{"message":"no credits"}}"#),
            Line::Error("no credits".into())
        );
    }

    #[test]
    fn keeps_the_system_message_and_the_newest_turns() {
        let long = "x".repeat(HISTORY_CHARS);
        let msgs = [
            Message { role: "system", content: "s" },
            Message { role: "user", content: &long },
            Message { role: "assistant", content: "a" },
            Message { role: "user", content: "now" },
        ];
        let kept: Vec<&str> = trimmed(&msgs).iter().map(|m| m.content).collect();
        assert_eq!(kept, ["s", "a", "now"]);
    }
}
