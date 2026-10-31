//! XKB key code and chord types shared across Sola components.

use serde::{Deserialize, Serialize};

/// XKB key code wrapper (evdev + 8 in the current stack).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct KeyCode(pub u32);

impl KeyCode {
    /// Raw numeric key code.
    #[inline]
    pub const fn raw(self) -> u32 {
        self.0
    }

    /// Start a chord with this key and no modifiers.
    #[inline]
    pub const fn chord(self) -> KeyChord {
        KeyChord::new(self)
    }

    /// Start a chord with Meta enabled.
    ///
    /// Example:
    /// `KeyCode::T.meta().shift()`
    #[inline]
    pub const fn meta(self) -> KeyChord {
        self.chord().meta()
    }

    /// Start a chord with Alt enabled.
    #[inline]
    pub const fn alt(self) -> KeyChord {
        self.chord().alt()
    }

    /// Start a chord with Ctrl enabled.
    #[inline]
    pub const fn ctrl(self) -> KeyChord {
        self.chord().ctrl()
    }

    /// Start a chord with Shift enabled.
    #[inline]
    pub const fn shift(self) -> KeyChord {
        self.chord().shift()
    }

    /// Start a chord with Meta+Shift enabled — the standard modifier
    /// for Sola's split/close-pane shortcuts (`⌘⇧→`, `⌘⇧W`, …).
    #[inline]
    pub const fn meta_shift(self) -> KeyChord {
        self.chord().meta().shift()
    }

    // --- Modifiers ---
    pub const LEFT_CTRL: Self = Self(37);
    pub const RIGHT_CTRL: Self = Self(105);

    pub const LEFT_SHIFT: Self = Self(50);
    pub const RIGHT_SHIFT: Self = Self(62);

    pub const LEFT_ALT: Self = Self(64);
    pub const RIGHT_ALT: Self = Self(108);

    pub const LEFT_META: Self = Self(133);
    pub const RIGHT_META: Self = Self(134);

    // --- Navigation / editing ---
    pub const ESCAPE: Self = Self(9);
    pub const BACKSPACE: Self = Self(22);
    pub const TAB: Self = Self(23);
    pub const ENTER: Self = Self(36);

    pub const LEFT: Self = Self(113);
    pub const RIGHT: Self = Self(114);
    pub const UP: Self = Self(111);
    pub const DOWN: Self = Self(116);
    pub const SPACE: Self = Self(65);
    pub const GRAVE: Self = Self(49);

    pub const BRACKET_LEFT: Self = Self(34);
    pub const BRACKET_RIGHT: Self = Self(35);
    pub const SEMICOLON: Self = Self(47);
    pub const APOSTROPHE: Self = Self(48);
    pub const BACKSLASH: Self = Self(51);
    pub const COMMA: Self = Self(59);
    pub const PERIOD: Self = Self(60);
    pub const SLASH: Self = Self(61);

    pub const HOME: Self = Self(110);
    pub const PAGE_UP: Self = Self(112);
    pub const END: Self = Self(115);
    pub const PAGE_DOWN: Self = Self(117);
    pub const INSERT: Self = Self(118);
    pub const DELETE: Self = Self(119);
    pub const PRINT: Self = Self(107);

    // --- Function keys (X11 keycodes; evdev + 8) ---
    pub const F1: Self = Self(67);
    pub const F2: Self = Self(68);
    pub const F3: Self = Self(69);
    pub const F4: Self = Self(70);
    pub const F5: Self = Self(71);
    pub const F6: Self = Self(72);
    pub const F7: Self = Self(73);
    pub const F8: Self = Self(74);
    pub const F9: Self = Self(75);
    pub const F10: Self = Self(76);
    pub const F11: Self = Self(95);
    pub const F12: Self = Self(96);

    // --- Alphanumeric ---
    pub const KEY_0: Self = Self(19);
    /// `-` / `_` (XKB = Linux KEY_MINUS + 8).
    pub const MINUS: Self = Self(20);
    /// `=` / `+` (XKB = Linux KEY_EQUAL + 8).
    pub const EQUAL: Self = Self(21);
    pub const KEY_1: Self = Self(10);
    pub const KEY_2: Self = Self(11);
    pub const KEY_3: Self = Self(12);
    pub const KEY_4: Self = Self(13);
    pub const KEY_5: Self = Self(14);
    pub const KEY_6: Self = Self(15);
    pub const KEY_7: Self = Self(16);
    pub const KEY_8: Self = Self(17);
    pub const KEY_9: Self = Self(18);
    pub const KP_EQUAL: Self = Self(125);

    pub const A: Self = Self(38);
    pub const B: Self = Self(56);
    pub const C: Self = Self(54);
    pub const D: Self = Self(40);
    pub const E: Self = Self(26);
    pub const F: Self = Self(41);
    pub const G: Self = Self(42);
    pub const H: Self = Self(43);
    pub const I: Self = Self(31);
    pub const J: Self = Self(44);
    pub const K: Self = Self(45);
    pub const L: Self = Self(46);
    pub const M: Self = Self(58);
    pub const N: Self = Self(57);
    pub const O: Self = Self(32);
    pub const P: Self = Self(33);
    pub const Q: Self = Self(24);
    pub const R: Self = Self(27);
    pub const S: Self = Self(39);
    pub const T: Self = Self(28);
    pub const U: Self = Self(30);
    pub const V: Self = Self(55);
    pub const W: Self = Self(25);
    pub const X: Self = Self(53);
    pub const Y: Self = Self(29);
    pub const Z: Self = Self(52);

    // --- Number pad ---
    pub const KP_0: Self = Self(90);
    pub const KP_1: Self = Self(87);
    pub const KP_2: Self = Self(88);
    pub const KP_3: Self = Self(89);
    pub const KP_4: Self = Self(83);
    pub const KP_5: Self = Self(84);
    pub const KP_6: Self = Self(85);
    pub const KP_7: Self = Self(79);
    pub const KP_8: Self = Self(80);
    pub const KP_9: Self = Self(81);
    pub const KP_DECIMAL: Self = Self(91);
    /// Numpad Enter — Cinema's default key.
    pub const KP_ENTER: Self = Self(104);
    /// Numpad `*` (KP_Multiply) — Floating's default key.
    pub const KP_MULTIPLY: Self = Self(63);
    /// Numpad `+` (KP_Add). XKB keycode = Linux KEY_KPPLUS (78) + 8.
    pub const KP_ADD: Self = Self(86);
    pub const KP_SUBTRACT: Self = Self(82);
    pub const KP_DIVIDE: Self = Self(106);

    /// Returns true if this key code is either left or right Meta.
    #[inline]
    pub const fn is_meta(self) -> bool {
        self.0 == Self::LEFT_META.0 || self.0 == Self::RIGHT_META.0
    }

    /// Short human-readable label suitable for menu shortcuts: a
    /// Mac-style glyph for navigation keys, the character itself for
    /// alphanumerics, "?" for a key outside [`KEYS`].
    pub fn display(self) -> &'static str {
        self.key().map_or("?", |k| k.label)
    }

    /// This key's row in [`KEYS`].
    pub fn key(self) -> Option<&'static Key> {
        KEYS.iter().find(|k| k.code == self)
    }

    /// The xkbcommon keysym River matches for this key (US layout).
    pub fn keysym(self) -> Option<u32> {
        self.key().map(|k| k.sym)
    }

    /// The other keysym of a number-pad key: what it sends with NumLock
    /// off (KP_4 is KP_Left). Bindings register both.
    pub fn keysym_alt(self) -> Option<u32> {
        self.key().and_then(|k| k.alt)
    }

    /// The key that sends `sym` (either of a number-pad key's keysyms).
    pub fn from_keysym(sym: u32) -> Option<Self> {
        KEYS.iter()
            .find(|k| k.sym == sym || k.alt == Some(sym))
            .map(|k| k.code)
    }
}

/// One key Sola can bind: its XKB keycode, the keysym River matches
/// (US layout), the NumLock-off keysym of a number-pad key, and its name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Key {
    pub code: KeyCode,
    pub sym: u32,
    pub alt: Option<u32>,
    pub label: &'static str,
}

const fn key(code: KeyCode, sym: u32, label: &'static str) -> Key {
    Key {
        code,
        sym,
        alt: None,
        label,
    }
}

const fn kp(code: KeyCode, sym: u32, alt: u32, label: &'static str) -> Key {
    Key {
        code,
        sym,
        alt: Some(alt),
        label,
    }
}

/// Every key a binding can use. The one table for keysyms (both ways) and
/// names; modifiers are not keys here.
pub const KEYS: &[Key] = &[
    key(KeyCode::ESCAPE, 0xFF1B, "Esc"),
    key(KeyCode::BACKSPACE, 0xFF08, "\u{232B}"), // ⌫
    key(KeyCode::TAB, 0xFF09, "\u{21E5}"),       // ⇥
    key(KeyCode::ENTER, 0xFF0D, "\u{21A9}"),     // ↩
    key(KeyCode::SPACE, 0x20, "Space"),
    key(KeyCode::LEFT, 0xFF51, "\u{2190}"),  // ←
    key(KeyCode::UP, 0xFF52, "\u{2191}"),    // ↑
    key(KeyCode::RIGHT, 0xFF53, "\u{2192}"), // →
    key(KeyCode::DOWN, 0xFF54, "\u{2193}"),  // ↓
    key(KeyCode::HOME, 0xFF50, "Home"),
    key(KeyCode::END, 0xFF57, "End"),
    key(KeyCode::PAGE_UP, 0xFF55, "PgUp"),
    key(KeyCode::PAGE_DOWN, 0xFF56, "PgDn"),
    key(KeyCode::INSERT, 0xFF63, "Ins"),
    key(KeyCode::DELETE, 0xFFFF, "\u{2326}"), // ⌦
    key(KeyCode::PRINT, 0xFF61, "PrtSc"),
    // The digit row.
    key(KeyCode::KEY_1, 0x31, "1"),
    key(KeyCode::KEY_2, 0x32, "2"),
    key(KeyCode::KEY_3, 0x33, "3"),
    key(KeyCode::KEY_4, 0x34, "4"),
    key(KeyCode::KEY_5, 0x35, "5"),
    key(KeyCode::KEY_6, 0x36, "6"),
    key(KeyCode::KEY_7, 0x37, "7"),
    key(KeyCode::KEY_8, 0x38, "8"),
    key(KeyCode::KEY_9, 0x39, "9"),
    key(KeyCode::KEY_0, 0x30, "0"),
    key(KeyCode::MINUS, 0x2D, "-"),
    key(KeyCode::EQUAL, 0x3D, "="),
    key(KeyCode::GRAVE, 0x60, "`"),
    // Punctuation.
    key(KeyCode::BRACKET_LEFT, 0x5B, "["),
    key(KeyCode::BRACKET_RIGHT, 0x5D, "]"),
    key(KeyCode::SEMICOLON, 0x3B, ";"),
    key(KeyCode::APOSTROPHE, 0x27, "'"),
    key(KeyCode::BACKSLASH, 0x5C, "\\"),
    key(KeyCode::COMMA, 0x2C, ","),
    key(KeyCode::PERIOD, 0x2E, "."),
    key(KeyCode::SLASH, 0x2F, "/"),
    // Letters (their keysyms are lower case).
    key(KeyCode::A, 0x61, "A"),
    key(KeyCode::B, 0x62, "B"),
    key(KeyCode::C, 0x63, "C"),
    key(KeyCode::D, 0x64, "D"),
    key(KeyCode::E, 0x65, "E"),
    key(KeyCode::F, 0x66, "F"),
    key(KeyCode::G, 0x67, "G"),
    key(KeyCode::H, 0x68, "H"),
    key(KeyCode::I, 0x69, "I"),
    key(KeyCode::J, 0x6A, "J"),
    key(KeyCode::K, 0x6B, "K"),
    key(KeyCode::L, 0x6C, "L"),
    key(KeyCode::M, 0x6D, "M"),
    key(KeyCode::N, 0x6E, "N"),
    key(KeyCode::O, 0x6F, "O"),
    key(KeyCode::P, 0x70, "P"),
    key(KeyCode::Q, 0x71, "Q"),
    key(KeyCode::R, 0x72, "R"),
    key(KeyCode::S, 0x73, "S"),
    key(KeyCode::T, 0x74, "T"),
    key(KeyCode::U, 0x75, "U"),
    key(KeyCode::V, 0x76, "V"),
    key(KeyCode::W, 0x77, "W"),
    key(KeyCode::X, 0x78, "X"),
    key(KeyCode::Y, 0x79, "Y"),
    key(KeyCode::Z, 0x7A, "Z"),
    // Function keys.
    key(KeyCode::F1, 0xFFBE, "F1"),
    key(KeyCode::F2, 0xFFBF, "F2"),
    key(KeyCode::F3, 0xFFC0, "F3"),
    key(KeyCode::F4, 0xFFC1, "F4"),
    key(KeyCode::F5, 0xFFC2, "F5"),
    key(KeyCode::F6, 0xFFC3, "F6"),
    key(KeyCode::F7, 0xFFC4, "F7"),
    key(KeyCode::F8, 0xFFC5, "F8"),
    key(KeyCode::F9, 0xFFC6, "F9"),
    key(KeyCode::F10, 0xFFC7, "F10"),
    key(KeyCode::F11, 0xFFC8, "F11"),
    key(KeyCode::F12, 0xFFC9, "F12"),
    // The number pad: NumLock on, then off.
    kp(KeyCode::KP_0, 0xFFB0, 0xFF9E, "KP0"), // KP_Insert
    kp(KeyCode::KP_1, 0xFFB1, 0xFF9C, "KP1"), // KP_End
    kp(KeyCode::KP_2, 0xFFB2, 0xFF99, "KP2"), // KP_Down
    kp(KeyCode::KP_3, 0xFFB3, 0xFF9B, "KP3"), // KP_Next
    kp(KeyCode::KP_4, 0xFFB4, 0xFF96, "KP4"), // KP_Left
    kp(KeyCode::KP_5, 0xFFB5, 0xFF9D, "KP5"), // KP_Begin
    kp(KeyCode::KP_6, 0xFFB6, 0xFF98, "KP6"), // KP_Right
    kp(KeyCode::KP_7, 0xFFB7, 0xFF95, "KP7"), // KP_Home
    kp(KeyCode::KP_8, 0xFFB8, 0xFF97, "KP8"), // KP_Up
    kp(KeyCode::KP_9, 0xFFB9, 0xFF9A, "KP9"), // KP_Prior
    kp(KeyCode::KP_DECIMAL, 0xFFAE, 0xFF9F, "KP."), // KP_Delete
    key(KeyCode::KP_ENTER, 0xFF8D, "KP\u{21A9}"),
    key(KeyCode::KP_MULTIPLY, 0xFFAA, "KP*"),
    key(KeyCode::KP_ADD, 0xFFAB, "KP+"),
    key(KeyCode::KP_SUBTRACT, 0xFFAD, "KP-"),
    key(KeyCode::KP_DIVIDE, 0xFFAF, "KP/"),
    key(KeyCode::KP_EQUAL, 0xFFBD, "KP="),
];

impl From<u32> for KeyCode {
    #[inline]
    fn from(value: u32) -> Self {
        Self(value)
    }
}

impl From<KeyCode> for u32 {
    #[inline]
    fn from(value: KeyCode) -> Self {
        value.0
    }
}

/// Keyboard chord: key + modifiers.
///
/// Designed for ergonomics with a fluent builder style:
/// `KeyCode::T.meta().shift()`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct KeyChord {
    pub keycode: KeyCode,
    pub meta: bool,
    pub alt: bool,
    pub ctrl: bool,
    pub shift: bool,
}

impl KeyChord {
    #[inline]
    pub const fn new(keycode: KeyCode) -> Self {
        Self {
            keycode,
            meta: false,
            alt: false,
            ctrl: false,
            shift: false,
        }
    }

    #[inline]
    pub const fn meta(mut self) -> Self {
        self.meta = true;
        self
    }

    #[inline]
    pub const fn alt(mut self) -> Self {
        self.alt = true;
        self
    }

    #[inline]
    pub const fn ctrl(mut self) -> Self {
        self.ctrl = true;
        self
    }

    #[inline]
    pub const fn shift(mut self) -> Self {
        self.shift = true;
        self
    }

    /// Convenience for consumers that still need numeric key code values.
    #[inline]
    pub const fn raw_keycode(self) -> u32 {
        self.keycode.raw()
    }

    /// Mac-style display string: ⌃⌥⇧⌘ prefixes followed by the key label.
    pub fn display(&self) -> String {
        let mut s = String::new();
        if self.ctrl {
            s.push('\u{2303}'); // ⌃
        }
        if self.alt {
            s.push('\u{2325}'); // ⌥
        }
        if self.shift {
            s.push('\u{21E7}'); // ⇧
        }
        if self.meta {
            s.push('\u{2318}'); // ⌘
        }
        s.push_str(self.keycode.display());
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kp_multiply_has_xkb_code_and_label() {
        assert_eq!(KeyCode::KP_MULTIPLY.raw(), 63);
        assert_eq!(KeyCode::KP_MULTIPLY.display(), "KP*");
    }

    #[test]
    fn kp_add_has_xkb_code_and_label() {
        assert_eq!(KeyCode::KP_ADD.raw(), 86);
        assert_eq!(KeyCode::KP_ADD.display(), "KP+");
    }

    #[test]
    fn every_key_is_listed_once() {
        for (i, k) in KEYS.iter().enumerate() {
            assert!(
                KEYS[i + 1..].iter().all(|o| o.code != k.code),
                "{} twice",
                k.label
            );
            assert!(
                KEYS[i + 1..]
                    .iter()
                    .all(|o| o.sym != k.sym && Some(o.sym) != k.alt && o.alt.is_none_or(|a| a != k.sym && Some(a) != k.alt)),
                "{}'s keysym twice",
                k.label
            );
        }
    }

    #[test]
    fn keysyms_round_trip() {
        for k in KEYS {
            assert_eq!(KeyCode::from_keysym(k.sym), Some(k.code));
            if let Some(alt) = k.alt {
                assert_eq!(KeyCode::from_keysym(alt), Some(k.code));
            }
        }
        assert_eq!(KeyCode::EQUAL.keysym(), Some(0x3D));
        assert_eq!(KeyCode::MINUS.keysym(), Some(0x2D));
        assert_eq!(KeyCode::KP_4.keysym_alt(), Some(0xFF96));
    }

    #[test]
    fn meta_shift_sets_both_modifiers() {
        let chord = KeyCode::W.meta_shift();
        assert!(chord.meta && chord.shift);
        assert!(!chord.ctrl && !chord.alt);
        assert_eq!(chord.keycode, KeyCode::W);
    }

    #[test]
    fn arrow_keycodes_display_glyphs() {
        assert_eq!(KeyCode::UP.display(), "\u{2191}");
        assert_eq!(KeyCode::DOWN.display(), "\u{2193}");
        assert_eq!(KeyCode::LEFT.display(), "\u{2190}");
        assert_eq!(KeyCode::RIGHT.display(), "\u{2192}");
    }

    #[test]
    fn chord_display_renders_mac_modifiers() {
        // Mac order is ⌃⌥⇧⌘ followed by the key label.
        assert_eq!(
            KeyCode::RIGHT.meta_shift().display(),
            "\u{21E7}\u{2318}\u{2192}"
        );
        assert_eq!(KeyCode::C.meta().display(), "\u{2318}C");
        assert_eq!(KeyCode::W.meta_shift().display(), "\u{21E7}\u{2318}W");
    }
}
