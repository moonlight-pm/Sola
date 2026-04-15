//! Shared core primitives for Sola.
//!
//! This crate centralizes low-level key primitives used across apps and the
//! compositor so we avoid scattered magic numbers.

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

    // --- Alphanumeric ---
    pub const KEY_0: Self = Self(19);
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

    // --- Numpad used by zoning ---
    pub const KP_0: Self = Self(90);
    pub const KP_2: Self = Self(88);
    pub const KP_4: Self = Self(83);
    pub const KP_5: Self = Self(84);
    pub const KP_6: Self = Self(85);
    pub const KP_8: Self = Self(80);

    /// Returns true if this key code is either left or right Meta.
    #[inline]
    pub const fn is_meta(self) -> bool {
        self.0 == Self::LEFT_META.0 || self.0 == Self::RIGHT_META.0
    }
}

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
}
