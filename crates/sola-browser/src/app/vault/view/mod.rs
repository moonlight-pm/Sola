//! The sheet: 360 wide in every state, hanging from the key button with
//! popover chrome, its height its content's up to 560. A state changes
//! what is inside, never the frame. The head says where you are, the
//! foot is a status line.

mod browse;
mod forms;
mod gate;
mod key;
mod parts;
mod passkey;
mod record;

use std::time::Duration;

use iced::widget::{Space, button, column, container, mouse_area, row, stack};
use iced::{Alignment, Background, Border, Element, Length, Padding, Theme};
use sola_kit::components::popover::{self, Placement, popover_anchored};
use sola_kit::components::style::{PAD_CONTROL_SM, RADIUS_LG, RADIUS_SM};
use sola_kit::components::{MenuItem, context_menu};

use super::update::VaultMsg;
use super::{Gate, Screen, rows};
use crate::app::{App, CHROME_HEIGHT, Msg};
use crate::engine::Engine;
use parts::{icon_button, ink, ink2, label, msg};

pub(crate) use parts::glyph;

/// The sheet's width, in every state.
pub(crate) const SHEET_W: f32 = 360.0;
/// It grows with its content up to this, then its list scrolls.
pub(crate) const SHEET_MAX_H: f32 = 560.0;
const HEAD_H: f32 = 44.0;
const FOOT_H: f32 = 34.0;
/// How long a copied field says so.
pub(crate) const COPIED_FOR: Duration = Duration::from_millis(1500);

/// A sheet field's widget id.
pub(crate) fn id(name: &str) -> iced::widget::Id {
    iced::widget::Id::from(format!("sola-browser-vault-{name}"))
}

/// The sheet's head: an optional back, a plate, who/where, its buttons.
pub(crate) struct Head<'a> {
    pub back: Option<VaultMsg>,
    pub plate: Option<Element<'a, Msg>>,
    pub title: String,
    pub sub: Option<String>,
    pub buttons: Vec<Element<'a, Msg>>,
}

impl<'a> Head<'a> {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            back: None,
            plate: None,
            title: title.into(),
            sub: None,
            buttons: Vec::new(),
        }
    }

    pub fn back(mut self, m: VaultMsg) -> Self {
        self.back = Some(m);
        self
    }

    pub fn plate(mut self, p: Element<'a, Msg>) -> Self {
        self.plate = Some(p);
        self
    }

    pub fn sub(mut self, s: impl Into<String>) -> Self {
        self.sub = Some(s.into());
        self
    }

    pub fn button(mut self, b: Element<'a, Msg>) -> Self {
        self.buttons.push(b);
        self
    }

    fn view(self) -> Element<'a, Msg> {
        let mut r = row![].spacing(10).align_y(Alignment::Center);
        let left = if let Some(back) = self.back {
            r = r.push(icon_button("chevron-left", 16, Some(back)));
            6.0
        } else {
            14.0
        };
        if let Some(p) = self.plate {
            r = r.push(p);
        }
        let mut who = column![
            label(self.title, 13.0, ink)
                .font(sola_kit::fonts::ui_medium())
                .wrapping(iced::widget::text::Wrapping::None)
        ]
        .spacing(2)
        .width(Length::Fill)
        .clip(true);
        if let Some(sub) = self.sub {
            who = who.push(label(sub, 11.0, ink2).wrapping(iced::widget::text::Wrapping::None));
        }
        r = r.push(who);
        for b in self.buttons {
            r = r.push(b);
        }
        container(r)
            .height(Length::Fixed(HEAD_H))
            .padding(Padding {
                top: 0.0,
                right: 8.0,
                bottom: 0.0,
                left,
            })
            .align_y(Alignment::Center)
            .into()
    }
}

/// What goes in the sheet: head, body, and either a foot (status) or
/// actions (the sheet's one primary).
pub(crate) struct Sheet<'a> {
    pub head: Head<'a>,
    pub body: Element<'a, Msg>,
    pub foot: Option<Element<'a, Msg>>,
}

impl<E: Engine> App<E> {
    /// The sheet under the key button, over the page (not dimming it); a
    /// click on the page closes it.
    pub(crate) fn view_vault_panel(&self) -> Element<'_, Msg> {
        let sheet = self.vault_sheet();
        let mut col = column![sheet.head.view(), sheet.body].width(Length::Fill);
        if let Some(foot) = sheet.foot {
            col = col.push(foot);
        }
        let panel = container(col)
            .width(Length::Fixed(SHEET_W))
            .max_height(SHEET_MAX_H)
            .clip(true)
            .style(sheet_style);
        // A click on the sheet stays on it (only the page around closes it).
        let panel = mouse_area(panel).on_press(Msg::Noop);
        let backdrop = mouse_area(Space::new().width(Length::Fill).height(Length::Fill))
            .on_press(msg(VaultMsg::Close));
        // Its right edge under the key button's (downloads sits right of it).
        let anchored = container(panel)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Alignment::End)
            .align_y(Alignment::Start)
            .padding(Padding {
                top: CHROME_HEIGHT + 4.0,
                right: crate::app::key_button_inset(),
                bottom: 0.0,
                left: 0.0,
            });
        stack![backdrop, anchored].into()
    }

    fn vault_sheet(&self) -> Sheet<'_> {
        let v = &self.vault;
        match v.gate() {
            Gate::Away => self.sheet_away(),
            Gate::Connecting => self.sheet_connecting(),
            Gate::SignIn => self.sheet_sign_in(),
            Gate::TwoStep => self.sheet_two_step(),
            Gate::Locked => self.sheet_unlock(),
            Gate::Open if v.reprompt.is_some() => self.sheet_reprompt(),
            Gate::Open => match v.screen {
                Screen::Browse => self.sheet_browse(),
                Screen::Record => self.sheet_record(),
                Screen::Edit => self.sheet_edit(),
                Screen::Create => self.sheet_create(),
                Screen::CreateSaved => self.sheet_created(),
                Screen::PasskeyPick => self.sheet_passkey_pick(),
                Screen::PasskeyCreate => self.sheet_passkey_create(),
            },
        }
    }

    /// The status line: when it synced (◆ when that failed), who.
    pub(crate) fn vault_foot(&self) -> Element<'_, Msg> {
        let Some(s) = self.vault.status.as_ref() else {
            return Space::new().into();
        };
        let failed = s.sync_error.is_some();
        let mark: Element<'_, Msg> = if failed {
            label("◆", 10.0, parts::warning).into()
        } else if s.syncing {
            parts::icon("refresh-cw", 12, ink2)
        } else {
            parts::icon("check", 12, ink2)
        };
        let line = if s.syncing {
            "Syncing…".to_string()
        } else {
            rows::synced_label(s.synced_at, super::update::now_secs(), failed)
        };
        foot_line(mark, line, Some(s.email.clone()))
    }

    /// `+` in a head: a new login.
    pub(crate) fn plus_button(&self) -> Element<'_, Msg> {
        icon_button("plus", 16, (!self.vault.busy).then_some(VaultMsg::NewLogin))
    }

    /// `···` in a head, with its menu: housekeeping out of the way.
    pub(crate) fn more_button(&self, item: Option<String>) -> Element<'_, Msg> {
        let base = icon_button("ellipsis", 16, Some(VaultMsg::More));
        if !self.vault.more {
            return base;
        }
        let s = self.vault.status.as_ref();
        let synced = s.map(|s| {
            rows::synced_label(s.synced_at, super::update::now_secs(), s.sync_error.is_some())
        });
        let email = s.map(|s| s.email.clone()).unwrap_or_default();
        let mut items = vec![MenuItem::action("Sync now", msg(VaultMsg::SyncNow))];
        if let Some(synced) = synced {
            items.push(MenuItem::disabled(synced));
        }
        items.push(MenuItem::separator());
        items.push(MenuItem::action("New login…", msg(VaultMsg::NewLogin)));
        let open = if item.is_some() {
            "Open in Vault"
        } else {
            "Open Vault"
        };
        items.push(MenuItem::action(open, msg(VaultMsg::OpenInVault(item))));
        items.push(MenuItem::separator());
        items.push(MenuItem::action("Log out", msg(VaultMsg::LogOut)));
        if !email.is_empty() {
            items.push(MenuItem::disabled(email));
        }
        popover_anchored(base, context_menu(items), msg(VaultMsg::MoreClose))
            .placement(Placement::End)
            .into()
    }

    /// The key button in the location bar, the vault's state at a glance
    /// ([`key`]): the key while unlocked, dimmed with a lock while locked,
    /// dimmed alone while signed out or away; a fingerprint while a site
    /// waits for a passkey; pressed while the sheet is up.
    pub(crate) fn vault_key_button(&self, muted: iced::Color) -> Element<'_, Msg> {
        let v = &self.vault;
        let open = v.is_open();
        let look = key::KeyLook::of(v.gate(), v.passkey.is_some());
        let fg = self.theme.extended_palette().background.base.text;
        button(key::glyph_for(look, 18, fg, muted, open))
            .padding(PAD_CONTROL_SM)
            .width(Length::Fixed(crate::app::NAV_BTN_W))
            .style(move |t: &Theme, status| {
                if open {
                    button::Style {
                        background: Some(Background::Color(sola_kit::theme::selection())),
                        text_color: ink(t),
                        border: Border {
                            radius: RADIUS_SM.into(),
                            ..Border::default()
                        },
                        ..button::Style::default()
                    }
                } else {
                    sola_kit::components::toolbar::style(t, status)
                }
            })
            .on_press(msg(VaultMsg::Toggle))
            .into()
    }
}

/// A one-line foot: a mark, a line that takes the room, a trailing word.
pub(crate) fn foot_line<'a>(
    mark: Element<'a, Msg>,
    line: String,
    trailing: Option<String>,
) -> Element<'a, Msg> {
    let mut r = row![
        mark,
        label(line, 11.0, ink2)
            .wrapping(iced::widget::text::Wrapping::None)
            .width(Length::Fill)
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    if let Some(t) = trailing {
        r = r.push(label(t, 11.0, ink2).wrapping(iced::widget::text::Wrapping::None));
    }
    column![
        container(Space::new().height(1.0).width(Length::Fill)).style(|t: &Theme| {
            container::Style {
                background: Some(Background::Color(parts::rule_color(t))),
                ..container::Style::default()
            }
        }),
        container(r)
            .height(Length::Fixed(FOOT_H - 1.0))
            .padding(Padding {
                top: 0.0,
                right: 10.0,
                bottom: 0.0,
                left: 14.0,
            })
            .align_y(Alignment::Center)
            .clip(true),
    ]
    .into()
}

/// The sheet's actions: one primary, wide; or Cancel and the primary
/// split to the ends.
pub(crate) fn actions<'a>(items: Vec<Element<'a, Msg>>, split: bool) -> Element<'a, Msg> {
    let mut r = row![].spacing(8).align_y(Alignment::Center);
    let n = items.len();
    for (i, it) in items.into_iter().enumerate() {
        if split && i + 1 == n && n > 1 {
            r = r.push(Space::new().width(Length::Fill));
        }
        r = r.push(it);
    }
    container(r)
        .padding(Padding {
            top: 10.0,
            right: 14.0,
            bottom: 14.0,
            left: 14.0,
        })
        .width(Length::Fill)
        .into()
}

/// The kit popover's chrome at the sheet's radius.
fn sheet_style(t: &Theme) -> container::Style {
    let mut s = popover::style(t);
    s.border.radius = RADIUS_LG.into();
    s
}

/// The body's padding: rows run nearly edge to edge; forms breathe.
pub(crate) fn body<'a>(content: impl Into<Element<'a, Msg>>, padded: bool) -> Element<'a, Msg> {
    let pad = if padded {
        Padding {
            top: 4.0,
            right: 14.0,
            bottom: 14.0,
            left: 14.0,
        }
    } else {
        Padding {
            top: 0.0,
            right: 8.0,
            bottom: 8.0,
            left: 8.0,
        }
    };
    container(content).padding(pad).width(Length::Fill).into()
}

/// The list's room: what the sheet has left under its head, the search
/// (30 and a gap) and the foot, less the body's bottom pad.
pub(crate) const LIST_MAX_H: f32 = SHEET_MAX_H - HEAD_H - FOOT_H - 30.0 - 8.0 - 8.0;
