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

/// Try `Topic::parse` and forward to the macro-generated
/// `Topic::to_json_value`. On parse failure, surface the raw bytes as
/// hex so unknown / corrupt traffic is still visible in the audit UI.
fn decode_payload(msg: &Message) -> (Value, Value) {
    if let Some(topic) = Topic::parse(msg) {
        return (topic.to_json_value(), Value::Null);
    }
    match &msg.payload {
        Some(bytes) => (Value::Null, Value::String(hex::encode(bytes))),
        None => (Value::Null, Value::Null),
    }
}
