/// Wayland seat protocol handler.
///
/// A "seat" represents a group of input devices (keyboard, mouse, touch)
/// belonging to one user. Manages input focus — which surface receives events.
use smithay::input::{Seat, SeatHandler, SeatState};
use smithay::input::pointer::CursorImageStatus;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;

use sola_bus::topics::Topic;

use crate::state::State;

impl SeatHandler for State {
    type KeyboardFocus = WlSurface;
    type PointerFocus = WlSurface;
    type TouchFocus = WlSurface;

    fn seat_state(&mut self) -> &mut SeatState<Self> {
        &mut self.seat_state
    }

    fn cursor_image(&mut self, _seat: &Seat<Self>, _image: CursorImageStatus) {}

    fn focus_changed(&mut self, _seat: &Seat<Self>, focused: Option<&Self::KeyboardFocus>) {
        // Don't emit FocusChanged when applying a shell Focus command —
        // the shell already knows the focus it set.
        if self.applying_shell_focus {
            return;
        }

        let Some(surface) = focused else { return };

        use smithay::wayland::seat::WaylandFocus;
        let app_id = self.space.elements().find_map(|window| {
            window.wl_surface()
                .filter(|s| s.as_ref() == surface)
                .and_then(|_| State::app_id(window))
        });

        let Some(app_id) = app_id else { return };

        let _ = self.bus.emit_sticky(Topic::FocusChanged(app_id));
    }
}

smithay::delegate_seat!(State);
