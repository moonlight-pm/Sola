//! CEF engine integration. The boundary between sola-kit and the CEF
//! Rust binding lives entirely in this module — the rest of the kit
//! does not know what engine is underneath.

pub mod browser;
pub mod distribution;
pub mod handlers;
pub mod init;
pub mod scheme;

pub use browser::Browser;
pub use init::short_circuit_if_subprocess;
