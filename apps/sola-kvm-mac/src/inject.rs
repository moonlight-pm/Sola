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
    /// Continuous motion (cheap path — CGEvent only on macOS).
    fn warp(&mut self, x: i32, y: i32);
    /// Hard snap (CGWarp) — enter / click resync.
    fn hard_warp(&mut self, x: i32, y: i32) {
        self.warp(x, y);
    }
    /// Re-anchor system cursor to last abs position before a click.
    fn resync_pointer(&mut self) {}
    fn button(&mut self, button: u8, pressed: bool);
    /// `pressed` false = release; true = press. `autorepeat` marks Linux EV_KEY=2.
    fn key(&mut self, keycode: u32, pressed: bool, autorepeat: bool);
    fn scroll(&mut self, dx: f32, dy: f32);
    fn leave(&mut self);

    /// Drop any tracked pressed-key state (default: no tracking).
    fn clear_key_tracking(&mut self) {}

    /// Dispatch a decoded packet.
    fn handle(&mut self, packet: &Packet) {
        match packet {
            Packet::Enter { x, y, edge } => {
                // Cold-enter first: wake display / IOPM / suppression before
                // any CG work so idle Mac isn't still power-gated mid-warp.
                crate::priority::on_enter_remote();
                // Dissociate once for the whole remote session — flipping
                // associate on every motion is a major source of "fighting"
                // lag right after crossover.
                let _ = platform::begin_remote_pointer();
                platform::reset_multi_click();
                // Clear any stuck Cmd/Shift left from a prior session or a
                // lost key-up (otherwise every click is a ⌘-click).
                self.clear_key_tracking();
                platform::release_all_modifiers();
                // Force CGWarp on enter (twice) so the cursor snaps to the
                // shared edge; continuous motion after this is CGEvent-only.
                tracing::info!(x, y, ?edge, "enter → warp");
                self.hard_warp(*x, *y);
                self.hard_warp(*x, *y);
            }
            Packet::Leave => {
                debug!("leave");
                self.leave();
                platform::reset_multi_click();
                platform::end_remote_pointer();
                // Release session IOPM assertions after pointer teardown.
                crate::priority::on_leave_remote();
            }
            Packet::Motion { x, y } => {
                // In case Enter was lost: dissociate + cold-wake on first motion.
                if platform::begin_remote_pointer() {
                    crate::priority::on_enter_remote();
                }
                self.warp(*x, *y);
            }
            Packet::Button { button, pressed } => {
                // Warp once on press so the click lands at the true abs position
                // (motion path may have only posted CGEvents since last warp).
                if *pressed != 0 {
                    self.resync_pointer();
                }
                self.button(*button, *pressed != 0);
            }
            Packet::Key { keycode, pressed } => {
                // Wire: 0=up, 1=down, 2=auto-repeat (Linux EV_KEY).
                match *pressed {
                    0 => self.key(*keycode, false, false),
                    2 => self.key(*keycode, true, true),
                    _ => self.key(*keycode, true, false),
                }
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
        // Continuous motion: CGEvent only. CGWarp every packet was the lag
        // source (metrics: inject_ms 8–14 on single motions, then backlog).
        platform::move_cursor(x, y, false);
    }

    fn hard_warp(&mut self, x: i32, y: i32) {
        platform::move_cursor(x, y, true);
    }

    fn resync_pointer(&mut self) {
        platform::resync_cursor();
    }

    fn clear_key_tracking(&mut self) {
        self.pressed_keys.clear();
    }

    fn button(&mut self, button: u8, pressed: bool) {
        platform::mouse_button(button, pressed);
    }

    fn key(&mut self, keycode: u32, pressed: bool, autorepeat: bool) {
        let Some(target) = keymap::linux_to_mac(keycode) else {
            warn!(
                keycode,
                name = keymap::linux_key_name(keycode).unwrap_or("?"),
                "unmapped Linux keycode; dropping"
            );
            return;
        };
        match target {
            keymap::MacTarget::Media(nx) => {
                // Media keys are not sticky "held letters"; track nothing.
                debug!(
                    keycode,
                    ?nx,
                    pressed,
                    name = keymap::linux_key_name(keycode).unwrap_or("?"),
                    "inject media"
                );
                // Auto-repeat on volume is fine (hold = keep changing).
                platform::media_key(nx, pressed || autorepeat);
            }
            keymap::MacTarget::Key(cg) => {
                if pressed {
                    if !self.pressed_keys.contains(&keycode) {
                        self.pressed_keys.push(keycode);
                    }
                } else {
                    self.pressed_keys.retain(|&k| k != keycode);
                }
                debug!(
                    keycode,
                    cg,
                    pressed,
                    autorepeat,
                    name = keymap::linux_key_name(keycode).unwrap_or("?"),
                    "inject key"
                );
                platform::key_event(cg, pressed, autorepeat);
            }
        }
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
                platform::key_event(cg, false, false);
            }
        }
        // Always zero every modifier VK + MOD_FLAGS even if tracking missed a
        // press (lost UDP key-down still leaves WindowServer thinking ⌘ is held).
        platform::release_all_modifiers();
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::CgKeyCode;
    use crate::click::MultiClick;
    use std::sync::atomic::{AtomicI32, AtomicU64, Ordering};
    use std::sync::Mutex;
    use std::time::Instant;
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
    /// kCGKeyboardEventAutorepeat — mark Linux EV_KEY value=2 as a true OS repeat.
    const K_CG_KEYBOARD_EVENT_AUTOREPEAT: CGEventField = 8;

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
        fn CGEventCreate(source: CGEventSourceRef) -> CGEventRef;
        fn CGEventSetType(event: CGEventRef, mouse_type: CGEventType);
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
    /// Multi-click counter for kCGMouseEventClickState (double-click etc.).
    static MULTI_CLICK: Mutex<MultiClick> = Mutex::new(MultiClick::new(
        crate::click::DEFAULT_INTERVAL,
        crate::click::DEFAULT_SLOP,
    ));
    /// Monotonic ms of last CGEventSourceSetLocalEventsSuppressionInterval call.
    static LAST_SUPPRESS_CFG_MS: AtomicU64 = AtomicU64::new(0);

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

    /// Re-apply suppression=0, but not on every motion (that was pure overhead).
    /// Always re-apply for clicks/keys; for motion, at most ~every 16 ms.
    fn configure_source_throttled(src: CGEventSourceRef, force: bool) {
        if src.is_null() {
            return;
        }
        if !force {
            // Cheap approx ms from Instant is awkward in atomics; use a coarse
            // counter of calls: force every 4th motion path is enough.
            let n = LAST_SUPPRESS_CFG_MS.fetch_add(1, Ordering::Relaxed);
            if n % 4 != 0 {
                return;
            }
        }
        configure_source(src);
    }

    pub fn reset_multi_click() {
        if let Ok(mut m) = MULTI_CLICK.lock() {
            m.reset();
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
                 enable «Sola KVM Mac» (/Applications/SolaKvmMac.app). \
                 Remove stale /opt/sola/bin entries. After install.sh uses a stable \
                 codesign cert, rebuilds keep the grant."
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

    /// Force key-up for every modifier VK and zero `MOD_FLAGS`.
    ///
    /// Called on Enter and Leave so a missed key-up (UDP loss, process restart,
    /// EVIOCGRAB cut mid-chord) cannot leave the Mac with permanent ⌘-click.
    pub fn release_all_modifiers() {
        const MOD_VKS: &[CgKeyCode] = &[
            VK_COMMAND,
            VK_RIGHT_COMMAND,
            VK_SHIFT,
            VK_RIGHT_SHIFT,
            VK_CONTROL,
            VK_RIGHT_CONTROL,
            VK_OPTION,
            VK_RIGHT_OPTION,
            // Caps: up only; we do not toggle on clear.
            VK_CAPS_LOCK,
        ];
        // Zero mask first so the key-up events carry flags=0.
        MOD_FLAGS.store(0, Ordering::Relaxed);
        for &cg in MOD_VKS {
            // Bypass update_mod_flags path that would re-touch the mask;
            // post a bare key-up so WindowServer drops the chord.
            post_key_up_raw(cg);
        }
        MOD_FLAGS.store(0, Ordering::Relaxed);
        info!("released all synthetic modifiers (Cmd/Shift/Ctrl/Opt)");
    }

    fn post_key_up_raw(cg: CgKeyCode) {
        unsafe {
            let src = source();
            configure_source_throttled(src, true);
            let ev = CGEventCreateKeyboardEvent(src, cg, false);
            if ev.is_null() {
                return;
            }
            CGEventSetFlags(ev, 0);
            CGEventPost(K_CG_HID_EVENT_TAP, ev);
            release_event(ev);
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

    /// True while we hold CGAssociate=false for the remote session.
    static REMOTE_DISSOCIATED: AtomicI32 = AtomicI32::new(0);

    /// Dissociate mouse↔cursor once for a remote session (idempotent).
    ///
    /// Returns `true` if this call newly entered remote (0→1), so callers can
    /// run cold-wake once when Enter was lost and the first packet is Motion.
    ///
    /// Barrier / Input Leap keep association off for the whole grab rather
    /// than toggling on every warp — the per-motion flip was a big cost on
    /// enter when motion rates spike.
    pub fn begin_remote_pointer() -> bool {
        if REMOTE_DISSOCIATED.swap(1, Ordering::Relaxed) == 0 {
            unsafe {
                let err = CGAssociateMouseAndMouseCursorPosition(false);
                if err != 0 {
                    error!(err, "CGAssociateMouseAndMouseCursorPosition(false) failed");
                } else {
                    debug!("pointer dissociated for remote session");
                }
            }
            true
        } else {
            false
        }
    }

    /// Re-associate mouse↔cursor when leaving remote (idempotent).
    pub fn end_remote_pointer() {
        if REMOTE_DISSOCIATED.swap(0, Ordering::Relaxed) != 0 {
            unsafe {
                let err = CGAssociateMouseAndMouseCursorPosition(true);
                if err != 0 {
                    error!(err, "CGAssociateMouseAndMouseCursorPosition(true) failed");
                } else {
                    debug!("pointer re-associated after leave");
                }
            }
        }
    }

    /// Move the synthetic cursor.
    ///
    /// * `force_warp = true` — `CGWarpMouseCursorPosition` (enter, click resync only).
    /// * `force_warp = false` — **CGEvent mouse-moved/dragged only** (hot path).
    ///
    /// No periodic hard-warps: metrics showed even occasional CGWarp under load
    /// contributed to multi-hundred-ms gaps. Enter + click resync is enough.
    pub fn move_cursor(x: i32, y: i32, force_warp: bool) {
        ensure_trusted_logged();
        LAST_X.store(x, Ordering::Relaxed);
        LAST_Y.store(y, Ordering::Relaxed);
        let pt = CGPoint {
            x: x as f64,
            y: y as f64,
        };

        unsafe {
            if force_warp {
                let err = CGWarpMouseCursorPosition(pt);
                if err != 0 {
                    error!(err, x, y, "CGWarpMouseCursorPosition failed");
                }
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

            let src = source();
            configure_source_throttled(src, force_warp);
            let ev = CGEventCreateMouseEvent(src, mouse_type, pt, button);
            if !ev.is_null() {
                CGEventPost(K_CG_HID_EVENT_TAP, ev);
                release_event(ev);
            } else {
                warn!("CGEventCreateMouseEvent returned null (Accessibility?)");
            }
        }
        debug!(x, y, force_warp, "move");
    }

    /// Hard-warp to the last known abs position (before a click).
    pub fn resync_cursor() {
        let x = LAST_X.load(Ordering::Relaxed);
        let y = LAST_Y.load(Ordering::Relaxed);
        move_cursor(x, y, true);
    }

    /// Back-compat name used by tests/stubs.
    pub fn warp_cursor(x: i32, y: i32) {
        move_cursor(x, y, true);
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
        let x = LAST_X.load(Ordering::Relaxed);
        let y = LAST_Y.load(Ordering::Relaxed);

        // Double-click / triple-click: AppKit reads kCGMouseEventClickState.
        // Always posting 1 made every click a fresh single-click.
        let click_state = {
            let mut mc = MULTI_CLICK.lock().unwrap_or_else(|e| e.into_inner());
            if pressed {
                mc.on_down(button, x, y, Instant::now())
            } else {
                mc.on_up(button)
            }
        };

        unsafe {
            let src = source();
            // Always re-zero suppression around clicks — warps re-arm it.
            configure_source_throttled(src, true);
            let ev = CGEventCreateMouseEvent(src, ty, pt, cg_btn);
            if !ev.is_null() {
                CGEventSetIntegerValueField(ev, K_CG_MOUSE_EVENT_CLICK_STATE, click_state);
                // Explicit flags from our mask only — do not inherit a stuck
                // system ⌘ from a prior lost key-up.
                CGEventSetFlags(ev, MOD_FLAGS.load(Ordering::Relaxed));
                CGEventPost(K_CG_HID_EVENT_TAP, ev);
                release_event(ev);
                debug!(
                    button,
                    pressed,
                    click_state,
                    x = pt.x,
                    y = pt.y,
                    "inject button"
                );
            } else {
                warn!(button, pressed, "button event null (Accessibility?)");
            }
        }
    }

    pub fn key_event(cg: CgKeyCode, pressed: bool, autorepeat: bool) {
        ensure_trusted_logged();
        // Update our synthetic modifier mask *before* building the event so
        // key-down of a character sees any modifier that just went down.
        // Auto-repeat does not change modifier ownership.
        if !autorepeat {
            update_mod_flags(cg, pressed);
        }
        let flags = MOD_FLAGS.load(Ordering::Relaxed);

        unsafe {
            // Always use the shared source (suppression interval 0). NULL
            // sources inherit the default 250ms suppression from warps.
            let src = source();
            configure_source_throttled(src, true);
            // Auto-repeat is still a key-down CGEvent with the autorepeat field set.
            let down = pressed || autorepeat;
            let ev = CGEventCreateKeyboardEvent(src, cg, down);
            if ev.is_null() {
                warn!(cg, pressed, autorepeat, "key event null (Accessibility?)");
                return;
            }

            CGEventSetFlags(ev, flags);
            if autorepeat {
                CGEventSetIntegerValueField(ev, K_CG_KEYBOARD_EVENT_AUTOREPEAT, 1);
            }

            // Text insertion needs a unicode string (Input Leap / Barrier do this).
            // **Never** attach unicode for Cmd/Ctrl chords — Cocoa then treats the
            // event as text input and shortcuts like ⌘C / ⌘V / ⌘T silently fail.
            let shortcut = (flags & (FLAG_COMMAND | FLAG_CONTROL)) != 0;
            if down && !shortcut {
                if let Some(ch) = unicode_for_cg(cg, flags) {
                    let chars = [ch];
                    CGEventKeyboardSetUnicodeString(ev, 1, chars.as_ptr());
                }
            }

            CGEventPost(K_CG_HID_EVENT_TAP, ev);
            release_event(ev);
            debug!(cg, pressed, autorepeat, flags, shortcut, "key posted");
        }
    }

    /// Post a system-defined NX media / brightness key (volume, play, etc.).
    ///
    /// Ordinary `CGEventCreateKeyboardEvent` does not drive the system volume
    /// HUD or media transport; those use NX_KEYTYPE aux control events.
    pub fn media_key(nx: crate::keymap::NxMediaKey, pressed: bool) {
        ensure_trusted_logged();
        // Prefer the classic kVK path for mute/volume — widely supported via
        // CG keyboard events. Fall through to NX aux for transport/brightness.
        match nx {
            crate::keymap::NxMediaKey::SoundUp => {
                key_event(0x48, pressed, false); // kVK_VolumeUp
                return;
            }
            crate::keymap::NxMediaKey::SoundDown => {
                key_event(0x49, pressed, false); // kVK_VolumeDown
                return;
            }
            crate::keymap::NxMediaKey::Mute => {
                key_event(0x4a, pressed, false); // kVK_Mute
                return;
            }
            _ => {}
        }
        post_nx_aux_key(nx as u8, pressed);
    }

    /// NX_SUBTYPE_AUX_CONTROL_BUTTONS system-defined event via CGEvent.
    ///
    /// Packing matches NSEvent `otherEventWithType:NSSystemDefined subtype:8
    /// data1:(key<<16)|((down?0xa:0xb)<<8) data2:-1` — the form Barrier /
    /// Input Leap / Hammerspoon use for media keys without IOKit connect.
    fn post_nx_aux_key(key_type: u8, down: bool) {
        // NSEventTypeSystemDefined / NX_SYSDEFINED
        const CG_EVENT_SYSTEM_DEFINED: CGEventType = 14;
        // Undocumented but stable field ids used by NSEvent→CGEvent for
        // compound system events (subtype + data1/data2).
        const K_CG_EVENT_SUBTYPE: CGEventField = 55; // 0x37
        const K_CG_EVENT_DATA1: CGEventField = 149; // 0x95
        const K_CG_EVENT_DATA2: CGEventField = 150; // 0x96
        const NX_SUBTYPE_AUX_CONTROL_BUTTONS: i64 = 8;

        let key_state: i64 = if down { 0x0a } else { 0x0b };
        let data1 = ((key_type as i64) << 16) | (key_state << 8);
        let flags: CGEventFlags = if down { 0xa00 } else { 0xb00 };

        unsafe {
            let src = source();
            configure_source_throttled(src, true);
            // NULL-source system events are accepted; we still use our private
            // source so suppression interval stays zero.
            let ev = CGEventCreate(src);
            if ev.is_null() {
                warn!(key_type, down, "media CGEventCreate null");
                return;
            }
            CGEventSetType(ev, CG_EVENT_SYSTEM_DEFINED);
            CGEventSetFlags(ev, flags);
            CGEventSetIntegerValueField(ev, K_CG_EVENT_SUBTYPE, NX_SUBTYPE_AUX_CONTROL_BUTTONS);
            CGEventSetIntegerValueField(ev, K_CG_EVENT_DATA1, data1);
            CGEventSetIntegerValueField(ev, K_CG_EVENT_DATA2, -1);
            CGEventPost(K_CG_HID_EVENT_TAP, ev);
            release_event(ev);
            debug!(key_type, down, data1, "media nx posted");
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
        // Tuned down from 48 → 24 → 12 after desk feedback (still half prior).
        const PIXELS_PER_DETENT: f32 = 12.0;
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
            configure_source_throttled(src, true);
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

    use std::sync::atomic::{AtomicBool, Ordering};
    static REMOTE: AtomicBool = AtomicBool::new(false);

    /// Returns `true` on first remote entry (matches macOS semantics).
    pub fn begin_remote_pointer() -> bool {
        !REMOTE.swap(true, Ordering::Relaxed)
    }

    pub fn end_remote_pointer() {
        REMOTE.store(false, Ordering::Relaxed);
    }

    pub fn reset_multi_click() {
        // no-op stub
    }

    pub fn release_all_modifiers() {
        info!("stub release_all_modifiers (not macOS)");
    }

    pub fn move_cursor(x: i32, y: i32, force_warp: bool) {
        info!(x, y, force_warp, "stub move (not macOS)");
    }

    pub fn resync_cursor() {
        info!("stub resync (not macOS)");
    }

    pub fn warp_cursor(x: i32, y: i32) {
        move_cursor(x, y, true);
    }

    pub fn mouse_button(button: u8, pressed: bool) {
        info!(button, pressed, "stub button (not macOS)");
    }

    pub fn key_event(cg: CgKeyCode, pressed: bool, autorepeat: bool) {
        info!(cg, pressed, autorepeat, "stub key (not macOS)");
    }

    pub fn media_key(nx: crate::keymap::NxMediaKey, pressed: bool) {
        info!(?nx, pressed, "stub media (not macOS)");
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
        fn hard_warp(&mut self, x: i32, y: i32) {
            self.events.push(format!("warp:{x},{y}"));
        }
        fn button(&mut self, button: u8, pressed: bool) {
            self.events
                .push(format!("button:{button}:{}", pressed as u8));
        }
        fn key(&mut self, keycode: u32, pressed: bool, autorepeat: bool) {
            if autorepeat {
                self.events.push(format!("key:{keycode}:2"));
            } else {
                self.events
                    .push(format!("key:{keycode}:{}", pressed as u8));
            }
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
                pressed: 2, // auto-repeat
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
                "key:30:2",
                "key:30:0",
                "scroll:0,-1",
                "leave",
            ]
        );
    }
}
