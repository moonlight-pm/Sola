//! Linux evdev keycode → macOS inject target.
//!
//! Evdev codes: `/usr/include/linux/input-event-codes.h` (`KEY_*`).
//! Mac virtual key codes: `HIToolbox/Events.h` (`kVK_*`).
//! Media / brightness use NX_KEYTYPE_* aux control events (not kVK).

/// macOS virtual key code (`CGKeyCode` as u16).
pub type CgKeyCode = u16;

/// NX_KEYTYPE_* values for system-defined aux media keys (IOHID / NSEvent).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum NxMediaKey {
    SoundUp = 0,
    SoundDown = 1,
    BrightnessUp = 2,
    BrightnessDown = 3,
    Mute = 7,
    Play = 16,
    Next = 17,
    Previous = 18,
    IlluminationToggle = 23,
    IlluminationDown = 22,
    IlluminationUp = 21,
}

/// Where a Linux key should go on the Mac inject path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MacTarget {
    /// Normal `CGEventCreateKeyboardEvent` virtual key.
    Key(CgKeyCode),
    /// System-defined media/brightness aux key (NX_KEYTYPE).
    Media(NxMediaKey),
}

/// Map a Linux evdev keycode to a Mac inject target.
pub fn linux_to_mac(keycode: u32) -> Option<MacTarget> {
    if let Some(m) = linux_to_media(keycode) {
        return Some(MacTarget::Media(m));
    }
    linux_to_cg(keycode).map(MacTarget::Key)
}

/// Media / brightness / transport keys (not ordinary typing keys).
pub fn linux_to_media(keycode: u32) -> Option<NxMediaKey> {
    Some(match keycode {
        113 => NxMediaKey::Mute,               // KEY_MUTE
        114 => NxMediaKey::SoundDown,          // KEY_VOLUMEDOWN
        115 => NxMediaKey::SoundUp,            // KEY_VOLUMEUP
        163 => NxMediaKey::Next,               // KEY_NEXTSONG
        164 => NxMediaKey::Play,               // KEY_PLAYPAUSE
        165 => NxMediaKey::Previous,           // KEY_PREVIOUSSONG
        166 => NxMediaKey::Play,               // KEY_STOPCD → stop-ish; Play toggle is closest
        168 => NxMediaKey::Previous,           // KEY_REWIND
        208 => NxMediaKey::Next,               // KEY_FASTFORWARD / FORWARD
        224 => NxMediaKey::BrightnessDown,     // KEY_BRIGHTNESSDOWN
        225 => NxMediaKey::BrightnessUp,       // KEY_BRIGHTNESSUP
        228 => NxMediaKey::IlluminationToggle, // KEY_KBDILLUMTOGGLE
        229 => NxMediaKey::IlluminationDown,   // KEY_KBDILLUMDOWN
        230 => NxMediaKey::IlluminationUp,     // KEY_KBDILLUMUP
        _ => return None,
    })
}

/// Map a Linux evdev keycode to a Mac `CGKeyCode`.
/// Returns `None` for unmapped keys (caller should log + drop).
pub fn linux_to_cg(keycode: u32) -> Option<CgKeyCode> {
    // Match arms ordered roughly by input-event-codes.h numbering.
    Some(match keycode {
        // --- control row ---
        1 => 0x35,  // KEY_ESC → kVK_Escape
        2 => 0x12,  // KEY_1 → kVK_ANSI_1
        3 => 0x13,  // KEY_2
        4 => 0x14,  // KEY_3
        5 => 0x15,  // KEY_4
        6 => 0x17,  // KEY_5
        7 => 0x16,  // KEY_6
        8 => 0x1a,  // KEY_7
        9 => 0x1c,  // KEY_8
        10 => 0x19, // KEY_9
        11 => 0x1d, // KEY_0
        12 => 0x1b, // KEY_MINUS → kVK_ANSI_Minus
        13 => 0x18, // KEY_EQUAL → kVK_ANSI_Equal
        14 => 0x33, // KEY_BACKSPACE → kVK_Delete (backspace)
        15 => 0x30, // KEY_TAB → kVK_Tab

        // --- Q row ---
        16 => 0x0c, // KEY_Q
        17 => 0x0d, // KEY_W
        18 => 0x0e, // KEY_E
        19 => 0x0f, // KEY_R
        20 => 0x11, // KEY_T
        21 => 0x10, // KEY_Y
        22 => 0x20, // KEY_U
        23 => 0x22, // KEY_I
        24 => 0x1f, // KEY_O
        25 => 0x23, // KEY_P
        26 => 0x21, // KEY_LEFTBRACE → kVK_ANSI_LeftBracket
        27 => 0x1e, // KEY_RIGHTBRACE → kVK_ANSI_RightBracket
        28 => 0x24, // KEY_ENTER → kVK_Return
        29 => 0x3b, // KEY_LEFTCTRL → kVK_Control

        // --- A row ---
        30 => 0x00, // KEY_A → kVK_ANSI_A
        31 => 0x01, // KEY_S
        32 => 0x02, // KEY_D
        33 => 0x03, // KEY_F
        34 => 0x05, // KEY_G
        35 => 0x04, // KEY_H
        36 => 0x26, // KEY_J
        37 => 0x28, // KEY_K
        38 => 0x25, // KEY_L
        39 => 0x29, // KEY_SEMICOLON
        40 => 0x27, // KEY_APOSTROPHE → kVK_ANSI_Quote
        41 => 0x32, // KEY_GRAVE → kVK_ANSI_Grave
        42 => 0x38, // KEY_LEFTSHIFT → kVK_Shift
        43 => 0x2a, // KEY_BACKSLASH → kVK_ANSI_Backslash

        // --- Z row ---
        44 => 0x06, // KEY_Z
        45 => 0x07, // KEY_X
        46 => 0x08, // KEY_C
        47 => 0x09, // KEY_V
        48 => 0x0b, // KEY_B
        49 => 0x2d, // KEY_N
        50 => 0x2e, // KEY_M
        51 => 0x2b, // KEY_COMMA
        52 => 0x2f, // KEY_DOT → kVK_ANSI_Period
        53 => 0x2c, // KEY_SLASH
        54 => 0x3c, // KEY_RIGHTSHIFT → kVK_RightShift
        56 => 0x3a, // KEY_LEFTALT → kVK_Option
        57 => 0x31, // KEY_SPACE → kVK_Space
        58 => 0x39, // KEY_CAPSLOCK → kVK_CapsLock

        // --- F-keys ---
        59 => 0x7a, // KEY_F1 → kVK_F1
        60 => 0x78, // KEY_F2
        61 => 0x63, // KEY_F3
        62 => 0x76, // KEY_F4
        63 => 0x60, // KEY_F5
        64 => 0x61, // KEY_F6
        65 => 0x62, // KEY_F7
        66 => 0x64, // KEY_F8
        67 => 0x65, // KEY_F9
        68 => 0x6d, // KEY_F10
        87 => 0x67, // KEY_F11
        88 => 0x6f, // KEY_F12

        // --- right modifiers / meta ---
        97 => 0x3e,  // KEY_RIGHTCTRL → kVK_RightControl
        100 => 0x3d, // KEY_RIGHTALT → kVK_RightOption
        125 => 0x37, // KEY_LEFTMETA → kVK_Command (⌘)
        126 => 0x36, // KEY_RIGHTMETA → kVK_RightCommand

        // --- navigation ---
        102 => 0x73, // KEY_HOME → kVK_Home
        103 => 0x7e, // KEY_UP → kVK_UpArrow
        104 => 0x74, // KEY_PAGEUP → kVK_PageUp
        105 => 0x7b, // KEY_LEFT → kVK_LeftArrow
        106 => 0x7c, // KEY_RIGHT → kVK_RightArrow
        107 => 0x77, // KEY_END → kVK_End
        108 => 0x7d, // KEY_DOWN → kVK_DownArrow
        109 => 0x79, // KEY_PAGEDOWN → kVK_PageDown
        110 => 0x72, // KEY_INSERT → kVK_Help (closest; rare on Mac)
        111 => 0x75, // KEY_DELETE → kVK_ForwardDelete

        // ISO extra key (non-US backslash / §) — map to ISO section
        86 => 0x0a, // KEY_102ND → kVK_ISO_Section

        // --- keypad ---
        55 => 0x43, // KEY_KPASTERISK → kVK_ANSI_KeypadMultiply
        71 => 0x59, // KEY_KP7 → kVK_ANSI_Keypad7
        72 => 0x5b, // KEY_KP8
        73 => 0x5c, // KEY_KP9
        74 => 0x4e, // KEY_KPMINUS → kVK_ANSI_KeypadMinus
        75 => 0x56, // KEY_KP4
        76 => 0x57, // KEY_KP5
        77 => 0x58, // KEY_KP6
        78 => 0x45, // KEY_KPPLUS → kVK_ANSI_KeypadPlus
        79 => 0x53, // KEY_KP1
        80 => 0x54, // KEY_KP2
        81 => 0x55, // KEY_KP3
        82 => 0x52, // KEY_KP0
        83 => 0x41, // KEY_KPDOT → kVK_ANSI_KeypadDecimal
        96 => 0x4c, // KEY_KPENTER → kVK_ANSI_KeypadEnter
        98 => 0x4b, // KEY_KPSLASH → kVK_ANSI_KeypadDivide
        117 => 0x51, // KEY_KPEQUAL → kVK_ANSI_KeypadEquals

        // Volume also has kVK aliases; media path prefers NX, but these remain
        // available if a caller asks for a CG keycode only.
        113 => 0x4a, // KEY_MUTE → kVK_Mute
        114 => 0x49, // KEY_VOLUMEDOWN → kVK_VolumeDown
        115 => 0x48, // KEY_VOLUMEUP → kVK_VolumeUp

        _ => return None,
    })
}

/// Human-readable name for common Linux keycodes (logging only).
pub fn linux_key_name(keycode: u32) -> Option<&'static str> {
    Some(match keycode {
        1 => "ESC",
        14 => "BACKSPACE",
        15 => "TAB",
        28 => "ENTER",
        29 => "LCTRL",
        30 => "A",
        42 => "LSHIFT",
        54 => "RSHIFT",
        56 => "LALT",
        57 => "SPACE",
        96 => "KPENTER",
        97 => "RCTRL",
        100 => "RALT",
        103 => "UP",
        105 => "LEFT",
        106 => "RIGHT",
        108 => "DOWN",
        113 => "MUTE",
        114 => "VOLDOWN",
        115 => "VOLUP",
        125 => "LMETA",
        126 => "RMETA",
        163 => "NEXT",
        164 => "PLAYPAUSE",
        165 => "PREV",
        224 => "BRIGHTDOWN",
        225 => "BRIGHTUP",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_letters_a_z() {
        // Spot-check a few; full table is the match arms.
        assert_eq!(linux_to_cg(30), Some(0x00)); // A
        assert_eq!(linux_to_cg(48), Some(0x0b)); // B
        assert_eq!(linux_to_cg(46), Some(0x08)); // C
        assert_eq!(linux_to_cg(17), Some(0x0d)); // W
        assert_eq!(linux_to_cg(44), Some(0x06)); // Z
    }

    #[test]
    fn maps_modifiers() {
        assert_eq!(linux_to_cg(29), Some(0x3b)); // LCtrl
        assert_eq!(linux_to_cg(42), Some(0x38)); // LShift
        assert_eq!(linux_to_cg(56), Some(0x3a)); // LAlt / Option
        assert_eq!(linux_to_cg(125), Some(0x37)); // LMeta / Command
        assert_eq!(linux_to_cg(126), Some(0x36)); // RMeta
        assert_eq!(linux_to_cg(97), Some(0x3e)); // RCtrl
        assert_eq!(linux_to_cg(100), Some(0x3d)); // RAlt
        assert_eq!(linux_to_cg(54), Some(0x3c)); // RShift
    }

    #[test]
    fn maps_arrows_and_nav() {
        assert_eq!(linux_to_cg(103), Some(0x7e)); // Up
        assert_eq!(linux_to_cg(108), Some(0x7d)); // Down
        assert_eq!(linux_to_cg(105), Some(0x7b)); // Left
        assert_eq!(linux_to_cg(106), Some(0x7c)); // Right
        assert_eq!(linux_to_cg(102), Some(0x73)); // Home
        assert_eq!(linux_to_cg(107), Some(0x77)); // End
    }

    #[test]
    fn maps_space_escape_tab_enter() {
        assert_eq!(linux_to_cg(57), Some(0x31)); // Space
        assert_eq!(linux_to_cg(1), Some(0x35)); // Esc
        assert_eq!(linux_to_cg(15), Some(0x30)); // Tab
        assert_eq!(linux_to_cg(28), Some(0x24)); // Enter
    }

    #[test]
    fn maps_f_keys() {
        assert_eq!(linux_to_cg(59), Some(0x7a)); // F1
        assert_eq!(linux_to_cg(68), Some(0x6d)); // F10
        assert_eq!(linux_to_cg(87), Some(0x67)); // F11
        assert_eq!(linux_to_cg(88), Some(0x6f)); // F12
    }

    #[test]
    fn unmapped_returns_none() {
        assert_eq!(linux_to_cg(0), None);
        assert_eq!(linux_to_cg(9999), None);
    }

    #[test]
    fn maps_media_to_nx() {
        assert_eq!(linux_to_media(113), Some(NxMediaKey::Mute));
        assert_eq!(linux_to_media(114), Some(NxMediaKey::SoundDown));
        assert_eq!(linux_to_media(115), Some(NxMediaKey::SoundUp));
        assert_eq!(linux_to_media(164), Some(NxMediaKey::Play));
        assert_eq!(
            linux_to_mac(114),
            Some(MacTarget::Media(NxMediaKey::SoundDown))
        );
        assert_eq!(linux_to_mac(30), Some(MacTarget::Key(0x00))); // A
    }

    #[test]
    fn maps_keypad_enter() {
        assert_eq!(linux_to_cg(96), Some(0x4c));
    }
}
