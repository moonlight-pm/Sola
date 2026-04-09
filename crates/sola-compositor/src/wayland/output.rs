/// Wayland output protocol handler.
///
/// The `wl_output` protocol advertises physical displays to clients. When a
/// client binds the output global, it receives information about the display's
/// resolution, physical size, make/model, and current mode. This lets clients
/// adapt their rendering (e.g., HiDPI scaling, choosing which monitor to
/// fullscreen on).
///
/// `OutputHandler` is called when a client binds a new `wl_output` instance.
/// The actual output creation and global advertisement happens in the
/// `output/scan` module.
///
/// See: https://docs.rs/smithay/0.7.0/smithay/wayland/output/trait.OutputHandler.html
use smithay::output::Output;
use smithay::reexports::wayland_server::protocol::wl_output::WlOutput;
use smithay::wayland::output::OutputHandler;

use crate::state::Sola;

impl OutputHandler for Sola {
    fn output_bound(&mut self, _output: Output, _wl_output: WlOutput) {
        // Will handle per-client output state in a later phase.
    }
}

// The delegate macro is in wayland/mod.rs since it doesn't need a handler trait
// import — it just wires up dispatch.
smithay::delegate_output!(Sola);
