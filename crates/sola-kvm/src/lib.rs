//! sola-kvm library surface — protocol, layout math, and config.
//!
//! The binary (`main.rs`) owns process lifecycle and UDP I/O. This crate
//! is the pure logic used by unit tests and (later) the Mac agent when
//! shared types are needed.

pub mod config;
pub mod layout;
pub mod protocol;
pub mod udp;

pub use config::Config;
pub use layout::{Layout, Side, Align};
pub use protocol::{Packet, PacketType, MAGIC, VERSION};
