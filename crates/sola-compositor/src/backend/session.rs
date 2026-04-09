/// Session management via libseat.
///
/// On Linux, accessing GPU and input hardware requires elevated privileges.
/// Rather than running the compositor as root, we use `libseat` — a library
/// that communicates with a seat daemon (e.g., `seatd` or `systemd-logind`)
/// to obtain file descriptors for hardware devices with the right permissions.
///
/// `libseat` also manages VT (virtual terminal) switching — when the user
/// presses Ctrl+Alt+F2 to switch to another TTY, the session is "deactivated"
/// and we must release hardware access until we're switched back.
///
/// See: https://docs.rs/smithay/0.7.0/smithay/backend/session/libseat/index.html
/// See: https://git.sr.ht/~kennylevinsen/seatd
use smithay::backend::session::libseat::{LibSeatSession, LibSeatSessionNotifier};
use smithay::backend::session::Session;

/// Create a new libseat session.
///
/// Returns the session handle (for opening device files) and a notifier
/// (calloop event source for VT switch events).
///
/// # Prerequisites
/// - `seatd` service must be running, OR systemd-logind must be available.
/// - The user must have permission to access the seat (typically via the
///   `seat` group).
pub fn create() -> anyhow::Result<(LibSeatSession, LibSeatSessionNotifier)> {
    let (session, notifier) = LibSeatSession::new()?;
    tracing::info!(seat = %session.seat(), "session opened via libseat");
    Ok((session, notifier))
}
