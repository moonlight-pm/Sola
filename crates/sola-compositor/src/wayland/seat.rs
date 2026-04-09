/// Wayland seat protocol handler.
///
/// A "seat" represents a group of input devices (keyboard, mouse, touch)
/// belonging to one user. Manages input focus — which surface receives events.
///
/// See: https://docs.rs/smithay/0.7.0/smithay/input/trait.SeatHandler.html
use smithay::input::{Seat, SeatHandler, SeatState};
use smithay::input::pointer::CursorImageStatus;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;

use crate::state::Sola;

impl SeatHandler for Sola {
    type KeyboardFocus = WlSurface;
    type PointerFocus = WlSurface;
    type TouchFocus = WlSurface;

    fn seat_state(&mut self) -> &mut SeatState<Self> {
        &mut self.seat_state
    }

    fn cursor_image(&mut self, _seat: &Seat<Self>, _image: CursorImageStatus) {}

    fn focus_changed(&mut self, _seat: &Seat<Self>, _focused: Option<&Self::KeyboardFocus>) {}
}

smithay::delegate_seat!(Sola);
