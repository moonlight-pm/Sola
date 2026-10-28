//! Named informational bots: each a directory in `~/Bots`, on the AI it
//! was made with (Grok, Claude, Gab AI); the host, its socket API and
//! call-plane methods; the Bots app.

pub mod acp;
pub mod ai;
pub mod calls;
pub mod catalog;
pub mod claude;
pub mod client;
pub mod foundation;
pub mod gab;
pub mod host;
pub mod http;
pub mod paths;
pub mod sse;
pub mod ui;

pub use ai::Ai;
pub use catalog::{BotRecord, Catalog};
pub use host::Host;
