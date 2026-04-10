/// Sola compositor — a Wayland compositor built on Smithay.
///
/// This crate contains the compositor core: backend initialization,
/// Wayland protocol handling, output management, and rendering.

pub mod backend;
pub mod cursor;
pub mod error;
pub mod input;
mod lifecycle;
pub mod output;
pub mod state;
pub mod types;
pub mod wayland;

use smithay::backend::session::Session;
use smithay::reexports::calloop::EventLoop;
use smithay::reexports::wayland_server::Display;

use error::CompositorError;

pub use state::State;

/// Start the compositor.
///
/// Initializes all subsystems in order, enters the event loop, then
/// performs graceful shutdown.
pub fn run() -> Result<(), CompositorError> {
    tracing::info!("sola compositor starting");

    let mut event_loop: EventLoop<State> =
        EventLoop::try_new().map_err(|e| CompositorError::EventLoop(e.to_string()))?;
    let mut display: Display<State> =
        Display::new().map_err(|e| CompositorError::Display(e.to_string()))?;

    // -- Session --
    let (session, session_notifier) = backend::session::create()?;
    let seat_name = session.seat();

    // -- GPU --
    let primary_gpu = backend::gpu::find_primary(&seat_name)?;
    let gpu_manager = backend::gpu::create_manager()?;

    let mut state = State::new(display.handle(), event_loop.handle(), session, gpu_manager, primary_gpu);

    // -- Cursor --
    if let Some((buffer, hotspot)) = cursor::load_default() {
        state.cursor_buffer = Some(buffer);
        state.cursor_hotspot = hotspot;
    } else {
        tracing::warn!("failed to load cursor from xcursor theme");
    }

    // -- Session notifier --
    event_loop
        .handle()
        .insert_source(session_notifier, |_, _, _| {})
        .map_err(|e| CompositorError::EventLoop(format!("session source: {e}")))?;

    // -- Devices --
    backend::udev::setup(&mut state, &event_loop)?;

    // -- Input --
    backend::input::setup(&event_loop.handle(), &state.session)?;

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

    // -- Wayland socket --
    let (socket_name, _socket_fd) = backend::socket::listen(&event_loop.handle(), None)?;
    unsafe { std::env::set_var("WAYLAND_DISPLAY", &socket_name) };

    // -- XWayland --
    wayland::xwayland::setup(&mut state, &event_loop)?;

    // -- Run --
    lifecycle::run_loop(&mut state, &mut display, &mut event_loop)?;

    // -- Shutdown --
    lifecycle::shutdown(state, display, event_loop);

    Ok(())
}
