//! `sola-debug screenshot` — capture the compositor output to a PNG.
//!
//! Sends `Topic::ScreenshotRequest` and waits for the matching
//! `Topic::ScreenshotResponse`. The actual capture is performed by
//! sola-river via the `wlr-screencopy-unstable-v1` Wayland protocol.

use std::path::Path;

use sola_bus::topics::{ScreenshotRequestPayload, Topic, TopicKind};

use crate::bus;

pub fn run(output: Option<&Path>, timeout_secs: u64) -> i32 {
    let mut client = bus::connect_or_exit();
    bus::subscribe(&mut client, &[TopicKind::ScreenshotResponse]);

    let request_id = bus::fresh_request_id();
    bus::emit(
        &mut client,
        Topic::ScreenshotRequest(ScreenshotRequestPayload {
            request_id,
            path: output.map(|p| p.to_path_buf()),
        }),
    );

    let deadline = bus::deadline(timeout_secs);
    let topic = bus::recv_until(&client, deadline, |t| match t {
        Topic::ScreenshotResponse(r) => r.request_id == request_id,
        _ => false,
    });

    match topic {
        Some(Topic::ScreenshotResponse(r)) => match r.result {
            Ok(path) => {
                println!("{}", path.display());
                0
            }
            Err(e) => {
                let body = serde_json::json!({ "error": e });
                println!("{}", serde_json::to_string_pretty(&body).unwrap_or(e));
                1
            }
        },
        _ => {
            eprintln!("sola-debug: timeout waiting for screenshot (is sola-river running?)");
            2
        }
    }
}
