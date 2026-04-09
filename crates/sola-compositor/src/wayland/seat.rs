/// Wayland seat protocol handler.
///
/// A "seat" in Wayland represents a group of input devices (keyboard, mouse,
/// touch) that belong together — typically one physical user's setup. The seat
/// protocol lets the compositor tell clients what input capabilities are
/// available and manages input focus (which surface receives keyboard/pointer
/// events).
///
/// The associated types (`KeyboardFocus`, `PointerFocus`, `TouchFocus`) tell
/// Smithay what types can receive input focus. For now we use `WlSurface`
/// directly — later these may become an enum that also handles XWayland
/// surfaces or other focus targets.
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

    fn cursor_image(&mut self, _seat: &Seat<Self>, _image: CursorImageStatus) {
        // Will handle cursor rendering in a later phase.
    }

    fn focus_changed(&mut self, _seat: &Seat<Self>, _focused: Option<&Self::KeyboardFocus>) {
        // Will handle focus tracking in a later phase.
    }
}

smithay::delegate_seat!(Sola);
