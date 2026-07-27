//! CGEvent injection (macOS) with a Linux stub for compile/test.
//!
//! On macOS this posts synthetic mouse/keyboard events via CoreGraphics.
//! Accessibility permission is required (see README).

use crate::keymap::{self, CgKeyCode};
use crate::protocol::Packet;
use tracing::{debug, warn};

/// High-level inject API used by the UDP agent loop.
pub trait Injector {
    fn warp(&mut self, x: i32, y: i32);
    fn button(&mut self, button: u8, pressed: bool);
    fn key(&mut self, keycode: u32, pressed: bool);
    fn scroll(&mut self, dx: f32, dy: f32);
    fn leave(&mut self);

    /// Dispatch a decoded packet.
    fn handle(&mut self, packet: &Packet) {
        match packet {
            Packet::Enter { x, y, edge } => {
                // Always absolute warp to the server's enter point (not residual
                // Mac cursor position). Warp twice to beat CG association races.
                tracing::info!(x, y, ?edge, "enter → warp");
                self.warp(*x, *y);
                self.warp(*x, *y);
            }
            Packet::Leave => {
                debug!("leave");
                self.leave();
            }
            Packet::Motion { x, y } => {
                self.warp(*x, *y);
            }
            Packet::Button { button, pressed } => {
                self.button(*button, *pressed != 0);
            }
            Packet::Key { keycode, pressed } => {
                self.key(*keycode, *pressed != 0);
            }
            Packet::Scroll { dx, dy } => {
                self.scroll(*dx, *dy);
            }
            Packet::Modifiers { mask } => {
                // v1: modifiers arrive as KEY press/release on Meta/Alt/Ctrl/Shift.
                // Explicit mask is optional; log for diagnostics.
                debug!(mask, "modifiers packet (no-op in v1; keys carry state)");
            }
        }
    }
}

/// Production injector (CoreGraphics on macOS, logging stub elsewhere).
pub struct CgInjector {
    /// Track pressed keys for optional stuck-key recovery on Leave.
    pressed_keys: Vec<u32>,
}

impl CgInjector {
    pub fn new() -> Self {
        Self {
            pressed_keys: Vec::new(),
        }
    }
}

impl Default for CgInjector {
    fn default() -> Self {
        Self::new()
    }
}

impl Injector for CgInjector {
    fn warp(&mut self, x: i32, y: i32) {
        platform::warp_cursor(x, y);
    }

    fn button(&mut self, button: u8, pressed: bool) {
        platform::mouse_button(button, pressed);
    }

    fn key(&mut self, keycode: u32, pressed: bool) {
        let Some(cg) = keymap::linux_to_cg(keycode) else {
            warn!(
                keycode,
                name = keymap::linux_key_name(keycode).unwrap_or("?"),
                "unmapped Linux keycode; dropping"
            );
            return;
        };
        if pressed {
            if !self.pressed_keys.contains(&keycode) {
                self.pressed_keys.push(keycode);
            }
        } else {
            self.pressed_keys.retain(|&k| k != keycode);
        }
        platform::key_event(cg, pressed);
    }

    fn scroll(&mut self, dx: f32, dy: f32) {
        platform::scroll(dx, dy);
    }

    fn leave(&mut self) {
        // Release any keys we think are still down so the Mac is not left
        // with stuck Cmd/Shift after an abrupt leave.
        let stuck: Vec<u32> = self.pressed_keys.drain(..).collect();
        for kc in stuck {
            if let Some(cg) = keymap::linux_to_cg(kc) {
                debug!(kc, "leave: releasing stuck key");
                platform::key_event(cg, false);
            }
        }
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::CgKeyCode;
    use std::sync::atomic::{AtomicI32, Ordering};
    use tracing::{debug, error, warn};

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CGPoint {
        x: f64,
        y: f64,
    }

    type CGEventRef = *mut std::ffi::c_void;
    type CGEventSourceRef = *mut std::ffi::c_void;
    type CGError = i32;
    type CGEventType = u32;
    type CGMouseButton = u32;
    type CGEventTapLocation = u32;
    type CGEventField = u32;

    const CG_EVENT_LEFT_MOUSE_DOWN: CGEventType = 1;
    const CG_EVENT_LEFT_MOUSE_UP: CGEventType = 2;
    const CG_EVENT_RIGHT_MOUSE_DOWN: CGEventType = 3;
    const CG_EVENT_RIGHT_MOUSE_UP: CGEventType = 4;
    const CG_EVENT_MOUSE_MOVED: CGEventType = 5;
    const CG_EVENT_LEFT_MOUSE_DRAGGED: CGEventType = 6;
    const CG_EVENT_RIGHT_MOUSE_DRAGGED: CGEventType = 7;
    const CG_EVENT_OTHER_MOUSE_DOWN: CGEventType = 25;
    const CG_EVENT_OTHER_MOUSE_UP: CGEventType = 26;
    const CG_EVENT_OTHER_MOUSE_DRAGGED: CGEventType = 27;

    const CG_MOUSE_BUTTON_LEFT: CGMouseButton = 0;
    const CG_MOUSE_BUTTON_RIGHT: CGMouseButton = 1;
    const CG_MOUSE_BUTTON_CENTER: CGMouseButton = 2;

    const K_CG_HID_EVENT_TAP: CGEventTapLocation = 0;
    const K_CG_SCROLL_WHEEL_EVENT_DELTA_AXIS_1: CGEventField = 11;
    const K_CG_SCROLL_WHEEL_EVENT_DELTA_AXIS_2: CGEventField = 12;

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGWarpMouseCursorPosition(new_cursor_position: CGPoint) -> CGError;
        fn CGAssociateMouseAndMouseCursorPosition(connected: bool) -> CGError;
        fn CGEventSourceCreate(state_id: i32) -> CGEventSourceRef;
        fn CGEventCreateMouseEvent(
            source: CGEventSourceRef,
            mouse_type: CGEventType,
            mouse_cursor_position: CGPoint,
            mouse_button: CGMouseButton,
        ) -> CGEventRef;
        fn CGEventCreateKeyboardEvent(
            source: CGEventSourceRef,
            virtual_key: u16,
            key_down: bool,
        ) -> CGEventRef;
        fn CGEventCreateScrollWheelEvent2(
            source: CGEventSourceRef,
            units: u32,
            wheel_count: u32,
            wheel1: i32,
            wheel2: i32,
            wheel3: i32,
        ) -> CGEventRef;
        fn CGEventPost(tap: CGEventTapLocation, event: CGEventRef);
        fn CGEventSetIntegerValueField(event: CGEventRef, field: CGEventField, value: i64);
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRelease(cf: *mut std::ffi::c_void);
    }

    static LAST_X: AtomicI32 = AtomicI32::new(0);
    static LAST_Y: AtomicI32 = AtomicI32::new(0);
    static LEFT_DOWN: AtomicI32 = AtomicI32::new(0);
    static RIGHT_DOWN: AtomicI32 = AtomicI32::new(0);
    static OTHER_DOWN: AtomicI32 = AtomicI32::new(0);

    fn source() -> CGEventSourceRef {
        // kCGEventSourceStateHIDSystemState = 1
        unsafe { CGEventSourceCreate(1) }
    }

    fn release(cf: *mut std::ffi::c_void) {
        if !cf.is_null() {
            unsafe { CFRelease(cf) };
        }
    }

    fn point() -> CGPoint {
        CGPoint {
            x: LAST_X.load(Ordering::Relaxed) as f64,
            y: LAST_Y.load(Ordering::Relaxed) as f64,
        }
    }

    pub fn warp_cursor(x: i32, y: i32) {
        LAST_X.store(x, Ordering::Relaxed);
        LAST_Y.store(y, Ordering::Relaxed);
        let pt = CGPoint {
            x: x as f64,
            y: y as f64,
        };
        unsafe {
            // Dissociate during warp so the OS does not immediately re-apply
            // the previous cursor position (common CGWarp pitfall).
            let _ = CGAssociateMouseAndMouseCursorPosition(false);
            let err = CGWarpMouseCursorPosition(pt);
            if err != 0 {
                error!(err, x, y, "CGWarpMouseCursorPosition failed");
                let _ = CGAssociateMouseAndMouseCursorPosition(true);
                return;
            }

            let left = LEFT_DOWN.load(Ordering::Relaxed) != 0;
            let right = RIGHT_DOWN.load(Ordering::Relaxed) != 0;
            let other = OTHER_DOWN.load(Ordering::Relaxed) != 0;
            let (mouse_type, button) = if left {
                (CG_EVENT_LEFT_MOUSE_DRAGGED, CG_MOUSE_BUTTON_LEFT)
            } else if right {
                (CG_EVENT_RIGHT_MOUSE_DRAGGED, CG_MOUSE_BUTTON_RIGHT)
            } else if other {
                (CG_EVENT_OTHER_MOUSE_DRAGGED, CG_MOUSE_BUTTON_CENTER)
            } else {
                (CG_EVENT_MOUSE_MOVED, CG_MOUSE_BUTTON_LEFT)
            };

            // Post a HID mouse-moved/dragged at the new point so apps and the
            // window server agree with the warp (warp alone is often ignored).
            let src = source();
            let ev = CGEventCreateMouseEvent(src, mouse_type, pt, button);
            if !ev.is_null() {
                CGEventPost(K_CG_HID_EVENT_TAP, ev);
                release(ev);
            } else {
                warn!("CGEventCreateMouseEvent returned null (Accessibility?)");
            }
            release(src);
            let _ = CGAssociateMouseAndMouseCursorPosition(true);
        }
        // Info once is too noisy for every motion; debug for stream, warn-level
        // only on failure above.
        debug!(x, y, "warp");
    }

    pub fn mouse_button(button: u8, pressed: bool) {
        let (down_ty, up_ty, cg_btn, flag) = match button {
            0 => (
                CG_EVENT_LEFT_MOUSE_DOWN,
                CG_EVENT_LEFT_MOUSE_UP,
                CG_MOUSE_BUTTON_LEFT,
                &LEFT_DOWN,
            ),
            1 => (
                CG_EVENT_RIGHT_MOUSE_DOWN,
                CG_EVENT_RIGHT_MOUSE_UP,
                CG_MOUSE_BUTTON_RIGHT,
                &RIGHT_DOWN,
            ),
            2 => (
                CG_EVENT_OTHER_MOUSE_DOWN,
                CG_EVENT_OTHER_MOUSE_UP,
                CG_MOUSE_BUTTON_CENTER,
                &OTHER_DOWN,
            ),
            other => {
                warn!(button = other, "unknown mouse button; dropping");
                return;
            }
        };
        flag.store(if pressed { 1 } else { 0 }, Ordering::Relaxed);
        let ty = if pressed { down_ty } else { up_ty };
        let pt = point();
        unsafe {
            let src = source();
            let ev = CGEventCreateMouseEvent(src, ty, pt, cg_btn);
            if !ev.is_null() {
                CGEventPost(K_CG_HID_EVENT_TAP, ev);
                release(ev);
                debug!(button, pressed, "button");
            } else {
                warn!(button, pressed, "button event null (Accessibility?)");
            }
            release(src);
        }
    }

    pub fn key_event(cg: CgKeyCode, pressed: bool) {
        unsafe {
            let src = source();
            let ev = CGEventCreateKeyboardEvent(src, cg, pressed);
            if !ev.is_null() {
                CGEventPost(K_CG_HID_EVENT_TAP, ev);
                release(ev);
                debug!(cg, pressed, "key");
            } else {
                warn!(cg, pressed, "key event null (Accessibility?)");
            }
            release(src);
        }
    }

    pub fn scroll(dx: f32, dy: f32) {
        let mut wheel1 = dy.round() as i32;
        let mut wheel2 = dx.round() as i32;
        if wheel1 == 0 && dy.abs() > f32::EPSILON {
            wheel1 = if dy > 0.0 { 1 } else { -1 };
        }
        if wheel2 == 0 && dx.abs() > f32::EPSILON {
            wheel2 = if dx > 0.0 { 1 } else { -1 };
        }
        if wheel1 == 0 && wheel2 == 0 {
            return;
        }
        unsafe {
            let src = source();
            // kCGScrollEventUnitLine = 0
            let ev = CGEventCreateScrollWheelEvent2(src, 0, 2, wheel1, wheel2, 0);
            if !ev.is_null() {
                CGEventSetIntegerValueField(
                    ev,
                    K_CG_SCROLL_WHEEL_EVENT_DELTA_AXIS_1,
                    wheel1 as i64,
                );
                CGEventSetIntegerValueField(
                    ev,
                    K_CG_SCROLL_WHEEL_EVENT_DELTA_AXIS_2,
                    wheel2 as i64,
                );
                CGEventPost(K_CG_HID_EVENT_TAP, ev);
                release(ev);
                debug!(wheel1, wheel2, "scroll");
            } else {
                warn!("scroll event null (Accessibility?)");
            }
            release(src);
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use super::CgKeyCode;
    use tracing::info;

    pub fn warp_cursor(x: i32, y: i32) {
        info!(x, y, "stub warp (not macOS)");
    }

    pub fn mouse_button(button: u8, pressed: bool) {
        info!(button, pressed, "stub button (not macOS)");
    }

    pub fn key_event(cg: CgKeyCode, pressed: bool) {
        info!(cg, pressed, "stub key (not macOS)");
    }

    pub fn scroll(dx: f32, dy: f32) {
        info!(dx, dy, "stub scroll (not macOS)");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{Edge, Packet};

    struct Recording {
        events: Vec<String>,
    }

    impl Injector for Recording {
        fn warp(&mut self, x: i32, y: i32) {
            self.events.push(format!("warp:{x},{y}"));
        }
        fn button(&mut self, button: u8, pressed: bool) {
            self.events
                .push(format!("button:{button}:{}", pressed as u8));
        }
        fn key(&mut self, keycode: u32, pressed: bool) {
            self.events
                .push(format!("key:{keycode}:{}", pressed as u8));
        }
        fn scroll(&mut self, dx: f32, dy: f32) {
            self.events.push(format!("scroll:{dx},{dy}"));
        }
        fn leave(&mut self) {
            self.events.push("leave".into());
        }
    }

    #[test]
    fn handle_dispatches_send_test_sequence() {
        let mut rec = Recording { events: vec![] };
        let packets = [
            Packet::Enter {
                edge: Edge::Right,
                x: 100,
                y: 200,
            },
            Packet::Motion { x: 150, y: 220 },
            Packet::Button {
                button: 0,
                pressed: 1,
            },
            Packet::Button {
                button: 0,
                pressed: 0,
            },
            Packet::Key {
                keycode: 30,
                pressed: 1,
            },
            Packet::Key {
                keycode: 30,
                pressed: 0,
            },
            Packet::Scroll {
                dx: 0.0,
                dy: -1.0,
            },
            Packet::Leave,
        ];
        for p in &packets {
            rec.handle(p);
        }
        assert_eq!(
            rec.events,
            vec![
                "warp:100,200",
                "warp:150,220",
                "button:0:1",
                "button:0:0",
                "key:30:1",
                "key:30:0",
                "scroll:0,-1",
                "leave",
            ]
        );
    }
}
