//! The generator: a password, a passphrase or a username, made by the
//! daemon from the options here. From a password field it fills that
//! field; from the menu it copies.

use iced::widget::{Space, column, container, row, slider, text, toggler};
use iced::{Alignment, Element, Length, Padding, Task, Theme};
use sola_kit::components::button as kit_btn;
use sola_kit::components::form::toggle_style;
use sola_kit::components::style::{SPACE_LG, SPACE_MD, SPACE_SM};
use sola_kit::components::text_input::text_input;
use sola_kit::fonts;
use sola_vault_proto::{GeneratorKind, Request, VaultError};
use zeroize::Zeroizing;

use super::Sheet;
use crate::model::generator::{LENGTH, WORDS, clamp, describe};
use crate::ui::ask::Ask;
use crate::ui::copy::CopyWhat;
use crate::ui::fields::Text;
use crate::ui::record::group;
use crate::ui::signin::{error_caption, head, sheet_sized};
use crate::ui::widgets::{self, PlateSize};
use crate::ui::{App, Msg, style};

pub struct GenSheet {
    pub value: Option<Text>,
    pub busy: bool,
    pub error: Option<String>,
    /// Where Use puts it; `None`: Copy.
    pub target: Option<GenTarget>,
}

/// A field of the item being edited.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GenTarget {
    Row(String),
}

#[derive(Debug, Clone)]
pub enum GenMsg {
    Kind(GeneratorKind),
    Length(u8),
    Words(u8),
    /// A slider was let go: make one with it.
    Settled,
    Lower(bool),
    Upper(bool),
    Numbers(bool),
    Special(bool),
    Ambiguous(bool),
    Capitalize(bool),
    IncludeNumber(bool),
    Separator(String),
    Again,
    Use,
}

impl App {
    pub(in crate::ui) fn open_generator(&mut self, target: Option<GenTarget>) -> Task<Msg> {
        self.popup = None;
        if target.is_some() && self.gen_options.kind == GeneratorKind::Username {
            self.gen_options.kind = GeneratorKind::Password;
        }
        self.sheet = Some(Sheet::Generator(GenSheet {
            value: None,
            busy: false,
            error: None,
            target,
        }));
        self.generate()
    }

    fn generate(&mut self) -> Task<Msg> {
        self.gen_options = clamp(self.gen_options.clone());
        if let Some(Sheet::Generator(g)) = self.sheet.as_mut() {
            g.busy = true;
            g.error = None;
        }
        let options = self.gen_options.clone();
        self.ask(Ask::Generate, Request::Generate { options })
    }

    pub(in crate::ui) fn on_generated(&mut self, result: Result<String, VaultError>) {
        if let Some(Sheet::Generator(g)) = self.sheet.as_mut() {
            g.busy = false;
            match result {
                Ok(v) => g.value = Some(Zeroizing::new(v)),
                Err(e) => g.error = Some(e.message),
            }
        }
    }

    pub(in crate::ui) fn generated_value(&self) -> Option<String> {
        match &self.sheet {
            Some(Sheet::Generator(g)) => g.value.as_ref().map(|v| v.to_string()),
            _ => None,
        }
    }

    pub(in crate::ui) fn on_generator(&mut self, m: GenMsg) -> Task<Msg> {
        let o = &mut self.gen_options;
        match m {
            GenMsg::Kind(k) => o.kind = k,
            GenMsg::Length(n) => {
                o.length = n;
                return Task::none();
            }
            GenMsg::Words(n) => {
                o.words = n;
                return Task::none();
            }
            GenMsg::Settled | GenMsg::Again => {}
            GenMsg::Lower(on) => o.lowercase = on,
            GenMsg::Upper(on) => o.uppercase = on,
            GenMsg::Numbers(on) => o.numbers = on,
            GenMsg::Special(on) => o.special = on,
            GenMsg::Ambiguous(on) => o.avoid_ambiguous = on,
            GenMsg::Capitalize(on) => o.capitalize = on,
            GenMsg::IncludeNumber(on) => o.include_number = on,
            GenMsg::Separator(s) => o.separator = s.chars().take(1).collect(),
            GenMsg::Use => {
                let target = match &self.sheet {
                    Some(Sheet::Generator(g)) => g.target.clone(),
                    _ => None,
                };
                if let (Some(target), Some(v)) = (target, self.generated_value()) {
                    self.sheet = None;
                    self.fill_generated(target, v);
                }
                return Task::none();
            }
        }
        self.generate()
    }

    pub(in crate::ui) fn view_generator<'a>(&'a self, g: &'a GenSheet) -> Element<'a, Msg> {
        let o = &self.gen_options;
        let kinds = row(
            [
                (GeneratorKind::Password, "Password"),
                (GeneratorKind::Passphrase, "Passphrase"),
                (GeneratorKind::Username, "Username"),
            ]
            .into_iter()
            .filter(|(k, _)| g.target.is_none() || *k != GeneratorKind::Username)
            .map(|(k, label)| {
                kit_btn::labeled_sm(label, if o.kind == k { kit_btn::secondary } else { kit_btn::ghost })
                    .on_press(Msg::Generator(GenMsg::Kind(k)))
                    .into()
            }),
        )
        .spacing(SPACE_SM);

        let value: Element<'a, Msg> = match (&g.value, &g.error) {
            (_, Some(e)) => error_caption(e),
            (Some(v), None) => text(v.as_str())
                .font(fonts::mono())
                .size(15)
                .width(Length::Fill)
                .into(),
            (None, None) => text("…").font(fonts::mono()).size(15).into(),
        };
        let result = container(
            row![
                container(value).width(Length::Fill),
                widgets::tool("lucide/refresh-cw", "Make another", Some(Msg::Generator(GenMsg::Again))),
                widgets::tool(
                    "lucide/copy",
                    "Copy",
                    g.value.as_ref().map(|_| Msg::Copy(CopyWhat::Generated)),
                ),
            ]
            .spacing(2)
            .align_y(Alignment::Center),
        )
        .padding(Padding {
            top: 10.0,
            right: 6.0,
            bottom: 10.0,
            left: 12.0,
        })
        .style(|t: &Theme| {
            let s = sola_kit::components::text_input::style(
                t,
                sola_kit::components::text_input::Status::Active,
            );
            container::Style {
                background: Some(s.background),
                border: s.border,
                ..container::Style::default()
            }
        });

        let options: Element<'a, Msg> = match o.kind {
            GeneratorKind::Password => group(vec![
                setting(
                    "Length",
                    stepper(
                        slider(LENGTH.0..=LENGTH.1, o.length, |n| Msg::Generator(GenMsg::Length(n)))
                            .on_release(Msg::Generator(GenMsg::Settled)),
                        o.length.to_string(),
                    ),
                ),
                switch("Capitals A–Z", o.uppercase, |b| GenMsg::Upper(b)),
                switch("Lower case a–z", o.lowercase, |b| GenMsg::Lower(b)),
                switch("Digits 0–9", o.numbers, |b| GenMsg::Numbers(b)),
                switch("Symbols !@#$%^&*", o.special, |b| GenMsg::Special(b)),
                switch("Avoid look-alikes (l, 1, O, 0)", o.avoid_ambiguous, |b| {
                    GenMsg::Ambiguous(b)
                }),
            ]),
            GeneratorKind::Passphrase => group(vec![
                setting(
                    "Words",
                    stepper(
                        slider(WORDS.0..=WORDS.1, o.words, |n| Msg::Generator(GenMsg::Words(n)))
                            .on_release(Msg::Generator(GenMsg::Settled)),
                        o.words.to_string(),
                    ),
                ),
                setting(
                    "Between words",
                    text_input("", &o.separator)
                        .on_input(|s| Msg::Generator(GenMsg::Separator(s)))
                        .font(fonts::mono())
                        .width(Length::Fixed(56.0))
                        .into(),
                ),
                switch("Capitalize", o.capitalize, |b| GenMsg::Capitalize(b)),
                switch("Include a number", o.include_number, |b| GenMsg::IncludeNumber(b)),
            ]),
            GeneratorKind::Username => Space::new().into(),
        };

        let primary = match &g.target {
            Some(_) => kit_btn::labeled("Use It", kit_btn::primary)
                .on_press_maybe(g.value.as_ref().map(|_| Msg::Generator(GenMsg::Use))),
            None => kit_btn::labeled("Copy", kit_btn::primary)
                .on_press_maybe(g.value.as_ref().map(|_| Msg::Copy(CopyWhat::Generated))),
        };
        let body = column![
            kinds,
            result,
            options,
            row![
                Space::new().width(Length::Fill),
                kit_btn::labeled("Close", kit_btn::ghost).on_press(Msg::CloseSheet),
                primary,
            ]
            .spacing(SPACE_SM)
            .align_y(Alignment::Center),
        ]
        .spacing(SPACE_LG);
        sheet_sized(
            head(
                None,
                widgets::glyph_plate("lucide/dices", PlateSize::Small),
                "Generator".into(),
                describe(o),
            ),
            body.into(),
            420.0,
        )
    }
}

fn setting<'a>(label: &'a str, control: Element<'a, Msg>) -> Element<'a, Msg> {
    container(
        row![
            text(label).font(fonts::ui()).size(13).width(Length::Fill),
            control
        ]
        .spacing(SPACE_MD)
        .align_y(Alignment::Center),
    )
    .padding(Padding::from([5.0, 10.0]))
    .width(Length::Fill)
    .into()
}

fn switch<'a>(label: &'a str, on: bool, msg: fn(bool) -> GenMsg) -> Element<'a, Msg> {
    setting(
        label,
        toggler(on)
            .on_toggle(move |b| Msg::Generator(msg(b)))
            .style(toggle_style)
            .into(),
    )
}

fn stepper<'a>(slider: impl Into<Element<'a, Msg>>, value: String) -> Element<'a, Msg> {
    row![
        container(slider.into()).width(Length::Fixed(150.0)),
        text(value)
            .width(Length::Fixed(28.0))
            .style(|t: &Theme| text::Style {
                color: Some(style::ink2(t)),
            }),
    ]
    .spacing(SPACE_SM)
    .align_y(Alignment::Center)
    .into()
}
