//! `window.alert` / `confirm` / `prompt` (and leave-page confirm).
//!
//! CEF OSR has no Chromium JS-dialog chrome. The helper's CEF
//! `JsDialogHandler` captures them; iced draws the same graphite modal as
//! notification / media permission and completes the CEF callback.

use std::sync::atomic::{AtomicBool, Ordering};

use iced::widget::{Space, column, container, mouse_area, row, scrollable, stack, text};
use iced::{Alignment, Element, Length};

use sola_kit::components::button as kit_button;
use sola_kit::components::card;
use sola_kit::components::style::{SPACE_MD, SPACE_SM};
use sola_kit::components::text_input::text_input;
use sola_kit::fonts;

use crate::notify;

/// True while a JS dialog overlay is on screen. The OSR shader checks this
/// so page keys do not reach CEF under the modal. Kind flags are for the
/// chrome `listen_with` fn-pointer (cannot close over App).
static OPEN: AtomicBool = AtomicBool::new(false);
static ALERT: AtomicBool = AtomicBool::new(false);
static PROMPT: AtomicBool = AtomicBool::new(false);

pub fn is_open() -> bool {
    OPEN.load(Ordering::Relaxed)
}

pub fn is_alert() -> bool {
    ALERT.load(Ordering::Relaxed)
}

pub fn is_prompt() -> bool {
    PROMPT.load(Ordering::Relaxed)
}

pub fn set_open(open: bool) {
    if !open {
        set_kind(None);
    }
}

pub fn set_kind(kind: Option<Kind>) {
    OPEN.store(kind.is_some(), Ordering::Relaxed);
    ALERT.store(matches!(kind, Some(Kind::Alert)), Ordering::Relaxed);
    PROMPT.store(matches!(kind, Some(Kind::Prompt)), Ordering::Relaxed);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Kind {
    Alert,
    Confirm,
    Prompt,
    BeforeUnload,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Ipc {
    pub id: u64,
    pub tab_id: u64,
    pub origin: String,
    pub kind: Kind,
    pub message: String,
    pub default_prompt: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum Event {
    Open(Ipc),
    Reset { ids: Vec<u64> },
}

pub fn prompt_input_id() -> iced::advanced::widget::Id {
    iced::advanced::widget::Id::new("js-dialog-prompt")
}

pub fn host_title(origin: &str) -> String {
    let host = notify::host_of(origin);
    if host.is_empty() {
        "This page".into()
    } else {
        host
    }
}

pub fn ok_label(kind: Kind) -> &'static str {
    match kind {
        Kind::BeforeUnload => "Leave",
        Kind::Alert | Kind::Confirm | Kind::Prompt => "OK",
    }
}

pub fn cancel_label(kind: Kind) -> Option<&'static str> {
    match kind {
        Kind::Alert => None,
        Kind::BeforeUnload => Some("Stay"),
        Kind::Confirm | Kind::Prompt => Some("Cancel"),
    }
}

/// Escape / backdrop: only `alert` has a single action (OK).
pub fn dismiss_succeeds(kind: Kind) -> bool {
    matches!(kind, Kind::Alert)
}

pub fn overlay<'a, Message: Clone + 'a>(
    dlg: &'a Ipc,
    prompt_value: &'a str,
    on_ok: Message,
    on_cancel: Message,
    on_input: impl Fn(String) -> Message + 'a,
) -> Element<'a, Message> {
    let title = text(host_title(&dlg.origin))
        .size(15)
        .font(fonts::ui_medium());
    let mut col = column![title].spacing(SPACE_SM).width(Length::Fixed(300.0));
    let message = dlg.message.trim();
    if !message.is_empty() {
        let body = text(message.to_string()).size(13).width(Length::Fill);
        col = col.push(
            container(scrollable(body))
                .width(Length::Fill)
                .max_height(220.0),
        );
    }
    if dlg.kind == Kind::Prompt {
        col = col.push(
            text_input("", prompt_value)
                .id(prompt_input_id())
                .size(13)
                .style(sola_kit::components::text_input::style)
                .width(Length::Fill)
                .on_input(on_input)
                .on_submit(on_ok.clone()),
        );
    }
    let mut actions = row![].spacing(SPACE_SM).align_y(Alignment::Center);
    actions = actions
        .push(kit_button::labeled(ok_label(dlg.kind), kit_button::primary).on_press(on_ok.clone()));
    if let Some(cancel) = cancel_label(dlg.kind) {
        actions = actions
            .push(kit_button::labeled(cancel, kit_button::ghost).on_press(on_cancel.clone()));
    }
    col = col.push(actions);
    let panel =
        card::modal(container(col).padding(SPACE_MD + SPACE_SM)).width(Length::Fixed(340.0));
    let backdrop_msg = if dismiss_succeeds(dlg.kind) {
        on_ok
    } else {
        on_cancel
    };
    let backdrop = mouse_area(
        container(Space::new().width(Length::Fill).height(Length::Fill)).style(|_t| {
            container::Style {
                background: Some(iced::Background::Color(iced::Color::from_rgba(
                    0.0, 0.0, 0.0, 0.22,
                ))),
                ..container::Style::default()
            }
        }),
    )
    .on_press(backdrop_msg);
    let centered = container(panel)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Alignment::Center)
        .align_y(Alignment::Center);
    stack![backdrop, centered].into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alert_dismiss_is_ok() {
        assert!(dismiss_succeeds(Kind::Alert));
        assert!(!dismiss_succeeds(Kind::Prompt));
        assert!(!dismiss_succeeds(Kind::Confirm));
        assert!(!dismiss_succeeds(Kind::BeforeUnload));
        assert_eq!(cancel_label(Kind::Alert), None);
        assert_eq!(ok_label(Kind::BeforeUnload), "Leave");
        assert_eq!(cancel_label(Kind::BeforeUnload), Some("Stay"));
    }

    #[test]
    fn host_falls_back() {
        assert_eq!(host_title("https://example.com/path"), "example.com");
        assert_eq!(host_title(""), "This page");
    }
}
