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

use std::sync::Arc;

use iced::widget::{container, row, scrollable};
use iced::{Element, Length, Padding, Subscription};

use sola_bus::topics::{MenuActionPayload, Topic};
use sola_kit::components::{SidebarItem, SidebarSection, sidebar};
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
    /// Order rendered in the sidebar. Grouped by [`Page::section`]:
    /// Welcome (no section header) → Theme → Layout → Components.
    pub const ALL: &'static [Page] = &[
        Page::Welcome,
        Page::Theme,
        Page::Divider,
        Page::Split,
        Page::Toolbar,
        Page::Text,
        Page::Button,
        Page::Badge,
        Page::Card,
        Page::Field,
        Page::Popover,
        Page::Sidebar,
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


    /// Section bucket for the sidebar. `None` means the page renders
    /// without a section header (currently only `Welcome` at the top).
    /// Mirrors sola-kit-legacy's Theme / Layout / Components grouping.
    pub fn section(self) -> Option<&'static str> {
        match self {
            Page::Welcome => None,
            Page::Theme => Some("Theme"),
            Page::Divider | Page::Split | Page::Toolbar => Some("Layout"),
            Page::Text
            | Page::Button
            | Page::Badge
            | Page::Card
            | Page::Field
            | Page::Popover
            | Page::Sidebar => Some("Components"),
        }
    }
}

#[derive(Clone, Debug)]
pub enum Msg {
    Select(Page),
    Toolbar(pages::toolbar::Msg),
    Field(pages::field::Msg),
    /// Bus message arriving via [`sola_kit::app::bus_subscription`].
    /// Wrapped in `Arc` to keep cloning cheap for iced's mpsc fanout.
    Bus(Arc<sola_bus::Message>),
    /// Overwrite the accent atom and broadcast the new theme.
    SetAccent(iced::Color),
    /// Swap one font role to a new family name. Origin is the Theme
    /// page's per-role pick_list; the storybook update reinstalls the
    /// fonts table and re-emits Topic::Theme.
    SetFont(FontRole, String),
    /// Demo placeholder for showcases whose components require an
    /// `on_press` (or similar callback) message but don't model
    /// interaction in the storybook.
    Noop,
}

/// Identifies which slot in [`theme::FontSelection`] a `SetFont` edit
/// targets. Kept here (not in `theme.rs`) because it's UI-shaped — the
/// pick_list per role wraps each `Family` into a different `SetFont(role, ..)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontRole {
    Ui,
    UiMedium,
    Display,
    Chrome,
    Mono,
}

pub struct Storybook {
    page: Page,
    toolbar: pages::toolbar::State,
    field: pages::field::State,
    /// Editable colour atoms — the storybook is the source of truth for
    /// the live theme. Mutating an atom rebuilds `theme` and re-emits
    /// `Topic::Theme` on the bus so other kit apps update in lockstep.
    atoms: theme::Atoms,
    /// Editable per-role font family selection. Same edit→emit loop as
    /// `atoms`; consumer-side `sola_kit::theme::from_bus_theme` installs
    /// the resulting `Fonts` table as a side effect.
    fonts: theme::FontSelection,
    /// Live iced theme — derived from `atoms` on every edit, also
    /// replaceable by a `Topic::Theme` delivery from another emitter.
    theme: iced::Theme,
}

impl Storybook {
    pub fn default() -> Self {
        let atoms = theme::Atoms::default();
        let fonts = theme::FontSelection::default();
        let theme = theme::iced_theme_from_atoms(&atoms);
        Self {
            page: Page::Welcome,
            toolbar: pages::toolbar::State::default(),
            field: pages::field::State::default(),
            atoms,
            fonts,
            theme,
        }
    }

    pub fn title(&self) -> String {
        format!("sola-kit · {}", self.page.label())
    }

    pub fn theme(&self) -> iced::Theme {
        self.theme.clone()
    }

    pub fn subscription(&self) -> Subscription<Msg> {
        sola_kit::app::bus_subscription().map(Msg::Bus)
    }

    pub fn update(&mut self, msg: Msg) {
        match msg {
            Msg::Select(page) => self.page = page,
            Msg::Toolbar(m) => self.toolbar.update(m),
            Msg::Field(m) => self.field.update(m),
            Msg::Bus(message) => match Topic::parse(&message) {
                Some(Topic::Theme(bus_theme)) => {
                    // External theme delivery (sticky-replay from
                    // sola-shell on first connect, or another editor's
                    // edit). Rebuild iced theme — from_bus_theme also
                    // reinstalls the fonts table as a side effect.
                    self.theme = theme::from_bus_theme(&bus_theme);
                }
                Some(Topic::MenuAction(MenuActionPayload { app_id, action_id }))
                    if app_id == "sola-kit" && action_id == "quit" =>
                {
                    std::process::exit(0);
                }
                _ => {}
            },
            Msg::SetAccent(color) => {
                self.atoms.accent = color;
                self.theme = theme::iced_theme_from_atoms(&self.atoms);
                self.broadcast_theme();
            }
            Msg::SetFont(role, family) => {
                match role {
                    FontRole::Ui => self.fonts.ui = family,
                    FontRole::UiMedium => self.fonts.ui_medium = family,
                    FontRole::Display => self.fonts.display = family,
                    FontRole::Chrome => self.fonts.chrome = family,
                    FontRole::Mono => self.fonts.mono = family,
                }
                // Install locally so the storybook's own widgets pick
                // up the swap before the bus delivery loops back.
                sola_kit::fonts::install(sola_kit::fonts::fonts_from_families(
                    &self.fonts.ui,
                    &self.fonts.ui_medium,
                    &self.fonts.display,
                    &self.fonts.chrome,
                    &self.fonts.mono,
                ));
                self.broadcast_theme();
            }
            Msg::Noop => {}
        }
    }


    /// Emit the current atoms + fonts as a `Topic::Theme`. Centralised
    /// so SetAccent / SetFont stay symmetric.
    fn broadcast_theme(&self) {
        let bus_theme = theme::bus_theme_from(&self.atoms, &self.fonts);
        match sola_kit::app::bus().lock() {
            Ok(mut bus) => {
                if let Err(err) = bus.emit(Topic::Theme(bus_theme)) {
                    tracing::warn!("failed to emit Topic::Theme: {err}");
                }
            }
            Err(err) => {
                tracing::warn!("bus mutex poisoned, can't emit Topic::Theme: {err}");
            }
        }
    }

    pub fn view(&self) -> Element<'_, Msg> {
        // Bucket pages into sections in `Page::ALL` order; the first
        // section keyed by a given header wins (matches legacy's
        // first-appearance ordering).
        let mut buckets: Vec<(Option<&'static str>, Vec<SidebarItem<Msg>>)> = Vec::new();
        for p in Page::ALL.iter().copied() {
            let item = SidebarItem::new(p.label(), Msg::Select(p)).active(p == self.page);
            let key = p.section();
            match buckets.iter_mut().find(|(k, _)| *k == key) {
                Some((_, items)) => items.push(item),
                None => buckets.push((key, vec![item])),
            }
        }
        let sections: Vec<SidebarSection<Msg>> = buckets
            .into_iter()
            .map(|(key, items)| match key {
                Some(label) => SidebarSection::new(label, items),
                None => SidebarSection::unlabeled(items),
            })
            .collect();

        let content = scrollable(
            container(self.page_view())
                .padding(Padding::from([20, 28]))
                .width(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Fill);

        row![sidebar(sections), content]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn page_view(&self) -> Element<'_, Msg> {
        match self.page {
            Page::Welcome => pages::welcome::view(),
            Page::Theme => pages::theme::view(&self.atoms, &self.fonts),
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
