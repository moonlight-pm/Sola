use std::collections::HashMap;
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::{fs, io};

use tracing::{error, info, warn};

use sola_bus::transport;

type ClientId = u64;

/// Per-client outbound queue depth. Large enough to absorb the back-to-back
/// bursts the shell emits (LaunchApp + RegisteredChords + Composition +
/// Focus) while still bounding memory if a client goes permanently silent.
const CLIENT_QUEUE_DEPTH: usize = 256;

struct BusState {
    /// Per-client outbound queue. Writers are owned by dedicated writer
    /// threads; broadcasters just try_send here and move on.
    clients: HashMap<ClientId, mpsc::SyncSender<sola_bus::Message>>,
    /// Latest sticky message per (topic, tag), replayed to newly connected clients.
    /// Multiple apps can have independent stickies on the same topic.
    sticky: HashMap<(String, String), sola_bus::Message>,
}

type SharedState = Arc<Mutex<BusState>>;

fn main() {
    let log_dir = "/opt/sola/log";
    let _ = std::fs::create_dir_all(log_dir);
    let file_appender = tracing_appender::rolling::never(log_dir, "sola.log");

    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "sola_bus=info".into());

    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;
    let stderr_layer = tracing_subscriber::fmt::layer().with_writer(std::io::stderr);
    let file_layer = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_writer(file_appender);
    tracing_subscriber::registry()
        .with(filter)
        .with(stderr_layer)
        .with(file_layer)
        .init();

    let socket_path = sola_bus::socket_path();

    // Remove stale socket if it exists
    let _ = fs::remove_file(&socket_path);

    let listener = UnixListener::bind(&socket_path).unwrap_or_else(|e| {
        panic!("failed to bind bus socket at {socket_path}: {e}");
    });

    info!(path = %socket_path, "bus listening");

    let state: SharedState = Arc::new(Mutex::new(BusState {
        clients: HashMap::new(),
        sticky: HashMap::new(),
    }));
    let mut next_id: ClientId = 0;

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let id = next_id;
                next_id += 1;

                let writer_stream = match stream.try_clone() {
                    Ok(s) => s,
                    Err(e) => {
                        error!(client = id, "failed to clone stream: {e}");
                        continue;
                    }
                };

                let (tx, rx) = mpsc::sync_channel::<sola_bus::Message>(CLIENT_QUEUE_DEPTH);

                // Writer thread: blocking writes to this client. Owned
                // exclusively here so no other thread can stall on this
                // fd. Reader and broadcasters only interact via `tx`.
                thread::spawn(move || {
                    writer_loop(id, writer_stream, rx);
                });

                let mut bus = state.lock().unwrap();

                // Replay sticky messages through the queue so they
                // preserve the connect-time ordering.
                for msg in bus.sticky.values() {
                    if tx.try_send(msg.clone()).is_err() {
                        warn!(client = id, "sticky replay queue full");
                    }
                }

                bus.clients.insert(id, tx);
                info!(client = id, "connected");
                drop(bus);

                let state = Arc::clone(&state);
                thread::spawn(move || {
                    handle_client(id, stream, &state);
                });
            }
            Err(e) => {
                error!("failed to accept connection: {e}");
            }
        }
    }
}

/// Drain the per-client queue and write each message to the socket with
/// blocking I/O. Exits when `rx` is closed (all senders dropped) or when
/// a write fails.
fn writer_loop(
    id: ClientId,
    mut stream: UnixStream,
    rx: mpsc::Receiver<sola_bus::Message>,
) {
    while let Ok(msg) = rx.recv() {
        if let Err(e) = transport::write_event(&mut stream, &msg) {
            match e.kind() {
                io::ErrorKind::BrokenPipe
                | io::ErrorKind::ConnectionReset
                | io::ErrorKind::UnexpectedEof => {
                    // Client already gone. Reader thread will clean up.
                }
                _ => warn!(client = id, "writer error: {e}"),
            }
            break;
        }
    }
}

fn handle_client(id: ClientId, mut reader: UnixStream, state: &SharedState) {
    loop {
        match transport::read_event(&mut reader) {
            Ok(Some(event)) => {
                log_bus_message(id, &event);

                let mut bus = state.lock().unwrap();

                if event.sticky {
                    let key = (event.topic.clone(), event.source.clone());
                    bus.sticky.insert(key, event.clone());
                }

                broadcast(id, &event, &mut bus);
            }
            Ok(None) => {
                info!(client = id, "disconnected");
                break;
            }
            Err(e) if e.kind() == io::ErrorKind::ConnectionReset => {
                info!(client = id, "disconnected (reset)");
                break;
            }
            Err(e) => {
                warn!(client = id, "read error: {e}");
                break;
            }
        }
    }

    // Dropping the sender causes the writer thread to exit on its next
    // recv().
    state.lock().unwrap().clients.remove(&id);
}

/// Append a line to /opt/sola/log/bus.log for every message that flows
/// through the bus. Separate file to avoid polluting the main sola.log.
fn log_bus_message(client: ClientId, event: &sola_bus::Message) {
    use std::io::Write;
    thread_local! {
        static LOG: std::cell::RefCell<Option<fs::File>> = std::cell::RefCell::new(
            fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open("/opt/sola/log/bus.log")
                .ok()
        );
    }
    LOG.with(|f| {
        if let Some(ref mut file) = *f.borrow_mut() {
            let elapsed = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default();
            let secs = elapsed.as_secs();
            let millis = elapsed.subsec_millis();
            let _ = writeln!(
                file,
                "{secs}.{millis:03} c={client} {}{} src={}",
                event.topic,
                if event.sticky { " [sticky]" } else { "" },
                event.source,
            );
        }
    });
}

fn broadcast(sender: ClientId, event: &sola_bus::Message, bus: &mut BusState) {
    for (&id, tx) in bus.clients.iter() {
        if id == sender {
            continue;
        }
        match tx.try_send(event.clone()) {
            Ok(()) => {}
            Err(mpsc::TrySendError::Full(_)) => {
                // Queue full — client is backed up. Drop to keep the bus
                // responsive; sticky topics will reconverge on the next
                // successful delivery or on reconnect.
                warn!(client = id, topic = %event.topic, "dropped (queue full)");
            }
            Err(mpsc::TrySendError::Disconnected(_)) => {
                // Writer thread exited; next handle_client cleanup will
                // remove this entry. Nothing more to do here.
            }
        }
    }
}
