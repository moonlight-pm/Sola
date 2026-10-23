//! A workspace pill's menu and the rename card (Next slice 5), in the
//! menu overlay under the pill. Items are shell `MenuAction`s
//! (`workspace.<verb>.<index>`), routed by [`parse`].

use iced::widget::{Id as WidgetId, column, text};
use iced::{Element, Length, Padding};
use sola_bus::topics::MenuItem;
use sola_kit::components::popover;
use sola_kit::components::text_input::{style as input_style, text_input};
use sola_kit::fonts;

use crate::app::workspaces::WorkspaceAction;
use crate::app::{Msg, Shell};

pub const RENAME_INPUT_ID: &str = "workspace-rename";
pub const MENU_HEIGHT: f32 = 200.0;
pub const RENAME_WIDTH: f32 = 260.0;
pub const RENAME_HEIGHT: f32 = 120.0;

pub fn menu(shell: &Shell, index: usize) -> Element<'static, Msg> {
    let count = shell.tabs.workspace_count();
    let item = |verb: &str, label: &str, disabled: bool| MenuItem::Action {
        id: format!("workspace.{verb}.{index}"),
        label: label.into(),
        shortcut: None,
        disabled,
        checked: false,
    };
    let items = vec![
        item("new", "New Workspace", false),
        item("rename", "Rename…", false),
        MenuItem::Divider,
        item("left", "Move Left", index == 0),
        item("right", "Move Right", index + 1 >= count),
        MenuItem::Divider,
        item("delete", "Delete", count < 2),
    ];
    super::view::items_card(items, Shell::APP_ID.to_string())
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
        assert_eq!(parse("workspace.delete.3"), Some(WorkspaceAction::Delete(3)));
        assert_eq!(parse("workspace.rename.x"), None);
        assert_eq!(parse("window.left"), None);
    }
}
