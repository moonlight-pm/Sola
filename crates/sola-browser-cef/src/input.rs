//! iced → CEF input event translation.
//!
//! Same shape as `sola-browser-wpe::input`, different sink:
//! CEF wants `MouseEvent` / `KeyEvent` with Windows VK_ codes
//! (per Chromium's `ui/events/keycodes/keyboard_codes.h`, mirrored
//! from Win32 `VK_*`) and `EVENTFLAG_*` modifier bits. Per CEF docs
//! a key press is three events: `RAWKEYDOWN` → optional `CHAR`
//! (for printable input) → `KEYUP`.

use cef::sys::cef_event_flags_t as F;
use iced::{
    Point, Rectangle,
    keyboard::{Key, Modifiers, key::Named},
    mouse,
};

use crate::engine::InputEvent;

// ── modifiers ───────────────────────────────────────────────────

/// iced `Modifiers` → CEF `EVENTFLAG_*` bitset.
pub fn modifiers_to_cef(m: Modifiers) -> u32 {
    let mut out: u32 = 0;
    if m.shift() {
        out |= F::EVENTFLAG_SHIFT_DOWN.0;
    }
    if m.control() {
        out |= F::EVENTFLAG_CONTROL_DOWN.0;
    }
    if m.alt() {
        out |= F::EVENTFLAG_ALT_DOWN.0;
    }
    if m.logo() {
        out |= F::EVENTFLAG_COMMAND_DOWN.0;
    }
    out
}

/// Like [`modifiers_to_cef`] but for mouse events: a held Super/⌘ (`logo`)
/// is also reported to CEF as CONTROL. On Linux, Chromium's "open link in a
/// new tab" disposition keys off CONTROL (COMMAND is the macOS modifier and
/// is ignored), so without this a ⌘-click never produces a popup and
/// `on_before_popup` never fires. Keyboard events keep the literal mapping
/// ([`modifiers_to_cef`]) so ⌘-shortcuts don't masquerade as Ctrl-shortcuts.
pub fn modifiers_to_cef_mouse(m: Modifiers) -> u32 {
    let mut out = modifiers_to_cef(m);
    if m.logo() {
        out |= F::EVENTFLAG_CONTROL_DOWN.0;
    }
    out
}

/// CEF's `EVENTFLAG_*_MOUSE_BUTTON` bit for a given button number.
/// We use the same 1-2-3 = L-M-R convention internally as on the
/// WPE side; the mapping to CEF's bits is fixed.
pub fn button_to_modifier(button: u32) -> u32 {
    match button {
        1 => F::EVENTFLAG_LEFT_MOUSE_BUTTON.0,
        2 => F::EVENTFLAG_MIDDLE_MOUSE_BUTTON.0,
        3 => F::EVENTFLAG_RIGHT_MOUSE_BUTTON.0,
        _ => 0,
    }
}

// ── pointer ─────────────────────────────────────────────────────

/// iced `mouse::Button` → our internal 1/2/3 button number.
/// (Translated to a CEF `MouseButtonType` at dispatch time.)
pub fn button_to_wpe_like(b: mouse::Button) -> Option<u32> {
    Some(match b {
        mouse::Button::Left => 1,
        // Middle button is intentionally inert in Sola — drop it so it never
        // reaches CEF (no middle-click new-tab popup, no autoscroll).
        mouse::Button::Middle => return None,
        mouse::Button::Right => 3,
        // CEF doesn't have explicit Back/Forward button events at
        // the OSR seams; drop them silently for now.
        _ => return None,
    })
}

pub fn project_cursor(point: Point, bounds: Rectangle, scale: f32) -> (i32, i32) {
    let x = ((point.x - bounds.x).max(0.0) * scale) as i32;
    let y = ((point.y - bounds.y).max(0.0) * scale) as i32;
    (x, y)
}

pub fn scroll_delta_to_cef(d: mouse::ScrollDelta) -> (i32, i32, bool) {
    match d {
        // Non-precise (line) wheel events: CEF's `send_mouse_wheel_event`
        // expects WHEEL_DELTA units — ±120 per notch (the Windows
        // convention Chromium follows internally), NOT raw pixels. Sending
        // ~20 made one notch scroll only ~1/6 of a step, hence the sluggish
        // feel; one line → one notch.
        mouse::ScrollDelta::Lines { x, y } => {
            ((x * 120.0) as i32, (y * 120.0) as i32, false)
        }
        // Precise (high-resolution / touchpad) deltas are already pixel
        // amounts; CEF consumes them directly when the precision flag is set.
        mouse::ScrollDelta::Pixels { x, y } => (x as i32, y as i32, true),
    }
}

// ── keyboard ────────────────────────────────────────────────────

/// iced `Key` → Windows-style VK code (per Chromium's
/// `keyboard_codes.h`). Covers the same set as the WPE keysym
/// table — printable ASCII + common named keys. Returns `None`
/// for keys we can't yet map.
pub fn key_to_vk(key: &Key) -> Option<u32> {
    Some(match key {
        Key::Character(s) => {
            let mut chars = s.chars();
            let first = chars.next()?;
            if chars.next().is_some() {
                return None;
            }
            let c = first.to_ascii_uppercase();
            match c {
                'A'..='Z' => c as u32,           // VK_A..VK_Z = 0x41..0x5A
                '0'..='9' => c as u32,           // VK_0..VK_9 = 0x30..0x39
                ' ' => 0x20,                     // VK_SPACE
                // Numpad / punctuation keys don't have stable VK
                // codes across keyboards; rely on CHAR events to
                // carry the actual codepoint.
                _ => return None,
            }
        }
        Key::Named(n) => named_to_vk(*n)?,
        Key::Unidentified => return None,
    })
}

fn named_to_vk(n: Named) -> Option<u32> {
    Some(match n {
        Named::Backspace => 0x08, // VK_BACK
        Named::Tab => 0x09,
        Named::Enter => 0x0D, // VK_RETURN
        Named::Shift => 0x10,
        Named::Control => 0x11,
        Named::Alt => 0x12, // VK_MENU
        Named::CapsLock => 0x14,
        Named::Escape => 0x1B,
        Named::Space => 0x20,
        Named::PageUp => 0x21,   // VK_PRIOR
        Named::PageDown => 0x22, // VK_NEXT
        Named::End => 0x23,
        Named::Home => 0x24,
        Named::ArrowLeft => 0x25,
        Named::ArrowUp => 0x26,
        Named::ArrowRight => 0x27,
        Named::ArrowDown => 0x28,
        Named::Insert => 0x2D,
        Named::Delete => 0x2E,
        Named::F1 => 0x70,
        Named::F2 => 0x71,
        Named::F3 => 0x72,
        Named::F4 => 0x73,
        Named::F5 => 0x74,
        Named::F6 => 0x75,
        Named::F7 => 0x76,
        Named::F8 => 0x77,
        Named::F9 => 0x78,
        Named::F10 => 0x79,
        Named::F11 => 0x7A,
        Named::F12 => 0x7B,
        Named::Meta | Named::Super => 0x5B, // VK_LWIN
        _ => return None,
    })
}

/// Pull the printable character to send in a CHAR event. iced's
/// `KeyPressed.text` already has the post-shift / post-IME
/// result; prefer it when present, fall back to `Key::Character`.
/// We take `Option<char>` so callers can deref any string-ish
/// thing into a leading codepoint without us depending on iced's
/// specific text type.
pub fn key_to_character(text_first: Option<char>, key: &Key) -> Option<u16> {
    if let Some(c) = text_first {
        return Some(c as u16);
    }
    if let Key::Character(s) = key {
        return s.chars().next().map(|c| c as u16);
    }
    None
}

// ── cursor ──────────────────────────────────────────────────────

/// Mirror of WPE's CursorKind — shape stays the same across both
/// crates so iced widgets / chrome can be engine-agnostic in
/// future. Discriminants are stable; new variants append.
#[repr(u32)]
#[derive(Copy, Clone, Debug, Default)]
pub enum CursorKind {
    #[default]
    Default = 0,
    Pointer = 1,
    Text = 2,
    Grab = 3,
    Grabbing = 4,
    Crosshair = 5,
    Move = 6,
    NotAllowed = 7,
    ResizingHorizontally = 8,
    ResizingVertically = 9,
    Working = 10,
}

impl CursorKind {
    pub fn from_u32(v: u32) -> Self {
        match v {
            1 => CursorKind::Pointer,
            2 => CursorKind::Text,
            3 => CursorKind::Grab,
            4 => CursorKind::Grabbing,
            5 => CursorKind::Crosshair,
            6 => CursorKind::Move,
            7 => CursorKind::NotAllowed,
            8 => CursorKind::ResizingHorizontally,
            9 => CursorKind::ResizingVertically,
            10 => CursorKind::Working,
            _ => CursorKind::Default,
        }
    }

    pub fn to_iced(self) -> mouse::Interaction {
        match self {
            CursorKind::Default => mouse::Interaction::default(),
            CursorKind::Pointer => mouse::Interaction::Pointer,
            CursorKind::Text => mouse::Interaction::Text,
            CursorKind::Grab => mouse::Interaction::Grab,
            CursorKind::Grabbing => mouse::Interaction::Grabbing,
            CursorKind::Crosshair => mouse::Interaction::Crosshair,
            CursorKind::Move => mouse::Interaction::Move,
            CursorKind::NotAllowed => mouse::Interaction::NotAllowed,
            CursorKind::ResizingHorizontally => mouse::Interaction::ResizingHorizontally,
            CursorKind::ResizingVertically => mouse::Interaction::ResizingVertically,
            CursorKind::Working => mouse::Interaction::Wait,
        }
    }
}

/// CEF's `CursorType` → our `CursorKind`. CEF's enum is bigger
/// (many resize directions, copy/alias variants, …) — we collapse
/// what iced can't directly represent into the closest match.
pub fn cef_cursor_to_kind(cursor_type: cef::CursorType) -> CursorKind {
    use cef::sys::cef_cursor_type_t as C;
    match *cursor_type.as_ref() {
        C::CT_POINTER => CursorKind::Default,
        C::CT_CROSS => CursorKind::Crosshair,
        C::CT_HAND => CursorKind::Pointer,
        C::CT_IBEAM | C::CT_VERTICALTEXT => CursorKind::Text,
        C::CT_WAIT | C::CT_PROGRESS => CursorKind::Working,
        C::CT_NOTALLOWED | C::CT_NODROP => CursorKind::NotAllowed,
        C::CT_GRAB => CursorKind::Grab,
        C::CT_GRABBING => CursorKind::Grabbing,
        C::CT_MOVE | C::CT_MIDDLEPANNING => CursorKind::Move,
        C::CT_EASTRESIZE
        | C::CT_WESTRESIZE
        | C::CT_EASTWESTRESIZE
        | C::CT_COLUMNRESIZE => CursorKind::ResizingHorizontally,
        C::CT_NORTHRESIZE
        | C::CT_SOUTHRESIZE
        | C::CT_NORTHSOUTHRESIZE
        | C::CT_ROWRESIZE => CursorKind::ResizingVertically,
        _ => CursorKind::Default,
    }
}

// ── helpers used by shader::Program::update ─────────────────────

pub fn pointer_move(
    x: i32,
    y: i32,
    held: u32,
    kbd_mods: u32,
) -> InputEvent {
    InputEvent::PointerMove {
        x,
        y,
        modifiers: kbd_mods | held,
    }
}

pub fn pointer_button(
    down: bool,
    button: u32,
    x: i32,
    y: i32,
    held: u32,
    kbd_mods: u32,
) -> InputEvent {
    InputEvent::PointerButton {
        down,
        x,
        y,
        button,
        modifiers: kbd_mods | held,
    }
}

pub fn scroll(
    x: i32,
    y: i32,
    delta_x: i32,
    delta_y: i32,
    precise: bool,
    held: u32,
    kbd_mods: u32,
) -> InputEvent {
    InputEvent::Scroll {
        x,
        y,
        delta_x,
        delta_y,
        precise,
        modifiers: kbd_mods | held,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mouse_super_maps_to_control_for_new_tab() {
        // ⌘/Super held during a click must reach CEF as CONTROL so Chromium's
        // Linux "open in new tab" disposition fires (COMMAND alone does not).
        let m = modifiers_to_cef_mouse(Modifiers::LOGO);
        assert_ne!(m & F::EVENTFLAG_CONTROL_DOWN.0, 0, "super must set CONTROL");
        // The literal COMMAND bit is still present; it's just harmless on Linux.
        assert_ne!(m & F::EVENTFLAG_COMMAND_DOWN.0, 0);
    }

    #[test]
    fn mouse_plain_has_no_synthetic_control() {
        // No modifiers → no CONTROL (a plain click must not look like ctrl-click).
        assert_eq!(modifiers_to_cef_mouse(Modifiers::empty()), 0);
        // Real Ctrl still maps through.
        assert_ne!(
            modifiers_to_cef_mouse(Modifiers::CTRL) & F::EVENTFLAG_CONTROL_DOWN.0,
            0
        );
    }

    #[test]
    fn keyboard_super_stays_command_only() {
        // The keyboard path must NOT synthesize CONTROL, or ⌘-shortcuts would
        // masquerade as Ctrl-shortcuts in the page.
        let m = modifiers_to_cef(Modifiers::LOGO);
        assert_eq!(m & F::EVENTFLAG_CONTROL_DOWN.0, 0);
        assert_ne!(m & F::EVENTFLAG_COMMAND_DOWN.0, 0);
    }

    #[test]
    fn line_scroll_uses_wheel_delta_units() {
        // One wheel notch (one "line") → one WHEEL_DELTA step (120), not pixels.
        let (dx, dy, precise) = scroll_delta_to_cef(mouse::ScrollDelta::Lines { x: 0.0, y: 1.0 });
        assert_eq!((dx, dy), (0, 120));
        assert!(!precise);
    }

    #[test]
    fn middle_button_is_inert() {
        assert_eq!(button_to_wpe_like(mouse::Button::Middle), None);
        assert_eq!(button_to_wpe_like(mouse::Button::Left), Some(1));
        assert_eq!(button_to_wpe_like(mouse::Button::Right), Some(3));
    }
}
