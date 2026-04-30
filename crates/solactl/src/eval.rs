//! `solactl eval` — evaluate a JS expression in a Sola app's WebView.
//!
//! Emits `Topic::Evaluate` and waits for the next `Topic::Evaluation`
//! whose `Message::source` matches the target app. The framework in
//! `sola-app` wraps the expression, runs it, and emits the result.
//!
//! Concurrent invocations against the same target race; the bus has no
//! request_id correlation. `solactl` is one-at-a-time by design.

use sola_bus::topics::{EvaluatePayload, Topic, TopicKind};

use crate::bus;

pub fn run(app: &str, window: Option<&str>, expression: &str, timeout_secs: u64) -> i32 {
    let mut client = bus::connect_or_exit();
    bus::subscribe(&mut client, &[TopicKind::Evaluation]);

    bus::emit(
        &mut client,
        Topic::Evaluate(EvaluatePayload {
            target_app: app.to_string(),
            window: window.map(str::to_string),
            expr: expression.to_string(),
        }),
    );

    let deadline = bus::deadline(timeout_secs);
    let topic = bus::recv_until(&client, deadline, |t, source| match t {
        Topic::Evaluation(_) => source == app,
        _ => false,
    });

    match topic {
        Some(Topic::Evaluation(r)) => match r.result {
            Ok(json) => {
                match serde_json::from_str::<serde_json::Value>(&json) {
                    Ok(v) => {
                        let pretty = serde_json::to_string_pretty(&v).unwrap_or(json);
                        println!("{pretty}");
                    }
                    Err(_) => println!("{json}"),
                }
                0
            }
            Err(e) => {
                let body = serde_json::json!({ "error": e });
                println!("{}", serde_json::to_string_pretty(&body).unwrap_or(e));
                1
            }
        },
        _ => {
            eprintln!(
                "solactl: timeout waiting for response from {app} (is it running?)"
            );
            2
        }
    }
}
