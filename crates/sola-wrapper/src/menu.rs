//! Menubar actions. Super+X/C/V/A never reach iced (River + the shell
//! bind them); chrome must handle `Topic::MenuAction`.

use sola_browser::engine::EditCmd;
use sola_browser::integration::{
    ACTION_EDIT_COPY, ACTION_EDIT_CUT, ACTION_EDIT_PASTE, ACTION_EDIT_SELECT_ALL,
};

pub fn edit_cmd(action_id: &str) -> Option<EditCmd> {
    match action_id {
        ACTION_EDIT_CUT => Some(EditCmd::Cut),
        ACTION_EDIT_COPY => Some(EditCmd::Copy),
        ACTION_EDIT_PASTE => Some(EditCmd::Paste),
        ACTION_EDIT_SELECT_ALL => Some(EditCmd::SelectAll),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sola_browser::integration::EDIT_MENU_ITEMS;

    #[test]
    fn edit_menu_has_super_chords() {
        let ids: Vec<&str> = EDIT_MENU_ITEMS.iter().map(|(id, ..)| *id).collect();
        assert_eq!(
            ids,
            [
                ACTION_EDIT_CUT,
                ACTION_EDIT_COPY,
                ACTION_EDIT_PASTE,
                ACTION_EDIT_SELECT_ALL
            ]
        );
        for (_, _, chord) in EDIT_MENU_ITEMS {
            assert!(
                chord.meta,
                "edit chords are Super-bound so the shell routes MenuAction"
            );
        }
    }

    #[test]
    fn edit_actions_map() {
        assert_eq!(edit_cmd(ACTION_EDIT_CUT), Some(EditCmd::Cut));
        assert_eq!(edit_cmd(ACTION_EDIT_COPY), Some(EditCmd::Copy));
        assert_eq!(edit_cmd(ACTION_EDIT_PASTE), Some(EditCmd::Paste));
        assert_eq!(edit_cmd(ACTION_EDIT_SELECT_ALL), Some(EditCmd::SelectAll));
        assert_eq!(edit_cmd("quit"), None);
        assert_eq!(edit_cmd("edit-undo"), None);
    }
}
