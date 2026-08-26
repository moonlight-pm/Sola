//! Page edit chords. Same Super+X/C/V/A path as sola-browser: the shell
//! steals the chords via the Edit menu; chrome reads/writes the Wayland
//! clipboard; CEF never sees the keys.

use sola_browser::engine::EditCmd;
use sola_core::{KeyChord, KeyCode};

pub const ACTION_CUT: &str = "edit-cut";
pub const ACTION_COPY: &str = "edit-copy";
pub const ACTION_PASTE: &str = "edit-paste";
pub const ACTION_SELECT_ALL: &str = "edit-select-all";

pub const MENU_ITEMS: [(&str, &str, KeyChord); 4] = [
    (ACTION_CUT, "Cut", KeyCode::X.meta()),
    (ACTION_COPY, "Copy", KeyCode::C.meta()),
    (ACTION_PASTE, "Paste", KeyCode::V.meta()),
    (ACTION_SELECT_ALL, "Select All", KeyCode::A.meta()),
];

pub fn edit_cmd_for_action(action_id: &str) -> Option<EditCmd> {
    match action_id {
        ACTION_CUT => Some(EditCmd::Cut),
        ACTION_COPY => Some(EditCmd::Copy),
        ACTION_PASTE => Some(EditCmd::Paste),
        ACTION_SELECT_ALL => Some(EditCmd::SelectAll),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_edit_chords() {
        assert_eq!(edit_cmd_for_action(ACTION_CUT), Some(EditCmd::Cut));
        assert_eq!(edit_cmd_for_action(ACTION_COPY), Some(EditCmd::Copy));
        assert_eq!(edit_cmd_for_action(ACTION_PASTE), Some(EditCmd::Paste));
        assert_eq!(
            edit_cmd_for_action(ACTION_SELECT_ALL),
            Some(EditCmd::SelectAll)
        );
        assert_eq!(edit_cmd_for_action("quit"), None);
        assert_eq!(edit_cmd_for_action("bogus"), None);
    }

    #[test]
    fn menu_ids_match_actions() {
        let ids: Vec<&str> = MENU_ITEMS.iter().map(|(id, _, _)| *id).collect();
        assert_eq!(
            ids,
            vec![ACTION_CUT, ACTION_COPY, ACTION_PASTE, ACTION_SELECT_ALL]
        );
        for (_, _, chord) in MENU_ITEMS {
            assert!(chord.meta, "edit chords are Super-bound");
            assert!(!chord.ctrl);
        }
    }
}
