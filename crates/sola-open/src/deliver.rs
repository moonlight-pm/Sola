//! Handing a target to its handler. A Sola app gets it through its `open`
//! call-plane method; when it is not running the session starts it (so it is
//! tracked and comes back after a desk restart) and the target waits for its
//! `open`. With no call plane, `<app> --open <target>` (the per-app path).
//! A `.desktop` handler runs its command.

use std::thread;
use std::time::{Duration, Instant};

use sola_call::{ArgSpec, ArgType, CallError, MethodSpec};

use crate::OpenError;
use crate::desktop::Entry;

/// The method a claiming app answers.
pub const OPEN_METHOD: &str = "open";

/// How long an app the session starts has to advertise `open`.
const START_WAIT: Duration = Duration::from_secs(20);

/// How long an app has to open a target it was handed.
const OPEN_TIMEOUT: Duration = Duration::from_secs(10);

/// The `open` method as a claiming app advertises it.
pub fn open_method() -> MethodSpec {
    MethodSpec {
        name: OPEN_METHOD.into(),
        summary: "Open a link or a file here (what Sola's opener and xdg-open send)".into(),
        args: vec![ArgSpec {
            name: "target".into(),
            long: Some("target".into()),
            short: Some('t'),
            ty: ArgType::String,
            required: true,
            help: "The link, or the file's absolute path".into(),
        }],
        timeout_ms: Some(OPEN_TIMEOUT.as_millis() as u64),
    }
}

pub fn to_app(app_id: &str, target: &str) -> Result<(), OpenError> {
    match running(app_id) {
        Ok(Running::Open(owner)) => return invoke(&owner, target),
        // Up, but from before `open`: its own handoff takes the target.
        Ok(Running::Without) => return open_flag(app_id, target),
        Ok(Running::No) => {}
        Err(_) => return open_flag(app_id, target),
    }
    if !launch(app_id) {
        return open_flag(app_id, target);
    }
    let deadline = Instant::now() + START_WAIT;
    while Instant::now() < deadline {
        if let Ok(Running::Open(owner)) = running(app_id) {
            return invoke(&owner, target);
        }
        thread::sleep(Duration::from_millis(100));
    }
    tracing::warn!(%app_id, "the session's app did not advertise open in time — its own handoff takes it");
    open_flag(app_id, target)
}

pub fn to_desktop(entry: &Entry, target: &str) -> Result<(), OpenError> {
    let argv = crate::desktop::argv(&entry.exec, target);
    let (program, args) = argv
        .split_first()
        .ok_or_else(|| OpenError::Failed(format!("{}: empty Exec", entry.id)))?;
    let mut child = std::process::Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| OpenError::Failed(format!("{}: {program}: {e}", entry.id)))?;
    tracing::info!(id = %entry.id, %program, "opened with a .desktop handler");
    thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

enum Running {
    /// In the call catalog with `open`, under this owner.
    Open(String),
    /// In the catalog without `open`.
    Without,
    No,
}

fn running(app_id: &str) -> Result<Running, CallError> {
    let owners = sola_call::catalog()?;
    let mine = owners.iter().filter(|o| o.app_id == app_id);
    let mut seen = false;
    for o in mine {
        seen = true;
        if o.methods.iter().any(|m| m.name == OPEN_METHOD) {
            return Ok(Running::Open(o.owner.clone()));
        }
    }
    Ok(if seen { Running::Without } else { Running::No })
}

fn invoke(owner: &str, target: &str) -> Result<(), OpenError> {
    let params = serde_json::json!({ "target": target });
    match sola_call::invoke(owner, OPEN_METHOD, params, OPEN_TIMEOUT) {
        Ok(_) => {
            tracing::info!(%owner, %target, "opened");
            Ok(())
        }
        Err(CallError::Remote(e)) => Err(OpenError::Failed(format!("{owner}: {e}"))),
        Err(e) => Err(OpenError::Failed(format!("{owner}.open: {e}"))),
    }
}

/// Ask the session to start `app_id` (its program, no arguments).
fn launch(app_id: &str) -> bool {
    use sola_bus::topics::{LaunchAppPayload, Topic};
    let mut bus = sola_bus::BusClient::new();
    if bus.connect().is_err() {
        return false;
    }
    let command = sola_core::paths::bin_path(app_id).display().to_string();
    let ok = bus
        .emit(Topic::LaunchApp(LaunchAppPayload {
            app_id: app_id.into(),
            command,
        }))
        .is_ok();
    if ok {
        tracing::info!(%app_id, "asked the session to start it");
    }
    ok
}

fn open_flag(app_id: &str, target: &str) -> Result<(), OpenError> {
    sola_core::opener::spawn(&sola_core::paths::bin_path(app_id), target).map_err(OpenError::Failed)
}
