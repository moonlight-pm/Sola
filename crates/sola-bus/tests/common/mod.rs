//! What the bus tests share: a bus of their own, and waiting on what it
//! says rather than on the clock.

use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use sola_bus::topics::TopicKind;
use sola_bus::{BusClient, Message};

/// When a test stops waiting for something that should come. A passing
/// test never waits this out.
pub const GIVE_UP: Duration = Duration::from_secs(10);

/// A bus of a test's own. It ends with the test, however the test ends.
pub struct Bus(Child);

impl Drop for Bus {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Start a bus on `path`, with its config beside the socket: a test
/// neither reads nor writes the desk's.
pub fn start_bus(path: &Path) -> Bus {
    let child = Command::new(env!("CARGO_BIN_EXE_sola-bus"))
        .env("SOLA_BUS_PATH", path)
        .env("XDG_CONFIG_HOME", path.parent().expect("socket has a folder"))
        .env("RUST_LOG", "sola_bus=warn")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn bus");
    Bus(child)
}

/// Wait for the bus to take connections. Nothing can say so yet, so this
/// one does poll; it connects because the socket exists before it listens.
pub fn wait_for_socket(path: &Path) {
    let deadline = Instant::now() + GIVE_UP;
    while Instant::now() < deadline {
        if UnixStream::connect(path).is_ok() {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("bus socket never appeared at {}", path.display());
}

/// Subscribe and return what the bus replays, through its `Replayed`.
/// That answers on the client's own connection, so it also says the bus
/// has taken everything the client sent before it.
pub fn subscribe(client: &mut BusClient, kinds: &[TopicKind]) -> Vec<Message> {
    client.subscribe(kinds).unwrap();
    receive_through(client, "Replayed")
}

/// What `client` receives, up to and including the first `topic`.
pub fn receive_through(client: &BusClient, topic: &str) -> Vec<Message> {
    let deadline = Instant::now() + GIVE_UP;
    let mut got = Vec::new();
    while Instant::now() < deadline {
        if let Some(m) = client.recv_timeout(Duration::from_millis(50)) {
            let done = m.topic == topic;
            got.push(m);
            if done {
                return got;
            }
        }
    }
    let topics: Vec<&str> = got.iter().map(|m| m.topic.as_str()).collect();
    panic!("{topic} never came; got {topics:?}");
}
