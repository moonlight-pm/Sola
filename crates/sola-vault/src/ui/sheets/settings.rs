//! Settings, as Settings' grouped rows: the account (and Sign Out), when
//! the vault locks, how long a copied value stays, and whether filling a
//! login copies its code. Each change goes to the daemon as it is made.

use iced::widget::{Space, column, container, row, text, toggler};
use iced::{Alignment, Element, Length, Padding, Task};
use sola_kit::components::button as kit_btn;
use sola_kit::components::form::toggle_style;
use sola_kit::components::select::{SelectOption, select};
use sola_kit::components::style::{SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XS};
use sola_kit::components::text as kit_text;
use sola_kit::fonts;
use sola_vault_proto::Request;

use super::Sheet;
use crate::ui::ask::Ask;
use crate::ui::record::group;
use crate::ui::signin::{head, sheet_sized};
use crate::ui::widgets::{self, PlateSize};
use crate::ui::{App, Msg, Popup};

/// Lock after this long idle (`None`: only on the rules below).
const IDLE: &[(Option<u32>, &str)] = &[
    (Some(1), "1 minute"),
    (Some(5), "5 minutes"),
    (Some(15), "15 minutes"),
    (Some(30), "30 minutes"),
    (Some(60), "1 hour"),
    (Some(240), "4 hours"),
    (None, "Never"),
];

/// Clear a copied value after this many seconds (0: never).
const CLEAR: &[(u32, &str)] = &[
    (10, "10 seconds"),
    (20, "20 seconds"),
    (30, "30 seconds"),
    (60, "1 minute"),
    (120, "2 minutes"),
    (300, "5 minutes"),
    (0, "Never"),
];

#[derive(Debug, Clone)]
pub enum SetMsg {
    IdleLock(Option<u32>),
    LockOnScreen(bool),
    ClipClear(u32),
    CopyCode(bool),
    WebVault,
}

impl App {
    pub(in crate::ui) fn open_settings(&mut self) -> Task<Msg> {
        self.popup = None;
        if !self.status.as_ref().is_some_and(|s| s.signed_in) {
            return Task::none();
        }
        self.sheet = Some(Sheet::Settings);
        self.ask(Ask::Settings, Request::Settings)
    }

    pub(in crate::ui) fn on_settings(&mut self, m: SetMsg) -> Task<Msg> {
        self.popup = None;
        let s = &mut self.settings;
        match m {
            SetMsg::IdleLock(m) => s.idle_lock_minutes = m,
            SetMsg::LockOnScreen(on) => s.lock_on_screen_lock = on,
            SetMsg::ClipClear(secs) => s.clipboard_clear_secs = secs,
            SetMsg::CopyCode(on) => s.copy_code_on_fill = on,
            SetMsg::WebVault => {
                if let Some(st) = &self.status {
                    sola_core::open_url_logged(&st.server.web_vault());
                }
                return Task::none();
            }
        }
        let settings = self.settings.clone();
        self.ask(Ask::SetSettings, Request::SetSettings { settings })
    }

    pub(in crate::ui) fn view_settings(&self) -> Element<'_, Msg> {
        let s = &self.settings;
        let (email, server) = self
            .status
            .as_ref()
            .map(|st| (st.email.clone(), st.server.label()))
            .unwrap_or_default();

        let account = group(vec![
            setting("Email", value(email)),
            setting("Server", value(server)),
            container(
                row![
                    kit_btn::labeled_sm("Open the Web Vault", kit_btn::secondary)
                        .on_press(Msg::Settings(SetMsg::WebVault)),
                    Space::new().width(Length::Fill),
                    kit_btn::labeled_sm("Sign Out…", kit_btn::danger_outline)
                        .on_press(Msg::AskSignOut),
                ]
                .align_y(Alignment::Center),
            )
            .padding(Padding::from([8.0, 10.0]))
            .into(),
        ]);

        let idle_label = IDLE
            .iter()
            .find(|(m, _)| *m == s.idle_lock_minutes)
            .map_or_else(|| custom_minutes(s.idle_lock_minutes), |(_, l)| (*l).to_string());
        let idle_open = self.popup == Some(Popup::IdleLock);
        let idle = select(
            idle_label,
            IDLE.iter().map(|(m, l)| {
                SelectOption::new(*l, *m == s.idle_lock_minutes, Msg::Settings(SetMsg::IdleLock(*m)))
            }),
            idle_open,
            Msg::Popup(if idle_open { None } else { Some(Popup::IdleLock) }),
            Msg::Popup(None),
        );
        let lock = group(vec![
            setting("Lock after", container(idle).width(Length::Fixed(160.0)).into()),
            setting(
                "Lock when the screen locks",
                toggler(s.lock_on_screen_lock)
                    .on_toggle(|b| Msg::Settings(SetMsg::LockOnScreen(b)))
                    .style(toggle_style)
                    .into(),
            ),
        ]);

        let clear_label = CLEAR
            .iter()
            .find(|(n, _)| *n == s.clipboard_clear_secs)
            .map_or_else(|| format!("{} seconds", s.clipboard_clear_secs), |(_, l)| (*l).to_string());
        let clear_open = self.popup == Some(Popup::ClipClear);
        let clear = select(
            clear_label,
            CLEAR.iter().map(|(n, l)| {
                SelectOption::new(*l, *n == s.clipboard_clear_secs, Msg::Settings(SetMsg::ClipClear(*n)))
            }),
            clear_open,
            Msg::Popup(if clear_open { None } else { Some(Popup::ClipClear) }),
            Msg::Popup(None),
        );
        let clipboard = group(vec![
            setting(
                "Clear a copied value after",
                container(clear).width(Length::Fixed(160.0)).into(),
            ),
            setting(
                "Filling a login copies its code",
                toggler(s.copy_code_on_fill)
                    .on_toggle(|b| Msg::Settings(SetMsg::CopyCode(b)))
                    .style(toggle_style)
                    .into(),
            ),
        ]);

        let body = column![
            section("Account", account, None),
            section("Lock", lock, Some("The vault also locks when the vault service restarts.")),
            section(
                "Copying and filling",
                clipboard,
                Some("A value leaves the clipboard only if you haven't copied something since."),
            ),
            row![
                Space::new().width(Length::Fill),
                kit_btn::labeled("Done", kit_btn::primary).on_press(Msg::CloseSheet),
            ],
        ]
        .spacing(SPACE_LG);
        sheet_sized(
            head(
                None,
                Some(widgets::glyph_plate("lucide/settings", PlateSize::Small)),
                "Vault Settings".into(),
                String::new(),
            ),
            body.into(),
            460.0,
        )
    }
}

fn section<'a>(title: &'a str, body: Element<'a, Msg>, caption: Option<&'a str>) -> Element<'a, Msg> {
    let mut col = column![
        container(kit_text::caption(title).style(kit_text::muted)).padding(Padding::ZERO.left(SPACE_XS)),
        body,
    ]
    .spacing(SPACE_SM);
    if let Some(c) = caption {
        col = col.push(
            container(kit_text::caption(c).style(kit_text::muted))
                .padding(Padding::ZERO.left(SPACE_XS).right(SPACE_XS)),
        );
    }
    col.into()
}

fn setting<'a>(label: &'a str, control: Element<'a, Msg>) -> Element<'a, Msg> {
    container(
        row![text(label).font(fonts::ui()).size(13).width(Length::Fill), control]
            .spacing(SPACE_MD)
            .align_y(Alignment::Center),
    )
    .padding(Padding::from([5.0, 10.0]))
    .height(Length::Fixed(40.0))
    .width(Length::Fill)
    .into()
}

fn value<'a>(s: String) -> Element<'a, Msg> {
    kit_text::body(s).style(kit_text::muted).into()
}

fn custom_minutes(m: Option<u32>) -> String {
    match m {
        Some(1) => "1 minute".into(),
        Some(n) => format!("{n} minutes"),
        None => "Never".into(),
    }
}
