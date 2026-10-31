//! Before the vault is open: the daemon away, signing in, the second
//! step, unlocking — and an item's master-password re-prompt. One thing
//! asked at a time; the remembered account is the head.

use iced::widget::{Space, button, column, container, row, stack, text};
use iced::{Alignment, Background, Border, Color, Element, Length, Padding, Theme};
use sola_kit::components::button as kit_button;
use sola_kit::components::style::{RADIUS_MD, RADIUS_SM};
use sola_kit::components::text_input::text_input;
use sola_vault_proto::TwoFactorKind;

use super::super::ServerPick;
use super::super::update::VaultMsg;
use super::parts::{self, icon, icon_button, ink, ink2, label, msg};
use super::{Head, Sheet, body, id};
use crate::app::{App, Msg};
use crate::engine::Engine;

impl<E: Engine> App<E> {
    pub(super) fn sheet_away(&self) -> Sheet<'_> {
        let retry = kit_button::labeled_sm("Try again", kit_button::secondary)
            .on_press(msg(VaultMsg::Retry));
        Sheet {
            head: Head::new("Vault")
                .plate(parts::kind_plate("lock", 22.0))
                .sub("Not running"),
            body: body(
                parts::empty(
                    "circle-alert",
                    "Vault isn't running",
                    "The browser fills from Sola's vault service. When it is back, try again.",
                    Some(retry.into()),
                ),
                false,
            ),
            foot: None,
        }
    }

    pub(super) fn sheet_connecting(&self) -> Sheet<'_> {
        Sheet {
            head: Head::new("Vault").plate(parts::kind_plate("lock", 22.0)),
            body: body(label("Asking the vault…", 12.0, ink2), true),
            foot: None,
        }
    }

    /// A site waiting on the vault says so above the form.
    fn passkey_lead(&self) -> Option<Element<'_, Msg>> {
        let p = self.vault.passkey.as_ref()?;
        let host = crate::vault::log_host(&p.req.origin);
        let line = if p.is_create() {
            format!("{host} wants to save a passkey. Unlock to continue.")
        } else {
            format!("{host} asks for a passkey. Unlock to choose one.")
        };
        Some(label(line, 12.5, ink2).wrapping(text::Wrapping::Word).into())
    }

    pub(super) fn sheet_sign_in(&self) -> Sheet<'_> {
        let v = &self.vault;
        let busy = v.busy;
        let mut col = column![].spacing(12).width(Length::Fill);
        if let Some(lead) = self.passkey_lead() {
            col = col.push(lead);
        }
        let email = text_input("Email", &v.forms.email)
            .id(id("email"))
            .size(13)
            .style(sola_kit::components::text_input::style)
            .on_input_maybe((!busy).then_some(|s| msg(VaultMsg::Email(s))))
            .on_submit(msg(VaultMsg::SignIn));
        col = col.push(email);
        col = col.push(self.master_password("password", &v.forms.password, VaultMsg::Password, VaultMsg::SignIn));
        let mut servers = row![label("Server", 11.0, ink2), Space::new().width(Length::Fill)]
            .spacing(2)
            .align_y(Alignment::Center);
        for (pick, word) in [
            (ServerPick::Us, "bitwarden.com"),
            (ServerPick::Eu, "bitwarden.eu"),
            (ServerPick::SelfHosted, "Self-hosted"),
        ] {
            let on = v.forms.server == pick;
            servers = servers.push(
                button(label(word, 12.0, if on { ink } else { ink2 }))
                    .padding(Padding::from([3, 8]))
                    .style(move |t: &Theme, s| tab_style(t, s, on))
                    .on_press_maybe((!busy).then_some(msg(VaultMsg::Server(pick)))),
            );
        }
        col = col.push(servers);
        if v.forms.server == ServerPick::SelfHosted {
            col = col.push(
                text_input("https://vault.example.com", &v.forms.server_url)
                    .id(id("server-url"))
                    .size(13)
                    .style(sola_kit::components::text_input::style)
                    .on_input_maybe((!busy).then_some(|s| msg(VaultMsg::ServerUrl(s))))
                    .on_submit(msg(VaultMsg::SignIn)),
            );
        }
        if let Some(e) = v.error.as_ref() {
            col = col.push(parts::error_line(e));
        }
        col = col.push(primary(if busy { "Signing in…" } else { "Sign in" }, (!busy).then_some(VaultMsg::SignIn)));
        Sheet {
            head: Head::new("Bitwarden")
                .plate(parts::kind_plate("lock", 22.0))
                .sub("Sign in to your vault"),
            body: body(col, true),
            foot: None,
        }
    }

    pub(super) fn sheet_two_step(&self) -> Sheet<'_> {
        let v = &self.vault;
        let busy = v.busy;
        let Some(step) = v.two_step.as_ref() else {
            return self.sheet_sign_in();
        };
        let hint = step
            .email_hint
            .clone()
            .filter(|h| !h.is_empty())
            .unwrap_or_else(|| "your email".into());
        let (title, lead) = match step.kind {
            TwoFactorKind::Authenticator => (
                "Two-step login",
                "Enter the 6-digit code from your authenticator app.".to_string(),
            ),
            TwoFactorKind::NewDevice => (
                "Check your email",
                format!("This browser is new to Bitwarden. It sent a code to {hint}."),
            ),
            TwoFactorKind::Email => ("Check your email", format!("Bitwarden sent a code to {hint}.")),
        };
        let mut col = column![label(lead, 12.5, ink2).wrapping(text::Wrapping::Word)]
            .spacing(12)
            .width(Length::Fill);
        if step.kind == TwoFactorKind::Authenticator {
            col = col.push(six_boxes(&v.forms.otp, busy));
        } else {
            col = col.push(
                text_input("Code", &v.forms.otp)
                    .id(id("otp"))
                    .size(14)
                    .font(sola_kit::fonts::mono())
                    .style(sola_kit::components::text_input::style)
                    .on_input_maybe((!busy).then_some(|s| msg(VaultMsg::Otp(s))))
                    .on_submit(msg(VaultMsg::Verify)),
            );
        }
        if let Some(e) = v.error.as_ref() {
            col = col.push(parts::error_line(e));
        }
        col = col.push(primary(if busy { "Verifying…" } else { "Verify" }, (!busy).then_some(VaultMsg::Verify)));
        if step.kind == TwoFactorKind::Authenticator && step.kinds.contains(&TwoFactorKind::Email) {
            col = col.push(parts::link("No app with you?", "Email a code instead", VaultMsg::UseEmailCode));
        } else if step.kind != TwoFactorKind::Authenticator {
            col = col.push(parts::link("Nothing came?", "Send it again", VaultMsg::SendCode));
        }
        if !step.unsupported.is_empty() {
            col = col.push(
                label(
                    format!(
                        "This account also has {}, which Sola can't use yet.",
                        step.unsupported.join(", ")
                    ),
                    11.0,
                    parts::ink3,
                )
                .wrapping(text::Wrapping::Word),
            );
        }
        Sheet {
            head: Head::new(title)
                .back(VaultMsg::CancelSignIn)
                .sub(v.forms.email.clone()),
            body: body(col, true),
            foot: None,
        }
    }

    pub(super) fn sheet_unlock(&self) -> Sheet<'_> {
        let v = &self.vault;
        let busy = v.busy;
        let email = v.status.as_ref().map(|s| s.email.clone()).unwrap_or_default();
        let mut col = column![].spacing(12).width(Length::Fill);
        if let Some(lead) = self.passkey_lead() {
            col = col.push(lead);
        }
        let mut field = column![self.master_password("unlock", &v.forms.unlock, VaultMsg::UnlockPassword, VaultMsg::Unlock)].spacing(5);
        if let Some(e) = v.error.as_ref() {
            field = field.push(parts::error_line(e));
        }
        col = col
            .push(field)
            .push(primary(if busy { "Unlocking…" } else { "Unlock" }, (!busy).then_some(VaultMsg::Unlock)))
            .push(parts::link("Not you?", "Use another account", VaultMsg::OtherAccount));
        Sheet {
            head: Head::new("Bitwarden")
                .plate(parts::kind_plate("lock", 22.0))
                .sub(email),
            body: body(col, true),
            foot: None,
        }
    }

    pub(super) fn sheet_reprompt(&self) -> Sheet<'_> {
        let v = &self.vault;
        let busy = v.busy;
        let item = v.reprompt.as_ref().map(|r| r.item().to_string()).unwrap_or_default();
        let (name, kind) = v
            .list
            .items
            .iter()
            .find(|i| i.id == item)
            .map(|i| (i.name.clone(), i.kind))
            .or_else(|| {
                v.passkey.as_ref().and_then(|p| {
                    p.candidates
                        .iter()
                        .find(|c| c.cipher_id == item)
                        .map(|c| (c.name.clone(), sola_vault_proto::ItemKind::Login))
                })
            })
            .unwrap_or_else(|| ("This item".into(), sola_vault_proto::ItemKind::Login));
        let field = text_input("Master password", &v.reprompt_password)
            .id(id("reprompt"))
            .secure(true)
            .size(13)
            .style(sola_kit::components::text_input::style)
            .on_input_maybe((!busy).then_some(|s| msg(VaultMsg::RepromptPassword(s))))
            .on_submit(msg(VaultMsg::RepromptSubmit));
        let mut col = column![
            container(
                column![
                    icon("shield", 22, parts::ink3),
                    label(
                        "You set this item to ask for your master password before it opens or fills.",
                        12.0,
                        ink2
                    )
                    .wrapping(text::Wrapping::Word)
                    .align_x(Alignment::Center)
                ]
                .spacing(8)
                .align_x(Alignment::Center)
            )
            .padding(Padding::from([12, 8]))
            .center_x(Length::Fill),
            field,
        ]
        .spacing(12)
        .width(Length::Fill);
        if let Some(e) = v.error.as_ref() {
            col = col.push(parts::error_line(e));
        }
        let cancel = kit_button::labeled("Cancel", kit_button::ghost).on_press(msg(VaultMsg::RepromptCancel));
        let go = iced::widget::button(
            container(text(if busy { "Checking…" } else { "Continue" }).size(13).font(sola_kit::fonts::ui()))
                .center_x(Length::Fill),
        )
        .padding(sola_kit::components::style::PAD_CONTROL)
        .width(Length::Fill)
        .style(kit_button::primary)
        .on_press_maybe((!busy).then_some(msg(VaultMsg::RepromptSubmit)));
        col = col.push(row![cancel, go].spacing(8).align_y(Alignment::Center));
        Sheet {
            head: Head::new(name.clone())
                .back(VaultMsg::RepromptCancel)
                .plate(parts::plate(&name, kind, 22.0))
                .sub("asks for your master password"),
            body: body(col, true),
            foot: None,
        }
    }

    /// The master password: a secure field with its eye.
    fn master_password<'a>(
        &'a self,
        name: &str,
        value: &'a str,
        on_input: fn(String) -> VaultMsg,
        submit: VaultMsg,
    ) -> Element<'a, Msg> {
        let busy = self.vault.busy;
        let shown = self.vault.forms.reveal;
        let field = text_input("Master password", value)
            .id(id(name))
            .secure(!shown)
            .size(13)
            .style(sola_kit::components::text_input::style)
            .width(Length::Fill)
            .on_input_maybe((!busy).then_some(move |s| msg(on_input(s))))
            .on_submit(msg(submit));
        row![
            field,
            icon_button(if shown { "eye-off" } else { "eye" }, 14, Some(VaultMsg::RevealPassword))
        ]
        .spacing(4)
        .align_y(Alignment::Center)
        .into()
    }
}

/// The sheet's one primary, as wide as the form.
fn primary<'a>(word: &'a str, on: Option<VaultMsg>) -> Element<'a, Msg> {
    iced::widget::button(container(text(word).size(13).font(sola_kit::fonts::ui())).center_x(Length::Fill))
        .padding(sola_kit::components::style::PAD_CONTROL)
        .width(Length::Fill)
        .style(kit_button::primary)
        .on_press_maybe(on.map(msg))
        .into()
}

/// Six boxes for an authenticator code over one field that takes the
/// keys (and a paste of the whole code).
fn six_boxes<'a>(otp: &'a str, busy: bool) -> Element<'a, Msg> {
    let digits: Vec<char> = otp.chars().collect();
    let mut boxes = row![].spacing(6);
    for i in 0..6 {
        let ch = digits.get(i).map(|c| c.to_string()).unwrap_or_default();
        let caret = i == digits.len().min(5);
        boxes = boxes.push(
            container(text(ch).size(20).font(sola_kit::fonts::mono()))
                .center_x(Length::Fixed(40.0))
                .center_y(Length::Fixed(46.0))
                .style(move |t: &Theme| {
                    let base = sola_kit::components::text_input::style(
                        t,
                        if caret {
                            sola_kit::components::text_input::Status::Focused { is_hovered: false }
                        } else {
                            sola_kit::components::text_input::Status::Active
                        },
                    );
                    container::Style {
                        background: Some(base.background),
                        border: Border {
                            radius: RADIUS_MD.into(),
                            ..base.border
                        },
                        ..container::Style::default()
                    }
                }),
        );
    }
    // The field that takes the keys: on top, drawn as nothing.
    let field = text_input("", otp)
        .id(id("otp"))
        .size(20)
        .padding(Padding::from([12, 10]))
        .width(Length::Fixed(6.0 * 40.0 + 5.0 * 6.0))
        .style(|_t: &Theme, _s| sola_kit::components::text_input::Style {
            background: Background::Color(Color::TRANSPARENT),
            border: Border::default(),
            icon: Color::TRANSPARENT,
            placeholder: Color::TRANSPARENT,
            value: Color::TRANSPARENT,
            selection: Color::TRANSPARENT,
        })
        .on_input_maybe((!busy).then_some(|s| msg(VaultMsg::Otp(s))))
        .on_submit(msg(VaultMsg::Verify));
    container(stack![boxes, field]).center_x(Length::Fill).into()
}

fn tab_style(t: &Theme, status: button::Status, on: bool) -> button::Style {
    let bg = if on {
        Some(sola_kit::theme::selection())
    } else if matches!(status, button::Status::Hovered) {
        Some(sola_kit::components::style::mix_white(parts::face(t), 0.05))
    } else {
        None
    };
    button::Style {
        background: bg.map(Background::Color),
        text_color: ink(t),
        border: Border {
            radius: RADIUS_SM.into(),
            ..Border::default()
        },
        ..button::Style::default()
    }
}
