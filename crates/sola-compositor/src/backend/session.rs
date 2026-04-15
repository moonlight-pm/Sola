use smithay::backend::session::Session;
/// Session management via libseat.
///
/// On Linux, accessing GPU and input hardware requires elevated privileges.
/// `libseat` communicates with a seat daemon (seatd/systemd-logind) to
/// obtain device file descriptors with the right permissions.
///
/// See: https://docs.rs/smithay/0.7.0/smithay/backend/session/libseat/index.html
use smithay::backend::session::libseat::{LibSeatSession, LibSeatSessionNotifier};

use crate::error::SessionError;

/// Create a new libseat session.
///
/// Returns the session handle (for opening device files) and a notifier
/// (calloop event source for VT switch events).
pub fn create() -> Result<(LibSeatSession, LibSeatSessionNotifier), SessionError> {
    let (session, notifier) =
        LibSeatSession::new().map_err(|e| SessionError::Open(e.to_string()))?;
    tracing::info!(seat = %session.seat(), "session opened via libseat");
    Ok((session, notifier))
}
