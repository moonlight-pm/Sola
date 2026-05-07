//! Driver for `zwp_virtual_keyboard_unstable_v1`.
//!
//! Owns the manager proxy and a single virtual keyboard attached to the
//! seat. Provides `synthesize_chord` for `Topic::SimulateKey` (used by
//! `solactl input` for testing/automation).
//!
//! Uses explicit `modifiers()` requests rather than synthesizing
//! modifier keycode press/release pairs:
//!   - Modifier state is always bracketed by a final clear, avoiding a
//!     "stuck modifier" failure mode.
//!   - The virtual keyboard's modifier state is isolated from the user's
//!     physical state in wlroots-family compositors.

use std::io::Write;
use std::os::fd::{AsFd, OwnedFd};
use std::time::{SystemTime, UNIX_EPOCH};

use rustix::fs::{MemfdFlags, memfd_create};
use tracing::{info, warn};
use wayland_client::{Connection, Dispatch, QueueHandle};

use crate::client::AppData;
use crate::protocol::virtual_keyboard_unstable_v1::{
    zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1,
    zwp_virtual_keyboard_v1::{self, ZwpVirtualKeyboardV1},
};

/// xkb real-modifier bitmask: Control is bit 2.
pub const CTRL_MASK: u32 = 1 << 2;
/// xkb real-modifier bitmask: Shift is bit 0.
pub const SHIFT_MASK: u32 = 1 << 0;
/// xkb real-modifier bitmask: Alt (Mod1) is bit 3.
pub const ALT_MASK: u32 = 1 << 3;
/// xkb real-modifier bitmask: Super/Meta (Mod4) is bit 6.
pub const META_MASK: u32 = 1 << 6;

/// xkb keymap format 1 (text V1).
const KEYMAP_FORMAT_TEXT_V1: u32 = 1;

/// wl_keyboard key state: pressed.
const KEY_PRESSED: u32 = 1;
/// wl_keyboard key state: released.
const KEY_RELEASED: u32 = 0;

/// The keymap we hand to the virtual keyboard. It has to match the layout
/// used by the physical keyboard (or at least define the keycodes we plan
/// to synthesize) so the compositor resolves `key(evdev_code)` to the
/// right keysym. This is the same standard us-layout keymap we'd use for
/// any other purpose.
const KEYMAP: &str = include_str!("../../keymaps/default.xkb");

#[derive(Default)]
pub struct VirtualKeyboardState {
    pub manager: Option<ZwpVirtualKeyboardManagerV1>,
    pub keyboard: Option<ZwpVirtualKeyboardV1>,
    pub keymap_set: bool,
}

/// Called once when both `wl_seat` and `zwp_virtual_keyboard_manager_v1`
/// are bound. Creates the per-seat virtual keyboard and uploads the
/// keymap. No-op if already created or prerequisites aren't bound yet.
pub fn init_if_ready(state: &mut AppData, qh: &QueueHandle<AppData>) {
    if state.virtual_keyboard.keyboard.is_some() {
        return;
    }
    let Some(manager) = state.virtual_keyboard.manager.as_ref() else {
        return;
    };
    let Some(seat) = state.wl_seat.as_ref() else {
        return;
    };

    let kb = manager.create_virtual_keyboard(seat, qh, ());

    // Upload the keymap via memfd. The keymap must be a valid xkb text-v1
    // keymap with a trailing NUL; the compositor mmaps it and fstats for
    // the size.
    match make_keymap_fd(KEYMAP) {
        Ok((fd, size)) => {
            kb.keymap(KEYMAP_FORMAT_TEXT_V1, fd.as_fd(), size);
            state.virtual_keyboard.keymap_set = true;
            info!(size, "virtual keyboard keymap uploaded");
        }
        Err(e) => {
            warn!(%e, "failed to build virtual keyboard keymap fd");
        }
    }

    state.virtual_keyboard.keyboard = Some(kb);
}

/// Synthesize a keystroke for an arbitrary KeyChord. Used by the
/// `Topic::SimulateKey` handler. The chord's `keycode` is in xkb space
/// (evdev + 8); we subtract 8 before passing to `kb.key`.
pub fn synthesize_chord(state: &AppData, chord: &sola_core::KeyChord) {
    let Some(kb) = state.virtual_keyboard.keyboard.as_ref() else {
        warn!("SimulateKey received but virtual keyboard not ready");
        return;
    };
    if !state.virtual_keyboard.keymap_set {
        warn!("virtual keyboard keymap not set; refusing to synthesize");
        return;
    }

    let mut mask: u32 = 0;
    if chord.shift {
        mask |= SHIFT_MASK;
    }
    if chord.ctrl {
        mask |= CTRL_MASK;
    }
    if chord.alt {
        mask |= ALT_MASK;
    }
    if chord.meta {
        mask |= META_MASK;
    }

    let evdev = chord.keycode.raw().saturating_sub(8);

    let t = now_ms();
    kb.modifiers(mask, 0, 0, 0);
    kb.key(t, evdev, KEY_PRESSED);
    kb.key(t + 1, evdev, KEY_RELEASED);
    kb.modifiers(0, 0, 0, 0);

    if let Some(conn) = state.conn.as_ref() {
        if let Err(e) = conn.flush() {
            warn!(%e, "failed to flush wayland after synthesizing chord");
        }
    }
}

fn now_ms() -> u32 {
    // Monotonic would be ideal but SystemTime works as an "undefined
    // base" per the protocol. u32 wraps at ~49.7 days uptime, matching
    // the wl_keyboard.key convention.
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    ms as u32
}

fn make_keymap_fd(text: &str) -> std::io::Result<(OwnedFd, u32)> {
    let fd = memfd_create(
        "sola-virtual-keyboard-keymap",
        MemfdFlags::CLOEXEC | MemfdFlags::ALLOW_SEALING,
    )?;
    let mut file = std::fs::File::from(fd);
    file.write_all(text.as_bytes())?;
    // The keymap string must be NUL-terminated.
    file.write_all(&[0])?;
    let size = (text.len() + 1) as u32;
    Ok((OwnedFd::from(file), size))
}

impl Dispatch<ZwpVirtualKeyboardManagerV1, ()> for AppData {
    fn event(
        _: &mut Self,
        _: &ZwpVirtualKeyboardManagerV1,
        _: <ZwpVirtualKeyboardManagerV1 as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // Manager emits no events.
    }
}

impl Dispatch<ZwpVirtualKeyboardV1, ()> for AppData {
    fn event(
        _: &mut Self,
        _: &ZwpVirtualKeyboardV1,
        _: zwp_virtual_keyboard_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // Virtual keyboard emits no events.
    }
}
