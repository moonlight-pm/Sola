use std::path::PathBuf;
use std::time::{Duration, Instant};

use sola_bus::BusClient;
use sola_bus::topics::{LaunchAppPayload, Theme, Topic, TopicKind};

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
    panic!("bus socket never appeared");
}

#[test]
fn subscribed_client_receives_filtered_topics() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bus");
    let mut bus = start_bus(path.clone());
    wait_for_socket(&path);
    let socket = path.to_string_lossy().to_string();

    let mut a = BusClient::new();
    a.set_app_id("a");
    a.connect_to(&socket).unwrap();
    a.subscribe(&[TopicKind::Shutdown]).unwrap();

    let mut b = BusClient::new();
    b.set_app_id("b");
    b.connect_to(&socket).unwrap();

    b.emit(Topic::Shutdown).unwrap();
    b.emit(Topic::LaunchApp(LaunchAppPayload {
        app_id: "brave".into(),
        command: "brave".into(),
    }))
    .unwrap();

    let deadline = Instant::now() + Duration::from_millis(500);
    let mut got_shutdown = false;
    let mut got_launch = false;
    while Instant::now() < deadline {
        if let Some(m) = a.recv_timeout(Duration::from_millis(50)) {
            match m.topic.as_str() {
                "Shutdown" => got_shutdown = true,
                "LaunchApp" => got_launch = true,
                _ => {}
            }
        }
    }
    assert!(got_shutdown, "subscriber should receive Shutdown");
    assert!(!got_launch, "subscriber should not receive LaunchApp");

    let _ = bus.kill();
}

#[test]
fn sticky_emit_loops_back_to_sender() {
    // Sticky/persistent topics carry shared state — the sender writing
    // a new value must receive their own emission through the same path
    // as everyone else, so framework-level subscribers in the sender's
    // process (e.g. the kit's bus pump lowering Topic::Theme to CSS)
    // see the update. Ephemeral topics keep the historical skip-self
    // semantics, verified separately below.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bus");
    let mut bus = start_bus(path.clone());
    wait_for_socket(&path);
    let socket = path.to_string_lossy().to_string();

    let mut a = BusClient::new();
    a.set_app_id("a");
    a.connect_to(&socket).unwrap();
    a.subscribe(&[TopicKind::Theme]).unwrap();
    std::thread::sleep(Duration::from_millis(50));

    a.emit(Topic::Theme(Theme::default())).unwrap();

    let deadline = Instant::now() + Duration::from_millis(500);
    let mut got_theme = false;
    while Instant::now() < deadline {
        if let Some(m) = a.recv_timeout(Duration::from_millis(50)) {
            if m.topic == "Theme" {
                got_theme = true;
                break;
            }
        }
    }
    assert!(got_theme, "sender should receive its own sticky/persistent emission");

    let _ = bus.kill();
}

#[test]
fn ephemeral_emit_skips_sender() {
    // Ephemeral topics (input events, one-shot signals) should NOT echo
    // back to the sender — that's the historical contract and the
    // reason the skip-self branch exists at all.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bus");
    let mut bus = start_bus(path.clone());
    wait_for_socket(&path);
    let socket = path.to_string_lossy().to_string();

    let mut a = BusClient::new();
    a.set_app_id("a");
    a.connect_to(&socket).unwrap();
    a.subscribe(&[TopicKind::Shutdown]).unwrap();
    std::thread::sleep(Duration::from_millis(50));

    a.emit(Topic::Shutdown).unwrap();

    let deadline = Instant::now() + Duration::from_millis(300);
    let mut got = false;
    while Instant::now() < deadline {
        if let Some(m) = a.recv_timeout(Duration::from_millis(50)) {
            if m.topic == "Shutdown" {
                got = true;
                break;
            }
        }
    }
    assert!(!got, "sender must not receive its own ephemeral emission");

    let _ = bus.kill();
}

#[test]
fn subscribe_replays_roster() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bus");
    let mut bus = start_bus(path.clone());
    wait_for_socket(&path);
    let socket = path.to_string_lossy().to_string();

    let mut a = BusClient::new();
    a.set_app_id("sola-session");
    a.connect_to(&socket).unwrap();
    std::thread::sleep(Duration::from_millis(50));

    let mut b = BusClient::new();
    b.set_app_id("b");
    b.connect_to(&socket).unwrap();
    b.subscribe(&[TopicKind::ClientConnected]).unwrap();

    let deadline = Instant::now() + Duration::from_millis(500);
    let mut saw = false;
    while Instant::now() < deadline {
        if let Some(m) = b.recv_timeout(Duration::from_millis(50)) {
            if let Some(Topic::ClientConnected(app_id)) = Topic::parse(&m) {
                if app_id == "sola-session" {
                    saw = true;
                    break;
                }
            }
        }
    }
    assert!(saw, "b should receive ClientConnected(sola-session)");

    let _ = bus.kill();
}
