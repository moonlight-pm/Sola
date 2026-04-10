use std::os::unix::net::UnixStream;
use std::sync::mpsc;
use std::thread;
use std::io;

use tracing::{info, warn};

use crate::{Message, transport};

/// A connection to the Sola Bus.
///
/// Sends messages to the bus and receives them via a background reader thread.
pub struct BusClient {
    writer: UnixStream,
    rx: mpsc::Receiver<Message>,
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

    /// Send a raw message to the bus.
    pub fn send(&mut self, message: &Message) -> io::Result<()> {
        transport::write_event(&mut self.writer, message)
    }

    /// Emit a typed topic to the bus.
    pub fn emit(&mut self, topic: crate::topics::Topic) -> io::Result<()> {
        let message = topic.to_message();
        self.send(&message)
    }

    /// Try to receive the next message without blocking.
    /// Returns `None` if no message is available.
    pub fn try_recv(&self) -> Option<Message> {
        self.rx.try_recv().ok()
    }

    /// Block until the next message is received.
    /// Returns `None` if the bus connection is closed.
    pub fn recv(&self) -> Option<Message> {
        self.rx.recv().ok()
    }
}

fn read_loop(mut reader: UnixStream, tx: mpsc::Sender<Message>) {
    loop {
        match transport::read_event(&mut reader) {
            Ok(Some(msg)) => {
                if tx.send(msg).is_err() {
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
