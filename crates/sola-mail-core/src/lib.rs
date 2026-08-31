//! IMAP/SMTP mail core — no UI kit.

pub mod bridge;
pub mod protocol;
pub mod worker;

/// Install the rustls crypto provider (required before IMAP/SMTP TLS).
pub fn install_crypto() {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
}
