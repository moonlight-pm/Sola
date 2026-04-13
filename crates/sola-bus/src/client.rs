use std::collections::VecDeque;
use std::os::unix::net::UnixStream;
use std::sync::mpsc;
use std::thread;
use std::io;

use tracing::{info, warn};

use crate::{Message, transport};

/// A connection to the Sola Bus.
///
/// Always constructable via `new()`. Messages sent before connection are
/// queued and flushed on successful `connect()`. Callers never need to
/// wrap this in `Option` — just call `emit()` at any time.
pub struct BusClient {
    writer: Option<UnixStream>,
    rx: Option<mpsc::Receiver<Message>>,
    queue: VecDeque<Message>,
}

impl BusClient {
    /// Create an unconnected bus client.
    ///
    /// Messages sent via `emit()` / `send()` are queued until `connect()` succeeds.
    pub fn new() -> Self {
        Self {
            writer: None,
            rx: None,
            queue: VecDeque::new(),
        }
    }

    /// Attempt to connect to the bus at the default socket path.
    ///
    /// On success, flushes any queued messages. Safe to call repeatedly —
    /// returns `Ok(())` immediately if already connected.
    pub fn connect(&mut self) -> io::Result<()> {
        if self.writer.is_some() {
            return Ok(());
        }
        let path = crate::socket_path();
        self.connect_to(&path)
    }

    /// Attempt to connect to the bus at a specific socket path.
    pub fn connect_to(&mut self, path: &str) -> io::Result<()> {
        if self.writer.is_some() {
            return Ok(());
        }

        let stream = UnixStream::connect(path)?;
        let reader = stream.try_clone()?;

        info!(path = %path, "connected to bus");

        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            read_loop(reader, tx);
        });

        self.writer = Some(stream);
        self.rx = Some(rx);

        self.flush_queue();

        Ok(())
    }

    /// Whether the client has an active bus connection.
    pub fn is_connected(&self) -> bool {
        self.writer.is_some()
    }

    /// Send a raw message to the bus, or queue it if not connected.
    pub fn send(&mut self, message: &Message) -> io::Result<()> {
        if let Some(ref mut writer) = self.writer {
            transport::write_event(writer, message)
        } else {
            self.queue.push_back(message.clone());
            Ok(())
        }
    }

    /// Emit a typed topic to the bus, or queue it if not connected.
    pub fn emit(&mut self, topic: crate::topics::Topic) -> io::Result<()> {
        let message = topic.to_message();
        self.send(&message)
    }

    /// Emit a typed topic as a sticky message.
    ///
    /// The bus retains the latest sticky message per topic and replays
    /// it to every newly connected client.
    pub fn emit_sticky(&mut self, topic: crate::topics::Topic) -> io::Result<()> {
        let mut message = topic.to_message();
        message.sticky = true;
        self.send(&message)
    }

    /// Try to receive the next message without blocking.
    /// Returns `None` if no message is available or not connected.
    pub fn try_recv(&self) -> Option<Message> {
        self.rx.as_ref()?.try_recv().ok()
    }

    /// Block until the next message is received.
    /// Returns `None` if the bus connection is closed or not connected.
    pub fn recv(&self) -> Option<Message> {
        self.rx.as_ref()?.recv().ok()
    }

    /// Flush queued messages after a successful connection.
    fn flush_queue(&mut self) {
        let writer = self.writer.as_mut().unwrap();
        while let Some(msg) = self.queue.pop_front() {
            if let Err(e) = transport::write_event(writer, &msg) {
                warn!("failed to flush queued message: {e}");
                break;
            }
        }
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
