//! Settings → Default Apps (`docs/specs/2026-10-31-sola-open-design.md`):
//! every link scheme and file type something opens, the app that opens it,
//! and the others that could. Choosing one writes
//! `~/.config/sola/handlers/defaults.json`, which Sola's opener reads on
//! every open; nothing else changes it but a request you said yes to.

use iced::widget::{Column, column, row, text};
use iced::{Alignment, Element, Length};
use sola_core::applications::ApplicationsConfig;
use sola_kit::components::button;
use sola_kit::components::style::{SPACE_SM, SPACE_XS};
use sola_kit::fonts;
use sola_kit::sola_open::Kind;
use sola_kit::sola_open::resolve::{self, Handler, Row, Sources};

use crate::page;

#[derive(Default)]
pub struct HandlersState {
    rows: Vec<Row>,
    open: Option<Kind>,
    note: Option<String>,
}

#[derive(Debug, Clone)]
pub enum HandlersMsg {
    Toggle(Kind),
    /// Make the handler with this id (an app id, a `.desktop` file) the one.
    Choose(Kind, String),
}

/// Read the claims, your choices and the `.desktop` handlers again.
pub fn refresh(state: &mut HandlersState) {
    state.rows = resolve::table(&Sources::live());
}

pub fn update(msg: HandlersMsg, state: &mut HandlersState) {
    match msg {
        HandlersMsg::Toggle(kind) => {
            state.open = (state.open.as_ref() != Some(&kind)).then_some(kind);
            state.note = None;
        }
        HandlersMsg::Choose(kind, id) => {
            state.note = sola_kit::sola_open::defaults::set(&kind, &id)
                .err()
                .map(|e| format!("Could not save: {e}"));
            if state.note.is_none() {
                tracing::info!(kind = %kind.key(), handler = %id, "chose the handler");
            }
            refresh(state);
        }
    }
}

pub fn view<'a>(state: &'a HandlersState, apps: &'a ApplicationsConfig) -> Element<'a, HandlersMsg> {
    let name = |h: &Handler| match h {
        Handler::App(id) => apps.get(id).map(|a| a.label.clone()).unwrap_or_else(|| id.clone()),
        Handler::Desktop(e) => e.name.clone(),
    };
    let rows_for = |schemes: bool| -> Vec<Element<'a, HandlersMsg>> {
        let mut out = Vec::new();
        for r in state
            .rows
            .iter()
            .filter(|r| matches!(r.kind, Kind::Scheme(_)) == schemes)
        {
            let open = state.open.as_ref() == Some(&r.kind);
            let now = r.handler.as_ref().map(&name).unwrap_or_else(|| "Nothing".into());
            out.push(
                page::item(r.kind.key())
                    .subtitle(now)
                    .open(open)
                    .on_press(HandlersMsg::Toggle(r.kind.clone()))
                    .into(),
            );
            if open {
                out.push(page::editor(choices(r, &name, state.note.as_deref())));
            }
        }
        if out.is_empty() {
            out.push(page::empty("Nothing claims any yet."));
        }
        out
    };
    page::page(vec![
        page::section(
            "Links",
            None,
            page::group(rows_for(true)),
            Some(
                "Links from other apps, pages and xdg-open. An app that claims one later never \
                 takes it over; it is listed here."
                    .into(),
            ),
        ),
        page::section(
            "Files",
            None,
            page::group(rows_for(false)),
            Some("Files opened from Mail, a download, or xdg-open, by their extension.".into()),
        ),
    ])
}

/// A row's editor: every handler that could open it, the one that does
/// marked.
fn choices<'a>(
    r: &'a Row,
    name: &dyn Fn(&Handler) -> String,
    note: Option<&'a str>,
) -> Element<'a, HandlersMsg> {
    let mut col = Column::new().spacing(SPACE_SM).width(Length::Fill);
    for h in &r.choices {
        let current = r.handler.as_ref().map(Handler::id) == Some(h.id());
        let style = if current { button::primary } else { button::secondary };
        let label = if current { format!("{} ✓", name(h)) } else { name(h) };
        let kind_note = match h {
            Handler::App(_) => "",
            Handler::Desktop(_) => ".desktop",
        };
        col = col.push(
            row![
                button::labeled_sm(label, style).on_press(HandlersMsg::Choose(r.kind.clone(), h.id().to_string())),
                text(kind_note).font(fonts::ui()).size(11),
            ]
            .spacing(SPACE_XS)
            .align_y(Alignment::Center),
        );
    }
    if let Some(note) = note {
        col = col.push(text(note).font(fonts::ui()).size(12));
    }
    column![col].into()
}
