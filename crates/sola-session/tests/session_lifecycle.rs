use std::path::PathBuf;
use std::time::{Duration, Instant};

use sola_bus::BusClient;
use sola_bus::topics::{LaunchAppPayload, Topic, TopicKind};

/// Locate a binary in the same target directory as the current test executable.
fn target_bin(name: &str) -> PathBuf {
    // The test binary lives at <target>/<profile>/deps/<test-name>-<hash>.
    // The built binaries live at <target>/<profile>/<name>.
    let mut dir = std::env::current_exe()
        .expect("current_exe")
        .parent()
        .expect("parent (deps)")
        .parent()
        .expect("parent (profile)")
        .to_path_buf();
    dir.push(name);
    dir
}

fn start_bus(path: PathBuf) -> std::process::Child {
    let exe = target_bin("sola-bus");
    std::process::Command::new(&exe)
        .env("SOLA_BUS_PATH", &path)
        .env("RUST_LOG", "warn")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap_or_else(|e| panic!("spawn bus ({exe:?}): {e}"))
}

fn start_session(bus_path: PathBuf) -> std::process::Child {
    let exe = target_bin("sola-session");
    std::process::Command::new(&exe)
        .env("SOLA_BUS_PATH", &bus_path)
        .env("RUST_LOG", "warn")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap_or_else(|e| panic!("spawn sola-session ({exe:?}): {e}"))
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
fn spawn_and_close_sleep() {
    let dir = tempfile::tempdir().unwrap();
    let bus_path = dir.path().join("bus");

    let mut bus = start_bus(bus_path.clone());
    wait_for_socket(&bus_path);
    let mut session = start_session(bus_path.clone());

    let socket = bus_path.to_string_lossy().to_string();
    let mut client = BusClient::new();
    client.set_app_id("test");
    client.connect_to(&socket).unwrap();
    client.subscribe(&[
        TopicKind::LaunchResult,
        TopicKind::UserAppExited,
        TopicKind::ClientConnected,
    ]).unwrap();

    // Wait for sola-session to be rostered.
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut session_ready = false;
    while Instant::now() < deadline {
        if let Some(m) = client.recv_timeout(Duration::from_millis(100)) {
            if let Some(Topic::ClientConnected(a)) = Topic::parse(&m) {
                if a == "sola-session" {
                    session_ready = true;
                    break;
                }
            }
        }
    }
    assert!(session_ready, "sola-session never identified on the bus");

    // Launch a long-running sleep.
    client.emit(Topic::LaunchApp(LaunchAppPayload {
        app_id: "sleep".into(),
        command: "/usr/bin/sleep 60".into(),
    })).unwrap();

    // Expect LaunchResult ok.
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut launched = false;
    while Instant::now() < deadline {
        if let Some(m) = client.recv_timeout(Duration::from_millis(100)) {
            if let Some(Topic::LaunchResult(p)) = Topic::parse(&m) {
                assert!(p.ok, "LaunchResult not ok: {:?}", p.error);
                launched = true;
                break;
            }
        }
    }
    assert!(launched, "never saw LaunchResult");

    // CloseApp → UserAppExited (SIGTERM fires at T+5s).
    client.emit(Topic::CloseApp("sleep".into())).unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut exited = false;
    while Instant::now() < deadline {
        if let Some(m) = client.recv_timeout(Duration::from_millis(200)) {
            if matches!(Topic::parse(&m), Some(Topic::UserAppExited(_))) {
                exited = true;
                break;
            }
        }
    }
    assert!(exited, "never saw UserAppExited");

    let _ = session.kill();
    let _ = bus.kill();
}
