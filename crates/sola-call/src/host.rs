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

use crate::protocol::{DEFAULT_TIMEOUT_MS, MethodSpec, OwnerCatalog, Role, TraceEvent, Wire};
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
    started: Instant,
    owner: String,
    method: String,
    caller: String,
}

struct Host {
    providers: HashMap<String, Provider>,
    pending: HashMap<String, Pending>,
    observers: HashMap<ClientId, mpsc::Sender<Wire>>,
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
        observers: HashMap::new(),
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
        let mut traces = Vec::new();
        for id in expired {
            if let Some(p) = host.pending.remove(&id) {
                let duration_ms = p.started.elapsed().as_millis() as u64;
                let _ = p.reply_tx.send(Wire::reply_err(&id, "timeout"));
                traces.push(TraceEvent::timeout(
                    id,
                    p.owner,
                    p.caller,
                    p.method,
                    duration_ms,
                ));
            }
        }
        drop(host);
        for event in traces {
            emit_trace(&state, event);
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
        Role::Observer => serve_observer(id, hello.1, reader, writer, state),
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
                    &app_id,
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
                methods: advertise.clone(),
                invoke_tx,
            },
        );
    }
    info!(client = id, %owner, %app_id, "provider registered");
    emit_trace(state, TraceEvent::advertise(&owner, advertise));
    emit_catalog(state);

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
                    let duration_ms = p.started.elapsed().as_millis() as u64;
                    let _ = p.reply_tx.send(Wire::Reply {
                        id: req_id.clone(),
                        ok,
                        error: error.clone(),
                        data: data.clone(),
                    });
                    emit_trace(
                        state,
                        TraceEvent::reply(
                            req_id,
                            p.owner,
                            p.caller,
                            p.method,
                            ok,
                            error,
                            data,
                            duration_ms,
                        ),
                    );
                }
            }
            Ok(Some(Wire::Advertise { methods })) => {
                {
                    let mut host = state.lock().unwrap();
                    if let Some(p) = host.providers.get_mut(&owner) {
                        p.methods = methods.clone();
                    }
                }
                emit_trace(state, TraceEvent::advertise(&owner, methods));
                emit_catalog(state);
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
    caller: &str,
    req_id: String,
    method: String,
    params: serde_json::Value,
    timeout_ms: Option<u64>,
    reply_tx: mpsc::Sender<Wire>,
) {
    emit_trace(
        state,
        TraceEvent::invoke(&req_id, owner, caller, &method, params.clone()),
    );
    let timeout = Duration::from_millis(timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS));
    let send = {
        let mut host = state.lock().unwrap();
        let Some(provider) = host.providers.get(owner) else {
            let err = format!("{owner} is not running");
            let _ = reply_tx.send(Wire::reply_err(&req_id, &err));
            drop(host);
            emit_trace(
                state,
                TraceEvent::reply(&req_id, owner, caller, &method, false, Some(err), None, 0),
            );
            return;
        };
        if !provider.methods.iter().any(|m| m.name == method) {
            let err = format!("{owner} has no method {method}");
            let _ = reply_tx.send(Wire::reply_err(&req_id, &err));
            drop(host);
            emit_trace(
                state,
                TraceEvent::reply(&req_id, owner, caller, &method, false, Some(err), None, 0),
            );
            return;
        }
        let fwd = Wire::Invoke {
            id: req_id.clone(),
            method: method.clone(),
            params,
            owner: None,
            timeout_ms,
        };
        let tx = provider.invoke_tx.clone();
        let started = Instant::now();
        host.pending.insert(
            req_id.clone(),
            Pending {
                reply_tx,
                deadline: started + timeout,
                started,
                owner: owner.to_string(),
                method: method.clone(),
                caller: caller.to_string(),
            },
        );
        (tx, fwd)
    };
    if send.0.send(send.1).is_err() {
        let pending = {
            let mut host = state.lock().unwrap();
            host.pending.remove(&req_id)
        };
        if let Some(p) = pending {
            let err = format!("{owner} is not running");
            let _ = p.reply_tx.send(Wire::reply_err(&req_id, &err));
            emit_trace(
                state,
                TraceEvent::reply(
                    req_id,
                    p.owner,
                    p.caller,
                    p.method,
                    false,
                    Some(err),
                    None,
                    0,
                ),
            );
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
    {
        let mut host = state.lock().unwrap();
        host.providers.remove(owner);
    }
    info!(client = id, %owner, "provider unregistered");
    emit_trace(state, TraceEvent::unregister(owner));
    emit_catalog(state);
}

fn serve_observer(
    id: ClientId,
    app_id: String,
    mut reader: UnixStream,
    writer: UnixStream,
    state: &Shared,
) -> Result<(), String> {
    info!(client = id, %app_id, "observer connected");
    let (out_tx, out_rx) = mpsc::channel::<Wire>();
    {
        let mut host = state.lock().unwrap();
        host.observers.insert(id, out_tx.clone());
    }
    let mut writer = writer;
    thread::spawn(move || {
        while let Ok(msg) = out_rx.recv() {
            if write_msg(&mut writer, &msg).is_err() {
                break;
            }
        }
    });
    let _ = out_tx.send(Wire::Catalog {
        owners: catalog_snapshot(state),
    });

    loop {
        match read_msg(&mut reader) {
            Ok(Some(Wire::List)) => {
                let _ = out_tx.send(Wire::Catalog {
                    owners: catalog_snapshot(state),
                });
            }
            Ok(Some(_)) => {}
            Ok(None) => break,
            Err(e) if is_disconnect(&e) => break,
            Err(e) => {
                unregister_observer(state, id);
                return Err(e.to_string());
            }
        }
    }
    unregister_observer(state, id);
    info!(client = id, %app_id, "observer disconnected");
    Ok(())
}

fn unregister_observer(state: &Shared, id: ClientId) {
    let mut host = state.lock().unwrap();
    host.observers.remove(&id);
}

fn observer_txs(state: &Shared) -> Vec<mpsc::Sender<Wire>> {
    let host = state.lock().unwrap();
    host.observers.values().cloned().collect()
}

fn emit_trace(state: &Shared, event: TraceEvent) {
    let msg = Wire::Trace(event);
    for tx in observer_txs(state) {
        let _ = tx.send(msg.clone());
    }
}

fn emit_catalog(state: &Shared) {
    let msg = Wire::Catalog {
        owners: catalog_snapshot(state),
    };
    for tx in observer_txs(state) {
        let _ = tx.send(msg.clone());
    }
}

fn is_disconnect(e: &io::Error) -> bool {
    matches!(
        e.kind(),
        io::ErrorKind::UnexpectedEof | io::ErrorKind::ConnectionReset | io::ErrorKind::BrokenPipe
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::methods;
    use crate::protocol::TraceKind;
    use crate::{ObserveEvent, catalog, invoke, start_observer, start_provider};
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
            assert!(err.to_string().contains("not running"), "got {err}");
        });
    }

    #[test]
    fn invoke_roundtrip() {
        let (_dir, path) = start_test_host();
        with_path(&path, || {
            let rx = start_provider("compositor", "sola-river", methods::compositor_methods());
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

    fn wait_catalog(
        rx: &std::sync::mpsc::Receiver<ObserveEvent>,
        pred: impl Fn(&[crate::protocol::OwnerCatalog]) -> bool,
    ) -> bool {
        for _ in 0..80 {
            match rx.recv_timeout(Duration::from_millis(25)) {
                Ok(ObserveEvent::Catalog(c)) if pred(&c) => return true,
                Ok(_) => {}
                Err(_) => {}
            }
        }
        false
    }

    #[test]
    fn observer_sees_catalog_and_roundtrip() {
        let (_dir, path) = start_test_host();
        with_path(&path, || {
            let rx = start_observer("test-monitor");
            assert!(
                wait_catalog(&rx, |c| c.is_empty()),
                "observer never received empty catalog"
            );

            let prx = start_provider("compositor", "sola-river", methods::compositor_methods());
            thread::spawn(move || {
                let inc = prx.recv_timeout(Duration::from_secs(2)).unwrap();
                inc.reply.ok(serde_json::json!({"ok": true}));
            });
            assert!(
                wait_catalog(&rx, |c| c.iter().any(|o| o.owner == "compositor")),
                "observer never saw compositor advertise"
            );

            let data = invoke(
                "compositor",
                "windows",
                serde_json::json!({}),
                Duration::from_secs(2),
            )
            .unwrap();
            assert_eq!(data["ok"], serde_json::json!(true));

            let mut saw_invoke = false;
            let mut saw_reply = false;
            for _ in 0..80 {
                match rx.recv_timeout(Duration::from_millis(25)) {
                    Ok(ObserveEvent::Trace(ev)) => match ev.kind {
                        TraceKind::Invoke => {
                            assert_eq!(ev.owner.as_deref(), Some("compositor"));
                            assert_eq!(ev.method.as_deref(), Some("windows"));
                            saw_invoke = true;
                        }
                        TraceKind::Reply => {
                            assert_eq!(ev.ok, Some(true));
                            saw_reply = true;
                        }
                        _ => {}
                    },
                    Ok(_) => {}
                    Err(_) => {}
                }
                if saw_invoke && saw_reply {
                    break;
                }
            }
            assert!(saw_invoke, "observer missed invoke trace");
            assert!(saw_reply, "observer missed reply trace");
        });
    }

    #[test]
    fn observer_sees_timeout() {
        let (_dir, path) = start_test_host();
        with_path(&path, || {
            let rx = start_observer("test-monitor");
            let _prx = start_provider("compositor", "sola-river", methods::compositor_methods());
            assert!(
                wait_catalog(&rx, |c| c.iter().any(|o| o.owner == "compositor")),
                "provider never advertised"
            );
            let err = invoke(
                "compositor",
                "windows",
                serde_json::json!({}),
                Duration::from_millis(200),
            )
            .unwrap_err();
            assert!(err.to_string().contains("timeout"), "got {err}");
            let mut saw = false;
            for _ in 0..80 {
                match rx.recv_timeout(Duration::from_millis(25)) {
                    Ok(ObserveEvent::Trace(ev)) if ev.kind == TraceKind::Timeout => {
                        saw = true;
                        break;
                    }
                    Ok(_) => {}
                    Err(_) => {}
                }
            }
            assert!(saw, "observer missed timeout trace");
        });
    }
}
