//! Sticky outbox: after sola-bus restarts, reconnecting clients re-emit the
//! stickies they still own so late subscribers (e.g. a restarted shell) can
//! rebuild menus without every app hand-rolling republish logic.

mod common;

use std::time::{Duration, Instant};

use sola_bus::topics::{AppMenuPayload, MenuDefinition, MenuItem, Topic, TopicKind};
use sola_bus::{BusClient, Message};

use common::{GIVE_UP, start_bus, subscribe, wait_for_socket};

fn wait_until_disconnected(client: &BusClient) {
    let deadline = Instant::now() + GIVE_UP;
    while Instant::now() < deadline {
        if !client.is_connected() {
            return;
        }
        // Drain so the reader thread can notice EOF / reset.
        let _ = client.try_recv();
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("client still connected after bus kill");
}

fn terminal_menu() -> AppMenuPayload {
    AppMenuPayload {
        app_id: "sola-terminal".into(),
        menus: vec![
            MenuDefinition {
                label: "Shell".into(),
                items: vec![MenuItem::Action {
                    id: "new-tab".into(),
                    label: "New Tab".into(),
                    shortcut: None,
                    disabled: false,
                    checked: false,
                }],
            },
            MenuDefinition {
                label: "Pane".into(),
                items: vec![],
            },
            MenuDefinition {
                label: "Edit".into(),
                items: vec![],
            },
        ],
    }
}

fn terminal_menu_in(got: &[Message]) -> Option<AppMenuPayload> {
    got.iter().find_map(|m| match Topic::parse(m) {
        Some(Topic::SetAppMenu(p)) if p.app_id == "sola-terminal" => Some(p),
        _ => None,
    })
}

/// App emits SetAppMenu → bus dies → app reconnects → new shell subscriber
/// receives the multi-menu sticky via replay.
#[test]
fn set_app_menu_survives_bus_restart_via_sticky_outbox() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bus");
    let bus = start_bus(&path);
    wait_for_socket(&path);
    let socket = path.to_string_lossy().to_string();

    // App publishes its menubar definition (mirrors terminal multi-menu).
    let mut app = BusClient::new();
    app.set_app_id("sola-terminal");
    app.connect_to(&socket).unwrap();
    app.emit(Topic::SetAppMenu(terminal_menu())).unwrap();
    // The host has the sticky before we kill it.
    subscribe(&mut app, &[]);

    // Replace the bus process — in-memory stickies are wiped.
    drop(bus);
    let _ = std::fs::remove_file(&path);

    wait_until_disconnected(&app);

    let _bus2 = start_bus(&path);
    wait_for_socket(&path);

    // App reconnects; sticky outbox re-emits SetAppMenu to the fresh host.
    app.connect_to(&socket).unwrap();
    assert!(app.is_connected());
    // Outbox re-emit is synchronous in connect_to; the host has it after this.
    subscribe(&mut app, &[]);

    // Shell (or any late joiner) subscribes and must see the menus again.
    let mut shell = BusClient::new();
    shell.set_app_id("sola-shell");
    shell.connect_to(&socket).unwrap();
    let got = subscribe(&mut shell, &[TopicKind::SetAppMenu]);

    let payload =
        terminal_menu_in(&got).expect("shell should receive SetAppMenu after app reconnect");
    let labels: Vec<&str> = payload.menus.iter().map(|m| m.label.as_str()).collect();
    assert_eq!(
        labels,
        vec!["Shell", "Pane", "Edit"],
        "full multi-menu payload must be restored, not a synthesized Quit-only menu"
    );
}

/// Retract must drop the outbox entry so reconnect does not resurrect it.
#[test]
fn retract_clears_sticky_outbox_so_reconnect_does_not_resurrect() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bus");
    let bus = start_bus(&path);
    wait_for_socket(&path);
    let socket = path.to_string_lossy().to_string();

    let mut app = BusClient::new();
    app.set_app_id("sola-terminal");
    app.connect_to(&socket).unwrap();
    let menu = terminal_menu();
    app.emit(Topic::SetAppMenu(menu.clone())).unwrap();
    app.retract(Topic::SetAppMenu(menu)).unwrap();
    subscribe(&mut app, &[]);

    drop(bus);
    let _ = std::fs::remove_file(&path);
    wait_until_disconnected(&app);

    let _bus2 = start_bus(&path);
    wait_for_socket(&path);
    app.connect_to(&socket).unwrap();
    // Anything the outbox re-emitted, the host has after this.
    subscribe(&mut app, &[]);

    let mut shell = BusClient::new();
    shell.set_app_id("sola-shell");
    shell.connect_to(&socket).unwrap();
    let got = subscribe(&mut shell, &[TopicKind::SetAppMenu]);

    assert!(
        terminal_menu_in(&got).is_none(),
        "retracted SetAppMenu must not reappear after reconnect"
    );
}
