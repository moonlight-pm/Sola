/// Focus target for the compositor's seat.
///
/// Wraps a `Window` and dispatches keyboard/pointer/touch events to the
/// correct underlying surface type. For X11 windows this means delegating
/// to `X11Surface` which handles SetInputFocus, WM_TAKE_FOCUS, and other
/// X11-specific input behavior. For Wayland windows it delegates to the
/// toplevel's `WlSurface`.
use std::borrow::Cow;
use std::fmt;

use smithay::desktop::Window;
use smithay::backend::input::KeyState;
use smithay::input::keyboard::{KeyboardTarget, KeysymHandle, ModifiersState};
use smithay::input::pointer::{
    AxisFrame, ButtonEvent, GestureHoldBeginEvent, GestureHoldEndEvent, GesturePinchBeginEvent,
    GesturePinchEndEvent, GesturePinchUpdateEvent, GestureSwipeBeginEvent, GestureSwipeEndEvent,
    GestureSwipeUpdateEvent, MotionEvent, PointerTarget, RelativeMotionEvent,
};
use smithay::input::touch::{
    DownEvent, OrientationEvent, ShapeEvent, TouchTarget, UpEvent,
};
use smithay::input::Seat;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::utils::{IsAlive, Serial};
use smithay::wayland::seat::WaylandFocus;
use smithay::xwayland::X11Surface;

use crate::state::State;

#[derive(Clone)]
pub enum FocusTarget {
    Window(Window),
}

impl fmt::Debug for FocusTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FocusTarget::Window(w) => f.debug_tuple("FocusTarget::Window").field(&w).finish(),
        }
    }
}

impl PartialEq for FocusTarget {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (FocusTarget::Window(a), FocusTarget::Window(b)) => a == b,
        }
    }
}

impl IsAlive for FocusTarget {
    fn alive(&self) -> bool {
        match self {
            FocusTarget::Window(w) => w.alive(),
        }
    }
}

impl From<Window> for FocusTarget {
    fn from(w: Window) -> Self {
        FocusTarget::Window(w)
    }
}

impl WaylandFocus for FocusTarget {
    fn wl_surface(&self) -> Option<Cow<'_, WlSurface>> {
        match self {
            FocusTarget::Window(w) => w.wl_surface(),
        }
    }
}

/// Helper: get the X11Surface from a Window, if it is an X11 window.
fn x11(w: &Window) -> Option<X11Surface> {
    w.x11_surface().cloned()
}

/// Helper: get the WlSurface from a Window's toplevel (Wayland window).
fn wl(w: &Window) -> Option<WlSurface> {
    w.toplevel().map(|t| t.wl_surface().clone())
}

// -- KeyboardTarget --

impl KeyboardTarget<State> for FocusTarget {
    fn enter(
        &self,
        seat: &Seat<State>,
        data: &mut State,
        keys: Vec<KeysymHandle<'_>>,
        serial: Serial,
    ) {
        let FocusTarget::Window(w) = self;
        if let Some(x11) = x11(w) {
            KeyboardTarget::enter(&x11, seat, data, keys, serial);
        } else if let Some(surface) = wl(w) {
            KeyboardTarget::enter(&surface, seat, data, keys, serial);
        }
    }

    fn leave(&self, seat: &Seat<State>, data: &mut State, serial: Serial) {
        let FocusTarget::Window(w) = self;
        if let Some(x11) = x11(w) {
            KeyboardTarget::leave(&x11, seat, data, serial);
        } else if let Some(surface) = wl(w) {
            KeyboardTarget::leave(&surface, seat, data, serial);
        }
    }

    fn key(
        &self,
        seat: &Seat<State>,
        data: &mut State,
        key: KeysymHandle<'_>,
        state: KeyState,
        serial: Serial,
        time: u32,
    ) {
        let FocusTarget::Window(w) = self;
        if let Some(x11) = x11(w) {
            KeyboardTarget::key(&x11, seat, data, key, state, serial, time);
        } else if let Some(surface) = wl(w) {
            KeyboardTarget::key(&surface, seat, data, key, state, serial, time);
        }
    }

    fn modifiers(
        &self,
        seat: &Seat<State>,
        data: &mut State,
        modifiers: ModifiersState,
        serial: Serial,
    ) {
        let FocusTarget::Window(w) = self;
        if let Some(x11) = x11(w) {
            KeyboardTarget::modifiers(&x11, seat, data, modifiers, serial);
        } else if let Some(surface) = wl(w) {
            KeyboardTarget::modifiers(&surface, seat, data, modifiers, serial);
        }
    }
}

// -- PointerTarget --

impl PointerTarget<State> for FocusTarget {
    fn enter(&self, seat: &Seat<State>, data: &mut State, event: &MotionEvent) {
        let FocusTarget::Window(w) = self;
        if let Some(x11) = x11(w) {
            PointerTarget::enter(&x11, seat, data, event);
        } else if let Some(surface) = wl(w) {
            PointerTarget::enter(&surface, seat, data, event);
        }
    }

    fn motion(&self, seat: &Seat<State>, data: &mut State, event: &MotionEvent) {
        let FocusTarget::Window(w) = self;
        if let Some(x11) = x11(w) {
            PointerTarget::motion(&x11, seat, data, event);
        } else if let Some(surface) = wl(w) {
            PointerTarget::motion(&surface, seat, data, event);
        }
    }

    fn relative_motion(
        &self,
        seat: &Seat<State>,
        data: &mut State,
        event: &RelativeMotionEvent,
    ) {
        let FocusTarget::Window(w) = self;
        if let Some(x11) = x11(w) {
            PointerTarget::relative_motion(&x11, seat, data, event);
        } else if let Some(surface) = wl(w) {
            PointerTarget::relative_motion(&surface, seat, data, event);
        }
    }

    fn button(&self, seat: &Seat<State>, data: &mut State, event: &ButtonEvent) {
        let FocusTarget::Window(w) = self;
        if let Some(x11) = x11(w) {
            PointerTarget::button(&x11, seat, data, event);
        } else if let Some(surface) = wl(w) {
            PointerTarget::button(&surface, seat, data, event);
        }
    }

    fn axis(&self, seat: &Seat<State>, data: &mut State, frame: AxisFrame) {
        let FocusTarget::Window(w) = self;
        if let Some(x11) = x11(w) {
            PointerTarget::axis(&x11, seat, data, frame);
        } else if let Some(surface) = wl(w) {
            PointerTarget::axis(&surface, seat, data, frame);
        }
    }

    fn frame(&self, seat: &Seat<State>, data: &mut State) {
        let FocusTarget::Window(w) = self;
        if let Some(x11) = x11(w) {
            PointerTarget::frame(&x11, seat, data);
        } else if let Some(surface) = wl(w) {
            PointerTarget::frame(&surface, seat, data);
        }
    }

    fn leave(&self, seat: &Seat<State>, data: &mut State, serial: Serial, time: u32) {
        let FocusTarget::Window(w) = self;
        if let Some(x11) = x11(w) {
            PointerTarget::leave(&x11, seat, data, serial, time);
        } else if let Some(surface) = wl(w) {
            PointerTarget::leave(&surface, seat, data, serial, time);
        }
    }

    fn gesture_swipe_begin(
        &self,
        seat: &Seat<State>,
        data: &mut State,
        event: &GestureSwipeBeginEvent,
    ) {
        let FocusTarget::Window(w) = self;
        if let Some(x11) = x11(w) {
            PointerTarget::gesture_swipe_begin(&x11, seat, data, event);
        } else if let Some(surface) = wl(w) {
            PointerTarget::gesture_swipe_begin(&surface, seat, data, event);
        }
    }

    fn gesture_swipe_update(
        &self,
        seat: &Seat<State>,
        data: &mut State,
        event: &GestureSwipeUpdateEvent,
    ) {
        let FocusTarget::Window(w) = self;
        if let Some(x11) = x11(w) {
            PointerTarget::gesture_swipe_update(&x11, seat, data, event);
        } else if let Some(surface) = wl(w) {
            PointerTarget::gesture_swipe_update(&surface, seat, data, event);
        }
    }

    fn gesture_swipe_end(
        &self,
        seat: &Seat<State>,
        data: &mut State,
        event: &GestureSwipeEndEvent,
    ) {
        let FocusTarget::Window(w) = self;
        if let Some(x11) = x11(w) {
            PointerTarget::gesture_swipe_end(&x11, seat, data, event);
        } else if let Some(surface) = wl(w) {
            PointerTarget::gesture_swipe_end(&surface, seat, data, event);
        }
    }

    fn gesture_pinch_begin(
        &self,
        seat: &Seat<State>,
        data: &mut State,
        event: &GesturePinchBeginEvent,
    ) {
        let FocusTarget::Window(w) = self;
        if let Some(x11) = x11(w) {
            PointerTarget::gesture_pinch_begin(&x11, seat, data, event);
        } else if let Some(surface) = wl(w) {
            PointerTarget::gesture_pinch_begin(&surface, seat, data, event);
        }
    }

    fn gesture_pinch_update(
        &self,
        seat: &Seat<State>,
        data: &mut State,
        event: &GesturePinchUpdateEvent,
    ) {
        let FocusTarget::Window(w) = self;
        if let Some(x11) = x11(w) {
            PointerTarget::gesture_pinch_update(&x11, seat, data, event);
        } else if let Some(surface) = wl(w) {
            PointerTarget::gesture_pinch_update(&surface, seat, data, event);
        }
    }

    fn gesture_pinch_end(
        &self,
        seat: &Seat<State>,
        data: &mut State,
        event: &GesturePinchEndEvent,
    ) {
        let FocusTarget::Window(w) = self;
        if let Some(x11) = x11(w) {
            PointerTarget::gesture_pinch_end(&x11, seat, data, event);
        } else if let Some(surface) = wl(w) {
            PointerTarget::gesture_pinch_end(&surface, seat, data, event);
        }
    }

    fn gesture_hold_begin(
        &self,
        seat: &Seat<State>,
        data: &mut State,
        event: &GestureHoldBeginEvent,
    ) {
        let FocusTarget::Window(w) = self;
        if let Some(x11) = x11(w) {
            PointerTarget::gesture_hold_begin(&x11, seat, data, event);
        } else if let Some(surface) = wl(w) {
            PointerTarget::gesture_hold_begin(&surface, seat, data, event);
        }
    }

    fn gesture_hold_end(
        &self,
        seat: &Seat<State>,
        data: &mut State,
        event: &GestureHoldEndEvent,
    ) {
        let FocusTarget::Window(w) = self;
        if let Some(x11) = x11(w) {
            PointerTarget::gesture_hold_end(&x11, seat, data, event);
        } else if let Some(surface) = wl(w) {
            PointerTarget::gesture_hold_end(&surface, seat, data, event);
        }
    }
}

// -- TouchTarget --

impl TouchTarget<State> for FocusTarget {
    fn down(
        &self,
        seat: &Seat<State>,
        data: &mut State,
        event: &DownEvent,
        seq: Serial,
    ) {
        let FocusTarget::Window(w) = self;
        if let Some(x11) = x11(w) {
            TouchTarget::down(&x11, seat, data, event, seq);
        } else if let Some(surface) = wl(w) {
            TouchTarget::down(&surface, seat, data, event, seq);
        }
    }

    fn up(
        &self,
        seat: &Seat<State>,
        data: &mut State,
        event: &UpEvent,
        seq: Serial,
    ) {
        let FocusTarget::Window(w) = self;
        if let Some(x11) = x11(w) {
            TouchTarget::up(&x11, seat, data, event, seq);
        } else if let Some(surface) = wl(w) {
            TouchTarget::up(&surface, seat, data, event, seq);
        }
    }

    fn motion(
        &self,
        seat: &Seat<State>,
        data: &mut State,
        event: &smithay::input::touch::MotionEvent,
        seq: Serial,
    ) {
        let FocusTarget::Window(w) = self;
        if let Some(x11) = x11(w) {
            TouchTarget::motion(&x11, seat, data, event, seq);
        } else if let Some(surface) = wl(w) {
            TouchTarget::motion(&surface, seat, data, event, seq);
        }
    }

    fn frame(&self, seat: &Seat<State>, data: &mut State, seq: Serial) {
        let FocusTarget::Window(w) = self;
        if let Some(x11) = x11(w) {
            TouchTarget::frame(&x11, seat, data, seq);
        } else if let Some(surface) = wl(w) {
            TouchTarget::frame(&surface, seat, data, seq);
        }
    }

    fn cancel(&self, seat: &Seat<State>, data: &mut State, seq: Serial) {
        let FocusTarget::Window(w) = self;
        if let Some(x11) = x11(w) {
            TouchTarget::cancel(&x11, seat, data, seq);
        } else if let Some(surface) = wl(w) {
            TouchTarget::cancel(&surface, seat, data, seq);
        }
    }

    fn shape(
        &self,
        seat: &Seat<State>,
        data: &mut State,
        event: &ShapeEvent,
        seq: Serial,
    ) {
        let FocusTarget::Window(w) = self;
        if let Some(x11) = x11(w) {
            TouchTarget::shape(&x11, seat, data, event, seq);
        } else if let Some(surface) = wl(w) {
            TouchTarget::shape(&surface, seat, data, event, seq);
        }
    }

    fn orientation(
        &self,
        seat: &Seat<State>,
        data: &mut State,
        event: &OrientationEvent,
        seq: Serial,
    ) {
        let FocusTarget::Window(w) = self;
        if let Some(x11) = x11(w) {
            TouchTarget::orientation(&x11, seat, data, event, seq);
        } else if let Some(surface) = wl(w) {
            TouchTarget::orientation(&surface, seat, data, event, seq);
        }
    }
}
