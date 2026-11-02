//! The keys: Emacs, as Joshua has them. Meta is Super.
//!
//! [`BINDINGS`] is Zed's Emacs base keymap (`assets/keymaps/linux/emacs.json`)
//! trimmed to what is used, then the overrides from his Zed `keymap.json`:
//!
//! | Key | Here | Zed base |
//! |-----|------|----------|
//! | `M-v` | Page up, centered | page up |
//! | `C-v` | Page down, centered | page down |
//! | `C-l` | Center the cursor | center/top/bottom cycle |
//! | `C-y` | Paste the clipboard | yank the kill ring |
//! | `M-w` | Copy, then drop the mark | copy |
//! | `M-a` | Select all | |
//! | `M-/` | Toggle comment | |
//! | `M-S-p` | Command palette | |
//!
//! `M-` is Super, in every row ("i use SUPER as meta", so Super+W copies and
//! doesn't close the window). Alt counts as well, being Meta to everything else.
//! The shell only grabs a Super key an app has on its menu or one of its own
//! (Space, H, K, L, Q, Tab, the digits…), so a Super key on neither arrives
//! here like any other: keep these off the menus (`app::menus`, and its test).
//! The rest of Zed's override file names keys it already has, or popups an
//! editor without LSP doesn't show.
//!
//! Tune it here: one row is one binding.

use iced::keyboard::key::Named;
use iced::keyboard::{Key, Modifiers};

use crate::command::{Command, Motion};

/// `C-` Ctrl, `M-` Meta (Super), `S-` Shift, as Emacs writes them.
pub const BINDINGS: &[(&str, Command)] = {
    use Command::*;
    use Motion::*;
    &[
        // Moving.
        ("C-f", Move(Forward)),
        ("<right>", Move(Forward)),
        ("C-b", Move(Backward)),
        ("<left>", Move(Backward)),
        ("C-n", Move(Down)),
        ("<down>", Move(Down)),
        ("C-p", Move(Up)),
        ("<up>", Move(Up)),
        ("C-a", Move(LineStart)),
        ("<home>", Move(LineStart)),
        ("C-e", Move(LineEnd)),
        ("<end>", Move(LineEnd)),
        ("M-m", Move(IndentStart)),
        ("M-f", Move(WordForward)),
        ("M-<right>", Move(WordForward)),
        ("M-b", Move(WordBackward)),
        ("M-<left>", Move(WordBackward)),
        ("M-}", Move(ParagraphForward)),
        ("C-<down>", Move(ParagraphForward)),
        ("M-{", Move(ParagraphBackward)),
        ("C-<up>", Move(ParagraphBackward)),
        ("M-<", Move(BufferStart)),
        ("C-<home>", Move(BufferStart)),
        ("C-x [", Move(BufferStart)),
        ("M->", Move(BufferEnd)),
        ("C-<end>", Move(BufferEnd)),
        ("C-x ]", Move(BufferEnd)),
        ("C-v", Move(PageDown)),
        ("<next>", Move(PageDown)),
        ("M-v", Move(PageUp)),
        ("<prior>", Move(PageUp)),
        ("C-l", Center),
        // Shift extends the region.
        ("S-<right>", Select(Forward)),
        ("S-<left>", Select(Backward)),
        ("S-<down>", Select(Down)),
        ("S-<up>", Select(Up)),
        ("S-<home>", Select(LineStart)),
        ("S-<end>", Select(LineEnd)),
        ("S-<next>", Select(PageDown)),
        ("S-<prior>", Select(PageUp)),
        // The mark.
        ("C-<space>", SetMark),
        ("C-@", SetMark),
        ("C-g", Cancel),
        ("<escape>", Cancel),
        ("C-x C-x", SwapPointAndMark),
        ("C-x h", SelectAll),
        ("M-a", SelectAll),
        // Editing.
        ("<return>", Newline),
        ("S-<return>", Newline),
        ("<tab>", Tab),
        ("S-<tab>", Dedent),
        ("<backspace>", DeleteBackward),
        ("S-<backspace>", DeleteBackward),
        ("C-d", DeleteForward),
        ("<delete>", DeleteForward),
        ("M-d", KillWord),
        ("M-<backspace>", BackwardKillWord),
        ("M-<delete>", BackwardKillWord),
        ("C-k", KillLine),
        ("C-w", Cut),
        ("M-w", Copy),
        ("C-y", Yank),
        ("C-t", Transpose),
        ("M-/", ToggleComment),
        ("M-;", ToggleComment),
        ("C-x C-;", ToggleComment),
        ("C-/", Undo),
        ("C-_", Undo),
        ("C-x u", Undo),
        // Finding and going.
        ("C-s", Find),
        ("C-r", Find),
        ("M-g g", GotoLine),
        ("M-g M-g", GotoLine),
        ("M-S-p", Palette),
        ("M-x", Palette),
        // Files.
        ("C-x C-f", OpenFile),
        ("C-x C-s", Save),
        ("C-x C-w", SaveAs),
        ("C-x k", CloseFile),
        ("C-x C-c", Quit),
    ]
};

/// The modifier `key` is, if it is one: what tracks it going down and up.
pub fn modifier_bit(key: &Key) -> Option<Modifiers> {
    match key {
        Key::Named(Named::Shift) => Some(Modifiers::SHIFT),
        Key::Named(Named::Control) => Some(Modifiers::CTRL),
        Key::Named(Named::Alt) => Some(Modifiers::ALT),
        Key::Named(Named::Super | Named::Meta | Named::Hyper) => Some(Modifiers::LOGO),
        _ => None,
    }
}

/// What's held during a press, from three sources, as sola-terminal has it.
/// The press's own mask can miss a modifier that is down on this Wayland stack,
/// so the keys seen going down count too. What was last announced counts for
/// all but Super: River can eat Super's release, which would leave it stuck.
pub fn merged_modifiers(event: Modifiers, announced: Modifiers, held: Modifiers) -> Modifiers {
    event | held | (announced & (Modifiers::SHIFT | Modifiers::CTRL | Modifiers::ALT))
}

/// One key press, as bindings name it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stroke {
    pub ctrl: bool,
    /// Super, or Alt.
    pub meta: bool,
    pub shift: bool,
    pub key: StrokeKey,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StrokeKey {
    /// A letter in lower case (its Shift is in [`Stroke::shift`]), or the
    /// symbol the key made: `<` is its own key, with no Shift to say.
    Char(char),
    Named(Named),
}

impl Stroke {
    /// A press as iced reports it. `modified` is the key with Shift applied,
    /// so `M-<` and `C-_` work on any layout. `None` for a key no binding
    /// could name (a modifier alone, say).
    pub fn from_iced(modified: &Key, modifiers: Modifiers) -> Option<Self> {
        let shift = modifiers.shift();
        let (key, shift) = match modified {
            Key::Character(s) => {
                let mut chars = s.chars();
                let c = chars.next()?;
                if chars.next().is_some() {
                    return None;
                }
                if c == ' ' {
                    (StrokeKey::Named(Named::Space), shift)
                } else if c.is_alphabetic() {
                    (StrokeKey::Char(c.to_lowercase().next()?), shift)
                } else {
                    (StrokeKey::Char(c), false)
                }
            }
            // A modifier alone is not a press: `C-x`, then Ctrl going down
            // on the way to `C-s`, must not end the chord.
            Key::Named(
                Named::Shift
                | Named::Control
                | Named::Alt
                | Named::AltGraph
                | Named::Super
                | Named::Meta
                | Named::Hyper
                | Named::Fn
                | Named::FnLock
                | Named::CapsLock
                | Named::NumLock
                | Named::ScrollLock
                | Named::Symbol
                | Named::SymbolLock,
            ) => return None,
            Key::Named(named) => (StrokeKey::Named(*named), shift),
            Key::Unidentified => return None,
        };
        Some(Self {
            ctrl: modifiers.control(),
            meta: modifiers.logo() || modifiers.alt(),
            shift,
            key,
        })
    }

    /// Parse one key of a binding: `C-x`, `M-S-p`, `<return>`.
    pub fn parse(mut spec: &str) -> Option<Self> {
        let (mut ctrl, mut meta, mut shift) = (false, false, false);
        loop {
            match spec.as_bytes() {
                [b'C', b'-', _, ..] => ctrl = true,
                [b'M', b'-', _, ..] => meta = true,
                [b'S', b'-', _, ..] => shift = true,
                _ => break,
            }
            spec = &spec[2..];
        }
        let key = match spec {
            "<right>" => StrokeKey::Named(Named::ArrowRight),
            "<left>" => StrokeKey::Named(Named::ArrowLeft),
            "<down>" => StrokeKey::Named(Named::ArrowDown),
            "<up>" => StrokeKey::Named(Named::ArrowUp),
            "<home>" => StrokeKey::Named(Named::Home),
            "<end>" => StrokeKey::Named(Named::End),
            "<next>" => StrokeKey::Named(Named::PageDown),
            "<prior>" => StrokeKey::Named(Named::PageUp),
            "<return>" => StrokeKey::Named(Named::Enter),
            "<tab>" => StrokeKey::Named(Named::Tab),
            "<backspace>" => StrokeKey::Named(Named::Backspace),
            "<delete>" => StrokeKey::Named(Named::Delete),
            "<escape>" => StrokeKey::Named(Named::Escape),
            "<space>" => StrokeKey::Named(Named::Space),
            one => {
                let mut chars = one.chars();
                let c = chars.next()?;
                if chars.next().is_some() {
                    return None;
                }
                StrokeKey::Char(c)
            }
        };
        Some(Self { ctrl, meta, shift, key })
    }

    /// No Ctrl or Meta: this press types, unless a binding has it.
    pub fn is_plain(&self) -> bool {
        !self.ctrl && !self.meta
    }

    fn describe(&self) -> String {
        let mut out = String::new();
        for (on, prefix) in [(self.ctrl, "C-"), (self.meta, "M-"), (self.shift, "S-")] {
            if on {
                out.push_str(prefix);
            }
        }
        match &self.key {
            StrokeKey::Char(c) => out.push(*c),
            StrokeKey::Named(named) => out.push_str(&format!("<{named:?}>")),
        }
        out
    }
}

/// What a key press came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Feed {
    Run(Command),
    /// The start of a longer binding: `C-x` so far.
    Pending(String),
    /// A prefix followed by a key that finishes nothing, or a bound-looking
    /// chord with no binding.
    Undefined(String),
    /// A plain key no binding has: it types.
    Plain,
}

pub struct Keymap {
    table: Vec<(Vec<Stroke>, Command)>,
    pending: Vec<Stroke>,
}

impl Keymap {
    pub fn new() -> Self {
        let table = BINDINGS
            .iter()
            .map(|(spec, command)| {
                let keys = spec
                    .split(' ')
                    .map(|one| Stroke::parse(one).unwrap_or_else(|| panic!("bad binding {spec}")))
                    .collect();
                (keys, *command)
            })
            .collect();
        Self { table, pending: Vec::new() }
    }

    /// The keys bound to `command`, for the palette: its shortest binding.
    pub fn key_of(command: Command) -> Option<&'static str> {
        BINDINGS
            .iter()
            .filter(|(_, c)| *c == command)
            .map(|(spec, _)| *spec)
            .min_by_key(|spec| spec.len())
    }

    pub fn clear(&mut self) {
        self.pending.clear();
    }

    pub fn feed(&mut self, stroke: Stroke) -> Feed {
        let was_pending = !self.pending.is_empty();
        // C-g gets out of a chord, whatever came before it.
        if was_pending && stroke.ctrl && stroke.key == StrokeKey::Char('g') {
            self.pending.clear();
            return Feed::Run(Command::Cancel);
        }
        self.pending.push(stroke);
        let exact = self
            .table
            .iter()
            .find(|(keys, _)| *keys == self.pending)
            .map(|(_, command)| *command);
        if let Some(command) = exact {
            self.pending.clear();
            return Feed::Run(command);
        }
        if self.table.iter().any(|(keys, _)| keys.starts_with(&self.pending)) {
            return Feed::Pending(self.describe());
        }
        let keys = self.describe();
        let plain = !was_pending && self.pending[0].is_plain();
        self.pending.clear();
        if plain { Feed::Plain } else { Feed::Undefined(keys) }
    }

    fn describe(&self) -> String {
        let parts: Vec<String> = self.pending.iter().map(Stroke::describe).collect();
        parts.join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(map: &mut Keymap, spec: &str) -> Feed {
        let mut last = Feed::Plain;
        for one in spec.split(' ') {
            last = map.feed(Stroke::parse(one).expect("a key"));
        }
        last
    }

    #[test]
    fn every_binding_parses_and_none_is_listed_twice() {
        let map = Keymap::new();
        for (i, (keys, _)) in map.table.iter().enumerate() {
            assert!(
                !map.table[..i].iter().any(|(other, _)| other == keys),
                "bound twice: {keys:?}"
            );
        }
    }

    #[test]
    fn no_binding_is_a_prefix_of_another() {
        let map = Keymap::new();
        for (keys, _) in &map.table {
            for (other, _) in &map.table {
                assert!(
                    keys == other || !other.starts_with(keys),
                    "{keys:?} hides {other:?}"
                );
            }
        }
    }

    /// Joshua's Zed `keymap.json`, row by row.
    #[test]
    fn his_zed_overrides_are_all_here() {
        let mut map = Keymap::new();
        for (key, want) in [
            ("C-g", Command::Cancel),
            ("C-n", Command::Move(Motion::Down)),
            ("M-/", Command::ToggleComment),
            ("C-y", Command::Yank),
            ("M-v", Command::Move(Motion::PageUp)),
            ("C-v", Command::Move(Motion::PageDown)),
            ("C-l", Command::Center),
            ("C-w", Command::Cut),
            ("M-w", Command::Copy),
            ("M-a", Command::SelectAll),
            ("M-S-p", Command::Palette),
        ] {
            assert_eq!(press(&mut map, key), Feed::Run(want), "{key}");
        }
    }

    #[test]
    fn the_emacs_core_is_here() {
        let mut map = Keymap::new();
        for (key, want) in [
            ("C-f", Command::Move(Motion::Forward)),
            ("C-b", Command::Move(Motion::Backward)),
            ("C-p", Command::Move(Motion::Up)),
            ("C-a", Command::Move(Motion::LineStart)),
            ("C-e", Command::Move(Motion::LineEnd)),
            ("M-f", Command::Move(Motion::WordForward)),
            ("M-b", Command::Move(Motion::WordBackward)),
            ("M-<", Command::Move(Motion::BufferStart)),
            ("M->", Command::Move(Motion::BufferEnd)),
            ("C-<space>", Command::SetMark),
            ("C-k", Command::KillLine),
            ("C-d", Command::DeleteForward),
            ("M-d", Command::KillWord),
            ("C-/", Command::Undo),
            ("C-_", Command::Undo),
            ("C-s", Command::Find),
            ("C-x C-s", Command::Save),
            ("C-x C-f", Command::OpenFile),
            ("C-x C-c", Command::Quit),
            ("C-x C-x", Command::SwapPointAndMark),
            ("C-x h", Command::SelectAll),
            ("C-x k", Command::CloseFile),
            ("C-x u", Command::Undo),
            ("M-g g", Command::GotoLine),
            ("M-x", Command::Palette),
        ] {
            assert_eq!(press(&mut map, key), Feed::Run(want), "{key}");
        }
    }

    #[test]
    fn a_prefix_waits_and_a_wrong_second_key_is_undefined() {
        let mut map = Keymap::new();
        assert_eq!(press(&mut map, "C-x"), Feed::Pending("C-x".into()));
        assert_eq!(press(&mut map, "q"), Feed::Undefined("C-x q".into()));
        // And it is over: the next key stands alone.
        assert_eq!(press(&mut map, "C-f"), Feed::Run(Command::Move(Motion::Forward)));
    }

    #[test]
    fn c_g_gets_out_of_a_chord() {
        let mut map = Keymap::new();
        press(&mut map, "C-x");
        assert_eq!(press(&mut map, "C-g"), Feed::Run(Command::Cancel));
        assert_eq!(press(&mut map, "s"), Feed::Plain);
    }

    #[test]
    fn plain_letters_type_and_unbound_chords_do_not() {
        let mut map = Keymap::new();
        assert_eq!(press(&mut map, "a"), Feed::Plain);
        assert_eq!(press(&mut map, "C-q"), Feed::Undefined("C-q".into()));
        assert_eq!(press(&mut map, "M-q"), Feed::Undefined("M-q".into()));
    }

    #[test]
    fn symbols_take_their_shift_from_the_key_they_made() {
        let meta_shift = Modifiers::LOGO | Modifiers::SHIFT;
        assert_eq!(Stroke::from_iced(&Key::Character("<".into()), meta_shift), Stroke::parse("M-<"));
        let ctrl_shift = Modifiers::CTRL | Modifiers::SHIFT;
        assert_eq!(Stroke::from_iced(&Key::Character("_".into()), ctrl_shift), Stroke::parse("C-_"));
        // A letter keeps it: Super+Shift+P is not Super+P.
        assert_eq!(Stroke::from_iced(&Key::Character("P".into()), meta_shift), Stroke::parse("M-S-p"));
    }

    #[test]
    fn super_is_meta_and_so_is_alt() {
        let w = Key::Character("w".into());
        assert_eq!(Stroke::from_iced(&w, Modifiers::LOGO), Stroke::parse("M-w"));
        assert_eq!(Stroke::from_iced(&w, Modifiers::ALT), Stroke::parse("M-w"));
        let mut map = Keymap::new();
        for key in ["M-f", "M-b", "M-d", "M-v", "M-w", "M-x", "M-<", "M->", "M-{", "M-}", "M-;", "M-m"] {
            let stroke = Stroke::parse(key).expect("a key");
            assert!(stroke.meta, "{key}");
            assert!(matches!(map.feed(stroke), Feed::Run(_)), "{key}");
        }
    }

    #[test]
    fn super_held_is_meta_even_when_the_press_does_not_say_so() {
        let held = merged_modifiers(Modifiers::empty(), Modifiers::empty(), Modifiers::LOGO);
        let w = Stroke::from_iced(&Key::Character("w".into()), held).expect("a key");
        assert!(!w.is_plain(), "Super+W must not type a w");
    }

    #[test]
    fn a_super_only_announced_is_stale_and_does_not_count() {
        let merged = merged_modifiers(Modifiers::empty(), Modifiers::LOGO, Modifiers::empty());
        assert!(!merged.logo());
        let merged = merged_modifiers(Modifiers::empty(), Modifiers::SHIFT, Modifiers::empty());
        assert!(merged.shift());
    }

    #[test]
    fn only_modifier_keys_have_a_bit() {
        assert_eq!(modifier_bit(&Key::Named(Named::Super)), Some(Modifiers::LOGO));
        assert_eq!(modifier_bit(&Key::Named(Named::Meta)), Some(Modifiers::LOGO));
        assert_eq!(modifier_bit(&Key::Character("w".into())), None);
    }

    #[test]
    fn space_is_one_key_however_it_is_reported() {
        let named = Stroke::from_iced(&Key::Named(Named::Space), Modifiers::CTRL);
        let typed = Stroke::from_iced(&Key::Character(" ".into()), Modifiers::CTRL);
        assert_eq!(named, Stroke::parse("C-<space>"));
        assert_eq!(typed, Stroke::parse("C-<space>"));
    }

    #[test]
    fn a_modifier_alone_is_no_key_so_a_chord_survives_it() {
        for named in [Named::Control, Named::Shift, Named::Alt, Named::Super] {
            assert_eq!(Stroke::from_iced(&Key::Named(named), Modifiers::CTRL), None, "{named:?}");
        }
    }

    #[test]
    fn key_of_names_the_shortest_binding() {
        assert_eq!(Keymap::key_of(Command::SelectAll), Some("M-a"));
        assert_eq!(Keymap::key_of(Command::Save), Some("C-x C-s"));
    }
}
