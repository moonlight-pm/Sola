//! Small helpers around `sola_bus::BusClient` for the CLI's
//! emit-and-await-event flow. Routing is by `Message::source` —
//! responses from a target app are tagged with the target's app_id by
//! the bus when the target emits.

use std::time::{Duration, Instant};

use sola_bus::BusClient;
use sola_bus::topics::{Topic, TopicKind};

/// Connect a fresh `BusClient` and identify as `sola-debug`. Exits the
/// process on failure (the CLI has nothing to do without the bus).
pub fn connect_or_exit() -> BusClient {
    let mut client = BusClient::new();
    client.set_app_id("sola-debug");
    if let Err(e) = client.connect() {
        eprintln!("sola-debug: bus connect failed: {e}");
        std::process::exit(3);
    }
    client
}

/// Receive messages until a `(topic, source)` pair satisfies `pred`,
/// or the deadline passes. Returns `None` on timeout.
pub fn recv_until<F>(
    client: &BusClient,
    deadline: Instant,
    mut pred: F,
) -> Option<Topic>
where
    F: FnMut(&Topic, &str) -> bool,
{
    loop {
        let now = Instant::now();
        if now >= deadline {
            return None;
        }
        let remaining = deadline - now;
        let msg = client.recv_timeout(remaining)?;
        let Some(topic) = Topic::parse(&msg) else {
            continue;
        };
        if pred(&topic, &msg.source) {
            return Some(topic);
        }
    }
}

/// Subscribe to a single topic kind, exiting on failure.
pub fn subscribe(client: &mut BusClient, kinds: &[TopicKind]) {
    if let Err(e) = client.subscribe(kinds) {
        eprintln!("sola-debug: bus subscribe failed: {e}");
        std::process::exit(3);
    }
}

/// Emit a topic, exiting on failure.
pub fn emit(client: &mut BusClient, topic: Topic) {
    if let Err(e) = client.emit(topic) {
        eprintln!("sola-debug: bus emit failed: {e}");
        std::process::exit(3);
    }
}

/// Convenience: convert a u64 timeout-in-seconds to a `Duration` deadline
/// from now. Caps at u64::MAX/2 nanoseconds to avoid overflow.
pub fn deadline(timeout_secs: u64) -> Instant {
    Instant::now() + Duration::from_secs(timeout_secs)
}
