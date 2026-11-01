//! The key button's glyph: the vault's state at a glance. Unlocked, the
//! key in full ink; locked, the key dimmed with a small lock on its
//! corner; signed out (or the service away), the key dimmed alone; a
//! fingerprint while a site waits for a passkey.

use std::sync::OnceLock;

use iced::widget::{stack, svg};
use iced::{Color, Element};
use sola_kit::components::icon::icon_svg_colored;

use super::super::Gate;
use super::glyph;
use crate::app::Msg;

/// What the key button shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KeyLook {
    Passkey,
    Unlocked,
    Locked,
    /// Signed out, signing in, or the service away.
    Out,
}

impl KeyLook {
    pub fn of(gate: Gate, passkey: bool) -> Self {
        if passkey {
            return Self::Passkey;
        }
        match gate {
            Gate::Open => Self::Unlocked,
            Gate::Locked => Self::Locked,
            Gate::Away | Gate::Connecting | Gate::SignIn | Gate::TwoStep => Self::Out,
        }
    }

    /// In full ink (else the bar's muted ink, unless the sheet is up).
    fn lit(self) -> bool {
        matches!(self, Self::Passkey | Self::Unlocked)
    }
}

/// Lucide's `key-round`, cut away at its lower right where the lock sits.
const KEY_NOTCHED: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><mask id="notch"><rect width="24" height="24" fill="white" stroke="none"/><rect x="10.5" y="10.5" width="14" height="14" rx="4" fill="black" stroke="none"/></mask><g mask="url(#notch)"><path d="M2.586 17.414A2 2 0 0 0 2 18.828V21a1 1 0 0 0 1 1h3a1 1 0 0 0 1-1v-1a1 1 0 0 1 1-1h1a1 1 0 0 0 1-1v-1a1 1 0 0 1 1-1h.172a2 2 0 0 0 1.414-.586l.814-.814a6.5 6.5 0 1 0-4-4z"/><circle cx="16.5" cy="7.5" r=".5" fill="currentColor"/></g></svg>"#;

/// Lucide's `lock` at half size in the same corner, its body filled to
/// read at a toolbar glyph's size.
const LOCK_MARK: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round"><g transform="translate(12 12) scale(0.5)" stroke-width="4"><rect width="18" height="11" x="3" y="11" rx="2" ry="2" fill="currentColor"/><path d="M7 11V7a5 5 0 0 1 10 0v4"/></g></svg>"#;

fn handle(cell: &'static OnceLock<svg::Handle>, src: &'static str) -> svg::Handle {
    cell.get_or_init(|| svg::Handle::from_memory(src.as_bytes())).clone()
}

/// The glyph at `size`: `ink` for what is lit and the lock, `muted` for
/// the dimmed key (`open`: the sheet is up, and the key is in ink).
pub(crate) fn glyph_for<'a>(look: KeyLook, size: u16, ink: Color, muted: Color, open: bool) -> Element<'a, Msg> {
    static NOTCHED: OnceLock<svg::Handle> = OnceLock::new();
    static LOCK: OnceLock<svg::Handle> = OnceLock::new();
    let key_ink = if look.lit() || open { ink } else { muted };
    match look {
        KeyLook::Passkey => icon_svg_colored(glyph("fingerprint-pattern"), size, ink),
        KeyLook::Unlocked | KeyLook::Out => icon_svg_colored(glyph("key-round"), size, key_ink),
        KeyLook::Locked => stack![
            icon_svg_colored(handle(&NOTCHED, KEY_NOTCHED), size, key_ink),
            icon_svg_colored(handle(&LOCK, LOCK_MARK), size, ink),
        ]
        .into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_out_locked_and_unlocked_look_apart() {
        assert_eq!(KeyLook::of(Gate::Open, false), KeyLook::Unlocked);
        assert_eq!(KeyLook::of(Gate::Locked, false), KeyLook::Locked);
        for out in [Gate::SignIn, Gate::TwoStep, Gate::Away, Gate::Connecting] {
            assert_eq!(KeyLook::of(out, false), KeyLook::Out);
        }
        assert_eq!(KeyLook::of(Gate::Locked, true), KeyLook::Passkey);
        assert!(KeyLook::Unlocked.lit() && !KeyLook::Locked.lit() && !KeyLook::Out.lit());
    }

    #[test]
    fn the_marks_are_whole_svgs() {
        for src in [KEY_NOTCHED, LOCK_MARK] {
            assert!(src.starts_with("<svg") && src.ends_with("</svg>"));
            assert!(src.contains(r#"viewBox="0 0 24 24""#));
            assert!(!src.contains("=\"#"), "no colour of its own: the theme tints it");
        }
    }
}
