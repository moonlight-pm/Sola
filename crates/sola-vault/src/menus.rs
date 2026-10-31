//! The titlebar's menus: Vault, File, Edit, Item, View, and the kit's
//! Window. Edit uses the kit's shared ids (`sola_kit::keys::COMMON_ACTIONS`)
//! so its keys stay theirs; no key here is a system one (Super+L is the
//! screen lock, so Lock is Super+Shift+L).

use sola_bus::topics::{AppMenuPayload, MenuDefinition, MenuItem};
use sola_core::{KeyChord, KeyCode};

use crate::model::sort::Sort;

pub const SETTINGS: &str = "settings";
pub const SYNC: &str = "sync";
pub const LOCK: &str = "lock";
pub const SIGN_OUT: &str = "sign_out";
pub const QUIT: &str = sola_kit::QUIT_ACTION_ID;

pub const NEW_ITEM: &str = "new_item";
pub const NEW_FOLDER: &str = "new_folder";
pub const EXPORT: &str = "export";
pub const GENERATOR: &str = "generator";

pub const UNDO: &str = "undo";
pub const REDO: &str = "redo";
pub const CUT: &str = "cut";
pub const COPY: &str = "copy";
pub const PASTE: &str = "paste";
pub const SELECT_ALL: &str = "select_all";
pub const FIND: &str = "find";

pub const EDIT_ITEM: &str = "edit_item";
pub const COPY_USERNAME: &str = "copy_username";
pub const COPY_PASSWORD: &str = "copy_password";
pub const COPY_CODE: &str = "copy_code";
pub const FAVORITE: &str = "favorite";
pub const CLONE: &str = "clone";
pub const MOVE_TO_FOLDER: &str = "move_to_folder";
pub const MOVE_TO_ORG: &str = "move_to_org";
pub const TRASH: &str = "trash";

pub const SORT_NAME: &str = "sort_name";
pub const SORT_RECENT: &str = "sort_recent";

/// Every menu, with View's check on `sort`.
pub fn app_menu(app_id: &str, sort: Sort) -> AppMenuPayload {
    AppMenuPayload {
        app_id: app_id.into(),
        menus: vec![
            menu(
                "Vault",
                vec![
                    item(SETTINGS, "Settings…", Some(KeyCode::COMMA.meta())),
                    MenuItem::Divider,
                    item(SYNC, "Sync Now", Some(KeyCode::R.meta())),
                    item(LOCK, "Lock", Some(KeyCode::L.meta_shift())),
                    item(SIGN_OUT, "Sign Out…", None),
                    MenuItem::Divider,
                    item(QUIT, "Quit Vault", Some(KeyCode::Q.meta())),
                ],
            ),
            menu(
                "File",
                vec![
                    item(NEW_ITEM, "New Item", Some(KeyCode::N.meta())),
                    item(NEW_FOLDER, "New Folder…", Some(KeyCode::N.meta_shift())),
                    MenuItem::Divider,
                    item(GENERATOR, "Generator…", Some(KeyCode::G.meta())),
                    MenuItem::Divider,
                    item(EXPORT, "Export…", None),
                ],
            ),
            menu(
                "Edit",
                vec![
                    item(UNDO, "Undo", Some(KeyCode::Z.meta())),
                    item(REDO, "Redo", Some(KeyCode::Z.meta_shift())),
                    MenuItem::Divider,
                    item(CUT, "Cut", Some(KeyCode::X.meta())),
                    item(COPY, "Copy", Some(KeyCode::C.meta())),
                    item(PASTE, "Paste", Some(KeyCode::V.meta())),
                    item(SELECT_ALL, "Select All", Some(KeyCode::A.meta())),
                    MenuItem::Divider,
                    item(FIND, "Find", Some(KeyCode::F.meta())),
                ],
            ),
            menu(
                "Item",
                vec![
                    item(EDIT_ITEM, "Edit", Some(KeyCode::E.meta())),
                    MenuItem::Divider,
                    item(COPY_USERNAME, "Copy Username", Some(KeyCode::U.meta())),
                    item(COPY_PASSWORD, "Copy Password", Some(KeyCode::P.meta())),
                    item(COPY_CODE, "Copy Code", Some(KeyCode::T.meta())),
                    MenuItem::Divider,
                    item(FAVORITE, "Favorite", None),
                    item(CLONE, "Clone", None),
                    item(MOVE_TO_FOLDER, "Move to Folder…", None),
                    item(MOVE_TO_ORG, "Move to Organization…", None),
                    MenuItem::Divider,
                    item(TRASH, "Move to Trash", None),
                ],
            ),
            menu(
                "View",
                vec![
                    check(SORT_NAME, "Sort by Name", sort == Sort::Name),
                    check(SORT_RECENT, "Sort by Last Used", sort == Sort::Recent),
                ],
            ),
            sola_kit::window_menu(),
        ],
    }
}

fn menu(label: &str, items: Vec<MenuItem>) -> MenuDefinition {
    MenuDefinition {
        label: label.into(),
        items,
    }
}

fn item(id: &str, label: &str, shortcut: Option<KeyChord>) -> MenuItem {
    MenuItem::Action {
        id: id.into(),
        label: label.into(),
        shortcut,
        disabled: false,
        checked: false,
    }
}

fn check(id: &str, label: &str, checked: bool) -> MenuItem {
    MenuItem::Action {
        id: id.into(),
        label: label.into(),
        shortcut: None,
        disabled: false,
        checked,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(menu: &AppMenuPayload) -> Vec<(String, Option<KeyChord>)> {
        menu.menus
            .iter()
            .flat_map(|m| &m.items)
            .filter_map(|i| match i {
                MenuItem::Action { id, shortcut, .. } => Some((id.clone(), *shortcut)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn menu_keys_survive_the_shared_keys() {
        let menu = app_menu("sola-vault", Sort::Name);
        assert_eq!(keys(&sola_kit::keys::apply(&menu)), keys(&menu));
    }

    #[test]
    fn edit_uses_the_shared_ids() {
        let menu = app_menu("sola-vault", Sort::Name);
        let edit = menu.menus.iter().find(|m| m.label == "Edit").unwrap();
        for (id, _, _, key) in sola_kit::keys::COMMON_ACTIONS {
            let found = edit.items.iter().any(|i| {
                matches!(i, MenuItem::Action { id: i, shortcut: Some(k), .. } if i == id && k == key)
            });
            assert!(found, "Edit lacks the shared {id}");
        }
    }

    #[test]
    fn no_key_is_the_screen_lock_or_used_twice() {
        let menu = app_menu("sola-vault", Sort::Name);
        let all: Vec<KeyChord> = keys(&menu).into_iter().filter_map(|(_, k)| k).collect();
        assert!(!all.contains(&KeyCode::L.meta()), "Super+L is the screen lock");
        let mut sorted: Vec<String> = all.iter().map(|k| format!("{k:?}")).collect();
        sorted.sort();
        let n = sorted.len();
        sorted.dedup();
        assert_eq!(sorted.len(), n, "two items share a key");
    }

    #[test]
    fn view_checks_the_sort() {
        let menu = app_menu("sola-vault", Sort::Recent);
        let checked: Vec<String> = menu
            .menus
            .iter()
            .flat_map(|m| &m.items)
            .filter_map(|i| match i {
                MenuItem::Action { id, checked: true, .. } => Some(id.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(checked, [SORT_RECENT]);
    }
}
