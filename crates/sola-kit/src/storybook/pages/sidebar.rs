//! Sidebar showcase — meta-page that renders the kit's own sidebar
//! component inside a constrained frame. Same component the storybook
//! uses for navigation on the left of every page.

use iced::widget::{column, container, row};
use iced::{Element, Length};

use sola_kit::components::card::style as card_style;
use sola_kit::components::text::{body, code, heading, muted};
use sola_kit::components::{SidebarItem, sidebar};

use crate::storybook::Msg;

pub fn view() -> Element<'static, Msg> {
    // Static demo items — clicking does nothing in the showcase
    // (consumer would route the message into its own state).
    let items = vec![
        SidebarItem::new("Inbox", Msg::Noop).active(true),
        SidebarItem::new("Drafts", Msg::Noop),
        SidebarItem::new("Sent", Msg::Noop),
        SidebarItem::new("Archive", Msg::Noop),
    ];

    let demo = container(
        row![sidebar(items), filler()]
            .width(Length::Fill)
            .height(Length::Fill),
    )
    .style(card_style)
    .height(Length::Fixed(280.0))
    .width(Length::Fill);

    column![
        heading("Sidebar"),
        body("Vertical column of selectable items. Active row uses BG_HOVER + accent text.")
            .style(muted),
        demo,
        code("sidebar(items: Vec<SidebarItem<Msg>>)").style(muted),
    ]
    .spacing(16)
    .into()
}

fn filler() -> Element<'static, Msg> {
    container(body("Content").style(muted))
        .padding(16)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}
