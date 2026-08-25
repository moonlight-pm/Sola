//! sola-call provider for the `compositor` owner.

use std::sync::mpsc;

use sola_bus::topics::{CaptureScreenPayload, CaptureTarget, PointerAction, PointerButton};
use sola_call::methods::{self, OWNER_COMPOSITOR};
use sola_call::{Incoming, ReplyTx};
use sola_core::KeyChord;

use crate::client::{AppData, screenshot, virtual_keyboard, virtual_pointer};

pub fn start() -> mpsc::Receiver<Incoming> {
    sola_call::start_provider(
        OWNER_COMPOSITOR,
        "sola-river",
        methods::compositor_methods(),
    )
}

pub fn poll(state: &mut AppData) {
    let mut batch = Vec::new();
    let mut dead = false;
    if let Some(rx) = state.call_rx.as_ref() {
        loop {
            match rx.try_recv() {
                Ok(inc) => batch.push(inc),
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    dead = true;
                    break;
                }
            }
        }
    }
    if dead {
        state.call_rx = None;
    }
    for inc in batch {
        dispatch(state, inc);
    }
}

fn dispatch(state: &mut AppData, inc: Incoming) {
    match inc.method.as_str() {
        "screenshot" => {
            let req = match screenshot_req(&inc.params) {
                Ok(r) => r,
                Err(e) => {
                    inc.reply.err(e);
                    return;
                }
            };
            screenshot::handle_call(state, req, inc.reply);
        }
        "windows" => inc.reply.ok(windows_json(state)),
        "input.click" => input_reply(inc.reply, click(state, &inc.params)),
        "input.move" => input_reply(inc.reply, move_ptr(state, &inc.params)),
        "input.scroll" => input_reply(inc.reply, scroll(state, &inc.params)),
        "input.key" => input_reply(inc.reply, key(state, &inc.params)),
        other => inc.reply.err(format!("unknown method {other}")),
    }
}

fn input_reply(reply: ReplyTx, result: Result<(), String>) {
    match result {
        Ok(()) => reply.ok(serde_json::json!({ "ok": true })),
        Err(e) => reply.err(e),
    }
}

fn screenshot_req(params: &serde_json::Value) -> Result<CaptureScreenPayload, String> {
    let path = params
        .get("output")
        .and_then(|v| v.as_str())
        .map(std::path::PathBuf::from);
    let target = if let Some(app_id) = params.get("app").and_then(|v| v.as_str()) {
        CaptureTarget::Window {
            app_id: app_id.to_string(),
            title: params
                .get("window")
                .and_then(|v| v.as_str())
                .map(str::to_string),
        }
    } else if let (Some(x), Some(y), Some(width), Some(height)) = (
        params.get("x").and_then(|v| v.as_i64()),
        params.get("y").and_then(|v| v.as_i64()),
        params.get("width").and_then(|v| v.as_i64()),
        params.get("height").and_then(|v| v.as_i64()),
    ) {
        CaptureTarget::Region {
            x: x as i32,
            y: y as i32,
            width: width as i32,
            height: height as i32,
        }
    } else {
        CaptureTarget::FullOutput
    };
    Ok(CaptureScreenPayload { path, target })
}

fn windows_json(state: &AppData) -> serde_json::Value {
    let windows = state.registry.as_windows();
    let mut grouped = serde_json::Map::new();
    for w in windows {
        let entry = serde_json::json!({
            "title": w.title,
            "window_id": w.window_id,
            "pid": w.pid,
        });
        grouped
            .entry(w.app_id)
            .or_insert_with(|| serde_json::json!([]))
            .as_array_mut()
            .unwrap()
            .push(entry);
    }
    serde_json::Value::Object(grouped)
}

fn click(state: &AppData, params: &serde_json::Value) -> Result<(), String> {
    let x = i32_param(params, "x")?;
    let y = i32_param(params, "y")?;
    let button = match params
        .get("button")
        .and_then(|v| v.as_str())
        .unwrap_or("left")
    {
        "left" | "l" => PointerButton::Left,
        "right" | "r" => PointerButton::Right,
        "middle" | "m" => PointerButton::Middle,
        other => return Err(format!("unknown button {other}")),
    };
    virtual_pointer::dispatch(state, &PointerAction::Click { button, x, y })
}

fn move_ptr(state: &AppData, params: &serde_json::Value) -> Result<(), String> {
    virtual_pointer::dispatch(
        state,
        &PointerAction::Move {
            x: i32_param(params, "x")?,
            y: i32_param(params, "y")?,
        },
    )
}

fn scroll(state: &AppData, params: &serde_json::Value) -> Result<(), String> {
    virtual_pointer::dispatch(
        state,
        &PointerAction::Scroll {
            dx: params.get("dx").and_then(|v| v.as_f64()).unwrap_or(0.0),
            dy: params.get("dy").and_then(|v| v.as_f64()).unwrap_or(5.0),
        },
    )
}

fn key(state: &AppData, params: &serde_json::Value) -> Result<(), String> {
    let chord: KeyChord = serde_json::from_value(
        params
            .get("chord")
            .cloned()
            .ok_or_else(|| "missing chord".to_string())?,
    )
    .map_err(|e| format!("chord: {e}"))?;
    virtual_keyboard::synthesize_chord(state, &chord)
}

fn i32_param(params: &serde_json::Value, name: &str) -> Result<i32, String> {
    params
        .get(name)
        .and_then(|v| v.as_i64())
        .map(|n| n as i32)
        .ok_or_else(|| format!("missing {name}"))
}
