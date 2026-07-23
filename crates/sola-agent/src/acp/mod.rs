//! Thin ACP NDJSON client for a long-lived worker thread.
//!
//! Hand-rolled (not the full async SDK) so the process model matches
//! sola-terminal: one OS thread owns the child and mpsc bridges to iced.

mod client;
mod transport;

pub use client::AcpClient;
pub use transport::ChildTransport;
