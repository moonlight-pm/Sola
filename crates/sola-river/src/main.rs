use std::path::Path;
use std::process::exit;
use std::time::Duration;

use calloop::EventLoop;
use calloop_wayland_source::WaylandSource;
use tracing::{error, info};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use sola_river::{bus, client, supervisor};

fn main() {
    init_tracing();
    info!("sola-river starting");

    let mut sup = match supervisor::RiverSupervisor::spawn(Path::new("/opt/sola/log/river.log"))
    {
        Ok(s) => s,
        Err(e) => {
            error!(%e, "failed to spawn river");
            exit(1);
        }
    };

    let socket_name = match sup.wait_for_socket() {
        Ok(n) => n,
        Err(e) => {
            error!(%e, "river socket never appeared");
            sup.shutdown();
            exit(1);
        }
    };

    // Point our own wayland-client at whatever socket River actually
    // opened. The `sola-wayland` file published by `wait_for_socket` is
    // what our sibling sola processes read.
    // SAFETY: no other threads in sola-river yet — single-threaded main.
    unsafe {
        std::env::set_var("WAYLAND_DISPLAY", &socket_name);
    }

    let mut bus = bus::BusClient::new();
    bus.ensure_connected();

    let (_conn, queue, mut data) = match client::connect(bus) {
        Ok(x) => x,
        Err(e) => {
            error!(%e, "wayland connect failed");
            sup.shutdown();
            exit(1);
        }
    };

    // Background thread polls supervisor for child exit. calloop signalfd
    // would be cleaner, but a 500ms poll is plenty for exit detection.
    let river_pid = sup.pid();
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_millis(500));
            // Send signal 0 to probe existence.
            // SAFETY: kill(pid, 0) is async-signal-safe and side-effect-free.
            let alive = unsafe { libc::kill(river_pid as i32, 0) } == 0;
            if !alive {
                error!(pid = river_pid, "river process is gone; sola-river exiting");
                std::process::exit(1);
            }
        }
    });

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

    sup.shutdown();
}

fn init_tracing() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "sola_river=info".into());
    let _ = std::fs::create_dir_all("/opt/sola/log");
    let file_appender = tracing_appender::rolling::never("/opt/sola/log", "sola-river.log");
    let stderr_layer = tracing_subscriber::fmt::layer().with_writer(std::io::stderr);
    let file_layer = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_writer(file_appender);
    tracing_subscriber::registry()
        .with(filter)
        .with(stderr_layer)
        .with(file_layer)
        .init();
}
