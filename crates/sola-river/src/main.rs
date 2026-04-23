use std::path::PathBuf;
use std::process::exit;
use std::time::Duration;

use calloop::EventLoop;
use calloop_wayland_source::WaylandSource;
use tracing::{error, info};

use sola_bus::topics::TopicKind;
use sola_river::{bus, client};

/// Filename (inside XDG_RUNTIME_DIR) where sola publishes the name of
/// the live wayland socket. Written by sola once it has spawned River
/// and confirmed the socket is accepting connections.
const SOLA_WAYLAND_NAME_FILE: &str = "sola-wayland";

fn main() {
    sola_core::log::init("sola-river");
    info!("sola-river starting");

    let socket_name = match read_wayland_socket_name() {
        Some(n) => n,
        None => {
            error!(
                file = SOLA_WAYLAND_NAME_FILE,
                "wayland socket name file not populated; is sola running?"
            );
            exit(1);
        }
    };

    // SAFETY: no other threads in sola-river yet — single-threaded main.
    unsafe {
        std::env::set_var("WAYLAND_DISPLAY", &socket_name);
    }

    let mut bus = bus::BusClient::new();
    bus.ensure_connected();
    bus.subscribe(&[
        TopicKind::Composition,
        TopicKind::Frame,
        TopicKind::Focus,
        TopicKind::RegisteredChords,
        TopicKind::Copy,
        TopicKind::Paste,
        TopicKind::CloseApp,
        TopicKind::Shutdown,
    ]);

    let (_conn, queue, mut data) = match client::connect(bus) {
        Ok(x) => x,
        Err(e) => {
            error!(%e, "wayland connect failed");
            exit(1);
        }
    };

    let mut event_loop: EventLoop<client::AppData> =
        EventLoop::try_new().expect("calloop event loop");
    let handle = event_loop.handle();

    WaylandSource::new(_conn, queue)
        .insert(handle.clone())
        .expect("insert wayland source");

    // Bus tick every 20ms. This cadence is low enough to coalesce burst-y
    // bus updates from the shell (Composition + Frame + Focus fired in
    // immediate succession on every keypress) into one manage/render cycle.
    handle
        .insert_source(
            calloop::timer::Timer::from_duration(Duration::from_millis(20)),
            |_, _, state: &mut client::AppData| {
                client::bus_tick(state);
                calloop::timer::TimeoutAction::ToDuration(Duration::from_millis(20))
            },
        )
        .expect("bus timer");

    info!("event loop running");
    if let Err(e) = event_loop.run(Duration::from_millis(500), &mut data, |_| {}) {
        error!(%e, "event loop exited with error");
    }
}

/// Read the wayland socket name sola published to
/// `$XDG_RUNTIME_DIR/sola-wayland`. Returns the trimmed contents or None
/// if the file is missing or empty.
fn read_wayland_socket_name() -> Option<String> {
    let runtime_dir = std::env::var_os("XDG_RUNTIME_DIR")?;
    let path = PathBuf::from(runtime_dir).join(SOLA_WAYLAND_NAME_FILE);
    let raw = std::fs::read_to_string(&path).ok()?;
    let name = raw.trim();
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

