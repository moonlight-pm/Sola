//! Smoke test: the message produced by Topic::to_message carries
//! an empty `keys` vec for an unkeyed persistent topic. The end-to-end
//! retract behavior (with a real keyed topic) is verified at Task 11.

use sola_bus::topics::Topic;

#[test]
fn unkeyed_topic_emits_empty_keys() {
    let topic = Topic::Zones(std::collections::HashMap::new());
    let msg = topic.to_message();
    assert!(msg.keys.is_empty());
}
