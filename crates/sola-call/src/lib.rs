mod client;
pub mod host;
pub mod methods;
pub mod protocol;
pub mod transport;

pub use client::{
    catalog, default_timeout, invoke, start_provider, CallError, Incoming, ReplyTx,
};
pub use protocol::{
    ArgSpec, ArgType, MethodSpec, OwnerCatalog, Role, Wire, DEFAULT_TIMEOUT_MS, new_id,
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
