//! Talking to `sola-vaultd`, only through `sola-vault-proto`: requests on
//! a worker thread that keeps one connection ([`ask`]), the slow ones
//! (a login's KDF, a full sync) each on a connection of their own
//! ([`ask_alone`]) so they hold nothing else up, and the daemon's events
//! on a thread of their own ([`link`]).

mod ask;
mod link;

pub use ask::{Answer, ask, ask_alone};
pub use link::{Link, subscription as link};

/// How Vault names itself to the daemon (its log, its peer check).
pub const CLIENT: &str = "sola-vault";
