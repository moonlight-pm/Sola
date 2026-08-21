//! Unix-socket hook receiver. HTTP POST over UDS, or a bare JSON object.

use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

use serde_json::Value;

use super::map::{self, MappedHook};

#[derive(Debug, Clone)]
pub struct Incoming {
    pub pane_id: String,
    pub mapped: MappedHook,
}

pub fn bind(path: &Path) -> std::io::Result<UnixListener> {
    let _ = std::fs::remove_file(path);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let listener = UnixListener::bind(path)?;
    // World-readable socket in XDG_RUNTIME_DIR; Grok hooks run as the user.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(listener)
}

pub fn serve(listener: UnixListener, tx: mpsc::Sender<Incoming>) {
    let _ = listener.set_nonblocking(false);
    for stream in listener.incoming() {
        let Ok(mut stream) = stream else {
            continue;
        };
        let _ = stream.set_read_timeout(Some(Duration::from_millis(1500)));
        match read_request(&mut stream) {
            Ok(Some(incoming)) => {
                let _ = tx.send(incoming);
                let _ = stream.write_all(b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n");
            }
            Ok(None) => {
                let _ = stream.write_all(b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n");
            }
            Err(_) => {
                let _ = stream.write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n");
            }
        }
    }
}

pub fn read_request(stream: &mut UnixStream) -> std::io::Result<Option<Incoming>> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    loop {
        let n = stream.read(&mut tmp)?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n]);
        if buf.len() > 256 * 1024 {
            break;
        }
        if looks_complete(&buf) {
            break;
        }
    }
    parse_buf(&buf)
}

fn looks_complete(buf: &[u8]) -> bool {
    if buf.starts_with(b"{") {
        return serde_json::from_slice::<Value>(buf).is_ok();
    }
    let Some(idx) = find_headers_end(buf) else {
        return false;
    };
    let headers = std::str::from_utf8(&buf[..idx]).unwrap_or("");
    let len = content_length(headers).unwrap_or(0);
    buf.len() >= idx + 4 + len
}

fn find_headers_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

fn content_length(headers: &str) -> Option<usize> {
    for line in headers.lines() {
        let lower = line.to_ascii_lowercase();
        if let Some(rest) = lower.strip_prefix("content-length:") {
            return rest.trim().parse().ok();
        }
    }
    None
}

fn header_value<'a>(headers: &'a str, name: &str) -> Option<&'a str> {
    let want = name.to_ascii_lowercase();
    for line in headers.lines() {
        if let Some((k, v)) = line.split_once(':') {
            if k.trim().eq_ignore_ascii_case(&want) {
                return Some(v.trim());
            }
        }
    }
    None
}

pub fn parse_buf(buf: &[u8]) -> std::io::Result<Option<Incoming>> {
    if buf.is_empty() {
        return Ok(None);
    }
    if buf[0] == b'{' {
        return incoming_from_json(None, buf);
    }
    let Some(idx) = find_headers_end(buf) else {
        return Ok(None);
    };
    let headers = std::str::from_utf8(&buf[..idx])
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let pane = header_value(headers, "X-Sola-Pane-Id").map(str::to_string);
    let body = &buf[idx + 4..];
    incoming_from_json(pane, body)
}

fn incoming_from_json(
    pane_header: Option<String>,
    body: &[u8],
) -> std::io::Result<Option<Incoming>> {
    if body.is_empty() {
        return Ok(None);
    }
    let value: Value = serde_json::from_slice(body)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let pane_id = pane_header
        .or_else(|| {
            value
                .get("paneId")
                .or_else(|| value.get("pane_id"))
                .and_then(|v| v.as_str())
                .map(str::to_string)
        })
        .unwrap_or_default();
    if pane_id.is_empty() {
        return Ok(None);
    }
    let event = if value.get("hookEventName").is_some() || value.get("hook_event_name").is_some() {
        value
    } else {
        value.get("payload").cloned().unwrap_or(value)
    };
    let Some(mapped) = map::map_grok(&event) else {
        return Ok(None);
    };
    Ok(Some(Incoming { pane_id, mapped }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_http_post() {
        let body = br#"{"hookEventName":"UserPromptSubmit","prompt":"hi"}"#;
        let req = format!(
            "POST /hook/grok HTTP/1.1\r\nX-Sola-Pane-Id: pane-1\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            std::str::from_utf8(body).unwrap()
        );
        let got = parse_buf(req.as_bytes()).unwrap().unwrap();
        assert_eq!(got.pane_id, "pane-1");
        assert_eq!(got.mapped.status, Some(crate::status::AgentStatus::Working));
        assert_eq!(got.mapped.prompt.as_deref(), Some("hi"));
    }

    #[test]
    fn parses_bare_json_envelope() {
        let raw = br#"{"paneId":"p2","hookEventName":"Stop"}"#;
        let got = parse_buf(raw).unwrap().unwrap();
        assert_eq!(got.pane_id, "p2");
        assert_eq!(got.mapped.status, Some(crate::status::AgentStatus::Done));
    }
}
