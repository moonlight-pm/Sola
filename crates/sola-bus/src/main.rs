use std::collections::HashMap;
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::{fs, io};

use tracing::{error, info, warn};

use sola_bus::transport;

type ClientId = u64;

struct BusState {
    clients: HashMap<ClientId, UnixStream>,
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

                let writer = match stream.try_clone() {
                    Ok(s) => s,
                    Err(e) => {
                        error!(client = id, "failed to clone stream: {e}");
                        continue;
                    }
                };

                let mut bus = state.lock().unwrap();

                // Replay sticky messages BEFORE setting non-blocking.
                // try_clone + dup share the same file description, so
                // set_nonblocking on any clone affects all of them.
                replay_sticky(id, &mut bus, &writer);

                // Non-blocking writes prevent a slow client from
                // deadlocking the entire bus. Set AFTER replay so the
                // client receives all sticky state reliably.
                writer.set_nonblocking(true).ok();

                bus.clients.insert(id, writer);
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

/// Send all sticky messages to a newly connected client.
fn replay_sticky(id: ClientId, bus: &mut BusState, writer: &UnixStream) {
    let mut writer = match writer.try_clone() {
        Ok(w) => w,
        Err(e) => {
            warn!(client = id, "failed to clone writer for sticky replay: {e}");
            return;
        }
    };

    for ((topic, tag), msg) in &bus.sticky {
        if let Err(e) = transport::write_event(&mut writer, msg) {
            warn!(client = id, topic = %topic, tag = %tag, "failed to replay sticky message: {e}");
        } else {
            tracing::debug!(client = id, topic = %topic, tag = %tag, "replayed sticky message");
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
    let mut dead: Vec<ClientId> = Vec::new();

    for (&id, stream) in bus.clients.iter_mut() {
        if id == sender {
            continue;
        }
        match transport::write_event(stream, event) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                // Client is slow — drop the message rather than blocking
                // the entire bus. Sticky messages will be replayed on the
                // next successful write or reconnect.
                tracing::debug!(client = id, topic = %event.topic, "dropped (client busy)");
            }
            Err(e) => {
                warn!(client = id, "write error: {e}");
                dead.push(id);
            }
        }
    }

    for id in dead {
        bus.clients.remove(&id);
    }
}
