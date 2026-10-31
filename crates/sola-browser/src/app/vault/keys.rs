//! The sheet's keys, from the chrome's `listen_with` (a fn pointer, so it
//! reads a process-wide flag): arrows choose, → opens, Enter acts, Tab
//! moves between fields, Escape steps back or closes.

use std::sync::atomic::Ordering;

use iced::event;
use iced::keyboard::{self, Key, key::Named};

use super::PANEL_OPEN;
use super::update::VaultMsg;

/// The sheet is up, so its keys are the vault's.
pub(crate) fn keys_open() -> bool {
    PANEL_OPEN.load(Ordering::Relaxed)
}

/// What a key press means while the sheet is up. Enter only when no field
/// took it (a field's own submit acts then).
pub(crate) fn panel_key(
    key: &Key,
    modifiers: keyboard::Modifiers,
    status: event::Status,
) -> Option<VaultMsg> {
    if modifiers.command() || modifiers.control() || modifiers.alt() {
        return None;
    }
    match key {
        Key::Named(Named::ArrowDown) => Some(VaultMsg::Down),
        Key::Named(Named::ArrowUp) => Some(VaultMsg::Up),
        Key::Named(Named::ArrowRight) => Some(VaultMsg::Right),
        Key::Named(Named::Tab) => Some(if modifiers.shift() {
            VaultMsg::FocusPrev
        } else {
            VaultMsg::FocusNext
        }),
        Key::Named(Named::Escape) => Some(VaultMsg::Escape),
        Key::Named(Named::Enter) if status == event::Status::Ignored => Some(VaultMsg::Enter),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enter_belongs_to_a_field_that_took_it() {
        let enter = Key::Named(Named::Enter);
        let none = keyboard::Modifiers::empty();
        assert!(matches!(
            panel_key(&enter, none, event::Status::Ignored),
            Some(VaultMsg::Enter)
        ));
        assert!(panel_key(&enter, none, event::Status::Captured).is_none());
        let down = Key::Named(Named::ArrowDown);
        assert!(matches!(
            panel_key(&down, none, event::Status::Captured),
            Some(VaultMsg::Down)
        ));
        assert!(panel_key(&down, keyboard::Modifiers::COMMAND, event::Status::Ignored).is_none());
    }
}
