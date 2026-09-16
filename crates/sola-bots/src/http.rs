//! Phone API: JSON over HTTP, bearer token, bind `HTTP_BIND`.

use std::io::Read;
use std::sync::Arc;
use std::thread;
use std::time::Instant;

use tiny_http::{Header, Method, Request, Response, Server, StatusCode};
use tracing::{info, warn};

use crate::auth::{HTTP_BIND, HTTP_TOKEN};
use crate::host::Host;

const MAX_BODY: u64 = 256 * 1024;

pub fn spawn(host: Arc<Host>) {
    thread::Builder::new()
        .name("bots-http".into())
        .spawn(move || serve(host))
        .expect("bots-http thread");
}

fn serve(host: Arc<Host>) {
    let server = match Server::http(HTTP_BIND) {
        Ok(s) => s,
        Err(e) => {
            warn!("http listen {HTTP_BIND}: {e}");
            return;
        }
    };
    info!(addr = HTTP_BIND, "bots http listening");
    for req in server.incoming_requests() {
        let host = Arc::clone(&host);
        thread::spawn(move || {
            if let Err(e) = handle(&host, req) {
                warn!("http: {e}");
            }
        });
    }
}

fn handle(host: &Arc<Host>, req: Request) -> Result<(), String> {
    let t0 = Instant::now();
    let method = req.method().clone();
    let url = req.url().to_string();
    let path = url.split('?').next().unwrap_or("/").to_string();
    let remote = req
        .remote_addr()
        .map(|a| a.to_string())
        .unwrap_or_else(|| "-".into());
    let result = handle_inner(host, req, &method, &path, &url);
    let ms = t0.elapsed().as_millis();
    match &result {
        Ok(code) => info!(%method, %path, %remote, %code, %ms, "http"),
        Err(e) => warn!(%method, %path, %remote, %ms, "http error: {e}"),
    }
    result.map(|_| ())
}

fn handle_inner(
    host: &Arc<Host>,
    mut req: Request,
    method: &Method,
    path: &str,
    url: &str,
) -> Result<u16, String> {
    if method == &Method::Options {
        return req
            .respond(cors(Response::empty(204)))
            .map_err(|e| e.to_string())
            .map(|()| 204);
    }

    if path == "/health" && method == &Method::Get {
        return respond(req, 200, serde_json::json!({ "ok": true }));
    }

    if !authorized(&req) {
        return respond(req, 401, serde_json::json!({ "error": "unauthorized" }));
    }

    if method == &Method::Get && path == "/bots" {
        return respond(req, 200, host.list_json());
    }
    if method == &Method::Get && path == "/poll" {
        let bot = query_param(url, "bot");
        return respond(req, 200, host.poll_json(bot.as_deref()));
    }
    if method == &Method::Get {
        if let Some(id) = path
            .strip_prefix("/bots/")
            .and_then(|r| r.strip_suffix("/transcript"))
        {
            return match host.transcript_json(id) {
                Ok(v) => respond(req, 200, v),
                Err(e) => respond(req, 404, serde_json::json!({ "error": e })),
            };
        }
    }
    if method == &Method::Post && path == "/bots" {
        let body = match read_json(&mut req) {
            Ok(v) => v,
            Err(e) => return respond(req, 400, serde_json::json!({ "error": e })),
        };
        let name = body
            .get("name")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string();
        return match host.create(&name) {
            Ok(v) => respond(req, 200, v),
            Err(e) => respond(req, 400, serde_json::json!({ "error": e })),
        };
    }
    if method == &Method::Delete {
        if let Some(id) = path.strip_prefix("/bots/") {
            if !id.is_empty() && !id.contains('/') {
                return match host.rm(id) {
                    Ok(v) => respond(req, 200, v),
                    Err(e) => respond(req, 400, serde_json::json!({ "error": e })),
                };
            }
        }
    }
    if method == &Method::Post {
        if let Some(id) = path
            .strip_prefix("/bots/")
            .and_then(|r| r.strip_suffix("/send"))
        {
            let body = match read_json(&mut req) {
                Ok(v) => v,
                Err(e) => return respond(req, 400, serde_json::json!({ "error": e })),
            };
            let text = body
                .get("text")
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .to_string();
            return match host.send(id, &text) {
                Ok(v) => respond(req, 200, v),
                Err(e) => respond(req, 400, serde_json::json!({ "error": e })),
            };
        }
        if let Some(id) = path
            .strip_prefix("/bots/")
            .and_then(|r| r.strip_suffix("/cancel"))
        {
            return match host.cancel(id) {
                Ok(v) => respond(req, 200, v),
                Err(e) => respond(req, 400, serde_json::json!({ "error": e })),
            };
        }
    }
    respond(req, 404, serde_json::json!({ "error": "not found" }))
}

fn query_param(url: &str, key: &str) -> Option<String> {
    let q = url.split_once('?')?.1;
    for pair in q.split('&') {
        let (k, v) = pair.split_once('=')?;
        if k == key && !v.is_empty() {
            return Some(v.to_string());
        }
    }
    None
}

fn authorized(req: &Request) -> bool {
    req.headers()
        .iter()
        .any(|h| h.field.equiv("Authorization") && bearer_ok(h.value.as_str()))
}

fn bearer_ok(value: &str) -> bool {
    let got = value
        .strip_prefix("Bearer ")
        .or_else(|| value.strip_prefix("bearer "))
        .unwrap_or(value)
        .as_bytes();
    constant_eq(got, HTTP_TOKEN.as_bytes())
}

fn constant_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter()
        .zip(b.iter())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

fn read_json(req: &mut Request) -> Result<serde_json::Value, String> {
    let mut buf = Vec::new();
    req.as_reader()
        .take(MAX_BODY + 1)
        .read_to_end(&mut buf)
        .map_err(|e| e.to_string())?;
    if buf.len() as u64 > MAX_BODY {
        return Err("body too large".into());
    }
    if buf.iter().all(|b| b.is_ascii_whitespace()) {
        return Ok(serde_json::json!({}));
    }
    serde_json::from_slice(&buf).map_err(|e| e.to_string())
}

fn respond(req: Request, code: u16, body: serde_json::Value) -> Result<u16, String> {
    req.respond(json_status(code, body))
        .map_err(|e| e.to_string())?;
    Ok(code)
}

fn json_status(code: u16, body: serde_json::Value) -> Response<std::io::Cursor<Vec<u8>>> {
    let bytes = serde_json::to_vec(&body).unwrap_or_else(|_| b"{}".to_vec());
    let len = bytes.len();
    cors(
        Response::new(
            StatusCode(code),
            vec![
                Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
                Header::from_bytes(&b"Connection"[..], &b"close"[..]).unwrap(),
            ],
            std::io::Cursor::new(bytes),
            Some(len),
            None,
        )
        .with_status_code(StatusCode(code)),
    )
}

fn cors<R: std::io::Read>(res: Response<R>) -> Response<R> {
    res.with_header(Header::from_bytes(&b"Access-Control-Allow-Origin"[..], &b"*"[..]).unwrap())
        .with_header(
            Header::from_bytes(
                &b"Access-Control-Allow-Headers"[..],
                &b"Authorization, Content-Type"[..],
            )
            .unwrap(),
        )
        .with_header(
            Header::from_bytes(
                &b"Access-Control-Allow-Methods"[..],
                &b"GET, POST, DELETE, OPTIONS"[..],
            )
            .unwrap(),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bearer_accepts_prefix_and_raw() {
        assert!(bearer_ok(&format!("Bearer {HTTP_TOKEN}")));
        assert!(bearer_ok(HTTP_TOKEN));
        assert!(!bearer_ok("Bearer deadbeef"));
        assert!(!bearer_ok(""));
    }

    #[test]
    fn query_param_reads_bot() {
        assert_eq!(
            query_param("/poll?bot=abc-123", "bot").as_deref(),
            Some("abc-123")
        );
        assert_eq!(query_param("/poll", "bot"), None);
        assert_eq!(query_param("/poll?bot=", "bot"), None);
    }
}
