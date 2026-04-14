/// sola-x — XWayland host for the Sola desktop shell.
///
/// Manages XWayland's lifecycle independently of the sola compositor.
/// X11 apps connect to XWayland, which connects to sola-x, which
/// presents their content to sola-compositor as proxy Wayland surfaces.
///
/// When sola-compositor restarts, sola-x reconnects and re-creates
/// proxy surfaces. XWayland and X11 apps are unaffected.
mod bridge;
mod client;
mod error;
mod server;
mod state;

use smithay::reexports::calloop::EventLoop;
use smithay::reexports::wayland_server::Display;

use error::Error;
use state::State;

fn main() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "sola_x=info,smithay=error".into());

    let log_dir = "/opt/sola/log";
    let _ = std::fs::create_dir_all(log_dir);
    let file_appender = tracing_appender::rolling::never(log_dir, "sola.log");

    let stderr_layer = tracing_subscriber::fmt::layer().with_writer(std::io::stderr);
    let file_layer = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_writer(file_appender);

    tracing_subscriber::registry()
        .with(filter)
        .with(stderr_layer)
        .with(file_layer)
        .init();

    tracing::info!("sola-x starting");

    if let Err(err) = run() {
        tracing::error!(%err, "sola-x exited with error");
        std::process::exit(1);
    }
}

use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

fn run() -> Result<(), Error> {
    let mut event_loop: EventLoop<State> =
        EventLoop::try_new().map_err(|e| Error::EventLoop(e.to_string()))?;
    let mut display: Display<State> =
        Display::new().map_err(|e| Error::Display(e.to_string()))?;

    let mut state = State::new(display.handle(), event_loop.handle());

    // -- Bus --
    if let Err(e) = state.bus.connect() {
        tracing::warn!("bus not available, running without: {e}");
    }

    // -- Wayland socket for XWayland --
    let socket_name = setup_wayland_socket(&event_loop, &mut state)?;
    unsafe { std::env::set_var("WAYLAND_DISPLAY", &socket_name) };

    // -- XWayland --
    server::xwayland::setup(&mut state, &event_loop)?;

    // -- Client connection to sola-compositor --
    // Retry until available (compositor may not be ready yet).
    connect_to_compositor(&mut state);

    // -- Main loop --
    tracing::info!("entering event loop");
    while state.running {
        process_bus(&mut state);

        // Server side: dispatch XWayland's Wayland messages.
        display
            .dispatch_clients(&mut state)
            .map_err(|e| Error::Display(e.to_string()))?;
        display
            .flush_clients()
            .map_err(|e| Error::Display(e.to_string()))?;

        // Client side: dispatch sola-compositor's events and forward input.
        if let Some(client) = &mut state.client {
            if client.dispatch().is_err() {
                tracing::warn!("compositor connection lost, will reconnect");
                state.client = None;
            } else {
                inject_input(&mut state);
                apply_pending_configures(&mut state);
                // Flush injected input events to XWayland immediately.
                let _ = display.flush_clients();
            }
        } else {
            // Try to reconnect periodically.
            connect_to_compositor(&mut state);
        }

        event_loop
            .dispatch(Some(std::time::Duration::from_millis(16)), &mut state)
            .map_err(|e| Error::EventLoop(e.to_string()))?;
    }

    Ok(())
}

/// Try to connect to sola-compositor as a Wayland client.
/// On reconnection, re-creates proxy surfaces for all existing X11 windows
/// and re-emits any user-locked geometries so the compositor's
/// `new_toplevel` fullscreen default doesn't override the user's zones.
fn connect_to_compositor(state: &mut State) {
    use sola_bus::topics::{Topic, WindowGeometry};

    if state.client.is_some() {
        return;
    }
    if let Some(mut conn) = client::ClientConnection::connect() {
        // Gather existing X11 windows to re-create proxies.
        let windows: Vec<(u32, String, String)> = state
            .x11_windows
            .iter()
            .map(|(&id, info)| (id, info.title.clone(), info.class.clone()))
            .collect();

        conn.recreate_proxies(&windows);
        state.client = Some(conn);

        // Re-emit locked sizes so the compositor re-syncs to the user's zone
        // instead of leaving each newly-created proxy at its fullscreen default.
        for info in state.x11_windows.values() {
            let Some(&(w, h)) = state.user_locked_sizes.get(&info.class) else {
                continue;
            };
            let geo = info.surface.geometry();
            let _ = state.bus.emit(Topic::SetWindowGeometry(WindowGeometry {
                app_id: info.class.clone(),
                x: geo.loc.x,
                y: geo.loc.y,
                width: w,
                height: h,
            }));
            tracing::info!(
                app_id = %info.class,
                width = w,
                height = h,
                "re-emitted locked geometry after reconnect"
            );
        }
    }
}

/// Find the server-side WlSurface for an X11 window ID.
fn server_surface_for_x11(state: &State, x11_id: u32) -> Option<smithay::reexports::wayland_server::protocol::wl_surface::WlSurface> {
    state.surface_to_x11.iter()
        .find(|&(_, &id)| id == x11_id)
        .map(|(surface, _)| surface.clone())
}

/// Inject input events received from sola-compositor into XWayland's seat.
fn inject_input(state: &mut State) {
    use smithay::input::keyboard::FilterResult;
    use smithay::input::pointer::{ButtonEvent, MotionEvent, AxisFrame};
    use smithay::utils::SERIAL_COUNTER;

    let events = match &mut state.client {
        Some(client) => client.drain_input(),
        None => return,
    };

    if events.is_empty() {
        return;
    }

    let pointer = state.seat.get_pointer().unwrap();
    let keyboard = state.seat.get_keyboard().unwrap();

    for event in events {
        match event {
            client::InputEvent::PointerEnter { x11_id, x, y } => {
                let focus = server_surface_for_x11(state, x11_id)
                    .map(|s| (s, (0.0, 0.0).into()));
                let serial = SERIAL_COUNTER.next_serial();
                pointer.motion(
                    state,
                    focus,
                    &MotionEvent {
                        location: (x, y).into(),
                        serial,
                        time: 0,
                    },
                );
                pointer.frame(state);
            }
            client::InputEvent::PointerLeave => {
                let serial = SERIAL_COUNTER.next_serial();
                pointer.motion(
                    state,
                    None,
                    &MotionEvent {
                        location: (0.0, 0.0).into(),
                        serial,
                        time: 0,
                    },
                );
                pointer.frame(state);
            }
            client::InputEvent::PointerMotion { x, y, time } => {
                // Keep current focus, just update position.
                let serial = SERIAL_COUNTER.next_serial();
                let current_focus = pointer.current_focus().map(|s| (s, (0.0, 0.0).into()));
                pointer.motion(
                    state,
                    current_focus,
                    &MotionEvent {
                        location: (x, y).into(),
                        serial,
                        time,
                    },
                );
                pointer.frame(state);
            }
            client::InputEvent::PointerButton { button, pressed, time } => {
                use smithay::backend::input::ButtonState;
                let serial = SERIAL_COUNTER.next_serial();
                pointer.button(
                    state,
                    &ButtonEvent {
                        serial,
                        time,
                        button,
                        state: if pressed {
                            ButtonState::Pressed
                        } else {
                            ButtonState::Released
                        },
                    },
                );
                pointer.frame(state);
            }
            client::InputEvent::PointerAxis { value, time } => {
                let frame = AxisFrame::new(time)
                    .value(smithay::backend::input::Axis::Vertical, value);
                pointer.axis(state, frame);
                pointer.frame(state);
            }
            client::InputEvent::Key { key, pressed, time } => {
                use smithay::backend::input::KeyState;
                use smithay::input::keyboard::Keycode;
                let serial = SERIAL_COUNTER.next_serial();
                keyboard.input::<(), _>(
                    state,
                    Keycode::new(key + 8), // evdev → xkb offset
                    if pressed { KeyState::Pressed } else { KeyState::Released },
                    serial,
                    time,
                    |_, _, _| FilterResult::Forward,
                );
            }
        }
    }
}

/// Process pending bus messages.
fn process_bus(state: &mut State) {
    use sola_bus::topics::Topic;

    while let Some(msg) = state.bus.try_recv() {
        let Some(topic) = Topic::parse(&msg) else { continue };
        match topic {
            Topic::OutputGeometry(geo) => {
                update_output_mode(state, geo.width, geo.height);
            }
            // The bus doesn't echo messages to the sender, so any
            // SetWindowGeometry we see came from another client — in
            // practice, sola's zoning code. Record the commanded size
            // so we can enforce it against X11 clients and against
            // compositor configures on reconnect.
            Topic::SetWindowGeometry(geo) => {
                let size = (geo.width, geo.height);
                let prev = state.user_locked_sizes.insert(geo.app_id.clone(), size);
                if prev != Some(size) {
                    tracing::info!(
                        app_id = %geo.app_id,
                        width = geo.width,
                        height = geo.height,
                        "locking X11 window size (user zone)"
                    );
                }
            }
            _ => {}
        }
    }
}

/// Update the virtual output's mode so XWayland (via XRandR) exposes
/// the real screen size to X11 clients.
fn update_output_mode(state: &mut State, width: i32, height: i32) {
    use smithay::output::Mode as WlMode;

    let mode = WlMode {
        size: (width, height).into(),
        refresh: 60_000,
    };
    state.output.change_current_state(Some(mode), None, None, None);
    state.output.set_preferred(mode);
    tracing::info!(width, height, "updated virtual output mode from compositor");
}

/// Apply any queued resize requests (from proxy xdg_toplevel configures)
/// to their matching X11 windows via xwm.
///
/// For user-locked apps, only configures whose size matches the user's
/// zone are applied — others are ignored to prevent the compositor from
/// overriding the zone (e.g. via new_toplevel's fullscreen default on
/// reconnect).
fn apply_pending_configures(state: &mut State) {
    use smithay::utils::Rectangle;

    let pending = match &mut state.client {
        Some(client) => client.drain_configures(),
        None => return,
    };

    for conf in pending {
        let Some(info) = state.x11_windows.get(&conf.x11_id) else {
            continue;
        };

        if let Some(&(lw, lh)) = state.user_locked_sizes.get(&info.class) {
            if conf.width as i32 != lw || conf.height as i32 != lh {
                tracing::info!(
                    app_id = %info.class,
                    configure_w = conf.width,
                    configure_h = conf.height,
                    locked_w = lw,
                    locked_h = lh,
                    "ignoring compositor configure for user-locked app"
                );
                continue;
            }
        }

        let geo = info.surface.geometry();
        let new_geo = Rectangle::new(
            geo.loc,
            (conf.width as i32, conf.height as i32).into(),
        );
        if let Err(err) = info.surface.configure(Some(new_geo)) {
            tracing::warn!(x11_id = conf.x11_id, ?err, "failed to configure X11 window");
        }
    }
}

/// Create a Wayland socket for XWayland to connect to.
/// Uses a distinct name (wayland-x0) to avoid conflicting with sola-compositor's socket.
fn setup_wayland_socket(
    event_loop: &EventLoop<'static, State>,
    _state: &mut State,
) -> Result<String, Error> {
    use smithay::wayland::socket::ListeningSocketSource;

    let listener = ListeningSocketSource::with_name("wayland-x0")
        .or_else(|_| ListeningSocketSource::new_auto())
        .map_err(|e| Error::Socket(e.to_string()))?;
    let socket_name = listener.socket_name().to_string_lossy().into_owned();

    event_loop
        .handle()
        .insert_source(listener, |client_stream, _, state| {
            let client_state = std::sync::Arc::new(server::compositor::ClientState::default());
            match state.display_handle.insert_client(client_stream, client_state) {
                Ok(_) => tracing::info!("XWayland connected as Wayland client"),
                Err(err) => tracing::error!(?err, "failed to accept XWayland client"),
            }
        })
        .map_err(|e| Error::EventLoop(e.to_string()))?;

    tracing::info!(%socket_name, "Wayland socket for XWayland");
    Ok(socket_name)
}
