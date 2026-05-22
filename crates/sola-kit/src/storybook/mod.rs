//! Storybook app — sidebar nav + showcase content pane.
//!
//! Each kit-shipped component (badge, button, card, divider, field,
//! popover, sidebar, split, text, theme atoms, toolbar) gets a page in
//! [`pages`]. The app keeps the selected page in state and re-renders
//! on `Select(Page)`.
//!
//! Pages are responsible for their own demo state — the storybook
//! shell only owns the page-selector. Adding a new page is: append a
//! `Page` variant, add a `pages::<name>::view()` module, hand it to
//! the sidebar list.

use iced::widget::{container, row, scrollable};
use iced::{Element, Length, Padding};

use sola_kit::components::{SidebarItem, sidebar};
use sola_kit::theme;

pub mod pages;

/// Which showcase page is currently rendered. Lives in the storybook's
/// state so navigation is parent-controlled (the sidebar component
/// doesn't track its own selection — see its module docs).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Page {
    Welcome,
    Theme,
    Text,
    Button,
    Badge,
    Card,
    Field,
    Divider,
    Popover,
    Sidebar,
    Split,
    Toolbar,
}

impl Page {
    /// Order rendered in the sidebar. Roughly grouped: foundations
    /// first (theme, text), then primitives (button/badge/card/field),
    /// then layout (divider/popover/sidebar/split/toolbar).
    pub const ALL: &'static [Page] = &[
        Page::Welcome,
        Page::Theme,
        Page::Text,
        Page::Button,
        Page::Badge,
        Page::Card,
        Page::Field,
        Page::Divider,
        Page::Popover,
        Page::Sidebar,
        Page::Split,
        Page::Toolbar,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Page::Welcome => "Welcome",
            Page::Theme => "Theme",
            Page::Text => "Text",
            Page::Button => "Button",
            Page::Badge => "Badge",
            Page::Card => "Card",
            Page::Field => "Field",
            Page::Divider => "Divider",
            Page::Popover => "Popover",
            Page::Sidebar => "Sidebar",
            Page::Split => "Split",
            Page::Toolbar => "Toolbar",
        }
    }
}

#[derive(Clone, Debug)]
pub enum Msg {
    Select(Page),
    Toolbar(pages::toolbar::Msg),
    Field(pages::field::Msg),
    /// Demo placeholder for showcases whose components require an
    /// `on_press` (or similar callback) message but don't model
    /// interaction in the storybook.
    Noop,
}

pub struct Storybook {
    page: Page,
    toolbar: pages::toolbar::State,
    field: pages::field::State,
}

impl Storybook {
    pub fn default() -> Self {
        Self {
            page: Page::Welcome,
            toolbar: pages::toolbar::State::default(),
            field: pages::field::State::default(),
        }
    }

    pub fn title(&self) -> String {
        format!("sola-kit · {}", self.page.label())
    }

    pub fn theme(&self) -> iced::Theme {
        theme::default_theme()
    }

    pub fn update(&mut self, msg: Msg) {
        match msg {
            Msg::Select(page) => self.page = page,
            Msg::Toolbar(m) => self.toolbar.update(m),
            Msg::Field(m) => self.field.update(m),
            Msg::Noop => {}
        }
    }

    pub fn view(&self) -> Element<'_, Msg> {
        let items: Vec<SidebarItem<Msg>> = Page::ALL
            .iter()
            .copied()
            .map(|p| SidebarItem::new(p.label(), Msg::Select(p)).active(p == self.page))
            .collect();

        let content = scrollable(
            container(self.page_view())
                .padding(Padding::from([20, 28]))
                .width(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Fill);

        row![sidebar(items), content]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn page_view(&self) -> Element<'_, Msg> {
        match self.page {
            Page::Welcome => pages::welcome::view(),
            Page::Theme => pages::theme::view(),
            Page::Text => pages::text::view(),
            Page::Button => pages::button::view(),
            Page::Badge => pages::badge::view(),
            Page::Card => pages::card::view(),
            Page::Field => pages::field::view(&self.field).map(Msg::Field),
            Page::Divider => pages::divider::view(),
            Page::Popover => pages::popover::view(),
            Page::Sidebar => pages::sidebar::view(),
            Page::Split => pages::split::view(),
            Page::Toolbar => pages::toolbar::view(&self.toolbar).map(Msg::Toolbar),
        }
    }
}
