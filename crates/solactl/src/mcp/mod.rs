//! `solactl mcp` — an MCP server (stdio) for the Sola call plane.
//!
//! Every live owner's methods are tools (see [`tools`]); the list follows
//! apps as they start and quit (`notifications/tools/list_changed`). This is
//! an adapter over the same methods `solactl` invokes, not a second
//! protocol: privilege is the call socket's (see D3). JSON-RPC 2.0, one
//! message per line. Nothing but protocol goes to stdout.

mod tools;

use std::collections::BTreeSet;
use std::io::{self, BufRead, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};

/// Newest first; an unknown client version gets the newest.
const PROTOCOLS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];
/// How often the tool list is compared against the live call plane.
const RESCAN: Duration = Duration::from_secs(2);

const INSTRUCTIONS: &str = "\
Sola desktop call plane. Tools are `<app>_<method>` for the apps running now \
(the list changes as apps start and quit); each mirrors `solactl <app> <method>`. \
Path arguments may be relative to your working directory.

Design (sola-design): a canvas is files. `<project>/design/<canvas>/canvas.toml` \
lists artboards (src, x, y, width, height or \"auto\"); each artboard is an HTML \
page beside it. Start with design_canvas_new (name; project = your working \
directory) — it makes the folder, lists it in the human's rail under the project, and \
opens it for you off their screen (they look when they like; other agents may be \
working on other canvases at the same time). design_open opens an existing one the \
same way. Page calls (status, view, wait, reload, screenshot, pdf, inspect) go to the \
canvas you last opened or made; pass `canvas` (its folder) for another. Edit the files \
directly, then design_reload or design_wait (both make the boards show the files on \
disk and wait for web fonts and layout), then design_screenshot (returns the image) \
or design_inspect (sizes, styles, overflow; `all` lists every match's box). \
design_selection is the element the human Super+clicked (it names its canvas). Only \
put a canvas on the human's screen (`raise`) when they ask to see it.

Boards are Chromium (CEF) pages that Design serves over http from the project \
root, each on its own host (`http://<board>--<canvas>.<project>.localhost:<port>/\
design/<canvas>/page.html`). Root-absolute URLs (`/x`) resolve against the project \
root; fetch, ES modules, and iframes work as on a site; dotfiles are not served. A \
`src` may carry `?query` / `#fragment` (one page, two states: \
`light.html?theme=dark`). `root = \"web/public\"` in canvas.toml makes root-absolute \
URLs look in that project folder first (product `/fonts/…`). Edits reload the \
canvas when they touch its folder, shared files in design/, or any project file a \
board loaded (a product stylesheet outside design/ included).

The human may pan, zoom, and look around (any canvas) while you work; nothing you \
read depends on their view. Screenshots and PDFs show the whole artboard (2×, or \
less for a very large one), inspect reads the page. design_view moves their camera \
only until they take it.";

type Out = Arc<Mutex<io::Stdout>>;

pub fn run(only: Vec<String>) -> i32 {
    let out: Out = Arc::new(Mutex::new(io::stdout()));
    let ready = Arc::new(AtomicBool::new(false));
    watch_tools(only.clone(), out.clone(), ready.clone());
    for line in io::stdin().lock().lines() {
        let Ok(line) = line else {
            break;
        };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str::<Value>(line) {
            Ok(msg) => handle(msg, &only, &out, &ready),
            Err(e) => send(&out, &error(Value::Null, -32700, &format!("parse error: {e}"))),
        }
    }
    0
}

fn handle(msg: Value, only: &[String], out: &Out, ready: &AtomicBool) {
    let method = msg.get("method").and_then(Value::as_str).unwrap_or_default();
    // Notifications (no id): only `initialized` matters.
    let Some(id) = msg.get("id").cloned() else {
        if method == "notifications/initialized" {
            ready.store(true, Ordering::Release);
        }
        return;
    };
    match method {
        "initialize" => send(out, &reply(id, initialize(&msg))),
        "ping" => send(out, &reply(id, json!({}))),
        "tools/list" => send(out, &reply(id, json!({"tools": list(only)}))),
        "tools/call" => {
            // Calls can wait tens of seconds (design_reload, browser wait);
            // keep reading so ping / list stay answered.
            let (out, only) = (out.clone(), only.to_vec());
            std::thread::spawn(move || send(&out, &reply(id, call(&msg, &only))));
        }
        "" => {}
        other => send(out, &error(id, -32601, &format!("method not found: {other}"))),
    }
}

fn initialize(msg: &Value) -> Value {
    let asked = msg
        .pointer("/params/protocolVersion")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let version = PROTOCOLS
        .iter()
        .find(|v| **v == asked)
        .unwrap_or(&PROTOCOLS[0]);
    json!({
        "protocolVersion": version,
        "capabilities": {"tools": {"listChanged": true}},
        "serverInfo": {"name": "sola", "version": env!("CARGO_PKG_VERSION")},
        "instructions": INSTRUCTIONS,
    })
}

/// Live tools; empty when sola-call is down.
fn live(only: &[String]) -> Vec<tools::Tool> {
    sola_call::catalog()
        .map(|c| tools::tools(&c, only))
        .unwrap_or_default()
}

fn list(only: &[String]) -> Vec<Value> {
    live(only).iter().map(tools::describe).collect()
}

fn call(msg: &Value, only: &[String]) -> Value {
    let name = msg
        .pointer("/params/name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    match live(only).into_iter().find(|t| t.name == name) {
        Some(tool) => tools::call(&tool, msg.pointer("/params/arguments")),
        None => tools::error_result(&format!(
            "no live tool {name:?} — is that app running? (tools follow the apps that are up)"
        )),
    }
}

/// Tell the client when apps start or quit (after it is initialized).
fn watch_tools(only: Vec<String>, out: Out, ready: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        let names = |only: &[String]| -> BTreeSet<String> {
            live(only).into_iter().map(|t| t.name).collect()
        };
        let mut last = names(&only);
        loop {
            std::thread::sleep(RESCAN);
            let now = names(&only);
            if now != last && ready.load(Ordering::Acquire) {
                send(
                    &out,
                    &json!({"jsonrpc": "2.0", "method": "notifications/tools/list_changed"}),
                );
            }
            last = now;
        }
    });
}

fn reply(id: Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

/// One line per message; a closed stdout ends the session quietly.
fn send(out: &Out, msg: &Value) {
    let line = msg.to_string();
    let Ok(mut w) = out.lock() else {
        return;
    };
    if writeln!(w, "{line}").and_then(|()| w.flush()).is_err() {
        std::process::exit(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negotiates_known_versions_else_newest() {
        let ask = |v: &str| json!({"params": {"protocolVersion": v}});
        assert_eq!(initialize(&ask("2025-03-26"))["protocolVersion"], "2025-03-26");
        assert_eq!(initialize(&ask("1999-01-01"))["protocolVersion"], PROTOCOLS[0]);
        let init = initialize(&ask("2025-06-18"));
        assert_eq!(init["capabilities"]["tools"]["listChanged"], true);
        assert!(init["instructions"].as_str().unwrap().contains("design_reload"));
        assert!(init["instructions"].as_str().unwrap().contains("design_canvas_new"));
    }

    #[test]
    fn replies_and_errors_are_json_rpc() {
        let r = reply(json!(7), json!({"ok": true}));
        assert_eq!(r["jsonrpc"], "2.0");
        assert_eq!(r["id"], 7);
        let e = error(json!("a"), -32601, "nope");
        assert_eq!(e["error"]["code"], -32601);
        assert!(!r.to_string().contains('\n'));
    }
}
