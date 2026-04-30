//! `solactl apps` — list running apps and their windows.
//!
//! Reads the sticky `Topic::Windows` snapshot and groups by `app_id`.

use std::collections::BTreeMap;
use std::time::Duration;

use sola_bus::topics::{Topic, TopicKind, Window};

use crate::bus;

pub fn run() -> i32 {
    let mut client = bus::connect_or_exit();
    bus::subscribe(&mut client, &[TopicKind::Windows]);

    // The bus replays the sticky `Windows` snapshot on subscribe. Wait
    // briefly; if no snapshot arrives, the shell isn't running yet.
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    let topic = bus::recv_until(&client, deadline, |t, _src| matches!(t, Topic::Windows(_)));

    let Some(Topic::Windows(windows)) = topic else {
        eprintln!("solactl: no Windows snapshot from bus (shell not running?)");
        return 3;
    };

    // Group by app_id (sorted) for stable output.
    let mut grouped: BTreeMap<String, Vec<&Window>> = BTreeMap::new();
    for w in &windows {
        grouped.entry(w.app_id.clone()).or_default().push(w);
    }

    let mut out = serde_json::Map::new();
    for (app_id, ws) in grouped {
        let entries: Vec<serde_json::Value> = ws
            .iter()
            .map(|w| {
                serde_json::json!({
                    "title": w.title,
                    "window_id": w.window_id,
                    "pid": w.pid,
                })
            })
            .collect();
        out.insert(app_id, serde_json::Value::Array(entries));
    }

    let pretty = serde_json::to_string_pretty(&out).unwrap_or_else(|_| "{}".to_string());
    println!("{pretty}");
    0
}
