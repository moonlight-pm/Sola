//! `sola-debug screenshot` — capture the compositor output to a PNG.
//!
//! Emits `Topic::CaptureScreen` and waits for `Topic::Screenshot` from
//! sola-river. The actual capture is delegated to `grim`.

use std::path::Path;

use sola_bus::topics::{CaptureScreenPayload, Topic, TopicKind};

use crate::bus;

pub fn run(output: Option<&Path>, timeout_secs: u64) -> i32 {
    let mut client = bus::connect_or_exit();
    bus::subscribe(&mut client, &[TopicKind::Screenshot]);

    bus::emit(
        &mut client,
        Topic::CaptureScreen(CaptureScreenPayload {
            path: output.map(|p| p.to_path_buf()),
        }),
    );

    let deadline = bus::deadline(timeout_secs);
    let topic = bus::recv_until(&client, deadline, |t, source| match t {
        Topic::Screenshot(_) => source == "sola-river",
        _ => false,
    });

    match topic {
        Some(Topic::Screenshot(r)) => match r.result {
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
