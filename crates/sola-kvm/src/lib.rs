//! sola-kvm library surface — protocol, layout math, config, and server state.
//!
//! The binary (`main.rs`) owns process lifecycle, input backends, and UDP I/O.
//! Pure enter/leave/motion logic lives in [`server`] for unit tests and reuse.

pub mod config;
pub mod input;
pub mod layout;
pub mod protocol;
pub mod run;
pub mod server;
pub mod udp;

pub use config::Config;
pub use layout::{Align, Layout, Side};
pub use protocol::{Packet, PacketType, MAGIC, VERSION};
pub use server::{InputEvent, Mode, Session, SideEffect, Step};
