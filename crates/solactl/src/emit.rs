//! `solactl emit` — emit any bus topic with a JSON payload.
//!
//! Lets the developer synthesize bus events for testing without writing
//! a one-off script. Mirrors the JSON shape that `sola-monitor` displays,
//! so a topic seen there can be re-emitted by copying the payload.

use sola_bus::topics::{Topic, TopicKind};

use crate::bus;

pub fn run(kind: &str, payload: &str) -> i32 {
    let Some(topic_kind) = TopicKind::from_str(kind) else {
        eprintln!(
            "solactl: unknown topic kind '{kind}'. Use one of:\n  {}",
            TopicKind::ALL
                .iter()
                .map(|k| k.as_str())
                .collect::<Vec<_>>()
                .join(", "),
        );
        return 3;
    };

    let value: serde_json::Value = match serde_json::from_str(payload) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("solactl: invalid JSON payload: {e}");
            return 3;
        }
    };

    let Some(topic) = Topic::from_json_kind(topic_kind, value) else {
        eprintln!(
            "solactl: payload doesn't match the schema for {kind}. Check `sola-monitor` to see the expected shape.",
        );
        return 3;
    };

    let mut client = bus::connect_or_exit();
    bus::emit(&mut client, topic);

    // The bus is async-write; give the writer thread a moment to flush
    // before we exit, otherwise the message may never reach the bus.
    std::thread::sleep(std::time::Duration::from_millis(50));

    println!("emitted {kind}");
    0
}
