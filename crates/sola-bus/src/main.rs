mod process;

use std::collections::{HashMap, HashSet};
use std::net::Shutdown;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::{fs, io};

use tracing::{error, info, warn};

use sola_bus::state;
use sola_bus::topics::{Topic, TopicKind};
use sola_bus::transport;

type ClientId = u64;

/// The sender of a message the bus makes itself (a retraction for an ended
/// process): no client's id.
const BUS_SENDER: ClientId = ClientId::MAX;

/// Per-client outbound queue depth, in deliveries: a replay is one. Large
/// enough to absorb the back-to-back bursts the shell emits (LaunchApp +
/// RegisteredChords + Composition + Focus); a client that falls this far
/// behind has its connection closed ([`deliver`]).
const CLIENT_QUEUE_DEPTH: usize = 256;

/// What a client's writer thread sends: one message, or a subscribe's
/// whole replay, which takes one place in the queue so it is never cut
/// short.
enum Delivery {
    One(sola_bus::Message),
    Replay(Vec<sola_bus::Message>),
}

/// A connected client, as the broadcasters see it.
struct Client {
    /// Its outbound queue. The writer thread owns the socket's write side;
    /// broadcasters only `try_send` here.
    tx: mpsc::SyncSender<Delivery>,
    /// The socket, to close it when the client falls behind.
    stream: UnixStream,
    /// Its process, when the kernel says.
    pid: Option<i32>,
}

/// A stored sticky message and its kind, known when it was stored.
struct Record {
    message: sola_bus::Message,
    kind: TopicKind,
    /// The process that wrote it last, for a sticky (not persistent) kind:
    /// the record goes when that process ends for good.
    owner: Option<Owner>,
}

struct Owner {
    app_id: String,
    pid: i32,
}

struct BusState {
    clients: HashMap<ClientId, Client>,
    /// Latest sticky message per (topic, keys), replayed to newly connected clients.
    /// `keys` is empty for unkeyed topics — at most one such sticky exists per topic kind.
    /// Keyed topics may have many concurrent stickies, addressed by their key values.
    sticky: HashMap<(String, Vec<String>), Record>,
    /// client_id → app_id for clients that have sent `$identify`.
    roster: HashMap<ClientId, String>,
    /// client_id → topic kinds the client has subscribed to.
    subscriptions: HashMap<ClientId, HashSet<sola_bus::topics::TopicKind>>,
    /// Path to the persistent-sticky file on disk. Set once at startup.
    state_path: PathBuf,
    /// Processes with a connection that have not ended (each watched once).
    alive: HashSet<i32>,
    /// Processes that said a supervisor restarts them (`$supervised`).
    supervised: HashSet<i32>,
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

    let state_path = state::state_path();
    let cfg_dir = sola_core::config::sola_config_dir();
    let mut restored = state::load(&state_path);
    let restored_shared = restored.len();
    restored.extend(state::load_namespaced_all(&cfg_dir));
    info!(
        path = %state_path.display(),
        shared = restored_shared,
        namespaced = restored.len() - restored_shared,
        total = restored.len(),
        "persistent stickies loaded",
    );

    let mut sticky = HashMap::new();
    for message in restored {
        if let Some(kind) = Topic::parse(&message).map(|t| t.kind()) {
            let key = (message.topic.clone(), message.keys.clone());
            sticky.insert(
                key,
                Record {
                    message,
                    kind,
                    owner: None,
                },
            );
        }
    }

    let state: SharedState = Arc::new(Mutex::new(BusState {
        clients: HashMap::new(),
        sticky,
        roster: HashMap::new(),
        subscriptions: HashMap::new(),
        state_path,
        alive: HashSet::new(),
        supervised: HashSet::new(),
    }));
    let mut next_id: ClientId = 0;

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let id = next_id;
                next_id += 1;

                let (writer_stream, closer) = match (stream.try_clone(), stream.try_clone()) {
                    (Ok(w), Ok(c)) => (w, c),
                    (Err(e), _) | (_, Err(e)) => {
                        error!(client = id, "failed to clone stream: {e}");
                        continue;
                    }
                };

                let (tx, rx) = mpsc::sync_channel::<Delivery>(CLIENT_QUEUE_DEPTH);

                // Writer thread: blocking writes to this client. Owned
                // exclusively here so no other thread can stall on this
                // fd. Reader and broadcasters only interact via `tx`.
                thread::spawn(move || {
                    writer_loop(id, writer_stream, rx);
                });

                let pid = process::peer_pid(&stream);
                let mut bus = state.lock().unwrap();
                bus.clients.insert(
                    id,
                    Client {
                        tx,
                        stream: closer,
                        pid,
                    },
                );
                info!(client = id, pid, "connected");
                let first_of_its_process = pid.is_some_and(|pid| bus.alive.insert(pid));
                drop(bus);
                if let (Some(pid), true) = (pid, first_of_its_process) {
                    let state = Arc::clone(&state);
                    thread::spawn(move || watch_process(pid, &state));
                }

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
fn writer_loop(id: ClientId, mut stream: UnixStream, rx: mpsc::Receiver<Delivery>) {
    while let Ok(delivery) = rx.recv() {
        let result = match delivery {
            Delivery::One(msg) => transport::write_event(&mut stream, &msg),
            Delivery::Replay(msgs) => msgs
                .iter()
                .try_for_each(|msg| transport::write_event(&mut stream, msg)),
        };
        if let Err(e) = result {
            match e.kind() {
                io::ErrorKind::BrokenPipe
                | io::ErrorKind::ConnectionReset
                | io::ErrorKind::UnexpectedEof
                | io::ErrorKind::NotConnected => {
                    // Client gone, or closed by `deliver`. Reader thread will clean up.
                }
                _ => warn!(client = id, "writer error: {e}"),
            }
            break;
        }
    }
}

/// Queue `delivery` for client `id`. A client whose queue is full has
/// fallen too far behind to be told everything: rather than drop what it
/// would miss, close its connection. It connects again, subscribes again
/// and gets a fresh replay (every client reconnects after a bus restart
/// the same way).
fn deliver(id: ClientId, delivery: Delivery, bus: &mut BusState) {
    let Some(client) = bus.clients.get(&id) else {
        return;
    };
    match client.tx.try_send(delivery) {
        Ok(()) | Err(mpsc::TrySendError::Disconnected(_)) => {}
        Err(mpsc::TrySendError::Full(_)) => {
            let app_id = bus.roster.get(&id).map(String::as_str).unwrap_or("?");
            warn!(client = id, app_id, "client fell behind; closing its connection so it subscribes again");
            let _ = client.stream.shutdown(Shutdown::Both);
            bus.clients.remove(&id);
            bus.subscriptions.remove(&id);
        }
    }
}

fn handle_client(id: ClientId, mut reader: UnixStream, state: &SharedState) {
    // Once a connection names its kinds, its numbered frames (sent for a
    // bus from before names) are ignored.
    let mut named = false;
    loop {
        match transport::read_event(&mut reader) {
            Ok(Some(event)) => {
                log_bus_message(id, &event);

                match event.topic.as_str() {
                    sola_bus::CONTROL_IDENTIFY => {
                        if let Ok(app_id) = sola_bus::topic::decode_payload::<String>(&event) {
                            handle_identify(id, app_id, state);
                        }
                    }
                    sola_bus::CONTROL_SUBSCRIBE_NAMES => match sola_bus::subscribe::decode_names(&event) {
                        Ok(kinds) => {
                            named = true;
                            handle_subscribe(id, kinds, state);
                        }
                        Err(e) => tracing::warn!(
                            client = id,
                            %e,
                            "subscribe decode failed — client stays on previous set"
                        ),
                    },
                    sola_bus::CONTROL_SUPERVISED => {
                        let mut bus = state.lock().unwrap();
                        if let Some(pid) = bus.clients.get(&id).and_then(|c| c.pid) {
                            bus.supervised.insert(pid);
                        }
                    }
                    sola_bus::CONTROL_SETTLED => handle_settled(id, state),
                    sola_bus::CONTROL_SUBSCRIBE if named => {}
                    sola_bus::CONTROL_SUBSCRIBE => match sola_bus::subscribe::decode_numbered(&event) {
                        Ok(kinds) => handle_subscribe(id, kinds, state),
                        Err(e) => tracing::warn!(
                            client = id,
                            %e,
                            "subscribe decode failed — client stays on previous set"
                        ),
                    },
                    _ => {
                        let Some(kind) = Topic::parse(&event).map(|t| t.kind()) else {
                            warn!(client = id, topic = %event.topic, "dropping unknown topic");
                            continue;
                        };
                        let mut bus = state.lock().unwrap();
                        let key = (event.topic.clone(), event.keys.clone());
                        if event.sticky {
                            let owner = owner_of(id, &event, kind, &bus);
                            bus.sticky.insert(
                                key,
                                Record {
                                    message: event.clone(),
                                    kind,
                                    owner,
                                },
                            );
                        } else if kind.behavior().is_sticky() {
                            // Retract: client signals removal by sending sticky=false on a
                            // sticky/persistent topic kind. Evict from in-memory map and,
                            // if persistent, from disk.
                            bus.sticky.remove(&key);
                            if kind.behavior().is_persistent() {
                                retract_persistent(&bus.state_path, kind, &event);
                            }
                        }
                        broadcast(id, &event, kind, &mut bus);
                        if event.sticky {
                            persist_if_needed(&event, &mut bus);
                        }
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
        let evt = Topic::ClientDisconnected(app_id).to_message();
        broadcast(id, &evt, TopicKind::ClientDisconnected, &mut bus);
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
        keys = ?event.keys,
    );
}

fn handle_identify(id: ClientId, app_id: String, state: &SharedState) {
    let mut bus = state.lock().unwrap();
    let prev = bus.roster.insert(id, app_id.clone());
    if prev.as_ref() == Some(&app_id) {
        return; // already identified with this id — no broadcast
    }
    info!(client = id, %app_id, "identified");
    let evt = Topic::ClientConnected(app_id).to_message();
    broadcast(id, &evt, TopicKind::ClientConnected, &mut bus);
}

fn broadcast(sender: ClientId, event: &sola_bus::Message, kind: TopicKind, bus: &mut BusState) {
    // Sticky/persistent topics carry *shared state*: the sender is writing a
    // canonical value that every subscriber — themselves included — should
    // observe through the same delivery path. Skipping self for sticky topics
    // means framework-level subscribers in the sender's own process (e.g. the
    // kit's bus pump that lowers Topic::Theme to CSS) miss the update, and the
    // sender's UI silently diverges from its own write.
    //
    // Ephemeral topics still skip self — input events, deliveries, and other
    // "fire and forget" messages don't want echoes.
    let echo_to_sender = kind.behavior().is_sticky();
    let to: Vec<ClientId> = bus
        .subscriptions
        .iter()
        .filter(|(id, kinds)| (**id != sender || echo_to_sender) && kinds.contains(&kind))
        .map(|(id, _)| *id)
        .collect();
    for id in to {
        deliver(id, Delivery::One(event.clone()), bus);
    }
}

/// If `event` is a persistent topic, write its current value to disk.
/// Routes by `Topic::path_for()` — namespaced topics land in their own
/// dedicated file, others go to the shared state.yaml.
///
/// Called with the bus lock held so the sticky map and disk stay
/// consistent; disk writes are rare (persistent topics carry config,
/// not traffic), so holding the lock is acceptable for now.
fn persist_if_needed(event: &sola_bus::Message, bus: &mut BusState) {
    let Some(topic) = Topic::parse(event) else {
        return;
    };
    let kind = topic.kind();
    if !kind.behavior().is_persistent() {
        return;
    }
    if kind.namespace().is_some() {
        let path = match topic.path_for_safe() {
            Ok(p) => p,
            Err(e) => {
                warn!(topic = kind.as_str(), %e, "namespaced path resolution failed");
                return;
            }
        };
        if let Err(e) = state::write_namespaced(&path, &topic) {
            warn!(topic = kind.as_str(), %e, "namespaced persistent write failed");
        }
    } else if let Err(e) = state::write_section(&bus.state_path, &topic) {
        warn!(topic = kind.as_str(), %e, "persistent write failed");
    }
}

/// Retract a persistent topic from disk. For namespaced topics, unlinks
/// the per-topic file (interpolating any `:placeholder` from the event's
/// keys). For unnamespaced topics, drops the section from state.yaml.
fn retract_persistent(state_path: &Path, kind: TopicKind, event: &sola_bus::Message) {
    if let Some(template) = kind.namespace() {
        let cfg = sola_core::config::sola_config_dir();
        let resolved =
            match sola_bus::topic::interpolate_namespace(template, kind.key_names(), &event.keys) {
                Ok(s) => s,
                Err(e) => {
                    warn!(topic = kind.as_str(), %e, "namespaced retract path failed");
                    return;
                }
            };
        let path = cfg.join(format!("{resolved}.yaml"));
        if let Err(e) = state::retract_namespaced(&path) {
            warn!(topic = kind.as_str(), path = %path.display(), %e, "namespaced retract failed");
        }
    } else if let Err(e) = state::retract_section(state_path, event) {
        warn!(topic = %event.topic, %e, "persistent retract failed");
    }
}

fn handle_subscribe(id: ClientId, kinds: Vec<TopicKind>, state: &SharedState) {
    let new_kinds: HashSet<_> = kinds.into_iter().collect();
    let mut bus = state.lock().unwrap();
    if !bus.clients.contains_key(&id) {
        return; // closed by `deliver`; it will connect again
    }
    let prev = bus
        .subscriptions
        .insert(id, new_kinds.clone())
        .unwrap_or_default();
    let added: HashSet<_> = new_kinds.difference(&prev).copied().collect();

    // The stickies whose kind is newly subscribed, the roster for
    // ClientConnected, then `Replayed`: one delivery, so the client gets
    // all of it or, if it is too far behind to take it, a closed
    // connection and a fresh subscribe.
    let mut replay: Vec<sola_bus::Message> = bus
        .sticky
        .values()
        .filter(|r| added.contains(&r.kind))
        .map(|r| r.message.clone())
        .collect();
    if added.contains(&TopicKind::ClientConnected) {
        replay.extend(
            bus.roster
                .values()
                .map(|app_id| Topic::ClientConnected(app_id.clone()).to_message()),
        );
    }
    info!(
        client = id,
        count = new_kinds.len(),
        added = added.len(),
        replayed = replay.len(),
        "subscribed"
    );
    replay.push(Topic::Replayed.to_message());
    deliver(id, Delivery::Replay(replay), &mut bus);
}

/// Who owns a sticky record client `id` just wrote: its process, for a
/// sticky kind. A persistent record is configuration and outlives its
/// writer, so it has none.
fn owner_of(id: ClientId, event: &sola_bus::Message, kind: TopicKind, bus: &BusState) -> Option<Owner> {
    if kind.behavior().is_persistent() {
        return None;
    }
    let pid = bus.clients.get(&id)?.pid?;
    let app_id = bus.roster.get(&id).cloned().unwrap_or_else(|| event.source.clone());
    Some(Owner { app_id, pid })
}

/// Wait for process `pid` to end, then retract its records, unless a
/// supervisor restarts it: then they wait for it to come back and settle.
fn watch_process(pid: i32, state: &SharedState) {
    if let Err(e) = process::wait_for_exit(pid) {
        // Its records stay until it retracts them, as before owners.
        warn!(pid, %e, "cannot watch a client's process; its records stay");
        return;
    }
    let mut bus = state.lock().unwrap();
    bus.alive.remove(&pid);
    if bus.supervised.remove(&pid) {
        let kept = bus.sticky.values().filter(|r| owned_by(r, |o| o.pid == pid)).count();
        info!(pid, kept, "a supervised process ended; its records wait for it");
        return;
    }
    let ended: Vec<_> = bus
        .sticky
        .iter()
        .filter(|(_, r)| owned_by(r, |o| o.pid == pid))
        .map(|(key, _)| key.clone())
        .collect();
    if !ended.is_empty() {
        info!(pid, retracted = ended.len(), "a process ended; retracting its records");
    }
    retract_records(ended, &mut bus);
}

/// `$settled` from client `id`: its process has sent its current state, so
/// retract what an ended process of the same app still owns.
fn handle_settled(id: ClientId, state: &SharedState) {
    let mut bus = state.lock().unwrap();
    let Some(app_id) = bus.roster.get(&id).cloned() else {
        return;
    };
    let alive = bus.alive.clone();
    let stale: Vec<_> = bus
        .sticky
        .iter()
        .filter(|(_, r)| owned_by(r, |o| o.app_id == app_id && !alive.contains(&o.pid)))
        .map(|(key, _)| key.clone())
        .collect();
    info!(client = id, %app_id, retracted = stale.len(), "settled");
    retract_records(stale, &mut bus);
}

fn owned_by(record: &Record, test: impl Fn(&Owner) -> bool) -> bool {
    record.owner.as_ref().is_some_and(test)
}

/// Remove records the bus retracts on an owner's behalf, and tell their
/// subscribers as the owner's own retraction would.
fn retract_records(keys: Vec<(String, Vec<String>)>, bus: &mut BusState) {
    for key in keys {
        let Some(record) = bus.sticky.remove(&key) else {
            continue;
        };
        let mut retraction = record.message;
        retraction.id = uuid::Uuid::now_v7();
        retraction.sticky = false;
        broadcast(BUS_SENDER, &retraction, record.kind, bus);
    }
}
