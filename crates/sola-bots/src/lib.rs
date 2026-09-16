//! Named informational bots: catalog, ACP host, call-plane methods.

pub mod acp;
pub mod auth;
pub mod calls;
pub mod catalog;
pub mod foundation;
pub mod host;
pub mod http;
pub mod paths;
pub mod ui;

pub use catalog::{BotRecord, Catalog};
pub use host::Host;
