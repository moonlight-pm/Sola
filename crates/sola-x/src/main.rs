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
    let file_appender = tracing_appender::rolling::never(log_dir, "sola-x.log");

    let stderr_layer = tracing_subscriber::fmt::layer().with_writer(std::io::stderr);
    let file_layer = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_writer(file_appender);

    tracing_subscriber::registry()
        .with(filter)
        .with(stderr_layer)
        .with(file_layer)
        .init();

    tracing::info!("sola-x starting (logs → stderr + {log_dir}/sola-x.log)");

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
    match sola_bus::BusClient::connect() {
        Ok(client) => {
            tracing::info!("connected to sola bus");
            state.bus = Some(client);
        }
        Err(e) => {
            tracing::warn!("bus not available, running without: {e}");
        }
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

        // Client side: dispatch sola-compositor's events.
        if let Some(client) = &mut state.client {
            if client.dispatch().is_err() {
                tracing::warn!("compositor connection lost, will reconnect");
                state.client = None;
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
fn connect_to_compositor(state: &mut State) {
    if state.client.is_some() {
        return;
    }
    if let Some(conn) = client::ClientConnection::connect() {
        state.client = Some(conn);
    }
}

/// Process pending bus messages.
fn process_bus(state: &mut State) {
    let Some(bus) = &state.bus else { return };
    while let Some(_msg) = bus.try_recv() {
        // Bus messages processed here as needed.
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
