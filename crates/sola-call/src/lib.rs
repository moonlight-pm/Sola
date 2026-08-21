mod client;
pub mod host;
pub mod methods;
pub mod protocol;
pub mod transport;

pub use client::{
    CallError, Incoming, ObserveEvent, ReplyTx, catalog, default_timeout, invoke, start_observer,
    start_provider,
};
pub use protocol::{
    ArgSpec, ArgType, DEFAULT_TIMEOUT_MS, MethodSpec, OwnerCatalog, Role, TraceEvent, TraceKind,
    Wire, new_id,
};

/// `$SOLA_CALL_PATH` or `$XDG_RUNTIME_DIR/sola-call`.
pub fn socket_path() -> String {
    if let Ok(path) = std::env::var("SOLA_CALL_PATH") {
        return path;
    }
    sola_core::env::runtime_dir()
        .join("sola-call")
        .to_string_lossy()
        .into_owned()
}
