//! Sidebar showcase — meta-page that renders the kit's own sidebar
//! component inside a constrained frame. Same component the storybook
//! uses for navigation on the left of every page.

use iced::widget::{column, container, row};
use iced::{Element, Length};

use sola_kit::components::card::style as card_style;
use sola_kit::components::text::{body, code, heading, muted};
use sola_kit::components::{SidebarItem, SidebarSection, sidebar};

use crate::storybook::Msg;

pub fn view() -> Element<'static, Msg> {
    // Two demo sections — Mailboxes (with header) and a sectionless
    // "Search" entry pinned on top — to exercise both the labeled and
    // unlabeled SidebarSection paths.
    let pinned = SidebarSection::unlabeled(vec![
        SidebarItem::new("Search", Msg::Noop),
    ]);
    let mailboxes = SidebarSection::new(
        "Mailboxes",
        vec![
            SidebarItem::new("Inbox", Msg::Noop).active(true),
            SidebarItem::new("Drafts", Msg::Noop),
            SidebarItem::new("Sent", Msg::Noop),
            SidebarItem::new("Archive", Msg::Noop),
        ],
    );

    let demo = container(
        row![sidebar(vec![pinned, mailboxes]), filler()]
            .width(Length::Fill)
            .height(Length::Fill),
    )
    .style(card_style)
    .height(Length::Fixed(320.0))
    .width(Length::Fill);

    column![
        heading("Sidebar"),
        body("Vertical column of selectable items grouped into optionally-labeled sections. Active row uses BG_HOVER + accent text; section headers use uppercase condensed-bold in the muted-text colour.")
            .style(muted),
        demo,
        code("sidebar(sections: Vec<SidebarSection<Msg>>)").style(muted),
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
