//! A record goes with the process that wrote it, unless a supervisor
//! restarts that process: then it waits for the process to come back and
//! settle. Persistent records are configuration and stay.
//!
//! The writers are processes of their own: this test binary run again as
//! [`child_writer`], which ends when the test closes its stdin.

mod common;

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};

use sola_bus::topics::{Topic, TopicKind, WindowFacts, WindowRect, ZoneLayout};
use sola_bus::{BusClient, Message};

use common::{receive_through, start_bus, subscribe, wait_for_socket};

fn geometry(window_id: u32) -> Topic {
    Topic::WindowFacts(WindowFacts {
        window_id,
        rect: Some(WindowRect {
            x: 0,
            y: 0,
            width: 800,
            height: 600,
        }),
        tab: None,
        parent: None,
    })
}

/// What a writer process does, set by the test that starts it.
#[derive(Default)]
struct Writer<'a> {
    app: &'a str,
    window: u32,
    supervised: bool,
    settle: bool,
    persistent: bool,
}

/// A writer process, until [`end`].
struct Running(Child);

/// Start a writer and wait until the bus has taken what it wrote.
fn start_writer(socket: &str, w: Writer) -> Running {
    let mut cmd = Command::new(std::env::current_exe().unwrap());
    cmd.args(["child_writer", "--exact", "--ignored", "--nocapture", "--test-threads=1"])
        .env("SOLA_BUS_TEST_SOCKET", socket)
        .env("SOLA_BUS_TEST_APP", w.app)
        .env("SOLA_BUS_TEST_WINDOW", w.window.to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    for (flag, on) in [
        ("SOLA_BUS_TEST_SUPERVISED", w.supervised),
        ("SOLA_BUS_TEST_SETTLE", w.settle),
        ("SOLA_BUS_TEST_PERSISTENT", w.persistent),
    ] {
        if on {
            cmd.env(flag, "1");
        }
    }
    let mut child = cmd.spawn().unwrap();
    let out = BufReader::new(child.stdout.take().unwrap());
    for line in out.lines() {
        // The harness has printed `test child_writer ... ` on this line.
        if line.unwrap().ends_with("ready") {
            return Running(child);
        }
    }
    panic!("the writer ended before it was ready");
}

impl Running {
    /// End the process and wait until it has.
    fn end(mut self) {
        drop(self.0.stdin.take());
        self.0.wait().unwrap();
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
#[ignore = "a writer process for the tests in this file"]
fn child_writer() {
    let Ok(socket) = std::env::var("SOLA_BUS_TEST_SOCKET") else {
        return;
    };
    let flag = |name: &str| std::env::var(name).is_ok();
    let window: u32 = std::env::var("SOLA_BUS_TEST_WINDOW").unwrap().parse().unwrap();

    let mut c = BusClient::new();
    c.set_app_id(std::env::var("SOLA_BUS_TEST_APP").unwrap());
    if flag("SOLA_BUS_TEST_SUPERVISED") {
        c.set_supervised();
    }
    c.connect_to(&socket).unwrap();
    c.emit(geometry(window)).unwrap();
    if flag("SOLA_BUS_TEST_PERSISTENT") {
        c.emit(Topic::ZoneLayout(ZoneLayout::default())).unwrap();
    }
    if flag("SOLA_BUS_TEST_SETTLE") {
        c.settled().unwrap();
    }
    // Its own Replayed: the bus has taken all of the above.
    subscribe(&mut c, &[TopicKind::Shutdown]);
    println!("ready");
    std::io::stdout().flush().unwrap();
    let _ = std::io::stdin().read_line(&mut String::new());
    std::process::exit(0);
}

fn bus() -> (tempfile::TempDir, common::Bus, String) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bus");
    let bus = start_bus(&path);
    wait_for_socket(&path);
    let socket = path.to_string_lossy().to_string();
    (dir, bus, socket)
}

fn watcher(socket: &str, kinds: &[TopicKind]) -> BusClient {
    let mut c = BusClient::new();
    c.set_app_id("watcher");
    c.connect_to(socket).unwrap();
    subscribe(&mut c, kinds);
    c
}

fn is_geometry(m: &Message, window: u32, sticky: bool) -> bool {
    m.topic == "WindowFacts" && m.sticky == sticky && m.keys == [window.to_string()]
}

/// What `client` receives through the first message that `done` picks.
fn receive_until(client: &BusClient, done: impl Fn(&Message) -> bool) -> Vec<Message> {
    let mut got = Vec::new();
    loop {
        let batch = receive_through(client, "WindowFacts");
        let finished = batch.last().is_some_and(&done);
        got.extend(batch);
        if finished {
            return got;
        }
    }
}

#[test]
fn an_ended_process_takes_its_records_with_it() {
    let (_dir, _bus, socket) = bus();
    let watcher = watcher(&socket, &[TopicKind::WindowFacts]);

    let writer = start_writer(
        &socket,
        Writer {
            app: "app",
            window: 7,
            ..Default::default()
        },
    );
    receive_until(&watcher, |m| is_geometry(m, 7, true));
    writer.end();
    receive_until(&watcher, |m| is_geometry(m, 7, false));
}

#[test]
fn a_supervised_process_keeps_its_records_until_it_settles_again() {
    let (_dir, _bus, socket) = bus();
    let watcher = watcher(&socket, &[TopicKind::WindowFacts]);

    let first = start_writer(
        &socket,
        Writer {
            app: "service",
            window: 1,
            supervised: true,
            ..Default::default()
        },
    );
    receive_until(&watcher, |m| is_geometry(m, 1, true));
    first.end();

    // Back, it writes window 2 and settles: window 1, which the ended
    // process still owns, goes then, after window 2.
    let _second = start_writer(
        &socket,
        Writer {
            app: "service",
            window: 2,
            supervised: true,
            settle: true,
            ..Default::default()
        },
    );
    let got = receive_until(&watcher, |m| is_geometry(m, 1, false));
    let two = got.iter().position(|m| is_geometry(m, 2, true));
    let gone = got.iter().position(|m| is_geometry(m, 1, false));
    assert!(
        two.is_some_and(|two| Some(two) < gone),
        "window 1 went before its writer came back: {got:?}"
    );
    assert!(!got.iter().any(|m| is_geometry(m, 2, false)));
}

#[test]
fn a_persistent_record_outlives_its_writer() {
    let (_dir, _bus, socket) = bus();
    let mut watcher = watcher(&socket, &[TopicKind::WindowFacts, TopicKind::ZoneLayout]);

    let writer = start_writer(
        &socket,
        Writer {
            app: "settings",
            window: 3,
            persistent: true,
            ..Default::default()
        },
    );
    receive_until(&watcher, |m| is_geometry(m, 3, true));
    writer.end();
    let mut got = receive_until(&watcher, |m| is_geometry(m, 3, false));
    // Everything the bus sent with that retraction has come by this
    // Replayed, the bus answering on the watcher's own connection.
    got.extend(subscribe(&mut watcher, &[TopicKind::WindowFacts, TopicKind::ZoneLayout, TopicKind::Shutdown]));
    assert!(
        !got.iter().any(|m| m.topic == "ZoneLayout" && !m.sticky),
        "the zone layout was retracted with its writer: {got:?}"
    );
}
