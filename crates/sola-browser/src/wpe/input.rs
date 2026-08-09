//! iced → WPE input event translation.
//!
//! iced 0.14 surfaces input via `shader::Program::update`. The
//! events it carries (`iced::core::Event`) speak in terms of
//! winit-shaped logical/physical keys and CSS-pixel cursor
//! positions. WPE's `WPEEvent` family wants X11-style keysyms,
//! evdev keycodes, and view-local pixel positions.
//!
//! This module is purely translation — no FFI, no threading. The
//! returned `InputEvent`s go through the existing
//! `mpsc::Sender<Cmd>` to the WPE worker, which calls into the
//! `wpe_event_*_new` constructors and `wpe_view_event`.

use iced::{
    keyboard::{self, Key, Modifiers, key::Named},
    mouse,
};

use super::engine::InputEvent;
use super::wpe_sys as sys;

/// Translate iced's modifier bitset to WPE's. Pointer-button modifier
/// bits are *not* set here — WPE expects only keyboard-side modifiers
/// from this function; the worker doesn't track held mouse buttons.
pub fn modifiers_to_wpe(m: Modifiers) -> u32 {
    let mut out: u32 = 0;
    if m.shift() {
        out |= sys::WPEModifiers_WPE_MODIFIER_KEYBOARD_SHIFT;
    }
    if m.control() {
        out |= sys::WPEModifiers_WPE_MODIFIER_KEYBOARD_CONTROL;
    }
    if m.alt() {
        out |= sys::WPEModifiers_WPE_MODIFIER_KEYBOARD_ALT;
    }
    if m.logo() {
        out |= sys::WPEModifiers_WPE_MODIFIER_KEYBOARD_META;
    }
    out
}

/// Iced's `mouse::Button` → WPE's button number convention.
/// X11 numbering: 1 = left, 2 = middle, 3 = right. The middle button is
/// intentionally dropped (`None`) so it never reaches WebKit — Sola doesn't
/// use middle-click for anything.
pub fn button_to_wpe(b: mouse::Button) -> Option<u32> {
    Some(match b {
        mouse::Button::Left => 1,
        mouse::Button::Middle => return None,
        mouse::Button::Right => 3,
        mouse::Button::Back => 8,
        mouse::Button::Forward => 9,
        mouse::Button::Other(n) => n as u32,
    })
}

/// Map a WPE button number to its `WPE_MODIFIER_POINTER_BUTTONn`
/// bit. WPE's seat tracks which buttons are *currently held* in
/// the modifier set of every `PointerMove` event; the web engine
/// uses these bits to distinguish a drag (button held during
/// move) from a plain hover. Buttons outside 1–5 produce 0,
/// meaning "no button modifier" — fine since those aren't in
/// the modifier-bit ABI.
pub fn button_to_modifier(button: u32) -> u32 {
    match button {
        1 => sys::WPEModifiers_WPE_MODIFIER_POINTER_BUTTON1,
        2 => sys::WPEModifiers_WPE_MODIFIER_POINTER_BUTTON2,
        3 => sys::WPEModifiers_WPE_MODIFIER_POINTER_BUTTON3,
        4 => sys::WPEModifiers_WPE_MODIFIER_POINTER_BUTTON4,
        5 => sys::WPEModifiers_WPE_MODIFIER_POINTER_BUTTON5,
        _ => 0,
    }
}

/// Iced scroll-wheel `ScrollDelta` → (delta_x, delta_y, precise).
/// CSS pixels for pixel deltas (precise = true). For line deltas
/// we multiply by ~20 to approximate "one line ≈ 20 px" which is
/// roughly what desktop browsers use.
pub fn scroll_delta_to_wpe(d: mouse::ScrollDelta) -> (f64, f64, bool) {
    match d {
        mouse::ScrollDelta::Lines { x, y } => (x as f64 * 20.0, y as f64 * 20.0, false),
        mouse::ScrollDelta::Pixels { x, y } => (x as f64, y as f64, true),
    }
}

/// Iced logical `Key` → X11 keysym (`XK_*`). Covers printable
/// ASCII and the most common named keys; returns `None` for keys
/// we don't (yet) translate. Web content is forgiving — many keys
/// "work" with just the right keyval even without a hardware
/// scancode — so this thin table is enough for typing into form
/// fields, navigating with arrows / tab / enter, and common
/// editor shortcuts. Extend as needed.
pub fn key_to_keysym(key: &Key) -> Option<u32> {
    Some(match key {
        // Printable text: SmolStr from iced, ASCII first codepoint
        // maps directly to keysym for the latin1 range. Multi-codepoint
        // text (emoji, IME) falls back to None — we'd need a proper
        // input-method bridge for that.
        Key::Character(s) => {
            let mut chars = s.chars();
            let first = chars.next()?;
            if chars.next().is_some() {
                return None;
            }
            let code = first as u32;
            if code < 0x100 {
                code
            } else if (0x01000000..=0x0110ffff).contains(&(code | 0x01000000)) {
                // Per X11 spec, codepoints > 0xff use the 0x01000000
                // prefix convention for arbitrary Unicode.
                0x01000000 | code
            } else {
                return None;
            }
        }
        Key::Named(named) => named_key_to_keysym(*named)?,
        // Unidentified keys come up for things like macOS function
        // keys; ignore.
        Key::Unidentified => return None,
    })
}

/// Subset of `keyboard::key::Named` → X11 keysym. Values from
/// `<X11/keysymdef.h>`; the high bits identify keypad / control /
/// function-key groups, the low byte differentiates within a group.
fn named_key_to_keysym(n: Named) -> Option<u32> {
    Some(match n {
        Named::Backspace => 0xff08,
        Named::Tab => 0xff09,
        Named::Enter => 0xff0d,
        Named::Escape => 0xff1b,
        Named::Space => 0x0020,
        Named::Delete => 0xffff,
        Named::Insert => 0xff63,
        Named::Home => 0xff50,
        Named::End => 0xff57,
        Named::PageUp => 0xff55,
        Named::PageDown => 0xff56,
        Named::ArrowLeft => 0xff51,
        Named::ArrowUp => 0xff52,
        Named::ArrowRight => 0xff53,
        Named::ArrowDown => 0xff54,
        Named::F1 => 0xffbe,
        Named::F2 => 0xffbf,
        Named::F3 => 0xffc0,
        Named::F4 => 0xffc1,
        Named::F5 => 0xffc2,
        Named::F6 => 0xffc3,
        Named::F7 => 0xffc4,
        Named::F8 => 0xffc5,
        Named::F9 => 0xffc6,
        Named::F10 => 0xffc7,
        Named::F11 => 0xffc8,
        Named::F12 => 0xffc9,
        Named::Shift => 0xffe1,
        Named::Control => 0xffe3,
        Named::Alt => 0xffe9,
        Named::Meta | Named::Super => 0xffeb,
        Named::CapsLock => 0xffe5,
        _ => return None,
    })
}

/// Compose an `InputEvent::Key` from iced's KeyPressed/KeyReleased
/// payload. Returns `None` if the key has no keysym mapping yet —
/// the caller drops it silently.
pub fn keyboard_event_to_wpe(
    down: bool,
    key: &Key,
    modifiers: Modifiers,
    time_ms: u32,
) -> Option<InputEvent> {
    let keyval = key_to_keysym(key)?;
    Some(InputEvent::Key {
        down,
        keyval,
        keycode: 0, /* hardware scancode — left blank; WebKit primarily uses keyval */
        modifiers: modifiers_to_wpe(modifiers),
        time_ms,
    })
}

pub use crate::CursorKind;

/// Map a freedesktop CSS cursor name (the strings WebKit passes
/// to `wpe_view_set_cursor_from_name`) to a `CursorKind`. Coverage
/// is intentionally broad on aliases — many CSS keywords map to
/// the same visual cursor. Unknown names fall back to `Default`.
pub fn parse_cursor_name(name: &str) -> CursorKind {
    match name {
        "default" | "auto" | "context-menu" | "help" => CursorKind::Default,
        "pointer" => CursorKind::Pointer,
        "text" | "vertical-text" => CursorKind::Text,
        "wait" | "progress" => CursorKind::Working,
        "crosshair" | "cell" => CursorKind::Crosshair,
        "grab" | "all-scroll" => CursorKind::Grab,
        "grabbing" => CursorKind::Grabbing,
        "move" => CursorKind::Move,
        "not-allowed" | "no-drop" => CursorKind::NotAllowed,
        "ew-resize" | "col-resize" | "e-resize" | "w-resize" => {
            CursorKind::ResizingHorizontally
        }
        "ns-resize" | "row-resize" | "n-resize" | "s-resize" => {
            CursorKind::ResizingVertically
        }
        // Corner-resize and "copy" / "alias" cursors don't have
        // exact iced equivalents — fall through to default rather
        // than picking a wrong one.
        _ => CursorKind::Default,
    }
}

/// Convenience: pull the `keyboard::Event` discriminant we care
/// about and dispatch to `keyboard_event_to_wpe`.
pub fn translate_keyboard(
    ev: &keyboard::Event,
    time_ms: u32,
) -> Option<InputEvent> {
    match ev {
        keyboard::Event::KeyPressed {
            key, modifiers, ..
        } => keyboard_event_to_wpe(true, key, *modifiers, time_ms),
        keyboard::Event::KeyReleased {
            key, modifiers, ..
        } => keyboard_event_to_wpe(false, key, *modifiers, time_ms),
        // Modifier-only updates don't need a synthetic event — the
        // next keyboard event will carry the current modifier set.
        keyboard::Event::ModifiersChanged(_) => None,
    }
}
