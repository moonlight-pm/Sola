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
    keyboard::{Key, Modifiers, key::Named},
    mouse,
};

use crate::cef::engine::InputEvent;

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
pub fn button_number(b: mouse::Button) -> Option<u32> {
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
/// `keyboard_codes.h`). Printable ASCII (letters, digits,
/// punctuation) + common named keys. Punctuation uses US-layout
/// OEM VKs; the CHAR event carries the real glyph. Returns `None`
/// only when we have neither a VK nor a CHAR to send.
pub fn key_to_vk(key: &Key) -> Option<u32> {
    Some(match key {
        Key::Character(s) => {
            let mut chars = s.chars();
            let first = chars.next()?;
            if chars.next().is_some() {
                return None;
            }
            character_to_vk(first)?
        }
        Key::Named(n) => named_to_vk(*n)?,
        Key::Unidentified => return None,
    })
}

/// US-layout OEM VKs for punctuation so RAWKEYDOWN is not dropped.
/// CHAR still carries the produced character (`.` vs `>` etc.).
fn character_to_vk(c: char) -> Option<u32> {
    let upper = c.to_ascii_uppercase();
    Some(match upper {
        'A'..='Z' => upper as u32, // VK_A..VK_Z
        '0'..='9' => upper as u32, // VK_0..VK_9
        ' ' => 0x20,               // VK_SPACE
        '.' | '>' => 0xBE,         // VK_OEM_PERIOD
        ',' | '<' => 0xBC,         // VK_OEM_COMMA
        '-' | '_' => 0xBD,         // VK_OEM_MINUS
        '=' | '+' => 0xBB,         // VK_OEM_PLUS
        ';' | ':' => 0xBA,         // VK_OEM_1
        '/' | '?' => 0xBF,         // VK_OEM_2
        '`' | '~' => 0xC0,         // VK_OEM_3
        '[' | '{' => 0xDB,         // VK_OEM_4
        '\\' | '|' => 0xDC,        // VK_OEM_5
        ']' | '}' => 0xDD,         // VK_OEM_6
        '\'' | '"' => 0xDE,        // VK_OEM_7
        '!' => b'1' as u32,
        '@' => b'2' as u32,
        '#' => b'3' as u32,
        '$' => b'4' as u32,
        '%' => b'5' as u32,
        '^' => b'6' as u32,
        '&' => b'7' as u32,
        '*' => b'8' as u32,
        '(' => b'9' as u32,
        ')' => b'0' as u32,
        _ => return None,
    })
}

/// iced key event → CEF `InputEvent::Key`. Sends a CHAR-capable
/// event even when there is no VK (Unidentified + `text`, or a
/// non-Latin glyph) so punctuation / composed characters are not
/// dropped on the floor.
pub fn translate_key(
    down: bool,
    key: &Key,
    text_first: Option<char>,
    modifiers: Modifiers,
) -> Option<InputEvent> {
    let character = if down {
        key_to_character(text_first, key)
    } else {
        None
    };
    let vk = key_to_vk(key).or_else(|| character.map(|c| c as u32))?;
    Some(InputEvent::Key {
        down,
        vk,
        character,
        modifiers: modifiers_to_cef(modifiers),
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

pub use crate::input::CursorKind;

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

pub const MULTI_CLICK_MS: u128 = 500;
pub const MULTI_CLICK_SLOP_PX: i32 = 4;

/// Next click count for a press. `prev` is `(button, x, y, elapsed_ms, count)`
/// of the last press, if any.
pub fn next_click_count(
    prev: Option<(u32, i32, i32, u128, u32)>,
    button: u32,
    x: i32,
    y: i32,
) -> u32 {
    let Some((pb, px, py, elapsed, count)) = prev else {
        return 1;
    };
    if pb != button || elapsed > MULTI_CLICK_MS {
        return 1;
    }
    if (x - px).abs() > MULTI_CLICK_SLOP_PX || (y - py).abs() > MULTI_CLICK_SLOP_PX {
        return 1;
    }
    count.saturating_add(1).min(3)
}

pub fn pointer_button(
    down: bool,
    button: u32,
    x: i32,
    y: i32,
    held: u32,
    kbd_mods: u32,
    click_count: u32,
) -> InputEvent {
    InputEvent::PointerButton {
        down,
        x,
        y,
        button,
        modifiers: kbd_mods | held,
        click_count: click_count.max(1),
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

/// Shift+wheel (no explicit X) is horizontal in every desktop browser.
pub fn apply_shift_scroll(delta_x: i32, delta_y: i32, shift: bool) -> (i32, i32) {
    if shift && delta_x == 0 && delta_y != 0 {
        (delta_y, 0)
    } else {
        (delta_x, delta_y)
    }
}

pub fn pointer_leave(x: i32, y: i32, held: u32, kbd_mods: u32) -> InputEvent {
    InputEvent::PointerLeave {
        x,
        y,
        modifiers: kbd_mods | held,
    }
}

/// iced preedit selection is UTF-8 bytes; CEF IME ranges are UTF-16 units.
pub fn utf8_to_utf16_index(text: &str, byte: usize) -> u32 {
    let b = byte.min(text.len());
    text.get(..b)
        .map(|s| s.encode_utf16().count() as u32)
        .unwrap_or(0)
}

pub fn ime_set_composition(
    text: String,
    selection: Option<std::ops::Range<usize>>,
) -> InputEvent {
    let (from, to) = match selection {
        Some(r) => (
            utf8_to_utf16_index(&text, r.start),
            utf8_to_utf16_index(&text, r.end),
        ),
        None => {
            let end = text.encode_utf16().count() as u32;
            (end, end)
        }
    };
    InputEvent::ImeSetComposition {
        text,
        selection_from: from,
        selection_to: to,
    }
}

pub fn ime_commit(text: String) -> InputEvent {
    InputEvent::ImeCommit { text }
}

pub fn ime_cancel() -> InputEvent {
    InputEvent::ImeCancel
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
    }

    #[test]
    fn click_count_triples_then_caps() {
        assert_eq!(next_click_count(None, 1, 10, 10), 1);
        assert_eq!(next_click_count(Some((1, 10, 10, 80, 1)), 1, 11, 10), 2);
        assert_eq!(next_click_count(Some((1, 11, 10, 80, 2)), 1, 10, 11), 3);
        assert_eq!(next_click_count(Some((1, 10, 11, 80, 3)), 1, 10, 10), 3);
    }

    #[test]
    fn click_count_resets_on_gap_or_move() {
        assert_eq!(next_click_count(Some((1, 10, 10, 800, 2)), 1, 10, 10), 1);
        assert_eq!(next_click_count(Some((1, 10, 10, 80, 2)), 1, 40, 10), 1);
        assert_eq!(next_click_count(Some((1, 10, 10, 80, 2)), 3, 10, 10), 1);
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
        assert_eq!(button_number(mouse::Button::Middle), None);
        assert_eq!(button_number(mouse::Button::Left), Some(1));
        assert_eq!(button_number(mouse::Button::Right), Some(3));
    }

    #[test]
    fn punctuation_has_oem_vk() {
        // Period was dropped entirely (no VK → no CHAR). Must map.
        let period = Key::Character(".".into());
        assert_eq!(key_to_vk(&period), Some(0xBE));
        let comma = Key::Character(",".into());
        assert_eq!(key_to_vk(&comma), Some(0xBC));
        let slash = Key::Character("/".into());
        assert_eq!(key_to_vk(&slash), Some(0xBF));
    }

    #[test]
    fn translate_key_sends_char_for_period() {
        let ev = translate_key(true, &Key::Character(".".into()), Some('.'), Modifiers::empty())
            .expect("period must produce a key event");
        match ev {
            InputEvent::Key {
                down,
                vk,
                character,
                ..
            } => {
                assert!(down);
                assert_eq!(vk, 0xBE);
                assert_eq!(character, Some('.' as u16));
            }
            other => panic!("expected Key, got {other:?}"),
        }
    }

    #[test]
    fn translate_key_unidentified_uses_text() {
        // Some compositors give Unidentified + text only.
        let ev = translate_key(true, &Key::Unidentified, Some('.'), Modifiers::empty())
            .expect("text-only period must not be dropped");
        match ev {
            InputEvent::Key {
                character, vk, ..
            } => {
                assert_eq!(character, Some('.' as u16));
                assert_eq!(vk, '.' as u32);
            }
            other => panic!("expected Key, got {other:?}"),
        }
    }

    #[test]
    fn shift_scroll_becomes_horizontal() {
        assert_eq!(apply_shift_scroll(0, 120, true), (120, 0));
        assert_eq!(apply_shift_scroll(0, 120, false), (0, 120));
        assert_eq!(apply_shift_scroll(40, 120, true), (40, 120));
    }

    #[test]
    fn utf8_to_utf16_handles_multibyte() {
        // "é" is 2 UTF-8 bytes, 1 UTF-16 unit.
        assert_eq!(utf8_to_utf16_index("éx", 0), 0);
        assert_eq!(utf8_to_utf16_index("éx", 2), 1);
        assert_eq!(utf8_to_utf16_index("éx", 3), 2);
    }

    #[test]
    fn ime_set_maps_byte_selection() {
        let ev = ime_set_composition("éx".into(), Some(0..2));
        match ev {
            InputEvent::ImeSetComposition {
                selection_from,
                selection_to,
                ..
            } => {
                assert_eq!((selection_from, selection_to), (0, 1));
            }
            other => panic!("expected ImeSetComposition, got {other:?}"),
        }
    }
}
