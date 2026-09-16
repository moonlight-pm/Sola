//! Phone API: JSON over HTTP, bearer token, bind `HTTP_BIND`.

use std::io::Read;
use std::sync::Arc;
use std::thread;

use tiny_http::{Header, Method, Request, Response, Server, StatusCode};
use tracing::{info, warn};

use crate::auth::{HTTP_BIND, HTTP_TOKEN};
use crate::host::Host;

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

fn handle(host: &Arc<Host>, mut req: Request) -> Result<(), String> {
    if req.method() == &Method::Options {
        return req
            .respond(cors(Response::empty(204)))
            .map_err(|e| e.to_string());
    }

    let path = req.url().split('?').next().unwrap_or("/").to_string();
    let method = req.method().clone();
    if path == "/health" && method == Method::Get {
        return respond(req, 200, serde_json::json!({ "ok": true }));
    }

    if !authorized(&req) {
        return respond(req, 401, serde_json::json!({ "error": "unauthorized" }));
    }

    if method == Method::Get && path == "/bots" {
        return respond(req, 200, host.list_json());
    }
    if method == Method::Get {
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
    if method == Method::Post {
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

fn authorized(req: &Request) -> bool {
    req.headers().iter().any(|h| {
        h.field.equiv("Authorization") && {
            let v = h.value.as_str();
            v == format!("Bearer {HTTP_TOKEN}") || v == HTTP_TOKEN
        }
    })
}

fn read_json(req: &mut Request) -> Result<serde_json::Value, String> {
    let mut buf = String::new();
    req.as_reader()
        .read_to_string(&mut buf)
        .map_err(|e| e.to_string())?;
    if buf.trim().is_empty() {
        return Ok(serde_json::json!({}));
    }
    serde_json::from_str(&buf).map_err(|e| e.to_string())
}

fn respond(req: Request, code: u16, body: serde_json::Value) -> Result<(), String> {
    req.respond(json_status(code, body)).map_err(|e| e.to_string())
}

fn json_status(code: u16, body: serde_json::Value) -> Response<std::io::Cursor<Vec<u8>>> {
    let bytes = serde_json::to_vec(&body).unwrap_or_else(|_| b"{}".to_vec());
    let len = bytes.len();
    cors(
        Response::new(
            StatusCode(code),
            vec![Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap()],
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
            Header::from_bytes(&b"Access-Control-Allow-Methods"[..], &b"GET, POST, OPTIONS"[..])
                .unwrap(),
        )
}
