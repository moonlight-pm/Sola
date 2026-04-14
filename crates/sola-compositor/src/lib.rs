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
pub mod wallpaper;
pub mod wayland;

use smithay::backend::session::Session;
use smithay::reexports::calloop::generic::Generic;
use smithay::reexports::calloop::{EventLoop, Interest, Mode, PostAction};
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

    // -- Wallpaper --
    if let Some(buffer) = wallpaper::load() {
        state.wallpaper_buffer = Some(buffer);
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
    if let Err(e) = state.bus.connect() {
        tracing::warn!("bus not available, running without: {e}");
    }

    // Register bus as a calloop event source so messages are dispatched
    // through the event loop instead of polled each frame.
    if let Some(Ok(notify)) = state.bus.try_clone_notify() {
        let source = Generic::new(notify, Interest::READ, Mode::Level);
        event_loop
            .handle()
            .insert_source(source, |_, _, state: &mut State| {
                lifecycle::dispatch_bus(state);
                Ok(PostAction::Continue)
            })
            .map_err(|e| CompositorError::EventLoop(format!("bus source: {e}")))?;
    }

    // -- Wayland socket --
    let (socket_name, _socket_fd) = backend::socket::listen(&event_loop.handle(), None)?;
    unsafe { std::env::set_var("WAYLAND_DISPLAY", &socket_name) };

    // -- Run --
    lifecycle::run_loop(&mut state, &mut display, &mut event_loop)?;

    // -- Shutdown --
    lifecycle::shutdown(state, display, event_loop);

    Ok(())
}
