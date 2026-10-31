//! Moving an item into an organization: choose the organization, then its
//! collections. It can't come back out, so the sheet says so.

use std::collections::BTreeSet;

use iced::widget::{Space, button, checkbox, column, row, text};
use iced::{Alignment, Element, Length, Padding, Task};
use sola_kit::components::button as kit_btn;
use sola_kit::components::form::checkbox_style;
use sola_kit::components::style::{SPACE_LG, SPACE_SM};
use sola_kit::components::text as kit_text;
use sola_kit::fonts;
use sola_vault_proto::Request;

use super::Sheet;
use crate::ui::ask::Ask;
use crate::ui::signin::{error_caption, head, lead_text, sheet_sized};
use crate::ui::widgets::{self, PlateSize};
use crate::ui::{App, Msg, style};

#[derive(Default)]
pub struct ShareSheet {
    pub org: Option<String>,
    pub collections: BTreeSet<String>,
    pub busy: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub enum ShareMsg {
    Org(String),
    Collection(String, bool),
    Go,
}

impl App {
    /// The open item can move into an organization.
    pub(in crate::ui) fn can_share(&self) -> bool {
        !self.orgs.is_empty()
            && self
                .selected_summary()
                .is_some_and(|s| !s.deleted && s.organization_id.is_none())
    }

    pub(in crate::ui) fn ask_share(&mut self) -> Task<Msg> {
        self.popup = None;
        if self.can_share() {
            let org = (self.orgs.len() == 1).then(|| self.orgs[0].id.clone());
            self.sheet = Some(Sheet::Share(ShareSheet {
                org,
                ..ShareSheet::default()
            }));
        }
        Task::none()
    }

    pub(in crate::ui) fn on_share(&mut self, m: ShareMsg) -> Task<Msg> {
        let Some(Sheet::Share(s)) = self.sheet.as_mut() else {
            return Task::none();
        };
        match m {
            ShareMsg::Org(id) => {
                if s.org.as_deref() != Some(id.as_str()) {
                    s.collections.clear();
                }
                s.org = Some(id);
                s.error = None;
                Task::none()
            }
            ShareMsg::Collection(id, on) => {
                if on {
                    s.collections.insert(id);
                } else {
                    s.collections.remove(&id);
                }
                Task::none()
            }
            ShareMsg::Go => {
                let (Some(org), false) = (s.org.clone(), s.collections.is_empty()) else {
                    s.error = Some("Choose at least one collection.".into());
                    return Task::none();
                };
                let Some(id) = self.selected.clone() else {
                    return Task::none();
                };
                s.busy = true;
                let collection_ids = s.collections.iter().cloned().collect();
                self.ask(
                    Ask::Share(id.clone()),
                    Request::ShareToOrganization {
                        id,
                        organization_id: org,
                        collection_ids,
                    },
                )
            }
        }
    }

    pub(in crate::ui) fn view_share<'a>(&'a self, s: &'a ShareSheet) -> Element<'a, Msg> {
        let name = self
            .selected_summary()
            .map(|i| i.name.clone())
            .unwrap_or_default();
        let mut body = column![lead_text(
            "Once it is in an organization, it can't come back out. Its members with the \
             collections can see it."
                .into()
        )]
        .spacing(SPACE_LG);
        let orgs = column(self.orgs.iter().map(|o| {
            let on = s.org.as_deref() == Some(o.id.as_str());
            button(
                row![
                    widgets::muted("lucide/building", 14.0),
                    text(o.name.as_str()).font(fonts::ui()).size(13).width(Length::Fill),
                    if on {
                        widgets::muted("lucide/check", 14.0)
                    } else {
                        Space::new().width(14).into()
                    },
                ]
                .spacing(10)
                .align_y(Alignment::Center),
            )
            .padding(Padding::from([7.0, 10.0]))
            .width(Length::Fill)
            .style(style::list_row(on))
            .on_press(Msg::Share(ShareMsg::Org(o.id.clone())))
            .into()
        }))
        .spacing(2);
        body = body.push(orgs);
        if let Some(org) = &s.org {
            let writable: Vec<_> = self
                .collections
                .iter()
                .filter(|c| c.organization_id == *org && !c.read_only)
                .collect();
            if writable.is_empty() {
                body = body.push(
                    kit_text::caption("You can't add items to any of its collections.")
                        .style(kit_text::muted),
                );
            } else {
                body = body.push(
                    column(writable.into_iter().map(|c| {
                        let id = c.id.clone();
                        checkbox(s.collections.contains(&c.id))
                            .label(c.name.as_str())
                            .on_toggle(move |on| Msg::Share(ShareMsg::Collection(id.clone(), on)))
                            .style(checkbox_style)
                            .size(14)
                            .text_size(13)
                            .into()
                    }))
                    .spacing(SPACE_SM)
                    .padding(Padding::from([0.0, 10.0])),
                );
            }
        }
        if let Some(e) = &s.error {
            body = body.push(error_caption(e));
        }
        body = body.push(
            row![
                Space::new().width(Length::Fill),
                kit_btn::labeled("Cancel", kit_btn::ghost).on_press(Msg::CloseSheet),
                kit_btn::labeled(if s.busy { "Moving…" } else { "Move" }, kit_btn::primary)
                    .on_press_maybe(
                        (!s.busy && !s.collections.is_empty())
                            .then_some(Msg::Share(ShareMsg::Go)),
                    ),
            ]
            .spacing(SPACE_SM)
            .align_y(Alignment::Center),
        );
        sheet_sized(
            head(
                None,
                Some(widgets::glyph_plate("lucide/building", PlateSize::Small)),
                "Move to an organization".into(),
                name,
            ),
            body.into(),
            400.0,
        )
    }
}
