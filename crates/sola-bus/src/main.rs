use std::collections::HashMap;
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::{env, fs, io};

use tracing::{error, info, warn};

use sola_bus::transport;

type ClientId = u64;
type Clients = Arc<Mutex<HashMap<ClientId, UnixStream>>>;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "sola_bus=info".parse().unwrap()),
        )
        .init();

    let socket_path = bus_socket_path();

    // Remove stale socket if it exists
    let _ = fs::remove_file(&socket_path);

    let listener = UnixListener::bind(&socket_path).unwrap_or_else(|e| {
        panic!("failed to bind bus socket at {socket_path}: {e}");
    });

    info!(path = %socket_path, "bus listening");

    let clients: Clients = Arc::new(Mutex::new(HashMap::new()));
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

                clients.lock().unwrap().insert(id, writer);
                info!(client = id, "connected");

                let clients = Arc::clone(&clients);
                thread::spawn(move || {
                    handle_client(id, stream, &clients);
                });
            }
            Err(e) => {
                error!("failed to accept connection: {e}");
            }
        }
    }
}

fn handle_client(id: ClientId, mut reader: UnixStream, clients: &Clients) {
    loop {
        match transport::read_event(&mut reader) {
            Ok(Some(event)) => {
                tracing::debug!(client = id, topic = %event.topic, "received");
                broadcast(id, &event, clients);
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

    clients.lock().unwrap().remove(&id);
}

fn broadcast(sender: ClientId, event: &sola_bus::Event, clients: &Clients) {
    let mut dead: Vec<ClientId> = Vec::new();

    let mut clients = clients.lock().unwrap();
    for (&id, stream) in clients.iter_mut() {
        if id == sender {
            continue;
        }
        if let Err(e) = transport::write_event(stream, event) {
            warn!(client = id, "write error: {e}");
            dead.push(id);
        }
    }

    for id in dead {
        clients.remove(&id);
    }
}

fn bus_socket_path() -> String {
    if let Ok(path) = env::var("SOLA_BUS_PATH") {
        return path;
    }
    let runtime_dir = env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
    format!("{runtime_dir}/sola-bus")
}
