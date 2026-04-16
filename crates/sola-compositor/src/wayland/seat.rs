use smithay::input::pointer::CursorImageStatus;
/// Wayland seat protocol handler.
///
/// A "seat" represents a group of input devices (keyboard, mouse, touch)
/// belonging to one user. Manages input focus — which surface receives events.
use smithay::input::{Seat, SeatHandler, SeatState};

use crate::focus::FocusTarget;
use crate::state::State;

impl SeatHandler for State {
    type KeyboardFocus = FocusTarget;
    type PointerFocus = FocusTarget;
    type TouchFocus = FocusTarget;

    fn seat_state(&mut self) -> &mut SeatState<Self> {
        &mut self.seat_state
    }

    fn cursor_image(&mut self, _seat: &Seat<Self>, _image: CursorImageStatus) {}

    fn focus_changed(&mut self, _seat: &Seat<Self>, _focused: Option<&Self::KeyboardFocus>) {}
}

smithay::delegate_seat!(State);
