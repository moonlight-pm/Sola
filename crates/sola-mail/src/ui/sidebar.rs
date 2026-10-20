//! Mailbox sidebar and the account connection card.

use iced::widget::{column, container, row};
use iced::{Element, Length, Padding};
use sola_kit::components::button as kit_btn;
use sola_kit::components::style::{SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XS};
use sola_kit::components::text as kit_text;
use sola_kit::components::{SidebarItem, SidebarSection, sidebar};

use super::format::truncate;
use super::widgets::status_dot;
use super::{App, Msg, style};
use crate::protocol::FolderRef;
use crate::protocol::boxes::is_canonical;
use crate::protocol::types::{folder_label, unread_badge};
use crate::worker::LinkState;

const SIDEBAR_W: f32 = 200.0;

#[derive(Debug, Clone)]
pub struct AccountLink {
    pub id: String,
    pub email: String,
    pub state: LinkState,
}

impl App {
    pub(super) fn view_sidebar(&self) -> Element<'_, Msg> {
        let selected = &self.list.folder;
        let boxes: Vec<SidebarItem<'_, Msg>> = self
            .folders
            .iter()
            .filter(|f| is_canonical(&f.name))
            .map(|f| {
                let target = FolderRef::Mailbox(f.name.clone());
                let mut item =
                    SidebarItem::new(folder_label(&f.name), Msg::SelectFolder(target.clone()))
                        .active(*selected == target);
                if let Some(badge) = unread_badge(f.unread) {
                    item = item.secondary(badge);
                }
                item
            })
            .collect();
        let mut sections = vec![SidebarSection::new("Mailboxes", boxes).fill()];
        if !self.smart.is_empty() {
            let smart = self
                .smart
                .iter()
                .map(|f| {
                    let target = FolderRef::Smart(f.name.clone());
                    let mut item =
                        SidebarItem::new(f.name.clone(), Msg::SelectFolder(target.clone()))
                            .active(*selected == target);
                    if let Some(badge) = unread_badge(f.unread) {
                        item = item.secondary(badge);
                    }
                    item
                })
                .collect();
            sections.push(SidebarSection::new("Smart", smart));
        }
        container(sidebar(sections))
            .width(Length::Fixed(SIDEBAR_W))
            .height(Length::Fill)
            .into()
    }

    pub(super) fn show_link_card(&self) -> bool {
        self.links.iter().any(|l| l.state != LinkState::Ready)
    }

    /// Bottom-right card: accounts still connecting, or failed (with Retry).
    pub(super) fn view_link_card(&self) -> Element<'_, Msg> {
        let mut rows = column![].spacing(SPACE_SM);
        let mut any_failed = false;
        for link in &self.links {
            let (label, failed) = match &link.state {
                LinkState::Ready => continue,
                LinkState::Connecting => ("Connecting…", false),
                LinkState::Failed(e) => (e.as_str(), true),
            };
            any_failed |= failed;
            let name = if link.email.is_empty() {
                link.id.clone()
            } else {
                link.email.clone()
            };
            let state = kit_text::caption(truncate(label, 42)).style(if failed {
                kit_text::danger
            } else {
                kit_text::muted
            });
            rows = rows.push(
                row![
                    status_dot(failed),
                    column![kit_text::caption(name), state]
                        .spacing(SPACE_XS)
                        .width(Length::Fill),
                ]
                .spacing(SPACE_MD)
                .align_y(iced::Alignment::Center),
            );
        }
        if any_failed {
            rows = rows.push(
                row![
                    kit_text::caption("Retrying automatically").style(kit_text::muted),
                    iced::widget::Space::new().width(Length::Fill),
                    kit_btn::labeled_sm("Retry Now", kit_btn::secondary).on_press(Msg::Refresh),
                ]
                .align_y(iced::Alignment::Center),
            );
        }
        container(rows)
            .padding(Padding::from([SPACE_MD, SPACE_LG]))
            .width(Length::Fixed(268.0))
            .style(style::card)
            .into()
    }
}
