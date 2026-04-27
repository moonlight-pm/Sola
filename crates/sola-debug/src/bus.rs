//! Small helpers around `sola_bus::BusClient` for the CLI's
//! request/response flow.

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use sola_bus::BusClient;
use sola_bus::topics::{Topic, TopicKind};

/// Generate a request id from the current monotonic-ish nanosecond clock.
/// Concurrent CLI invocations get different ids; the responder echoes the
/// id back so cross-talk is impossible.
pub fn fresh_request_id() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(1)
}

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

/// Receive messages until a `Topic` matching `pred` arrives, or the deadline
/// passes. Returns `None` on timeout.
pub fn recv_until<F>(
    client: &BusClient,
    deadline: Instant,
    mut pred: F,
) -> Option<Topic>
where
    F: FnMut(&Topic) -> bool,
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
        if pred(&topic) {
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
