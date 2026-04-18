mod client;
mod message;
pub mod topic;
pub mod topics;
pub mod transport;

use std::env;

pub use client::BusClient;
pub use message::{CONTROL_IDENTIFY, CONTROL_SUBSCRIBE, Message};

/// Returns the bus socket path from `$SOLA_BUS_PATH` or the default
/// `$XDG_RUNTIME_DIR/sola-bus`.
pub fn socket_path() -> String {
    if let Ok(path) = env::var("SOLA_BUS_PATH") {
        return path;
    }
    let runtime_dir = env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
    format!("{runtime_dir}/sola-bus")
}
