//! A subscriber gets everything it asked for, or its connection closes:
//! the bus never drops part of a replay or a live message unannounced.

mod common;

use std::os::unix::net::UnixStream;

use sola_bus::topics::{Topic, TopicKind, WindowFacts, WindowRect};
use sola_bus::{BusClient, subscribe, transport};

use common::{GIVE_UP, start_bus, subscribe as subscribe_through, wait_for_socket};

/// More records than fit a client's queue (256) and its socket's buffer,
/// so a client that has not read yet would lose some to a lossy bus.
const RECORDS: u32 = 4000;

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

/// A subscriber on a bare socket, so the test decides when it reads. A
/// read that waits out [`GIVE_UP`] fails rather than hangs.
fn raw_subscriber(socket: &str, kinds: &[TopicKind]) -> UnixStream {
    let mut stream = UnixStream::connect(socket).unwrap();
    stream.set_read_timeout(Some(GIVE_UP)).unwrap();
    for frame in subscribe::frames(kinds) {
        transport::write_event(&mut stream, &frame).unwrap();
    }
    stream
}

#[test]
fn a_replay_larger_than_the_queue_arrives_whole() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bus");
    let _bus = start_bus(&path);
    wait_for_socket(&path);
    let socket = path.to_string_lossy().to_string();

    let mut writer = BusClient::new();
    writer.set_app_id("writer");
    writer.connect_to(&socket).unwrap();
    for id in 0..RECORDS {
        writer.emit(geometry(id)).unwrap();
    }
    // Its own Replayed says the bus has taken every record before it.
    subscribe_through(&mut writer, &[TopicKind::Shutdown]);

    let mut reader = raw_subscriber(&socket, &[TopicKind::WindowFacts]);
    let mut records = 0;
    loop {
        let message = transport::read_event(&mut reader)
            .unwrap()
            .expect("the bus closed the connection before Replayed");
        match message.topic.as_str() {
            "WindowFacts" => records += 1,
            "Replayed" => break,
            other => panic!("unexpected {other}"),
        }
    }
    assert_eq!(records, RECORDS);
}

#[test]
fn a_subscriber_that_falls_behind_is_closed_not_shorted() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bus");
    let _bus = start_bus(&path);
    wait_for_socket(&path);
    let socket = path.to_string_lossy().to_string();

    // Subscribed (its empty replay read, so the bus has taken the
    // subscription), and not reading while the records go out.
    let mut stalled = raw_subscriber(&socket, &[TopicKind::WindowFacts]);
    let first = transport::read_event(&mut stalled).unwrap().unwrap();
    assert_eq!(first.topic, "Replayed");

    let mut writer = BusClient::new();
    writer.set_app_id("writer");
    writer.connect_to(&socket).unwrap();
    subscribe_through(&mut writer, &[TopicKind::Shutdown]);
    for id in 0..RECORDS {
        writer.emit(geometry(id)).unwrap();
    }
    subscribe_through(&mut writer, &[TopicKind::Shutdown, TopicKind::Theme]);

    // It reads what was written before the bus gave up on it, then the
    // end of the connection: never some of them and then silence.
    let mut records = 0;
    loop {
        match transport::read_event(&mut stalled) {
            Ok(Some(message)) if message.topic == "WindowFacts" => records += 1,
            Ok(Some(_)) => {}
            Ok(None) => break,
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {
                panic!("the bus left a stalled subscriber open after {records} of {RECORDS} records")
            }
            Err(_) => break,
        }
    }
    assert!(records < RECORDS, "a stalled subscriber took all {records}");
}
