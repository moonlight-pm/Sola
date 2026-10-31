//! The bar across the list and the record: the place and how many it
//! holds; search (⌘F), a folder's Rename and Delete, and New (+) with a
//! menu of kinds.

use iced::font::Weight;
use iced::widget::text::Wrapping;
use iced::widget::{Space, container, row, stack, text};
use iced::{Alignment, Element, Font, Length, Padding};
use sola_kit::components::context_menu::{self, MenuItem};
use sola_kit::components::popover::{Placement, popover_anchored};
use sola_kit::components::style::{BAR_H, SPACE_MD, SPACE_SM, SPACE_XS};
use sola_kit::components::text as kit_text;
use sola_kit::components::text_input::text_input;
use sola_kit::fonts;
use sola_vault_proto::ItemKind;
use zeroize::Zeroizing;

use super::fields::Field;
use super::widgets;
use super::{App, Msg, Popup, Section, style};
use crate::model::section::{count, visible};

/// The search field's width.
const SEARCH_W: f32 = 240.0;

impl App {
    pub(super) fn view_bar(&self) -> Element<'_, Msg> {
        let title = text(self.section_title())
            .font(Font {
                weight: Weight::Semibold,
                ..fonts::ui()
            })
            .size(14)
            .wrapping(Wrapping::None);
        let total = count(&self.items, &self.section);
        let figure = if self.query.trim().is_empty() {
            format_count(total)
        } else {
            let found = visible(&self.items, &self.section, &self.query).len();
            format!("{} of {}", format_count(found), format_count(total))
        };
        let left = row![
            container(title).padding(Padding::ZERO.left(6)),
            kit_text::caption(figure).style(kit_text::muted),
        ]
        .spacing(SPACE_SM)
        .align_y(Alignment::Center);

        let placeholder = match &self.section {
            Section::All => "Search vault".to_string(),
            _ => format!("Search {}", self.section_title()),
        };
        let field = text_input(&placeholder, &self.query)
            .id(Field::Search.id())
            .on_input(|v| Msg::Input(Field::Search, Zeroizing::new(v)))
            .on_submit(Msg::Submit(Field::Search))
            .size(13)
            .padding(Padding {
                top: 4.0,
                right: 9.0,
                bottom: 4.0,
                left: 28.0,
            })
            .style(style::search_input)
            .width(Length::Fixed(SEARCH_W));
        let glass = container(widgets::muted("lucide/search", 13.0))
            .height(Length::Fill)
            .padding(Padding::ZERO.left(9))
            .align_y(Alignment::Center);
        let mut right = row![stack![field, glass].height(Length::Shrink)]
            .spacing(SPACE_XS)
            .align_y(Alignment::Center);
        if !self.query.is_empty() {
            right = right.push(widgets::tool("lucide/x", "Clear search", Some(Msg::SearchClear)));
        }
        right = right.push(Space::new().width(SPACE_SM));
        if let Section::Folder(id) = &self.section {
            right = right.push(self.folder_menu(id));
        }
        if self.section != Section::Trash {
            right = right.push(self.new_menu());
        }

        container(
            row![container(left).width(Length::Fill).clip(true), right]
                .spacing(SPACE_MD)
                .align_y(Alignment::Center),
        )
        .width(Length::Fill)
        .height(Length::Fixed(BAR_H))
        .padding(Padding::from([0.0, 10.0]))
        .align_y(Alignment::Center)
        .style(style::canvas)
        .into()
    }

    /// + : a new item of the place's kind, or the menu of kinds.
    fn new_menu(&self) -> Element<'_, Msg> {
        let open = self.popup == Some(Popup::NewItem);
        let trigger = widgets::tool_toggle(
            "lucide/plus",
            "New item",
            open,
            Msg::Popup(if open { None } else { Some(Popup::NewItem) }),
        );
        if !open {
            return trigger;
        }
        let mut items: Vec<MenuItem<Msg>> = self
            .listed_kinds()
            .into_iter()
            .map(|k| MenuItem::action(new_label(k), Msg::NewItem(k)))
            .collect();
        items.push(MenuItem::separator());
        items.push(MenuItem::action("Folder…", Msg::NewFolder));
        popover_anchored(trigger, context_menu::menu(items), Msg::Popup(None))
            .placement(Placement::Below)
            .into()
    }

    fn folder_menu<'a>(&'a self, id: &'a str) -> Element<'a, Msg> {
        let open = self.popup == Some(Popup::FolderMore);
        let trigger = widgets::tool_toggle(
            "lucide/folder-pen",
            "Folder",
            open,
            Msg::Popup(if open { None } else { Some(Popup::FolderMore) }),
        );
        if !open {
            return trigger;
        }
        let items = vec![
            MenuItem::action("Rename Folder…", Msg::RenameFolder(id.to_string())),
            MenuItem::action("Delete Folder…", Msg::AskDeleteFolder(id.to_string())),
        ];
        popover_anchored(trigger, context_menu::menu(items), Msg::Popup(None))
            .placement(Placement::Below)
            .into()
    }
}

fn new_label(kind: ItemKind) -> &'static str {
    match kind {
        ItemKind::Login => "Login",
        ItemKind::Card => "Card",
        ItemKind::Identity => "Identity",
        ItemKind::SecureNote => "Note",
        ItemKind::SshKey => "SSH Key",
        ItemKind::BankAccount => "Bank Account",
        ItemKind::DriversLicense => "Driver's License",
        ItemKind::Passport => "Passport",
    }
}

/// `1234` → `1,234`.
pub fn format_count(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::format_count;

    #[test]
    fn counts_group_thousands() {
        assert_eq!(format_count(0), "0");
        assert_eq!(format_count(838), "838");
        assert_eq!(format_count(1234), "1,234");
        assert_eq!(format_count(1234567), "1,234,567");
    }
}
