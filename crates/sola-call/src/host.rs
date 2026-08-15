//! In-process call host. The `sola-call` binary is a thin wrapper.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use tracing::{error, info, warn};

use crate::protocol::{MethodSpec, OwnerCatalog, Role, Wire, DEFAULT_TIMEOUT_MS};
use crate::transport::{read_msg, write_msg};

type ClientId = u64;

struct Provider {
    app_id: String,
    methods: Vec<MethodSpec>,
    invoke_tx: mpsc::Sender<Wire>,
}

struct Pending {
    reply_tx: mpsc::Sender<Wire>,
    deadline: Instant,
}

struct Host {
    providers: HashMap<String, Provider>,
    pending: HashMap<String, Pending>,
}

type Shared = Arc<Mutex<Host>>;

/// Bind `path` (0600) and serve forever.
pub fn bind_and_serve(path: &str) -> ! {
    let _ = fs::remove_file(path);
    if let Some(dir) = std::path::Path::new(path).parent() {
        let _ = fs::create_dir_all(dir);
    }
    let listener = UnixListener::bind(path).unwrap_or_else(|e| {
        panic!("failed to bind call socket at {path}: {e}");
    });
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    info!(path, "call host listening");
    serve(listener);
}

pub fn serve(listener: UnixListener) -> ! {
    let state: Shared = Arc::new(Mutex::new(Host {
        providers: HashMap::new(),
        pending: HashMap::new(),
    }));
    {
        let state = Arc::clone(&state);
        thread::spawn(move || reap_timeouts(state));
    }
    let mut next_id: ClientId = 0;
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let id = next_id;
                next_id += 1;
                let state = Arc::clone(&state);
                thread::spawn(move || {
                    if let Err(e) = handle_client(id, stream, &state) {
                        warn!(client = id, "{e}");
                    }
                });
            }
            Err(e) => error!("accept failed: {e}"),
        }
    }
    panic!("call host listener closed");
}

/// Serve on a background thread (tests).
pub fn serve_background(listener: UnixListener) {
    thread::spawn(move || serve(listener));
}

fn reap_timeouts(state: Shared) {
    loop {
        thread::sleep(Duration::from_millis(200));
        let now = Instant::now();
        let mut host = state.lock().unwrap();
        let expired: Vec<String> = host
            .pending
            .iter()
            .filter(|(_, p)| p.deadline <= now)
            .map(|(id, _)| id.clone())
            .collect();
        for id in expired {
            if let Some(p) = host.pending.remove(&id) {
                let _ = p.reply_tx.send(Wire::reply_err(id, "timeout"));
            }
        }
    }
}

fn handle_client(id: ClientId, stream: UnixStream, state: &Shared) -> Result<(), String> {
    let mut reader = stream.try_clone().map_err(|e| e.to_string())?;
    let writer = stream;
    let hello = match read_msg(&mut reader).map_err(|e| e.to_string())? {
        Some(Wire::Hello {
            role,
            app_id,
            owner,
        }) => (role, app_id, owner),
        Some(_) => return Err("first message must be hello".into()),
        None => return Ok(()),
    };
    match hello.0 {
        Role::Caller => serve_caller(id, hello.1, reader, writer, state),
        Role::Provider => {
            let owner = hello.2.ok_or("provider hello missing owner")?;
            serve_provider(id, owner, hello.1, reader, writer, state)
        }
    }
}

fn serve_caller(
    id: ClientId,
    app_id: String,
    mut reader: UnixStream,
    writer: UnixStream,
    state: &Shared,
) -> Result<(), String> {
    info!(client = id, %app_id, "caller connected");
    let (reply_tx, reply_rx) = mpsc::channel::<Wire>();
    let mut writer = writer;
    thread::spawn(move || {
        while let Ok(msg) = reply_rx.recv() {
            if write_msg(&mut writer, &msg).is_err() {
                break;
            }
        }
    });

    loop {
        match read_msg(&mut reader) {
            Ok(Some(Wire::List)) => {
                let catalog = catalog_snapshot(state);
                let _ = reply_tx.send(Wire::Catalog { owners: catalog });
            }
            Ok(Some(Wire::Invoke {
                id: req_id,
                method,
                params,
                owner,
                timeout_ms,
            })) => {
                let Some(owner) = owner else {
                    let _ = reply_tx.send(Wire::reply_err(req_id, "invoke missing owner"));
                    continue;
                };
                dispatch_invoke(
                    state,
                    &owner,
                    req_id,
                    method,
                    params,
                    timeout_ms,
                    reply_tx.clone(),
                );
            }
            Ok(Some(_)) => {}
            Ok(None) => break,
            Err(e) if is_disconnect(&e) => break,
            Err(e) => return Err(e.to_string()),
        }
    }
    info!(client = id, %app_id, "caller disconnected");
    Ok(())
}

fn serve_provider(
    id: ClientId,
    owner: String,
    app_id: String,
    mut reader: UnixStream,
    writer: UnixStream,
    state: &Shared,
) -> Result<(), String> {
    let advertise = match read_msg(&mut reader).map_err(|e| e.to_string())? {
        Some(Wire::Advertise { methods }) => methods,
        Some(_) => return Err("provider must advertise after hello".into()),
        None => return Ok(()),
    };

    let (invoke_tx, invoke_rx) = mpsc::channel::<Wire>();
    {
        let mut host = state.lock().unwrap();
        if host.providers.remove(&owner).is_some() {
            warn!(%owner, "replacing previous provider");
        }
        host.providers.insert(
            owner.clone(),
            Provider {
                app_id: app_id.clone(),
                methods: advertise,
                invoke_tx,
            },
        );
    }
    info!(client = id, %owner, %app_id, "provider registered");

    let mut writer = writer;
    thread::spawn(move || {
        while let Ok(msg) = invoke_rx.recv() {
            if write_msg(&mut writer, &msg).is_err() {
                break;
            }
        }
    });

    loop {
        match read_msg(&mut reader) {
            Ok(Some(Wire::Reply {
                id: req_id,
                ok,
                error,
                data,
            })) => {
                let pending = {
                    let mut host = state.lock().unwrap();
                    host.pending.remove(&req_id)
                };
                if let Some(p) = pending {
                    let _ = p.reply_tx.send(Wire::Reply {
                        id: req_id,
                        ok,
                        error,
                        data,
                    });
                }
            }
            Ok(Some(Wire::Advertise { methods })) => {
                let mut host = state.lock().unwrap();
                if let Some(p) = host.providers.get_mut(&owner) {
                    p.methods = methods;
                }
            }
            Ok(Some(_)) => {}
            Ok(None) => break,
            Err(e) if is_disconnect(&e) => break,
            Err(e) => {
                unregister_provider(state, &owner, id);
                return Err(e.to_string());
            }
        }
    }

    unregister_provider(state, &owner, id);
    Ok(())
}

fn dispatch_invoke(
    state: &Shared,
    owner: &str,
    req_id: String,
    method: String,
    params: serde_json::Value,
    timeout_ms: Option<u64>,
    reply_tx: mpsc::Sender<Wire>,
) {
    let timeout = Duration::from_millis(timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS));
    let send = {
        let mut host = state.lock().unwrap();
        let Some(provider) = host.providers.get(owner) else {
            let _ = reply_tx.send(Wire::reply_err(
                req_id,
                format!("{owner} is not running"),
            ));
            return;
        };
        if !provider.methods.iter().any(|m| m.name == method) {
            let _ = reply_tx.send(Wire::reply_err(
                req_id,
                format!("{owner} has no method {method}"),
            ));
            return;
        }
        let fwd = Wire::Invoke {
            id: req_id.clone(),
            method,
            params,
            owner: None,
            timeout_ms,
        };
        let tx = provider.invoke_tx.clone();
        host.pending.insert(
            req_id.clone(),
            Pending {
                reply_tx,
                deadline: Instant::now() + timeout,
            },
        );
        (tx, fwd)
    };
    if send.0.send(send.1).is_err() {
        let mut host = state.lock().unwrap();
        if let Some(p) = host.pending.remove(&req_id) {
            let _ = p
                .reply_tx
                .send(Wire::reply_err(req_id, format!("{owner} is not running")));
        }
    }
}

fn catalog_snapshot(state: &Shared) -> Vec<OwnerCatalog> {
    let host = state.lock().unwrap();
    let mut owners: Vec<OwnerCatalog> = host
        .providers
        .iter()
        .map(|(owner, p)| OwnerCatalog {
            owner: owner.clone(),
            app_id: p.app_id.clone(),
            methods: p.methods.clone(),
        })
        .collect();
    owners.sort_by(|a, b| a.owner.cmp(&b.owner));
    owners
}

fn unregister_provider(state: &Shared, owner: &str, id: ClientId) {
    let mut host = state.lock().unwrap();
    host.providers.remove(owner);
    info!(client = id, %owner, "provider unregistered");
}

fn is_disconnect(e: &io::Error) -> bool {
    matches!(
        e.kind(),
        io::ErrorKind::UnexpectedEof
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::BrokenPipe
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::methods;
    use crate::{catalog, invoke, start_provider};
    use std::time::Duration;

    fn start_test_host() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sola-call");
        let path_s = path.to_string_lossy().into_owned();
        let listener = UnixListener::bind(&path).unwrap();
        serve_background(listener);
        // Wait until the socket accepts.
        for _ in 0..50 {
            if UnixStream::connect(&path).is_ok() {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        (dir, path_s)
    }

    static ENV: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn with_path<T>(path: &str, f: impl FnOnce() -> T) -> T {
        let _guard = ENV.lock().unwrap();
        // SAFETY: ENV mutex makes this the only test mutating the var.
        let prev = std::env::var("SOLA_CALL_PATH").ok();
        unsafe { std::env::set_var("SOLA_CALL_PATH", path) };
        let out = f();
        match prev {
            Some(v) => unsafe { std::env::set_var("SOLA_CALL_PATH", v) },
            None => unsafe { std::env::remove_var("SOLA_CALL_PATH") },
        }
        out
    }

    #[test]
    fn fail_if_owner_down() {
        let (_dir, path) = start_test_host();
        with_path(&path, || {
            let err = invoke(
                "compositor",
                "windows",
                serde_json::json!({}),
                Duration::from_secs(1),
            )
            .unwrap_err();
            assert!(
                err.to_string().contains("not running"),
                "got {err}"
            );
        });
    }

    #[test]
    fn invoke_roundtrip() {
        let (_dir, path) = start_test_host();
        with_path(&path, || {
            let rx = start_provider(
                "compositor",
                "sola-river",
                methods::compositor_methods(),
            );
            thread::spawn(move || {
                let inc = rx.recv_timeout(Duration::from_secs(2)).unwrap();
                assert_eq!(inc.method, "windows");
                inc.reply.ok(serde_json::json!({"sola-shell": []}));
            });
            // Provider reconnects; wait until catalog sees it.
            let mut seen = false;
            for _ in 0..50 {
                if catalog()
                    .ok()
                    .is_some_and(|c| c.iter().any(|o| o.owner == "compositor"))
                {
                    seen = true;
                    break;
                }
                thread::sleep(Duration::from_millis(20));
            }
            assert!(seen, "provider never advertised");
            let data = invoke(
                "compositor",
                "windows",
                serde_json::json!({}),
                Duration::from_secs(2),
            )
            .unwrap();
            assert_eq!(data["sola-shell"], serde_json::json!([]));
        });
    }
}
