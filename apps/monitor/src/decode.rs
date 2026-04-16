use serde_json::{Value, json};
use sola_bus::Message;
use sola_bus::topics::Topic;

/// Convert a raw bus message into a JSON event for the frontend.
pub fn message_to_json(msg: &Message) -> Value {
    let topic_name = &msg.topic;
    let timestamp = msg.timestamp_ms();
    let id = msg.id.to_string();

    let (payload, raw_hex) = decode_payload(msg);

    // Source: prefer msg.source, then extract app_id from payload
    let source = if !msg.source.is_empty() {
        msg.source.clone()
    } else if let Some(app_id) = payload.get("app_id").and_then(|v| v.as_str()) {
        app_id.to_string()
    } else {
        String::new()
    };

    json!({
        "event": "bus_message",
        "msgId": id,
        "timestamp": timestamp,
        "topic": topic_name,
        "sticky": msg.sticky,
        "source": source,
        "payload": payload,
        "rawHex": raw_hex,
    })
}

/// Attempt to decode the payload via Topic::parse, falling back to hex.
fn decode_payload(msg: &Message) -> (Value, Value) {
    if let Some(topic) = Topic::parse(msg) {
        let payload = topic_to_json(&topic);
        return (payload, Value::Null);
    }

    // Unknown topic or decode failure — return raw hex
    match &msg.payload {
        Some(bytes) => (Value::Null, Value::String(hex::encode(bytes))),
        None => (Value::Null, Value::Null),
    }
}

/// Convert a parsed Topic's payload into a JSON value.
fn topic_to_json(topic: &Topic) -> Value {
    match topic {
        Topic::Windows(v) => serde_json::to_value(v).unwrap_or_default(),
        Topic::LaunchApp(v) => serde_json::to_value(v).unwrap_or_default(),
        Topic::Composition(v) => serde_json::to_value(v).unwrap_or_default(),
        Topic::Frame(v) => serde_json::to_value(v).unwrap_or_default(),
        Topic::Focus(v) => serde_json::to_value(v).unwrap_or_default(),
        Topic::SetWindowPolicy(v) => serde_json::to_value(v).unwrap_or_default(),
        Topic::OutputGeometry(v) => serde_json::to_value(v).unwrap_or_default(),
        Topic::MouseEntered(v) => serde_json::to_value(v).unwrap_or_default(),
        Topic::SetAppMenu(v) => serde_json::to_value(v).unwrap_or_default(),
        Topic::MenuAction(v) => serde_json::to_value(v).unwrap_or_default(),
        Topic::ShellKeyBindings(v) => serde_json::to_value(v).unwrap_or_default(),
        Topic::OpenUrl(v) => serde_json::to_value(v).unwrap_or_default(),
        Topic::Shutdown => Value::Null,
    }
}
