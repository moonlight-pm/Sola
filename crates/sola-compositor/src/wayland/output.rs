/// Wayland output protocol handler.
///
/// `wl_output` advertises physical displays to clients — resolution,
/// physical size, make/model, current mode.
///
/// See: https://docs.rs/smithay/0.7.0/smithay/wayland/output/trait.OutputHandler.html
use smithay::output::Output;
use smithay::reexports::wayland_server::protocol::wl_output::WlOutput;
use smithay::wayland::output::OutputHandler;

use crate::state::Sola;

impl OutputHandler for Sola {
    fn output_bound(&mut self, _output: Output, _wl_output: WlOutput) {}
}

smithay::delegate_output!(Sola);
