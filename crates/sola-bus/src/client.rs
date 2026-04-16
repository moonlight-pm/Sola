use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::os::unix::io::{AsRawFd, RawFd};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;

use tracing::{info, warn};

use crate::{Message, transport};

/// A connection to the Sola Bus.
///
/// Always constructable via `new()`. Messages sent before connection are
/// queued and flushed on successful `connect()`. Callers never need to
/// wrap this in `Option` — just call `emit()` at any time.
pub struct BusClient {
    app_id: String,
    writer: Option<UnixStream>,
    rx: Option<mpsc::Receiver<Message>>,
    /// Read end of a notification pipe. Becomes readable when the reader
    /// thread delivers a message to `rx`. Event-loop callers watch this fd
    /// instead of polling `try_recv()`.
    notify_read: Option<UnixStream>,
    /// Set to false by the reader thread when it exits. Lets `is_connected()`
    /// distinguish a live connection from a half-open one (writer alive,
    /// reader dead) so callers can reconnect.
    reader_alive: Option<Arc<AtomicBool>>,
    queue: VecDeque<Message>,
}

impl BusClient {
    /// Create an unconnected bus client.
    ///
    /// Messages sent via `emit()` / `send()` are queued until `connect()` succeeds.
    pub fn new() -> Self {
        Self {
            app_id: String::new(),
            writer: None,
            rx: None,
            notify_read: None,
            reader_alive: None,
            queue: VecDeque::new(),
        }
    }

    /// Set the app identity used to tag sticky messages.
    pub fn set_app_id(&mut self, id: impl Into<String>) {
        self.app_id = id.into();
    }

    /// Attempt to connect to the bus at the default socket path.
    ///
    /// On success, flushes any queued messages. Safe to call repeatedly —
    /// returns `Ok(())` immediately if already connected.
    pub fn connect(&mut self) -> io::Result<()> {
        self.drop_if_reader_dead();
        if self.writer.is_some() {
            return Ok(());
        }
        let path = crate::socket_path();
        self.connect_to(&path)
    }

    /// Attempt to connect to the bus at a specific socket path.
    pub fn connect_to(&mut self, path: &str) -> io::Result<()> {
        // If the previous reader thread died, drop the half-open state so
        // we really re-open the socket instead of returning Ok.
        self.drop_if_reader_dead();
        if self.writer.is_some() {
            return Ok(());
        }

        let stream = UnixStream::connect(path)?;
        let reader = stream.try_clone()?;

        // Notification pipe: reader thread writes a byte after each message,
        // so event-loop callers can watch notify_read instead of polling.
        let (notify_read, notify_write) = UnixStream::pair()?;
        notify_read.set_nonblocking(true)?;

        info!(path = %path, "connected to bus");

        let (tx, rx) = mpsc::channel();
        let alive = Arc::new(AtomicBool::new(true));
        let alive_for_thread = alive.clone();
        thread::spawn(move || {
            read_loop(reader, tx, notify_write);
            alive_for_thread.store(false, Ordering::Release);
        });

        self.writer = Some(stream);
        self.rx = Some(rx);
        self.notify_read = Some(notify_read);
        self.reader_alive = Some(alive);

        self.flush_queue();

        Ok(())
    }

    /// Whether the client has an active bus connection.
    ///
    /// Returns false if the reader thread has exited, even if the writer
    /// socket is still open — in that case no messages are being delivered,
    /// so the caller should reconnect.
    pub fn is_connected(&self) -> bool {
        self.writer.is_some()
            && self
                .reader_alive
                .as_ref()
                .is_some_and(|a| a.load(Ordering::Acquire))
    }

    fn drop_if_reader_dead(&mut self) {
        let dead = self
            .reader_alive
            .as_ref()
            .is_some_and(|a| !a.load(Ordering::Acquire));
        if dead {
            self.writer = None;
            self.rx = None;
            self.notify_read = None;
            self.reader_alive = None;
        }
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
        let mut message = topic.to_message();
        message.sticky_tag = self.app_id.clone();
        self.send(&message)
    }

    /// Emit a typed topic as a sticky message.
    ///
    /// The bus retains the latest sticky per (topic, app_id) and replays
    /// all stickies to every newly connected client. Multiple apps can
    /// have independent stickies on the same topic.
    pub fn emit_sticky(&mut self, topic: crate::topics::Topic) -> io::Result<()> {
        let mut message = topic.to_message();
        message.sticky = true;
        message.sticky_tag = self.app_id.clone();
        self.send(&message)
    }

    /// Returns a raw fd that becomes readable when bus messages arrive.
    ///
    /// Event-loop callers (glib, calloop) watch this fd instead of polling
    /// `try_recv()`. After the fd signals readable, call `drain_notify()`
    /// then `try_recv()` in a loop.
    ///
    /// Returns `None` if not connected.
    pub fn notify_fd(&self) -> Option<RawFd> {
        self.notify_read.as_ref().map(|s| s.as_raw_fd())
    }

    /// Drain pending notification bytes after `notify_fd()` signals readable.
    ///
    /// Must be called before `try_recv()` to clear the notification pipe,
    /// otherwise the event loop will keep waking.
    pub fn drain_notify(&self) {
        let Some(stream) = self.notify_read.as_ref() else {
            return;
        };
        let mut buf = [0u8; 64];
        let mut r: &UnixStream = stream;
        loop {
            match r.read(&mut buf) {
                Ok(0) => break,
                Ok(_) => continue,
                Err(_) => break, // WouldBlock or error
            }
        }
    }

    /// Clone the notification fd as an owned stream, for calloop registration.
    ///
    /// The caller takes ownership of the clone; the original is retained.
    /// Returns `None` if not connected.
    pub fn try_clone_notify(&self) -> Option<io::Result<UnixStream>> {
        self.notify_read.as_ref().map(|s| s.try_clone())
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

    /// Block until a message arrives or the timeout expires.
    /// Returns `None` on timeout, disconnect, or if not connected.
    pub fn recv_timeout(&self, timeout: std::time::Duration) -> Option<Message> {
        self.rx.as_ref()?.recv_timeout(timeout).ok()
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

fn read_loop(mut reader: UnixStream, tx: mpsc::Sender<Message>, mut notify: UnixStream) {
    loop {
        match transport::read_event(&mut reader) {
            Ok(Some(msg)) => {
                if tx.send(msg).is_err() {
                    break; // receiver dropped
                }
                let _ = notify.write_all(&[1u8]);
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
