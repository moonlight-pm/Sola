use std::collections::{HashMap, HashSet};
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
    /// client_id → app_id for clients that have sent `$identify`.
    roster: HashMap<ClientId, String>,
    /// client_id → topic kinds the client has subscribed to via `$subscribe`.
    subscriptions: HashMap<ClientId, HashSet<sola_bus::topics::TopicKind>>,
}

type SharedState = Arc<Mutex<BusState>>;

fn main() {
    sola_core::log::init("sola-bus");

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
        roster: HashMap::new(),
        subscriptions: HashMap::new(),
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

                match event.topic.as_str() {
                    sola_bus::CONTROL_IDENTIFY => {
                        if let Ok(app_id) =
                            sola_bus::topic::decode_payload::<String>(&event)
                        {
                            handle_identify(id, app_id, state);
                        }
                    }
                    sola_bus::CONTROL_SUBSCRIBE => {
                        if let Ok(kinds) = sola_bus::topic::decode_payload::<
                            Vec<sola_bus::topics::TopicKind>,
                        >(&event)
                        {
                            handle_subscribe(id, kinds, state);
                        }
                    }
                    _ => {
                        let mut bus = state.lock().unwrap();
                        if event.sticky {
                            let key = (event.topic.clone(), event.source.clone());
                            bus.sticky.insert(key, event.clone());
                        }
                        broadcast(id, &event, &mut bus);
                    }
                }
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

    let mut bus = state.lock().unwrap();
    if let Some(app_id) = bus.roster.remove(&id) {
        let evt = sola_bus::topics::Topic::ClientDisconnected(app_id).to_message();
        broadcast(id, &evt, &mut bus);
    }
    bus.clients.remove(&id);
    bus.subscriptions.remove(&id);
}

/// Emit a tracing event for every message that flows through the bus.
/// Lands in the shared sola.log; enable with
/// `RUST_LOG=sola_bus::traffic=trace` to see the audit stream.
fn log_bus_message(client: ClientId, event: &sola_bus::Message) {
    tracing::trace!(
        target: "sola_bus::traffic",
        client,
        topic = %event.topic,
        sticky = event.sticky,
        source = %event.source,
    );
}

fn handle_identify(id: ClientId, app_id: String, state: &SharedState) {
    let mut bus = state.lock().unwrap();
    let prev = bus.roster.insert(id, app_id.clone());
    if prev.as_ref() == Some(&app_id) {
        return; // already identified with this id — no broadcast
    }
    info!(client = id, %app_id, "identified");
    let evt = sola_bus::topics::Topic::ClientConnected(app_id).to_message();
    broadcast(id, &evt, &mut bus);
}

fn broadcast(sender: ClientId, event: &sola_bus::Message, bus: &mut BusState) {
    let kind = match sola_bus::topics::Topic::parse(event) {
        Some(t) => t.kind(),
        None => {
            warn!(topic = %event.topic, "broadcast dropping unknown topic");
            return;
        }
    };
    for (&id, tx) in bus.clients.iter() {
        if id == sender {
            continue;
        }
        let wants = bus
            .subscriptions
            .get(&id)
            .is_some_and(|s| s.contains(&kind));
        if !wants {
            continue;
        }
        match tx.try_send(event.clone()) {
            Ok(()) => {}
            Err(mpsc::TrySendError::Full(_)) => {
                warn!(client = id, topic = %event.topic, "dropped (queue full)");
            }
            Err(mpsc::TrySendError::Disconnected(_)) => {}
        }
    }
}

fn handle_subscribe(
    id: ClientId,
    kinds: Vec<sola_bus::topics::TopicKind>,
    state: &SharedState,
) {
    let new_kinds: HashSet<_> = kinds.into_iter().collect();
    let mut bus = state.lock().unwrap();
    let prev = bus
        .subscriptions
        .insert(id, new_kinds.clone())
        .unwrap_or_default();
    let added: HashSet<_> = new_kinds.difference(&prev).copied().collect();
    info!(
        client = id,
        count = new_kinds.len(),
        added = added.len(),
        "subscribed"
    );

    let Some(tx) = bus.clients.get(&id).cloned() else {
        return;
    };

    // Replay stickies whose kind is newly subscribed.
    for msg in bus.sticky.values() {
        if let Some(kind) = sola_bus::topics::Topic::parse(msg).map(|t| t.kind()) {
            if added.contains(&kind) {
                let _ = tx.try_send(msg.clone());
            }
        }
    }

    // Roster replay for ClientConnected.
    if added.contains(&sola_bus::topics::TopicKind::ClientConnected) {
        for app_id in bus.roster.values() {
            let evt = sola_bus::topics::Topic::ClientConnected(app_id.clone()).to_message();
            let _ = tx.try_send(evt);
        }
    }
}
