//! The workspace menu and the rename card (Next slice 5), in the menu
//! overlay under the tab bar's head: the workspaces (the showing one
//! checked), then what to do with the showing one. Items are shell
//! `MenuAction`s (`workspace.<verb>.<index>`), routed by [`parse`].

use iced::widget::{Id as WidgetId, column, text};
use iced::{Element, Length, Padding};
use sola_bus::topics::MenuItem;
use sola_kit::components::popover;
use sola_kit::components::text_input::{style as input_style, text_input};
use sola_kit::fonts;

use crate::app::workspaces::WorkspaceAction;
use crate::app::{Msg, Shell};

pub const RENAME_INPUT_ID: &str = "workspace-rename";
pub const RENAME_HEIGHT: f32 = 120.0;
/// A menu row, a divider, and the card around them (with room to spare).
const ROW_H: f32 = 24.0;
const DIVIDER_H: f32 = 10.0;
const CARD_PAD: f32 = 40.0;

/// The menu for workspace `index` (the showing one): every workspace to
/// show (Super+1…9), then New, Rename, Move and Delete for this one.
pub fn menu(shell: &Shell, index: usize) -> Element<'static, Msg> {
    super::view::items_card(items(shell, index), Shell::APP_ID.to_string())
}

/// The menu's height for the overlay.
pub fn menu_height(shell: &Shell) -> f32 {
    let items = items(shell, shell.tabs.showing());
    let dividers = items.iter().filter(|i| matches!(i, MenuItem::Divider)).count();
    let rows = items.len() - dividers;
    rows as f32 * ROW_H + dividers as f32 * DIVIDER_H + CARD_PAD
}

fn items(shell: &Shell, index: usize) -> Vec<MenuItem> {
    let count = shell.tabs.workspace_count();
    let item = |verb: &str, label: &str, disabled: bool| MenuItem::Action {
        id: format!("workspace.{verb}.{index}"),
        label: label.into(),
        shortcut: None,
        disabled,
        checked: false,
    };
    let mut items: Vec<MenuItem> = shell
        .tabs
        .workspaces()
        .into_iter()
        .enumerate()
        .map(|(i, ws)| MenuItem::Action {
            id: format!("workspace.show.{i}"),
            label: ws.name,
            shortcut: crate::keys::WORKSPACE_KEYS.get(i).map(|k| k.meta()),
            disabled: false,
            checked: i == index,
        })
        .collect();
    items.extend([
        MenuItem::Divider,
        item("new", "New Workspace", false),
        item("rename", "Rename…", false),
        MenuItem::Divider,
        item("left", "Move Left", index == 0),
        item("right", "Move Right", index + 1 >= count),
        MenuItem::Divider,
        item("delete", "Delete", count < 2),
    ]);
    items
}

pub fn rename(shell: &Shell) -> Element<'_, Msg> {
    let value = shell
        .workspace
        .renaming
        .as_ref()
        .map_or("", |(_, name)| name.as_str());
    let field = text_input("Name", value)
        .id(WidgetId::new(RENAME_INPUT_ID))
        .on_input(Msg::WorkspaceRenameInput)
        .on_submit(Msg::WorkspaceRenameCommit)
        .style(input_style)
        .padding(Padding::from([6, 8]))
        .size(13)
        .width(Length::Fill);
    let card = column![
        text("Rename Workspace").font(fonts::chrome()).size(12),
        field,
        text("Return to rename, Esc to cancel")
            .font(fonts::chrome())
            .size(11),
    ]
    .spacing(8)
    .padding(8)
    .width(Length::Fill);
    popover(card).width(Length::Fill).into()
}

/// A workspace menu row's action, from its id.
pub fn parse(action_id: &str) -> Option<WorkspaceAction> {
    let rest = action_id.strip_prefix("workspace.")?;
    let (verb, index) = rest.split_once('.')?;
    let index: usize = index.parse().ok()?;
    Some(match verb {
        "show" => WorkspaceAction::Show(index),
        "new" => WorkspaceAction::New,
        "rename" => WorkspaceAction::Rename(index),
        "left" => WorkspaceAction::MoveLeft(index),
        "right" => WorkspaceAction::MoveRight(index),
        "delete" => WorkspaceAction::Delete(index),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_ids_round_trip() {
        assert_eq!(parse("workspace.new.0"), Some(WorkspaceAction::New));
        assert_eq!(parse("workspace.show.2"), Some(WorkspaceAction::Show(2)));
        assert_eq!(parse("workspace.delete.3"), Some(WorkspaceAction::Delete(3)));
        assert_eq!(parse("workspace.rename.x"), None);
        assert_eq!(parse("window.left"), None);
    }
}
