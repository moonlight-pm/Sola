//! Caller and provider clients.

use std::io;
use std::os::unix::net::UnixStream;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use tracing::{info, warn};

use crate::protocol::{
    DEFAULT_TIMEOUT_MS, MethodSpec, OwnerCatalog, Role, TraceEvent, Wire, new_id,
};
use crate::socket_path;
use crate::transport::{read_msg, write_msg};

#[derive(Debug)]
pub enum CallError {
    Connect(String),
    Io(String),
    Remote(String),
    Timeout,
    Protocol(String),
}

impl std::fmt::Display for CallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Connect(s) => write!(f, "{s}"),
            Self::Io(s) => write!(f, "{s}"),
            Self::Remote(s) => write!(f, "{s}"),
            Self::Timeout => write!(f, "timeout waiting for reply"),
            Self::Protocol(s) => write!(f, "{s}"),
        }
    }
}

impl From<io::Error> for CallError {
    fn from(e: io::Error) -> Self {
        Self::Io(e.to_string())
    }
}

fn connect_stream() -> Result<UnixStream, CallError> {
    let path = socket_path();
    UnixStream::connect(&path)
        .map_err(|_| CallError::Connect(format!("call host is not running ({path})")))
}

/// One-shot caller: hello, one request, wait for reply, disconnect.
pub fn invoke(
    owner: &str,
    method: &str,
    params: serde_json::Value,
    timeout: Duration,
) -> Result<serde_json::Value, CallError> {
    let mut stream = connect_stream()?;
    write_msg(
        &mut stream,
        &Wire::Hello {
            role: Role::Caller,
            app_id: "solactl".into(),
            owner: None,
        },
    )?;
    let id = new_id();
    write_msg(
        &mut stream,
        &Wire::Invoke {
            id: id.clone(),
            method: method.to_string(),
            params,
            owner: Some(owner.to_string()),
            timeout_ms: Some(timeout.as_millis() as u64),
        },
    )?;
    stream.set_read_timeout(Some(timeout + Duration::from_millis(250)))?;
    let deadline = Instant::now() + timeout;
    loop {
        if Instant::now() >= deadline {
            return Err(CallError::Timeout);
        }
        match read_msg(&mut stream)? {
            Some(Wire::Reply {
                id: rid,
                ok,
                error,
                data,
            }) if rid == id => {
                if ok {
                    return Ok(data.unwrap_or(serde_json::Value::Null));
                }
                return Err(CallError::Remote(error.unwrap_or_else(|| "failed".into())));
            }
            Some(_) => continue,
            None => {
                return Err(CallError::Protocol(
                    "call host closed the connection".into(),
                ));
            }
        }
    }
}

pub fn catalog() -> Result<Vec<OwnerCatalog>, CallError> {
    let mut stream = connect_stream()?;
    write_msg(
        &mut stream,
        &Wire::Hello {
            role: Role::Caller,
            app_id: "solactl".into(),
            owner: None,
        },
    )?;
    write_msg(&mut stream, &Wire::List)?;
    stream.set_read_timeout(Some(Duration::from_secs(3)))?;
    match read_msg(&mut stream)? {
        Some(Wire::Catalog { owners }) => Ok(owners),
        Some(other) => Err(CallError::Protocol(format!(
            "expected catalog, got {other:?}"
        ))),
        None => Err(CallError::Protocol("call host closed".into())),
    }
}

/// Reply handle handed to a provider's main loop.
///
/// `Clone` so iced `Message` enums can carry it. The host accepts the
/// first reply for an id; later clones are ignored.
#[derive(Clone)]
pub struct ReplyTx {
    id: String,
    tx: mpsc::Sender<Wire>,
}

impl ReplyTx {
    pub fn ok(self, data: serde_json::Value) {
        let _ = self.tx.send(Wire::reply_ok(self.id, data));
    }

    pub fn err(self, msg: impl Into<String>) {
        let _ = self.tx.send(Wire::reply_err(self.id, msg));
    }
}

#[derive(Clone)]
pub struct Incoming {
    pub method: String,
    pub params: serde_json::Value,
    pub reply: ReplyTx,
}

impl std::fmt::Debug for Incoming {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Incoming")
            .field("method", &self.method)
            .field("params", &self.params)
            .finish()
    }
}

/// Start a reconnecting provider. Incoming invokes are sent on the returned
/// channel. Dropping the receiver stops nothing — the IO thread keeps
/// reconnecting so the process stays registered.
pub fn start_provider(
    owner: &str,
    app_id: &str,
    methods: Vec<MethodSpec>,
) -> mpsc::Receiver<Incoming> {
    let (tx, rx) = mpsc::channel();
    let owner = owner.to_string();
    let app_id = app_id.to_string();
    thread::Builder::new()
        .name(format!("sola-call-{owner}"))
        .spawn(move || provider_loop(owner, app_id, methods, tx))
        .expect("spawn call provider");
    rx
}

fn provider_loop(
    owner: String,
    app_id: String,
    methods: Vec<MethodSpec>,
    incoming: mpsc::Sender<Incoming>,
) {
    loop {
        match serve_provider(&owner, &app_id, &methods, &incoming) {
            Ok(()) => info!(%owner, "call provider disconnected"),
            Err(e) => warn!(%owner, "call provider: {e}"),
        }
        thread::sleep(Duration::from_millis(400));
    }
}

fn serve_provider(
    owner: &str,
    app_id: &str,
    methods: &[MethodSpec],
    incoming: &mpsc::Sender<Incoming>,
) -> Result<(), String> {
    let stream = UnixStream::connect(socket_path()).map_err(|e| e.to_string())?;
    let mut writer = stream.try_clone().map_err(|e| e.to_string())?;
    let mut reader = stream;
    write_msg(
        &mut writer,
        &Wire::Hello {
            role: Role::Provider,
            app_id: app_id.to_string(),
            owner: Some(owner.to_string()),
        },
    )
    .map_err(|e| e.to_string())?;
    write_msg(
        &mut writer,
        &Wire::Advertise {
            methods: methods.to_vec(),
        },
    )
    .map_err(|e| e.to_string())?;
    info!(%owner, %app_id, "call provider advertised");

    let (reply_tx, reply_rx) = mpsc::channel::<Wire>();
    thread::Builder::new()
        .name(format!("sola-call-{owner}-w"))
        .spawn(move || {
            while let Ok(msg) = reply_rx.recv() {
                if write_msg(&mut writer, &msg).is_err() {
                    break;
                }
            }
        })
        .map_err(|e| e.to_string())?;

    loop {
        match read_msg(&mut reader) {
            Ok(Some(Wire::Invoke {
                id, method, params, ..
            })) => {
                let incoming_msg = Incoming {
                    method,
                    params,
                    reply: ReplyTx {
                        id,
                        tx: reply_tx.clone(),
                    },
                };
                if incoming.send(incoming_msg).is_err() {
                    return Ok(());
                }
            }
            Ok(Some(_)) => {}
            Ok(None) => return Ok(()),
            Err(e) => return Err(e.to_string()),
        }
    }
}

/// Default invoke timeout for CLIs that do not take `--timeout`.
pub fn default_timeout() -> Duration {
    Duration::from_millis(DEFAULT_TIMEOUT_MS)
}

/// Events delivered to a call-plane observer (monitor).
#[derive(Debug, Clone)]
pub enum ObserveEvent {
    Catalog(Vec<OwnerCatalog>),
    Trace(TraceEvent),
    /// Socket closed or connect failed; the loop will retry.
    Down,
}

/// Reconnecting observer. Catalog snapshots and traces are sent on the
/// returned channel. Dropping the receiver does not stop the IO thread.
pub fn start_observer(app_id: &str) -> mpsc::Receiver<ObserveEvent> {
    let (tx, rx) = mpsc::channel();
    let app_id = app_id.to_string();
    thread::Builder::new()
        .name("sola-call-observer".into())
        .spawn(move || observer_loop(app_id, tx))
        .expect("spawn call observer");
    rx
}

fn observer_loop(app_id: String, tx: mpsc::Sender<ObserveEvent>) {
    loop {
        match serve_observer(&app_id, &tx) {
            Ok(()) => info!("call observer disconnected"),
            Err(e) => warn!("call observer: {e}"),
        }
        let _ = tx.send(ObserveEvent::Down);
        thread::sleep(Duration::from_millis(400));
    }
}

fn serve_observer(app_id: &str, tx: &mpsc::Sender<ObserveEvent>) -> Result<(), String> {
    let stream = UnixStream::connect(socket_path()).map_err(|e| e.to_string())?;
    let mut writer = stream.try_clone().map_err(|e| e.to_string())?;
    let mut reader = stream;
    write_msg(
        &mut writer,
        &Wire::Hello {
            role: Role::Observer,
            app_id: app_id.to_string(),
            owner: None,
        },
    )
    .map_err(|e| e.to_string())?;
    info!(%app_id, "call observer connected");

    loop {
        match read_msg(&mut reader) {
            Ok(Some(Wire::Catalog { owners })) => {
                if tx.send(ObserveEvent::Catalog(owners)).is_err() {
                    return Ok(());
                }
            }
            Ok(Some(Wire::Trace(event))) => {
                if tx.send(ObserveEvent::Trace(event)).is_err() {
                    return Ok(());
                }
            }
            Ok(Some(_)) => {}
            Ok(None) => return Ok(()),
            Err(e) => return Err(e.to_string()),
        }
    }
}
