use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A single message on the Sola Bus.
///
/// Every bus message is an event. There are no requests, responses, or RPCs —
/// just events that any client can emit, and any client can listen for.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    /// Unique, monotonic identifier. UUIDv7 embeds a millisecond-precision
    /// timestamp that can be extracted for logging and ordering.
    pub id: Uuid,

    /// Topic string in `category:event-name` format.
    /// Example: "shell:show-switcher", "shell:apps"
    pub topic: String,

    /// Arbitrary binary payload, deserialized by the consumer.
    /// The bus does not inspect this — it's opaque bytes.
    pub payload: Option<Vec<u8>>,
}

impl Event {
    /// Create a new event with the given topic and no payload.
    pub fn new(topic: impl Into<String>) -> Self {
        Self {
            id: Uuid::now_v7(),
            topic: topic.into(),
            payload: None,
        }
    }

    /// Create a new event with the given topic and payload.
    pub fn with_payload(topic: impl Into<String>, payload: Vec<u8>) -> Self {
        Self {
            id: Uuid::now_v7(),
            topic: topic.into(),
            payload: Some(payload),
        }
    }

    /// Extract the millisecond timestamp from the UUIDv7 id.
    pub fn timestamp_ms(&self) -> u64 {
        let bytes = self.id.as_bytes();
        let ms = ((bytes[0] as u64) << 40)
            | ((bytes[1] as u64) << 32)
            | ((bytes[2] as u64) << 24)
            | ((bytes[3] as u64) << 16)
            | ((bytes[4] as u64) << 8)
            | (bytes[5] as u64);
        ms
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_creates_event_without_payload() {
        let event = Event::new("shell:test");
        assert_eq!(event.topic, "shell:test");
        assert!(event.payload.is_none());
    }

    #[test]
    fn with_payload_creates_event_with_payload() {
        let data = vec![1, 2, 3];
        let event = Event::with_payload("shell:test", data.clone());
        assert_eq!(event.topic, "shell:test");
        assert_eq!(event.payload.unwrap(), data);
    }

    #[test]
    fn timestamp_is_recent() {
        let before = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let event = Event::new("shell:test");
        let after = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let ts = event.timestamp_ms();
        assert!(ts >= before && ts <= after, "timestamp {ts} not in [{before}, {after}]");
    }

    #[test]
    fn ids_are_unique() {
        let a = Event::new("shell:test");
        let b = Event::new("shell:test");
        assert_ne!(a.id, b.id);
    }

    #[test]
    fn ids_are_monotonic() {
        let a = Event::new("shell:test");
        let b = Event::new("shell:test");
        assert!(b.id > a.id);
    }
}
