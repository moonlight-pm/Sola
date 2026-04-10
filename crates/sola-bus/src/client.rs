use std::os::unix::net::UnixStream;
use std::sync::mpsc;
use std::thread;
use std::io;

use tracing::{info, warn};

use crate::{Event, transport};

/// A connection to the Sola Bus.
///
/// Sends events by writing directly to the socket. Receives events via a
/// background reader thread that pushes them into a channel.
pub struct BusClient {
    writer: UnixStream,
    rx: mpsc::Receiver<Event>,
}

impl BusClient {
    /// Connect to the bus at the default socket path.
    pub fn connect() -> io::Result<Self> {
        let path = crate::socket_path();
        Self::connect_to(&path)
    }

    /// Connect to the bus at a specific socket path.
    pub fn connect_to(path: &str) -> io::Result<Self> {
        let stream = UnixStream::connect(path)?;
        let reader = stream.try_clone()?;

        info!(path = %path, "connected to bus");

        let (tx, rx) = mpsc::channel();

        thread::spawn(move || {
            read_loop(reader, tx);
        });

        Ok(Self { writer: stream, rx })
    }

    /// Send an event to the bus.
    pub fn send(&mut self, event: &Event) -> io::Result<()> {
        transport::write_event(&mut self.writer, event)
    }

    /// Try to receive the next event without blocking.
    /// Returns `None` if no event is available.
    pub fn try_recv(&self) -> Option<Event> {
        self.rx.try_recv().ok()
    }

    /// Block until the next event is received.
    /// Returns `None` if the bus connection is closed.
    pub fn recv(&self) -> Option<Event> {
        self.rx.recv().ok()
    }
}

fn read_loop(mut reader: UnixStream, tx: mpsc::Sender<Event>) {
    loop {
        match transport::read_event(&mut reader) {
            Ok(Some(event)) => {
                if tx.send(event).is_err() {
                    break; // receiver dropped
                }
            }
            Ok(None) => {
                info!("bus connection closed");
                break;
            }
            Err(e) if e.kind() == io::ErrorKind::ConnectionReset => {
                info!("bus connection reset");
                break;
            }
            Err(e) => {
                warn!("bus read error: {e}");
                break;
            }
        }
    }
}

