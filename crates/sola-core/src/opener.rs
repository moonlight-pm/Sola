//! How the openers ([`crate::open_url`], [`crate::open_image`],
//! [`crate::open_calendar`]) start their app: `<app> --open <target>`.
//!
//! That process hands the target to the running app. With none running it
//! has the session (`sola-session`) start the app and then hands the target
//! over, so the app runs inside the session: tracked, and brought back when
//! the desk restarts. Only with no session to ask does it become the app.
//! Started here directly, the app would run outside the session, and a desk
//! restart would lose it.

use std::path::Path;
use std::process::{Command, Stdio};

/// The flag that marks an opener's launch.
pub const OPEN_FLAG: &str = "--open";

/// Spawn `bin --open target`, detached; a thread reaps it once it has
/// handed the target over and exited.
pub fn spawn(bin: &Path, target: &str) -> Result<(), String> {
    let mut child = Command::new(bin)
        .arg(OPEN_FLAG)
        .arg(target)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("spawn {}: {e}", bin.display()))?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// This process was started by an opener.
pub fn is_open_launch() -> bool {
    std::env::args().skip(1).any(|a| a == OPEN_FLAG)
}
