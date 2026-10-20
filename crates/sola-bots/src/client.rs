//! Local HTTP client for sola-botsd (`127.0.0.1:HTTP_PORT`).
//! Same REST + SSE surface as the iPhone app.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use serde_json::{Value, json};

use crate::auth::{HTTP_PORT, HTTP_TOKEN};

const CONNECT: Duration = Duration::from_secs(2);
const REST_READ: Duration = Duration::from_secs(8);
const SSE_READ: Duration = Duration::from_secs(45);

fn addr() -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], HTTP_PORT))
}

pub fn get(path: &str) -> Result<Value, String> {
    exchange("GET", path, None)
}

pub fn post(path: &str, body: &Value) -> Result<Value, String> {
    let bytes = serde_json::to_vec(body).map_err(|e| e.to_string())?;
    exchange("POST", path, Some(&bytes))
}

pub fn delete(path: &str) -> Result<Value, String> {
    exchange("DELETE", path, None)
}

fn exchange(method: &str, path: &str, body: Option<&[u8]>) -> Result<Value, String> {
    let mut stream = connect(REST_READ)?;
    write_request(&mut stream, method, path, "application/json", true, body)?;
    let (code, payload) = read_entity(&mut stream)?;
    let value: Value = if payload.iter().all(|b| b.is_ascii_whitespace()) {
        json!({})
    } else {
        serde_json::from_slice(&payload).map_err(|e| e.to_string())?
    };
    if (200..300).contains(&code) {
        Ok(value)
    } else {
        let err = value
            .get("error")
            .and_then(|e| e.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("HTTP {code}"));
        Err(err)
    }
}

/// Blocking SSE read. Calls `emit` for each event. Returns when the
/// server closes or `emit` returns false.
pub fn read_events(mut emit: impl FnMut(&str, Value) -> bool) -> Result<(), String> {
    let mut stream = connect(SSE_READ)?;
    write_request(
        &mut stream,
        "GET",
        "/events",
        "text/event-stream",
        false,
        None,
    )?;
    let mut header_reader = BufReader::new(stream);
    let (code, headers) = read_headers(&mut header_reader)?;
    if !(200..300).contains(&code) {
        return Err(format!("HTTP {code} on /events"));
    }
    let chunked = headers
        .get("transfer-encoding")
        .is_some_and(|s| s.to_ascii_lowercase().contains("chunked"));
    if chunked {
        let mut reader = BufReader::new(ChunkDecoder::new(header_reader));
        read_sse_lines(&mut reader, &mut emit)
    } else {
        read_sse_lines(&mut header_reader, &mut emit)
    }
}

fn read_sse_lines(
    reader: &mut impl BufRead,
    emit: &mut impl FnMut(&str, Value) -> bool,
) -> Result<(), String> {
    let mut block = String::new();
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line).map_err(|e| e.to_string())?;
        if n == 0 {
            return Ok(());
        }
        if line == "\n" || line == "\r\n" {
            if let Some((event, data)) = parse_sse_block(&block) {
                if !emit(&event, data) {
                    return Ok(());
                }
            }
            block.clear();
        } else {
            block.push_str(&line);
        }
    }
}

/// HTTP/1.1 chunked decoder. tiny_http streams SSE as chunked.
struct ChunkDecoder<R: BufRead> {
    inner: R,
    remaining: usize,
    done: bool,
}

impl<R: BufRead> ChunkDecoder<R> {
    fn new(inner: R) -> Self {
        Self {
            inner,
            remaining: 0,
            done: false,
        }
    }
}

impl<R: BufRead> Read for ChunkDecoder<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.done || buf.is_empty() {
            return Ok(0);
        }
        if self.remaining == 0 {
            let mut line = String::new();
            let n = self.inner.read_line(&mut line)?;
            if n == 0 {
                self.done = true;
                return Ok(0);
            }
            let size_s = line.trim().split(';').next().unwrap_or("").trim();
            let size = usize::from_str_radix(size_s, 16)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            if size == 0 {
                self.done = true;
                return Ok(0);
            }
            self.remaining = size;
        }
        let want = self.remaining.min(buf.len());
        let got = self.inner.read(&mut buf[..want])?;
        if got == 0 {
            self.done = true;
            return Ok(0);
        }
        self.remaining -= got;
        if self.remaining == 0 {
            let mut crlf = [0u8; 2];
            self.inner.read_exact(&mut crlf)?;
        }
        Ok(got)
    }
}

fn connect(read_timeout: Duration) -> Result<TcpStream, String> {
    let stream = TcpStream::connect_timeout(&addr(), CONNECT)
        .map_err(|e| format!("sola-botsd is not listening on 127.0.0.1:{HTTP_PORT} ({e})"))?;
    stream
        .set_read_timeout(Some(read_timeout))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(CONNECT))
        .map_err(|e| e.to_string())?;
    Ok(stream)
}

fn write_request(
    stream: &mut TcpStream,
    method: &str,
    path: &str,
    accept: &str,
    close: bool,
    body: Option<&[u8]>,
) -> Result<(), String> {
    let mut head = format!(
        "{method} {path} HTTP/1.1\r\n\
         Host: 127.0.0.1:{HTTP_PORT}\r\n\
         Authorization: Bearer {HTTP_TOKEN}\r\n\
         Accept: {accept}\r\n"
    );
    if close {
        head.push_str("Connection: close\r\n");
    }
    if let Some(body) = body {
        head.push_str("Content-Type: application/json\r\n");
        head.push_str(&format!("Content-Length: {}\r\n", body.len()));
        head.push_str("\r\n");
        stream
            .write_all(head.as_bytes())
            .and_then(|_| stream.write_all(body))
            .and_then(|_| stream.flush())
            .map_err(|e| e.to_string())
    } else {
        head.push_str("\r\n");
        stream
            .write_all(head.as_bytes())
            .and_then(|_| stream.flush())
            .map_err(|e| e.to_string())
    }
}

fn read_headers(reader: &mut impl BufRead) -> Result<(u16, HashMap<String, String>), String> {
    let mut status = String::new();
    reader.read_line(&mut status).map_err(|e| e.to_string())?;
    if status.is_empty() {
        return Err("empty response".into());
    }
    let code = status
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| format!("bad status: {status}"))?;
    let mut headers = HashMap::new();
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).map_err(|e| e.to_string())?;
        if line.is_empty() || line == "\n" || line == "\r\n" {
            break;
        }
        if let Some((k, v)) = line.split_once(':') {
            headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }
    Ok((code, headers))
}

fn read_entity(stream: &mut TcpStream) -> Result<(u16, Vec<u8>), String> {
    let mut reader = BufReader::new(stream);
    let (code, headers) = read_headers(&mut reader)?;
    let mut body = Vec::new();
    if let Some(len) = headers
        .get("content-length")
        .and_then(|s| s.parse::<usize>().ok())
    {
        body.resize(len, 0);
        reader.read_exact(&mut body).map_err(|e| e.to_string())?;
    } else {
        reader.read_to_end(&mut body).map_err(|e| e.to_string())?;
    }
    Ok((code, body))
}

pub fn parse_sse_block(block: &str) -> Option<(String, Value)> {
    let mut event = "message".to_string();
    let mut data = String::new();
    for line in block.lines() {
        let line = line.trim_end_matches('\r');
        if line.is_empty() || line.starts_with(':') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("event:") {
            event = rest.trim().to_string();
        } else if let Some(rest) = line.strip_prefix("data:") {
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(rest.trim_start());
        }
    }
    if data.is_empty() {
        return None;
    }
    let value = serde_json::from_str(&data).ok()?;
    Some((event, value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_delta_frame() {
        let (ev, v) =
            parse_sse_block("event: delta\ndata: {\"id\":\"a\",\"text\":\"hi\"}\n").unwrap();
        assert_eq!(ev, "delta");
        assert_eq!(v["text"], "hi");
    }

    #[test]
    fn skips_keepalive_comment() {
        assert!(parse_sse_block(": keepalive\n").is_none());
    }

    #[test]
    fn decodes_chunked_sse() {
        let payload = "event: bots\ndata: {\"bots\":[]}\n\n";
        let chunked = format!("{:x}\r\n{}\r\n0\r\n\r\n", payload.len(), payload);
        let mut dec = ChunkDecoder::new(std::io::Cursor::new(chunked.into_bytes()));
        let mut out = String::new();
        dec.read_to_string(&mut out).unwrap();
        assert_eq!(out, payload);
    }
}
