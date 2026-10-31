//! Opening between apps. [`open`]: a link or a file to the handler Sola
//! keeps for it ([`sola_open`]); [`open_target`]: the `open` call a
//! claiming app answers. [`start`]: an opener's launch (`<app> --open
//! <target>`, see [`sola_core::opener`]) with the app not running — the
//! session starts the app, as itself, and this process hands it the target
//! once it is ready.

use std::thread;
use std::time::{Duration, Instant};

use sola_bus::topics::{LaunchAppPayload, Topic, TopicKind, Window};

/// How long an opener's launch waits for the app it asked the session for.
pub const START_WAIT: Duration = Duration::from_secs(20);

/// Ask the session to start `app_id` (its program in Sola's bin directory,
/// no arguments) and wait until `ready`, given the live window list, says it
/// can take a handoff. False when there is no bus to ask through, or the app
/// was not ready within [`START_WAIT`]: the caller then becomes the app.
pub fn start(app_id: &str, ready: impl Fn(&[Window]) -> bool) -> bool {
    let mut bus = sola_bus::BusClient::new();
    if bus.connect().is_err() || bus.subscribe(&[TopicKind::Windows]).is_err() {
        return false;
    }
    let command = sola_core::paths::bin_path(app_id).display().to_string();
    let launch = Topic::LaunchApp(LaunchAppPayload {
        app_id: app_id.into(),
        command,
    });
    if bus.emit(launch).is_err() {
        return false;
    }
    let mut windows: Vec<Window> = Vec::new();
    let deadline = Instant::now() + START_WAIT;
    while Instant::now() < deadline {
        while let Some(msg) = bus.try_recv() {
            if let Some(Topic::Windows(ws)) = Topic::parse(&msg) {
                windows = ws;
            }
        }
        if ready(&windows) {
            return true;
        }
        thread::sleep(Duration::from_millis(100));
    }
    tracing::warn!(%app_id, "the session's app was not ready in time — starting here");
    false
}

/// A window of `app_id` is up: its bus subscription is in place.
pub fn has_window(windows: &[Window], app_id: &str) -> bool {
    windows.iter().any(|w| w.app_id == app_id)
}

/// Open a link or a file with the handler Sola keeps for it
/// ([`sola_open::open`]), off the UI thread: a link clicked in an app.
pub fn open(target: &str) {
    sola_open::open_detached(target);
}

/// The target of an `open` call (the method [`crate::app::BusSetup::opens`]
/// adds), or `None` for any other call.
pub fn open_target(inc: &sola_call::Incoming) -> Option<String> {
    (inc.method == sola_open::OPEN_METHOD)
        .then(|| inc.params.get("target")?.as_str().map(str::to_string))
        .flatten()
}
