//! The keyboard: `wl_keyboard` through xkbcommon, turned into edits of the
//! password. No repeat: holding a key types it once.

use std::os::fd::OwnedFd;

use xkbcommon::xkb::{self, keysyms};

/// What a key press does to the password.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Edit {
    Type(String),
    Erase,
    Clear,
    Submit,
}

pub struct Keys {
    ctx: xkb::Context,
    state: Option<xkb::State>,
}

impl Keys {
    pub fn new() -> Self {
        Self {
            ctx: xkb::Context::new(xkb::CONTEXT_NO_FLAGS),
            state: None,
        }
    }

    /// The compositor's keymap (`wl_keyboard.keymap`, xkb v1 text).
    pub fn keymap(&mut self, fd: OwnedFd, size: u32) {
        let keymap = unsafe {
            xkb::Keymap::new_from_fd(
                &self.ctx,
                fd,
                size as usize,
                xkb::KEYMAP_FORMAT_TEXT_V1,
                xkb::KEYMAP_COMPILE_NO_FLAGS,
            )
        };
        match keymap {
            Ok(Some(k)) => self.state = Some(xkb::State::new(&k)),
            Ok(None) => tracing::warn!("keymap did not compile"),
            Err(e) => tracing::warn!("keymap: {e}"),
        }
    }

    pub fn modifiers(&mut self, depressed: u32, latched: u32, locked: u32, group: u32) {
        if let Some(s) = &mut self.state {
            s.update_mask(depressed, latched, locked, 0, 0, group);
        }
    }

    /// A press of evdev `key` (`wl_keyboard.key`); releases change nothing.
    pub fn press(&self, key: u32) -> Option<Edit> {
        let s = self.state.as_ref()?;
        let code = xkb::Keycode::new(key + 8);
        let sym = s.key_get_one_sym(code).raw();
        let ctrl = s.mod_name_is_active(xkb::MOD_NAME_CTRL, xkb::STATE_MODS_EFFECTIVE);
        match sym {
            keysyms::KEY_Return | keysyms::KEY_KP_Enter => Some(Edit::Submit),
            keysyms::KEY_BackSpace => Some(Edit::Erase),
            keysyms::KEY_Escape => Some(Edit::Clear),
            keysyms::KEY_u | keysyms::KEY_U if ctrl => Some(Edit::Clear),
            _ if ctrl => None,
            _ => {
                let text = s.key_get_utf8(code);
                (!text.is_empty() && !text.chars().any(char::is_control)).then_some(Edit::Type(text))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn us() -> Option<Keys> {
        let ctx = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
        let keymap =
            xkb::Keymap::new_from_names(&ctx, "", "", "us", "", None, xkb::KEYMAP_COMPILE_NO_FLAGS)?;
        Some(Keys {
            state: Some(xkb::State::new(&keymap)),
            ctx,
        })
    }

    #[test]
    fn keys_edit_the_password() {
        let Some(mut k) = us() else {
            eprintln!("no xkeyboard-config here; skipped");
            return;
        };
        const A: u32 = 30;
        const ENTER: u32 = 28;
        const BACKSPACE: u32 = 14;
        const ESC: u32 = 1;
        const U: u32 = 22;
        assert_eq!(k.press(A), Some(Edit::Type("a".into())));
        assert_eq!(k.press(ENTER), Some(Edit::Submit));
        assert_eq!(k.press(BACKSPACE), Some(Edit::Erase));
        assert_eq!(k.press(ESC), Some(Edit::Clear));
        let shift = 1; // Shift is the keymap's first modifier
        k.modifiers(shift, 0, 0, 0);
        assert_eq!(k.press(A), Some(Edit::Type("A".into())));
        let ctrl = 1 << 2;
        k.modifiers(ctrl, 0, 0, 0);
        assert_eq!(k.press(U), Some(Edit::Clear));
        assert_eq!(k.press(A), None);
    }
}
