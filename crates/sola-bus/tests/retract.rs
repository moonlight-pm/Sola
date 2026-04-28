//! Integration tests for `keys` plumbing on `Topic::to_message`.
//! End-to-end retract behavior runs through the bus host's `handle_client`
//! and is exercised in real usage; this file covers the message-shaping
//! pieces that downstream consumers rely on.

use sola_bus::topics::{AppMenuPayload, Topic};

#[test]
fn unkeyed_topic_emits_empty_keys() {
    let topic = Topic::Zones(std::collections::HashMap::new());
    let msg = topic.to_message();
    assert!(msg.keys.is_empty());
}

#[test]
fn set_app_menu_emits_app_id_as_key() {
    let topic = Topic::SetAppMenu(AppMenuPayload {
        app_id: "sola-browser".into(),
        menus: vec![],
    });
    let msg = topic.to_message();
    assert_eq!(msg.keys, vec!["sola-browser".to_string()]);
}

#[test]
fn two_apps_set_app_menu_have_independent_keys() {
    let a = Topic::SetAppMenu(AppMenuPayload {
        app_id: "sola-browser".into(),
        menus: vec![],
    })
    .to_message();
    let b = Topic::SetAppMenu(AppMenuPayload {
        app_id: "sola-terminal".into(),
        menus: vec![],
    })
    .to_message();
    assert_ne!(a.keys, b.keys);
}
