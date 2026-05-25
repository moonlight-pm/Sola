//! Vertical sidebar — column of selectable items grouped into
//! optionally-labeled sections.
//!
//! Pattern: caller builds a `Vec<SidebarSection<_>>` (each holding its
//! own `Vec<SidebarItem<_>>`) from its own state and hands it to
//! `sidebar(sections)`. The component is parent-controlled (no internal
//! selection state) so the consumer's update fn stays the single source
//! of truth for which item is active.
//!
//! Style fns read from `theme.extended_palette()` only — the kit's
//! atom→slot bindings live in [`crate::theme::sola_extended`]. To
//! restyle the sidebar globally, edit that mapping; this file should
//! never see a raw `hex::*`.

use iced::widget::{Space, button, column, container, text};
use iced::{Background, Border, Color, Element, Length, Padding, Theme};

use crate::fonts;

/// One row in the sidebar. `active` flips on the visual state; `message`
/// is what the parent receives when the row is clicked.
pub struct SidebarItem<Message> {
    pub label: String,
    pub active: bool,
    pub message: Message,
}

impl<Message> SidebarItem<Message> {
    pub fn new(label: impl Into<String>, message: Message) -> Self {
        Self { label: label.into(), active: false, message }
    }

    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }
}

/// A group of sidebar rows with an optional uppercase header label.
/// Unlabeled sections render as a plain item group (useful for a top
/// "Welcome" entry that sits above the first headed section).
pub struct SidebarSection<Message> {
    pub label: Option<String>,
    pub items: Vec<SidebarItem<Message>>,
}

impl<Message> SidebarSection<Message> {
    pub fn new(label: impl Into<String>, items: Vec<SidebarItem<Message>>) -> Self {
        Self { label: Some(label.into()), items }
    }

    pub fn unlabeled(items: Vec<SidebarItem<Message>>) -> Self {
        Self { label: None, items }
    }
}

/// Default sidebar width — matches the storybook's nav column. Public
/// so consumers can lay out alongside it (`width = Fill - SIDEBAR_WIDTH`).
pub const SIDEBAR_WIDTH: f32 = 200.0;

pub fn sidebar<'a, Message>(
    sections: Vec<SidebarSection<Message>>,
) -> Element<'a, Message>
where
    Message: Clone + 'a,
{
    let mut col = column![].spacing(2).padding(Padding::from([8, 6]));
    for (i, section) in sections.into_iter().enumerate() {
        if i > 0 {
            col = col.push(Space::new().height(Length::Fixed(12.0)));
        }
        if let Some(label) = section.label {
            col = col.push(section_header(label));
        }
        for item in section.items {
            col = col.push(sidebar_item(item));
        }
    }
    container(col)
        .style(style)
        .width(Length::Fixed(SIDEBAR_WIDTH))
        .height(Length::Fill)
        .into()
}

fn section_header<'a, Message: 'a>(label: String) -> Element<'a, Message> {
    container(
        text(label.to_uppercase())
            .font(fonts::chrome())
            .size(11)
            .style(|theme: &Theme| {
                let p = theme.extended_palette();
                iced::widget::text::Style { color: Some(p.secondary.base.text) }
            }),
    )
    .padding(Padding::from([6, 10]))
    .into()
}

fn sidebar_item<'a, Message>(item: SidebarItem<Message>) -> Element<'a, Message>
where
    Message: Clone + 'a,
{
    let label = text(item.label).font(fonts::ui()).size(13);
    let active = item.active;
    button(label)
        .style(move |t, status| item_style(t, status, active))
        .padding(Padding::from([6, 10]))
        .width(Length::Fill)
        .on_press(item.message)
        .into()
}

/// Container style for the sidebar track — the raised panel that the
/// rows sit on top of. Pass via `container(...).style(sola_kit::components::sidebar::style)`.
pub fn style(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    container::Style {
        background: Some(Background::Color(p.background.weaker.color)),
        border: Border {
            color: p.background.strongest.color,
            width: 0.0,
            radius: 0.0.into(),
        },
        ..container::Style::default()
    }
}

/// Style fn for an individual sidebar row. Exposed so consumers
/// building custom row widgets (e.g. with leading icons) can match the
/// kit's visual language.
pub fn item_style(theme: &Theme, status: button::Status, active: bool) -> button::Style {
    let p = theme.extended_palette();
    let bg = if active {
        p.background.strong.color
    } else {
        match status {
            button::Status::Hovered => p.background.strong.color,
            _ => Color::TRANSPARENT,
        }
    };
    let text_color = if active {
        p.primary.base.color
    } else {
        p.background.base.text
    };
    button::Style {
        background: Some(Background::Color(bg)),
        text_color,
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: 4.0.into(),
        },
        shadow: Default::default(),
        snap: false,
    }
}

/// Vertical filler — useful when a caller wants the sidebar to push
/// later content to the bottom. The storybook uses this between its
/// component list and a future "About" link.
pub fn flex_spacer<'a, Message: 'a>() -> Element<'a, Message> {
    Space::new().height(Length::Fill).into()
}
