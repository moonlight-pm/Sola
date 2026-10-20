//! Keyboard: Escape layering and the single-key message shortcuts.

use iced::Task;
use iced::keyboard::key::Named;
use iced::keyboard::{Key, Modifiers};

use super::{App, Msg};

impl App {
    /// `captured`: a focused widget (Search, a compose field) already
    /// used the key — single-letter shortcuts must not fire then.
    pub(super) fn on_key(&mut self, key: Key, mods: Modifiers, captured: bool) -> Task<Msg> {
        let escape = matches!(key, Key::Named(Named::Escape));
        if self.picker.is_some() {
            if escape {
                self.picker = None;
            }
            return Task::none();
        }
        if escape && self.confirm.is_some() {
            self.confirm = None;
            return Task::none();
        }
        if let Some(c) = self.compose.as_mut() {
            if !escape {
                return Task::none();
            }
            if c.from_open {
                c.from_open = false;
            } else if c.confirm_discard {
                c.confirm_discard = false;
                c.then_open = None;
            } else {
                return self.update(Msg::CancelCompose);
            }
            return Task::none();
        }
        if captured {
            return Task::none();
        }
        if escape {
            if self.list.search.is_some() || !self.search_input.is_empty() {
                return self.update(Msg::SearchClear);
            }
            return Task::none();
        }
        match key {
            Key::Named(Named::ArrowUp) => return self.select_prev(true),
            Key::Named(Named::ArrowDown) => return self.select_next(true),
            _ => {}
        }
        if mods.control() || mods.alt() || mods.logo() {
            return Task::none();
        }
        let Key::Character(c) = &key else {
            return Task::none();
        };
        match c.as_str() {
            "x" => self.update(Msg::MoveSelected("Junk".into())),
            "i" => self.update(Msg::MoveSelected("INBOX".into())),
            "a" => self.update(Msg::MoveSelected("Archive".into())),
            "d" => self.update(Msg::MoveSelected("Trash".into())),
            "u" => self.update(Msg::Undo),
            "w" => self.select_prev(false),
            "s" => self.select_next(false),
            _ => Task::none(),
        }
    }
}
