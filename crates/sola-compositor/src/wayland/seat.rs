/// Wayland seat protocol handler.
///
/// A "seat" represents a group of input devices (keyboard, mouse, touch)
/// belonging to one user. Manages input focus — which surface receives events.
///
/// See: https://docs.rs/smithay/0.7.0/smithay/input/trait.SeatHandler.html
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
        let Some(surface) = focused else { return };

        // Find the app_id for the focused surface.
        use smithay::wayland::seat::WaylandFocus;
        let app_id = self.space.elements().find_map(|window| {
            window.wl_surface()
                .filter(|s| s.as_ref() == surface)
                .and_then(|_| State::app_id(window))
        });

        let Some(app_id) = app_id else { return };

        // Update MRU: move to front.
        self.mru_apps.retain(|id| id != &app_id);
        self.mru_apps.insert(0, app_id.clone());

        let _ = self.bus.emit_sticky(Topic::FocusChanged(app_id));
        crate::lifecycle::emit_apps_list(self);
    }
}

smithay::delegate_seat!(State);
