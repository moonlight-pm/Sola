//! Shared input scaffolding for the browser shader / chrome.
//!
//! The engine keeps keymaps and native event constructors; this module
//! owns the cursor vocabulary and coordinate projection helpers.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use iced::keyboard::{Key, Modifiers};
use iced::{Point, Rectangle, mouse};

const M_SHIFT: u8 = 1;
const M_CTRL: u8 = 2;
const M_ALT: u8 = 4;
const M_LOGO: u8 = 8;

/// Last iced keyboard modifiers, written from the chrome subscription so
/// the page shader sees Super/Ctrl even when a chrome field has focus.
static LAST_MODS: AtomicU8 = AtomicU8::new(0);
/// Super/Meta key down, even when `ModifiersChanged` never set LOGO
/// (compositor often eats Super as its own modifier).
static SUPER_HELD: AtomicBool = AtomicBool::new(false);

pub fn store_modifiers(m: Modifiers) {
    let mut bits = 0u8;
    if m.shift() {
        bits |= M_SHIFT;
    }
    if m.control() {
        bits |= M_CTRL;
    }
    if m.alt() {
        bits |= M_ALT;
    }
    if m.logo() {
        bits |= M_LOGO;
    }
    LAST_MODS.store(bits, Ordering::Relaxed);
}

pub fn stored_modifiers() -> Modifiers {
    let bits = LAST_MODS.load(Ordering::Relaxed);
    let mut m = Modifiers::empty();
    if bits & M_SHIFT != 0 {
        m |= Modifiers::SHIFT;
    }
    if bits & M_CTRL != 0 {
        m |= Modifiers::CTRL;
    }
    if bits & M_ALT != 0 {
        m |= Modifiers::ALT;
    }
    if bits & M_LOGO != 0 || SUPER_HELD.load(Ordering::Relaxed) {
        m |= Modifiers::LOGO;
    }
    m
}

/// Track Super/Meta by key name. Call from chrome + the page shader.
pub fn note_super_key(down: bool) {
    SUPER_HELD.store(down, Ordering::Relaxed);
}

/// XK_Super_L / XK_Super_R. River delivers these as `Topic::Chord` because
/// bare Super_L is a registered binding (switcher confirm) — iced never sees
/// the key, so [`note_super_key`] would stay false without the bus path.
pub const KEYSYM_SUPER_L: u32 = 0xFFEB;
pub const KEYSYM_SUPER_R: u32 = 0xFFEC;

pub fn is_super_keysym(keysym: u32) -> bool {
    keysym == KEYSYM_SUPER_L || keysym == KEYSYM_SUPER_R
}

/// Apply a River chord press/release to [`SUPER_HELD`]. Returns whether this
/// was a Super key (other chords are ignored).
pub fn apply_super_chord(pressed: bool, keysym: u32) -> bool {
    if !is_super_keysym(keysym) {
        return false;
    }
    note_super_key(pressed);
    true
}

pub fn is_super_key(key: &Key) -> bool {
    matches!(
        key,
        Key::Named(iced::keyboard::key::Named::Super)
            | Key::Named(iced::keyboard::key::Named::Meta)
    )
}

/// Chrome-owned edit chords (⌘C/X/V/A/Z/Y). The shell routes these via
/// the Edit menu; they must not also reach CEF or the page double-applies.
pub fn is_chrome_edit_shortcut(key: &Key, mods: Modifiers) -> bool {
    if !mods.logo() {
        return false;
    }
    let Key::Character(s) = key else {
        return false;
    };
    matches!(
        s.chars().next().map(|c| c.to_ascii_lowercase()),
        Some('c' | 'x' | 'v' | 'a' | 'z' | 'y')
    )
}

/// Browser-menu chords that chrome should handle even if the bus
/// `MenuAction` path is down: Super+R reload, Super+T/W/L, Super+F find,
/// Super+G find next.
pub fn chrome_nav_shortcut(key: &Key, mods: Modifiers) -> Option<char> {
    if !mods.logo() || mods.alt() || mods.shift() || mods.control() {
        return None;
    }
    let Key::Character(s) = key else {
        return None;
    };
    match s.chars().next().map(|c| c.to_ascii_lowercase()) {
        Some(c @ ('r' | 't' | 'w' | 'l' | 'g' | 'f')) => Some(c),
        _ => None,
    }
}

/// Super+Shift+T — reopen the most recently closed tab (LIFO stack).
pub fn is_reopen_closed_shortcut(key: &Key, mods: Modifiers) -> bool {
    if !mods.logo() || !mods.shift() || mods.alt() || mods.control() {
        return false;
    }
    let Key::Character(s) = key else {
        return false;
    };
    s.eq_ignore_ascii_case("t")
}

pub fn is_chrome_nav_shortcut(key: &Key, mods: Modifiers) -> bool {
    chrome_nav_shortcut(key, mods).is_some()
        || is_reopen_closed_shortcut(key, mods)
        || is_find_prev_shortcut(key, mods)
}

/// Super+Shift+G — find previous.
pub fn is_find_prev_shortcut(key: &Key, mods: Modifiers) -> bool {
    if !mods.logo() || !mods.shift() || mods.alt() || mods.control() {
        return false;
    }
    let Key::Character(s) = key else {
        return false;
    };
    s.eq_ignore_ascii_case("g")
}

/// Cursor shape carried across the worker→iced boundary as a plain `u32`
/// (via `AtomicU32`). Discriminants are stable; new variants append.
#[repr(u32)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
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

/// Project a window-local cursor point into the webview's device-pixel
/// space (the size last sent via `Cmd::Resize`).
pub fn project_cursor_f64(point: Point, bounds: Rectangle, scale: f32) -> (f64, f64) {
    let x = ((point.x - bounds.x).max(0.0) * scale) as f64;
    let y = ((point.y - bounds.y).max(0.0) * scale) as f64;
    (x, y)
}

/// Same as [`project_cursor_f64`] but integer pixels.
pub fn project_cursor_i32(point: Point, bounds: Rectangle, scale: f32) -> (i32, i32) {
    let (x, y) = project_cursor_f64(point, bounds, scale);
    (x as i32, y as i32)
}

/// Derive the scale factor the shader last requested from widget bounds
/// width vs last physical size.
pub fn scale_from_last_size(bounds: Rectangle, last_req_w: u32, fallback: f32) -> f32 {
    if bounds.width > 0.0 {
        (last_req_w as f32 / bounds.width).max(0.5)
    } else {
        fallback.max(0.5)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_roundtrip() {
        assert_eq!(CursorKind::from_u32(1), CursorKind::Pointer);
        assert_eq!(CursorKind::from_u32(99), CursorKind::Default);
    }

    #[test]
    fn chrome_edit_shortcut_is_logo_plus_letter() {
        assert!(is_chrome_edit_shortcut(
            &Key::Character("v".into()),
            Modifiers::LOGO
        ));
        assert!(is_chrome_edit_shortcut(
            &Key::Character("V".into()),
            Modifiers::LOGO
        ));
        assert!(!is_chrome_edit_shortcut(
            &Key::Character("v".into()),
            Modifiers::empty()
        ));
        assert!(!is_chrome_edit_shortcut(
            &Key::Character("t".into()),
            Modifiers::LOGO
        ));
        assert_eq!(
            chrome_nav_shortcut(&Key::Character("r".into()), Modifiers::LOGO),
            Some('r')
        );
        assert_eq!(
            chrome_nav_shortcut(&Key::Character("g".into()), Modifiers::LOGO),
            Some('g')
        );
        assert_eq!(
            chrome_nav_shortcut(&Key::Character("f".into()), Modifiers::LOGO),
            Some('f')
        );
        assert!(is_find_prev_shortcut(
            &Key::Character("g".into()),
            Modifiers::LOGO | Modifiers::SHIFT
        ));
        assert!(is_chrome_nav_shortcut(
            &Key::Character("G".into()),
            Modifiers::LOGO
        ));
        assert!(is_chrome_nav_shortcut(
            &Key::Character("g".into()),
            Modifiers::LOGO | Modifiers::SHIFT
        ));
        assert!(is_chrome_nav_shortcut(
            &Key::Character("R".into()),
            Modifiers::LOGO
        ));
        assert!(!is_chrome_nav_shortcut(
            &Key::Character("r".into()),
            Modifiers::empty()
        ));
        assert!(is_reopen_closed_shortcut(
            &Key::Character("t".into()),
            Modifiers::LOGO | Modifiers::SHIFT
        ));
        assert!(is_reopen_closed_shortcut(
            &Key::Character("T".into()),
            Modifiers::LOGO | Modifiers::SHIFT
        ));
        assert!(!is_reopen_closed_shortcut(
            &Key::Character("t".into()),
            Modifiers::LOGO
        ));
        assert!(is_chrome_nav_shortcut(
            &Key::Character("t".into()),
            Modifiers::LOGO | Modifiers::SHIFT
        ));
    }

    #[test]
    fn stored_modifiers_roundtrip() {
        store_modifiers(Modifiers::LOGO | Modifiers::SHIFT);
        let m = stored_modifiers();
        assert!(m.logo());
        assert!(m.shift());
        assert!(!m.control());
        store_modifiers(Modifiers::empty());
        assert_eq!(stored_modifiers(), Modifiers::empty());
        note_super_key(true);
        assert!(stored_modifiers().logo());
        note_super_key(false);
        assert!(!stored_modifiers().logo());
    }

    #[test]
    fn super_keysym_is_l_or_r() {
        assert!(is_super_keysym(KEYSYM_SUPER_L));
        assert!(is_super_keysym(KEYSYM_SUPER_R));
        assert!(!is_super_keysym(0xFF09)); // Tab
    }

    #[test]
    fn bus_super_chord_sets_held() {
        note_super_key(false);
        assert!(apply_super_chord(true, KEYSYM_SUPER_L));
        assert!(stored_modifiers().logo());
        assert!(apply_super_chord(false, KEYSYM_SUPER_L));
        assert!(!stored_modifiers().logo());
        assert!(!apply_super_chord(true, 0x20)); // Space
        assert!(!stored_modifiers().logo());
    }
}
