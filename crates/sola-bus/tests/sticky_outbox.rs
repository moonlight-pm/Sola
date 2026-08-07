//! Sticky outbox: after sola-bus restarts, reconnecting clients re-emit the
//! stickies they still own so late subscribers (e.g. a restarted shell) can
//! rebuild menus without every app hand-rolling republish logic.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use sola_bus::BusClient;
use sola_bus::topics::{AppMenuPayload, MenuDefinition, MenuItem, Topic, TopicKind};

fn start_bus(path: PathBuf) -> std::process::Child {
    let exe = env!("CARGO_BIN_EXE_sola-bus");
    std::process::Command::new(exe)
        .env("SOLA_BUS_PATH", &path)
        .env("RUST_LOG", "sola_bus=warn")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn bus")
}

fn wait_for_socket(path: &PathBuf) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if path.exists() {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("bus socket never appeared at {}", path.display());
}

fn wait_until_disconnected(client: &BusClient) {
    let deadline = Instant::now() + Duration::from_secs(2);
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

/// App emits SetAppMenu → bus dies → app reconnects → new shell subscriber
/// receives the multi-menu sticky via replay.
#[test]
fn set_app_menu_survives_bus_restart_via_sticky_outbox() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bus");
    let mut bus = start_bus(path.clone());
    wait_for_socket(&path);
    let socket = path.to_string_lossy().to_string();

    // App publishes its menubar definition (mirrors terminal multi-menu).
    let mut app = BusClient::new();
    app.set_app_id("sola-terminal");
    app.connect_to(&socket).unwrap();
    app.emit(Topic::SetAppMenu(terminal_menu())).unwrap();
    // Let the host process the sticky before we kill it.
    std::thread::sleep(Duration::from_millis(50));

    // Replace the bus process — in-memory stickies are wiped.
    let _ = bus.kill();
    let _ = bus.wait();
    let _ = std::fs::remove_file(&path);

    wait_until_disconnected(&app);

    let mut bus2 = start_bus(path.clone());
    wait_for_socket(&path);

    // App reconnects; sticky outbox re-emits SetAppMenu to the fresh host.
    app.connect_to(&socket).unwrap();
    assert!(app.is_connected());
    // Outbox re-emit is synchronous in connect_to; brief settle for host map.
    std::thread::sleep(Duration::from_millis(50));

    // Shell (or any late joiner) subscribes and must see the menus again.
    let mut shell = BusClient::new();
    shell.set_app_id("sola-shell");
    shell.connect_to(&socket).unwrap();
    shell.subscribe(&[TopicKind::SetAppMenu]).unwrap();

    let deadline = Instant::now() + Duration::from_millis(800);
    let mut got: Option<AppMenuPayload> = None;
    while Instant::now() < deadline {
        if let Some(m) = shell.recv_timeout(Duration::from_millis(50)) {
            if let Some(Topic::SetAppMenu(p)) = Topic::parse(&m) {
                if p.app_id == "sola-terminal" {
                    got = Some(p);
                    break;
                }
            }
        }
    }

    let payload = got.expect("shell should receive SetAppMenu after app reconnect");
    let labels: Vec<&str> = payload.menus.iter().map(|m| m.label.as_str()).collect();
    assert_eq!(
        labels,
        vec!["Shell", "Pane", "Edit"],
        "full multi-menu payload must be restored, not a synthesized Quit-only menu"
    );

    let _ = bus2.kill();
}

/// Retract must drop the outbox entry so reconnect does not resurrect it.
#[test]
fn retract_clears_sticky_outbox_so_reconnect_does_not_resurrect() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bus");
    let mut bus = start_bus(path.clone());
    wait_for_socket(&path);
    let socket = path.to_string_lossy().to_string();

    let mut app = BusClient::new();
    app.set_app_id("sola-terminal");
    app.connect_to(&socket).unwrap();
    let menu = terminal_menu();
    app.emit(Topic::SetAppMenu(menu.clone())).unwrap();
    app.retract(Topic::SetAppMenu(menu)).unwrap();
    std::thread::sleep(Duration::from_millis(50));

    let _ = bus.kill();
    let _ = bus.wait();
    let _ = std::fs::remove_file(&path);
    wait_until_disconnected(&app);

    let mut bus2 = start_bus(path.clone());
    wait_for_socket(&path);
    app.connect_to(&socket).unwrap();
    std::thread::sleep(Duration::from_millis(50));

    let mut shell = BusClient::new();
    shell.set_app_id("sola-shell");
    shell.connect_to(&socket).unwrap();
    shell.subscribe(&[TopicKind::SetAppMenu]).unwrap();

    let deadline = Instant::now() + Duration::from_millis(400);
    let mut saw_terminal = false;
    while Instant::now() < deadline {
        if let Some(m) = shell.recv_timeout(Duration::from_millis(50)) {
            if let Some(Topic::SetAppMenu(p)) = Topic::parse(&m) {
                if p.app_id == "sola-terminal" {
                    saw_terminal = true;
                    break;
                }
            }
        }
    }
    assert!(
        !saw_terminal,
        "retracted SetAppMenu must not reappear after reconnect"
    );

    let _ = bus2.kill();
}
