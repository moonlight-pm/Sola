mod common;

use sola_bus::BusClient;
use sola_bus::topics::{LaunchAppPayload, Theme, Topic, TopicKind};

use common::{receive_through, start_bus, subscribe, wait_for_socket};

#[test]
fn subscribed_client_receives_filtered_topics() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bus");
    let _bus = start_bus(&path);
    wait_for_socket(&path);
    let socket = path.to_string_lossy().to_string();

    let mut a = BusClient::new();
    a.set_app_id("a");
    a.connect_to(&socket).unwrap();
    subscribe(&mut a, &[TopicKind::Shutdown]);

    let mut b = BusClient::new();
    b.set_app_id("b");
    b.connect_to(&socket).unwrap();

    // LaunchApp first: one client's emits arrive in order, so it would
    // have come by the time Shutdown does.
    b.emit(Topic::LaunchApp(LaunchAppPayload {
        app_id: "brave".into(),
        command: "brave".into(),
    }))
    .unwrap();
    b.emit(Topic::Shutdown).unwrap();

    let got = receive_through(&a, "Shutdown");
    assert!(
        !got.iter().any(|m| m.topic == "LaunchApp"),
        "subscriber should not receive LaunchApp"
    );
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
    let _bus = start_bus(&path);
    wait_for_socket(&path);
    let socket = path.to_string_lossy().to_string();

    let mut a = BusClient::new();
    a.set_app_id("a");
    a.connect_to(&socket).unwrap();
    subscribe(&mut a, &[TopicKind::Theme]);

    a.emit(Topic::Theme(Theme::default())).unwrap();

    receive_through(&a, "Theme");
}

#[test]
fn ephemeral_emit_skips_sender() {
    // Ephemeral topics (input events, one-shot signals) should NOT echo
    // back to the sender — that's the historical contract and the
    // reason the skip-self branch exists at all.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bus");
    let _bus = start_bus(&path);
    wait_for_socket(&path);
    let socket = path.to_string_lossy().to_string();

    let mut a = BusClient::new();
    a.set_app_id("a");
    a.connect_to(&socket).unwrap();
    subscribe(&mut a, &[TopicKind::Shutdown, TopicKind::Theme]);

    // A sticky does echo, and in order: an echoed Shutdown would come
    // before it.
    a.emit(Topic::Shutdown).unwrap();
    a.emit(Topic::Theme(Theme::default())).unwrap();

    let got = receive_through(&a, "Theme");
    assert!(
        !got.iter().any(|m| m.topic == "Shutdown"),
        "sender must not receive its own ephemeral emission"
    );
}

#[test]
fn subscribe_replays_roster() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bus");
    let _bus = start_bus(&path);
    wait_for_socket(&path);
    let socket = path.to_string_lossy().to_string();

    let mut a = BusClient::new();
    a.set_app_id("sola-session");
    a.connect_to(&socket).unwrap();
    // The bus has a in its roster, so b gets it as replay.
    subscribe(&mut a, &[]);

    let mut b = BusClient::new();
    b.set_app_id("b");
    b.connect_to(&socket).unwrap();
    let got = subscribe(&mut b, &[TopicKind::ClientConnected]);

    assert!(
        got.iter().any(|m| matches!(
            Topic::parse(m),
            Some(Topic::ClientConnected(app_id)) if app_id == "sola-session"
        )),
        "b should receive ClientConnected(sola-session)"
    );
}

#[test]
fn a_test_bus_keeps_to_its_own_config() {
    // A persistent emit writes a file, and the desk's theme is one of them.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bus");
    let _bus = start_bus(&path);
    wait_for_socket(&path);
    let socket = path.to_string_lossy().to_string();

    let mut a = BusClient::new();
    a.set_app_id("a");
    a.connect_to(&socket).unwrap();
    a.emit(Topic::Theme(Theme::default())).unwrap();
    // The bus writes it before it reads this client's next frame.
    subscribe(&mut a, &[]);

    assert!(dir.path().join("sola/theme/current.yaml").exists());
}
