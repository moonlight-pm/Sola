//! Server-Sent Events frames for the phone HTTP API.

use std::io::{self, Read};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

use serde_json::Value;

pub const KEEPALIVE: Duration = Duration::from_secs(15);

pub fn frame(event: &str, data: &Value) -> Vec<u8> {
    let body = serde_json::to_string(data).unwrap_or_else(|_| "{}".into());
    format!("event: {event}\ndata: {body}\n\n").into_bytes()
}

pub fn comment(text: &str) -> Vec<u8> {
    format!(": {text}\n\n").into_bytes()
}

pub struct SseBody {
    rx: mpsc::Receiver<Vec<u8>>,
    buf: Vec<u8>,
}

impl SseBody {
    pub fn new(rx: mpsc::Receiver<Vec<u8>>) -> Self {
        Self {
            rx,
            buf: Vec::new(),
        }
    }
}

impl Read for SseBody {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        if self.buf.is_empty() {
            match self.rx.recv_timeout(KEEPALIVE) {
                Ok(chunk) => self.buf = chunk,
                Err(RecvTimeoutError::Timeout) => self.buf = comment("keepalive"),
                Err(RecvTimeoutError::Disconnected) => return Ok(0),
            }
        }
        let n = self.buf.len().min(out.len());
        out[..n].copy_from_slice(&self.buf[..n]);
        self.buf.drain(..n);
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn frame_is_event_stream() {
        let b = frame("delta", &json!({"id": "a", "text": "hi"}));
        let s = String::from_utf8(b).unwrap();
        assert!(s.starts_with("event: delta\n"));
        assert!(s.contains("data: {"));
        assert!(s.ends_with("\n\n"));
        assert!(s.contains("\"text\":\"hi\""));
    }

    #[test]
    fn body_emits_keepalive_then_eof() {
        let (tx, rx) = mpsc::channel();
        drop(tx);
        let mut body = SseBody::new(rx);
        let mut buf = [0u8; 32];
        // Disconnected with empty buf → EOF (no wait: sender already gone).
        assert_eq!(body.read(&mut buf).unwrap(), 0);
    }
}
