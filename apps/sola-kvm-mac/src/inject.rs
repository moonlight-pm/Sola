//! CGEvent injection (macOS) with a Linux stub for compile/test.
//!
//! On macOS this posts synthetic mouse/keyboard events via CoreGraphics.
//! Accessibility permission is required (see README).

use crate::keymap::{self, CgKeyCode};
use crate::protocol::Packet;
use tracing::{debug, warn};

/// Log Accessibility trust at agent startup (and open Settings if untrusted).
pub fn check_accessibility_at_startup() {
    platform::check_accessibility_at_startup();
}

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
        tracing::info!(
            keycode,
            cg,
            pressed,
            name = keymap::linux_key_name(keycode).unwrap_or("?"),
            "inject key"
        );
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
    use std::sync::atomic::{AtomicI32, AtomicU64, Ordering};
    use tracing::{debug, error, info, warn};

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
    type CGEventFlags = u64;

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
    /// kCGMouseEventClickState — single click = 1
    const K_CG_MOUSE_EVENT_CLICK_STATE: CGEventField = 1;

    // kCGEventSourceStatePrivate — synthetic events must not share physical HID state.
    const K_CG_EVENT_SOURCE_STATE_PRIVATE: i32 = -1;

    // CGEventFlags (from CGEventTypes.h)
    const FLAG_ALPHA_SHIFT: CGEventFlags = 0x0001_0000;
    const FLAG_SHIFT: CGEventFlags = 0x0002_0000;
    const FLAG_CONTROL: CGEventFlags = 0x0004_0000;
    const FLAG_ALTERNATE: CGEventFlags = 0x0008_0000;
    const FLAG_COMMAND: CGEventFlags = 0x0010_0000;

    // kVK_* used for modifier tracking
    const VK_SHIFT: CgKeyCode = 0x38;
    const VK_RIGHT_SHIFT: CgKeyCode = 0x3c;
    const VK_CONTROL: CgKeyCode = 0x3b;
    const VK_RIGHT_CONTROL: CgKeyCode = 0x3e;
    const VK_OPTION: CgKeyCode = 0x3a;
    const VK_RIGHT_OPTION: CgKeyCode = 0x3d;
    const VK_COMMAND: CgKeyCode = 0x37;
    const VK_RIGHT_COMMAND: CgKeyCode = 0x36;
    const VK_CAPS_LOCK: CgKeyCode = 0x39;

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
        fn CGEventSetFlags(event: CGEventRef, flags: CGEventFlags);
        fn CGEventKeyboardSetUnicodeString(
            event: CGEventRef,
            length: usize,
            string: *const u16,
        );
        /// Critical for KVM: continuous CGWarp suppresses subsequent local
        /// (including our own synthetic) click/key events for ~250ms by default.
        fn CGEventSourceSetLocalEventsSuppressionInterval(
            source: CGEventSourceRef,
            seconds: f64,
        );
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRelease(cf: *mut std::ffi::c_void);
    }

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrusted() -> u8;
    }

    static LAST_X: AtomicI32 = AtomicI32::new(0);
    static LAST_Y: AtomicI32 = AtomicI32::new(0);
    static LEFT_DOWN: AtomicI32 = AtomicI32::new(0);
    static RIGHT_DOWN: AtomicI32 = AtomicI32::new(0);
    static OTHER_DOWN: AtomicI32 = AtomicI32::new(0);
    /// Synthetic modifier flags we own (not the physical Mac keyboard).
    static MOD_FLAGS: AtomicU64 = AtomicU64::new(0);
    static TRUSTED_LOGGED: AtomicI32 = AtomicI32::new(0);
    /// Process-wide event source with suppression disabled (see `source()`).
    static EVENT_SOURCE: std::sync::atomic::AtomicPtr<std::ffi::c_void> =
        std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());

    /// Long-lived private event source with **zero** local-event suppression.
    ///
    /// `CGWarpMouseCursorPosition` (used every motion packet) otherwise starts
    /// a ~0.25s suppression window that is continuously refreshed by the motion
    /// stream — so clicks and keys appear to "do nothing" while the cursor still
    /// moves. Barrier / Input Leap / lan-mouse all set this to 0.
    fn source() -> CGEventSourceRef {
        use std::sync::atomic::Ordering as Ord;
        let existing = EVENT_SOURCE.load(Ord::Acquire);
        if !existing.is_null() {
            return existing;
        }
        unsafe {
            let src = CGEventSourceCreate(K_CG_EVENT_SOURCE_STATE_PRIVATE);
            if src.is_null() {
                // Fall back: combined session state.
                let src = CGEventSourceCreate(0);
                configure_source(src);
                let _ = EVENT_SOURCE.compare_exchange(
                    std::ptr::null_mut(),
                    src,
                    Ord::AcqRel,
                    Ord::Acquire,
                );
                return EVENT_SOURCE.load(Ord::Acquire);
            }
            configure_source(src);
            match EVENT_SOURCE.compare_exchange(
                std::ptr::null_mut(),
                src,
                Ord::AcqRel,
                Ord::Acquire,
            ) {
                Ok(_) => src,
                Err(winner) => {
                    // Another thread won — release ours, use theirs.
                    CFRelease(src);
                    winner
                }
            }
        }
    }

    fn configure_source(src: CGEventSourceRef) {
        if src.is_null() {
            return;
        }
        // Zero out the default 0.25s post-synthetic suppression window.
        // Without this, a 500 Hz motion warp stream permanently blocks clicks/keys.
        unsafe {
            CGEventSourceSetLocalEventsSuppressionInterval(src, 0.0);
        }
    }

    /// Keep the process-wide source alive; do not CFRelease it after each post.
    fn release_event(ev: CGEventRef) {
        if !ev.is_null() {
            unsafe { CFRelease(ev) };
        }
    }

    fn point() -> CGPoint {
        CGPoint {
            x: LAST_X.load(Ordering::Relaxed) as f64,
            y: LAST_Y.load(Ordering::Relaxed) as f64,
        }
    }

    pub fn check_accessibility_at_startup() {
        TRUSTED_LOGGED.store(0, Ordering::Relaxed);
        ensure_trusted_logged();
    }

    fn ensure_trusted_logged() {
        if TRUSTED_LOGGED.swap(1, Ordering::Relaxed) != 0 {
            return;
        }
        // TCC can list a *stale* path as allowed after an ad-hoc re-sign while
        // AXIsProcessTrusted is still false — that exact state makes CGWarp work
        // (cursor moves) while CGEventPost for clicks/keys is silently dropped.
        let trusted = unsafe { AXIsProcessTrusted() } != 0;
        if trusted {
            info!("AXIsProcessTrusted=true (Accessibility granted)");
        } else {
            warn!(
                "AXIsProcessTrusted=false — clicks/keys will no-op. \
                 System Settings → Privacy & Security → Accessibility: \
                 REMOVE sola-kvm-mac, re-add /opt/sola/bin/sola-kvm-mac, enable it. \
                 (Toggle alone is not enough after a re-sign; cursor warp still works without AX.)"
            );
            // Open the pane so the user can re-grant the *current* binary.
            let _ = std::process::Command::new("open")
                .arg(
                    "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility",
                )
                .spawn();
        }
    }



    fn update_mod_flags(cg: CgKeyCode, pressed: bool) {
        let bit = match cg {
            VK_SHIFT | VK_RIGHT_SHIFT => FLAG_SHIFT,
            VK_CONTROL | VK_RIGHT_CONTROL => FLAG_CONTROL,
            VK_OPTION | VK_RIGHT_OPTION => FLAG_ALTERNATE,
            VK_COMMAND | VK_RIGHT_COMMAND => FLAG_COMMAND,
            VK_CAPS_LOCK => FLAG_ALPHA_SHIFT,
            _ => return,
        };
        if pressed {
            MOD_FLAGS.fetch_or(bit, Ordering::Relaxed);
        } else {
            MOD_FLAGS.fetch_and(!bit, Ordering::Relaxed);
        }
    }

    /// US/QWERTY glyph for a virtual key, honoring current shift/caps.
    fn unicode_for_cg(cg: CgKeyCode, flags: CGEventFlags) -> Option<u16> {
        let shift = (flags & FLAG_SHIFT) != 0;
        let caps = (flags & FLAG_ALPHA_SHIFT) != 0;
        let upper = shift ^ caps;
        // Letters a–z
        let letter = |lower: char| -> u16 {
            let c = if upper {
                lower.to_ascii_uppercase()
            } else {
                lower
            };
            c as u16
        };
        Some(match cg {
            0x00 => letter('a'),
            0x0b => letter('b'),
            0x08 => letter('c'),
            0x02 => letter('d'),
            0x0e => letter('e'),
            0x03 => letter('f'),
            0x05 => letter('g'),
            0x04 => letter('h'),
            0x22 => letter('i'),
            0x26 => letter('j'),
            0x28 => letter('k'),
            0x25 => letter('l'),
            0x2e => letter('m'),
            0x2d => letter('n'),
            0x1f => letter('o'),
            0x23 => letter('p'),
            0x0c => letter('q'),
            0x0f => letter('r'),
            0x01 => letter('s'),
            0x11 => letter('t'),
            0x20 => letter('u'),
            0x09 => letter('v'),
            0x0d => letter('w'),
            0x07 => letter('x'),
            0x10 => letter('y'),
            0x06 => letter('z'),
            // Digits / shifted symbols
            0x12 => (if shift { b'!' } else { b'1' }) as u16,
            0x13 => (if shift { b'@' } else { b'2' }) as u16,
            0x14 => (if shift { b'#' } else { b'3' }) as u16,
            0x15 => (if shift { b'$' } else { b'4' }) as u16,
            0x17 => (if shift { b'%' } else { b'5' }) as u16,
            0x16 => (if shift { b'^' } else { b'6' }) as u16,
            0x1a => (if shift { b'&' } else { b'7' }) as u16,
            0x1c => (if shift { b'*' } else { b'8' }) as u16,
            0x19 => (if shift { b'(' } else { b'9' }) as u16,
            0x1d => (if shift { b')' } else { b'0' }) as u16,
            0x1b => (if shift { b'_' } else { b'-' }) as u16,
            0x18 => (if shift { b'+' } else { b'=' }) as u16,
            0x21 => (if shift { b'{' } else { b'[' }) as u16,
            0x1e => (if shift { b'}' } else { b']' }) as u16,
            0x2a => (if shift { b'|' } else { b'\\' }) as u16,
            0x29 => (if shift { b':' } else { b';' }) as u16,
            0x27 => (if shift { b'"' } else { b'\'' }) as u16,
            0x32 => (if shift { b'~' } else { b'`' }) as u16,
            0x2b => (if shift { b'<' } else { b',' }) as u16,
            0x2f => (if shift { b'>' } else { b'.' }) as u16,
            0x2c => (if shift { b'?' } else { b'/' }) as u16,
            0x31 => b' ' as u16,         // space
            0x30 => b'\t' as u16,        // tab
            0x24 | 0x4c => b'\r' as u16, // return / keypad enter
            _ => return None,
        })
    }

    pub fn warp_cursor(x: i32, y: i32) {
        ensure_trusted_logged();
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
            // Re-apply suppression=0 on the shared source after every warp —
            // CGWarp itself can re-arm the system suppression window.
            let src = source();
            configure_source(src);
            let ev = CGEventCreateMouseEvent(src, mouse_type, pt, button);
            if !ev.is_null() {
                CGEventPost(K_CG_HID_EVENT_TAP, ev);
                release_event(ev);
            } else {
                warn!("CGEventCreateMouseEvent returned null (Accessibility?)");
            }
            let _ = CGAssociateMouseAndMouseCursorPosition(true);
        }
        // Info once is too noisy for every motion; debug for stream, warn-level
        // only on failure above.
        debug!(x, y, "warp");
    }

    pub fn mouse_button(button: u8, pressed: bool) {
        ensure_trusted_logged();
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
            // Ensure warps haven't re-armed suppression right before the click.
            configure_source(src);
            let ev = CGEventCreateMouseEvent(src, ty, pt, cg_btn);
            if !ev.is_null() {
                // Without click-state, some AppKit targets ignore the down/up pair.
                CGEventSetIntegerValueField(ev, K_CG_MOUSE_EVENT_CLICK_STATE, 1);
                CGEventPost(K_CG_HID_EVENT_TAP, ev);
                release_event(ev);
                info!(button, pressed, x = pt.x, y = pt.y, "inject button");
            } else {
                warn!(button, pressed, "button event null (Accessibility?)");
            }
        }
    }

    pub fn key_event(cg: CgKeyCode, pressed: bool) {
        ensure_trusted_logged();
        // Update our synthetic modifier mask *before* building the event so
        // key-down of a character sees any modifier that just went down.
        update_mod_flags(cg, pressed);
        let flags = MOD_FLAGS.load(Ordering::Relaxed);

        unsafe {
            // Always use the shared source (suppression interval 0). NULL
            // sources inherit the default 250ms suppression from warps.
            let src = source();
            configure_source(src);
            let ev = CGEventCreateKeyboardEvent(src, cg, pressed);
            if ev.is_null() {
                warn!(cg, pressed, "key event null (Accessibility?)");
                return;
            }

            CGEventSetFlags(ev, flags);

            // Many Cocoa apps ignore bare virtual keycodes for text insertion
            // unless a unicode string is attached (Input Leap / Barrier do this).
            if let Some(ch) = unicode_for_cg(cg, flags) {
                let chars = [ch];
                CGEventKeyboardSetUnicodeString(ev, 1, chars.as_ptr());
            }

            CGEventPost(K_CG_HID_EVENT_TAP, ev);
            release_event(ev);
            debug!(cg, pressed, flags, "key posted");
        }
    }

    pub fn scroll(dx: f32, dy: f32) {
        ensure_trusted_logged();
        // Linux REL_WHEEL and macOS CG scroll axes are opposite for the usual
        // "finger/wheel → content" direction — invert both axes on inject.
        let dx = -dx;
        let dy = -dy;
        // One Linux detent as a single CG *line* tick feels glacial in Cocoa.
        // Scale into pixel units so speed roughly matches a native Mac mouse.
        // (detent → ~48px; fractional hi-res values from novus scale smoothly.)
        const PIXELS_PER_DETENT: f32 = 48.0;
        let mut wheel1 = (dy * PIXELS_PER_DETENT).round() as i32;
        let mut wheel2 = (dx * PIXELS_PER_DETENT).round() as i32;
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
            configure_source(src);
            // kCGScrollEventUnitPixel = 1 (line = 0 is too coarse/slow).
            let ev = CGEventCreateScrollWheelEvent2(src, 1, 2, wheel1, wheel2, 0);
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
                release_event(ev);
                debug!(wheel1, wheel2, "scroll");
            } else {
                warn!("scroll event null (Accessibility?)");
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use super::CgKeyCode;
    use tracing::info;

    pub fn check_accessibility_at_startup() {
        info!("stub accessibility check (not macOS)");
    }

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
                "warp:100,200", // Enter warps twice so CG sticks
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
