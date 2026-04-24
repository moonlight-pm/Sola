mod client;
mod message;
mod registry;
pub mod state;
pub mod topic;
pub mod topics;
pub mod transport;

pub use client::BusClient;
pub use message::{CONTROL_IDENTIFY, CONTROL_SUBSCRIBE, Message};
pub use registry::{BusHandler, BusRegistry};

/// Returns the bus socket path from `$SOLA_BUS_PATH` or the default
/// `$XDG_RUNTIME_DIR/sola-bus`.
pub fn socket_path() -> String {
    if let Ok(path) = std::env::var("SOLA_BUS_PATH") {
        return path;
    }
    sola_core::env::runtime_dir()
        .join("sola-bus")
        .to_string_lossy()
        .into_owned()
}
