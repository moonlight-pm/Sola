//! `sola-debug eval` — evaluate a JS expression in a Sola app's WebView.
//!
//! Sends `Topic::DebugRequest`, waits for the matching `Topic::DebugResponse`.
//! The framework in `sola-app` handles wrapping the expression and
//! routing the result back.

use sola_bus::topics::{
    DebugOp, DebugRequestPayload, DebugResult, Topic, TopicKind,
};

use crate::bus;

pub fn run(app: &str, window: Option<&str>, expression: &str, timeout_secs: u64) -> i32 {
    let mut client = bus::connect_or_exit();
    bus::subscribe(&mut client, &[TopicKind::DebugResponse]);

    let request_id = bus::fresh_request_id();
    bus::emit(
        &mut client,
        Topic::DebugRequest(DebugRequestPayload {
            request_id,
            target_app: app.to_string(),
            op: DebugOp::Eval {
                window: window.map(str::to_string),
                expr: expression.to_string(),
            },
        }),
    );

    let deadline = bus::deadline(timeout_secs);
    let topic = bus::recv_until(&client, deadline, |t| match t {
        Topic::DebugResponse(r) => r.request_id == request_id,
        _ => false,
    });

    match topic {
        Some(Topic::DebugResponse(r)) => match r.result {
            DebugResult::Json(json) => {
                // Re-parse + pretty-print so output is human-readable.
                match serde_json::from_str::<serde_json::Value>(&json) {
                    Ok(v) => {
                        let pretty = serde_json::to_string_pretty(&v).unwrap_or(json);
                        println!("{pretty}");
                    }
                    Err(_) => println!("{json}"),
                }
                0
            }
            DebugResult::Error(e) => {
                let body = serde_json::json!({ "error": e });
                println!("{}", serde_json::to_string_pretty(&body).unwrap_or(e));
                1
            }
        },
        _ => {
            eprintln!(
                "sola-debug: timeout waiting for response from {app} (is it running?)"
            );
            2
        }
    }
}
