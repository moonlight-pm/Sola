/// XDG decoration protocol handler.
///
/// Controls whether window decorations (title bar, borders) are drawn by
/// the client (CSD) or the compositor (SSD). We always request SSD so
/// State controls all chrome. Clients that support this protocol will
/// stop drawing their own title bars.
///
/// Note: we don't actually render server-side decorations yet — clients
/// just get told not to draw theirs. Decorations will be part of the
/// WebView shell chrome in later phases.
///
/// See: https://wayland.app/protocols/xdg-decoration-unstable-v1
use smithay::wayland::shell::xdg::ToplevelSurface;
use smithay::wayland::shell::xdg::decoration::XdgDecorationHandler;
use smithay::reexports::wayland_protocols::xdg::decoration::zv1::server::zxdg_toplevel_decoration_v1::Mode;

use crate::state::State;

impl XdgDecorationHandler for State {
    fn new_decoration(&mut self, toplevel: ToplevelSurface) {
        // Tell the client we want server-side decorations.
        toplevel.with_pending_state(|state| {
            state.decoration_mode = Some(Mode::ServerSide);
        });
        toplevel.send_configure();
    }

    fn request_mode(&mut self, toplevel: ToplevelSurface, _mode: Mode) {
        // Always override to SSD regardless of what the client requests.
        toplevel.with_pending_state(|state| {
            state.decoration_mode = Some(Mode::ServerSide);
        });
        toplevel.send_configure();
    }

    fn unset_mode(&mut self, toplevel: ToplevelSurface) {
        toplevel.with_pending_state(|state| {
            state.decoration_mode = Some(Mode::ServerSide);
        });
        toplevel.send_configure();
    }
}

smithay::delegate_xdg_decoration!(State);
