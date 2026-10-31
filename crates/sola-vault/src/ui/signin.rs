//! Before the vault opens: signing in (email, master password, server),
//! the second step (six boxes for an authenticator, one field for an
//! emailed code), unlocking a remembered account, and a calm card while
//! the service is away. One card in the window's middle, as the canvas's
//! sheet (`design/vault/signin.html`).

use iced::widget::{Space, button, column, container, row, stack, text};
use iced::{Alignment, Element, Length, Padding, Task, Theme};
use sola_kit::components::button as kit_btn;
use sola_kit::components::select::{SelectOption, select};
use sola_kit::components::style::{PAD_CONTROL, SPACE_LG, SPACE_SM, SPACE_XS};
use sola_kit::components::text as kit_text;
use sola_kit::components::text_input::{self as kit_input, text_input};
use sola_kit::fonts;
use sola_vault_proto::{ErrorCode, LoginOutcome, Request, Server, TwoFactorKind, VaultError};
use zeroize::Zeroizing;

use super::ask::Ask;
use super::fields::{Field, Text};
use super::widgets::{self, PlateSize};
use super::{App, Msg, Phase, Popup, style};
use crate::model::otp;

/// The card's width.
const CARD_W: f32 = 360.0;
/// A code box.
const BOX_W: f32 = 40.0;
const BOX_H: f32 = 46.0;
const BOX_GAP: f32 = 6.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ServerChoice {
    #[default]
    Us,
    Eu,
    SelfHosted,
}

impl ServerChoice {
    fn label(self) -> &'static str {
        match self {
            ServerChoice::Us => "bitwarden.com",
            ServerChoice::Eu => "bitwarden.eu",
            ServerChoice::SelfHosted => "Self-hosted",
        }
    }

    fn of(server: &Server) -> Self {
        match server {
            Server::Us => ServerChoice::Us,
            Server::Eu => ServerChoice::Eu,
            Server::SelfHosted { .. } => ServerChoice::SelfHosted,
        }
    }
}

/// Which field a sign-in error goes under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Under {
    Email,
    Password,
    Server,
    Form,
}

#[derive(Default)]
pub struct SignIn {
    pub email: String,
    pub password: Text,
    pub server: ServerChoice,
    pub url: String,
    pub reveal: bool,
    pub busy: bool,
    pub error: Option<(Under, String)>,
    pub two_step: Option<TwoStep>,
}

impl SignIn {
    pub fn clear_secrets(&mut self) {
        self.password = Text::default();
        self.two_step = None;
        self.busy = false;
        self.error = None;
    }

    fn error_under(&self, under: Under) -> Option<&str> {
        self.error
            .as_ref()
            .filter(|(u, _)| *u == under)
            .map(|(_, m)| m.as_str())
    }

    fn server(&self) -> Server {
        match self.server {
            ServerChoice::Us => Server::Us,
            ServerChoice::Eu => Server::Eu,
            ServerChoice::SelfHosted => Server::SelfHosted {
                url: self.url.trim().to_string(),
            },
        }
    }
}

pub struct TwoStep {
    pub kinds: Vec<TwoFactorKind>,
    pub kind: TwoFactorKind,
    pub email_hint: Option<String>,
    pub email_sent: bool,
    pub unsupported: Vec<String>,
    pub code: Text,
    pub busy: bool,
    pub error: Option<String>,
    /// "Sent." after Send it again.
    pub notice: Option<String>,
}

#[derive(Default)]
pub struct Unlock {
    pub password: Text,
    pub reveal: bool,
    pub busy: bool,
    pub error: Option<String>,
}

impl App {
    // ── Actions ──────────────────────────────────────────────────────

    pub(super) fn sign_in(&mut self) -> Task<Msg> {
        let s = &mut self.signin;
        if s.busy {
            return Task::none();
        }
        let email = s.email.trim().to_string();
        let problem = if email.is_empty() {
            Some((Under::Email, "Enter your email address."))
        } else if s.password.is_empty() {
            Some((Under::Password, "Enter your master password."))
        } else {
            s.server().address_problem().map(|why| (Under::Server, why))
        };
        if let Some((under, message)) = problem {
            s.error = Some((under, message.into()));
            return Task::none();
        }
        s.busy = true;
        s.error = None;
        let req = Request::Login {
            email,
            password: s.password.to_string(),
            server: s.server(),
        };
        self.ask(Ask::Login, req)
    }

    pub(super) fn on_login(&mut self, outcome: Result<LoginOutcome, VaultError>) -> Task<Msg> {
        self.signin.busy = false;
        if let Some(step) = self.signin.two_step.as_mut() {
            step.busy = false;
        }
        match outcome {
            Ok(LoginOutcome::Unlocked) => {
                self.signin.clear_secrets();
                self.ask(Ask::Status, Request::Status)
            }
            Ok(LoginOutcome::NeedsTwoFactor {
                kinds,
                preferred,
                email_hint,
                email_sent,
                unsupported,
            }) => {
                let kind = if kinds.contains(&preferred) {
                    preferred
                } else {
                    kinds.first().copied().unwrap_or(preferred)
                };
                self.signin.two_step = Some(TwoStep {
                    kinds,
                    kind,
                    email_hint,
                    email_sent,
                    unsupported,
                    code: Text::default(),
                    busy: false,
                    error: None,
                    notice: None,
                });
                first_code_field(kind).focus()
            }
            Err(e) if self.signin.two_step.is_some() => {
                let step = self.signin.two_step.as_mut().expect("checked");
                step.error = Some(match e.code {
                    ErrorCode::BadPassword | ErrorCode::Invalid => {
                        "That code didn't work. Try the newest one.".into()
                    }
                    _ => e.message,
                });
                step.code = Text::default();
                first_code_field(step.kind).focus()
            }
            Err(e) => {
                let under = match e.code {
                    ErrorCode::BadPassword => Under::Password,
                    ErrorCode::Network => Under::Server,
                    _ => Under::Form,
                };
                self.signin.error = Some((under, e.message));
                Task::none()
            }
        }
    }

    pub(super) fn verify_two_step(&mut self) -> Task<Msg> {
        let Some(step) = self.signin.two_step.as_mut() else {
            return Task::none();
        };
        if step.busy {
            return Task::none();
        }
        let code = match step.kind {
            TwoFactorKind::Authenticator => {
                Some(step.code.to_string()).filter(|c| otp::complete(c))
            }
            _ => Some(step.code.trim().to_string()).filter(|c| !c.is_empty()),
        };
        let Some(code) = code else {
            step.error = Some("Enter the whole code.".into());
            return Task::none();
        };
        step.busy = true;
        step.error = None;
        let kind = step.kind;
        self.ask(Ask::TwoFactor, Request::TwoFactor { kind, code })
    }

    pub(super) fn use_kind(&mut self, kind: TwoFactorKind) -> Task<Msg> {
        let Some(step) = self.signin.two_step.as_mut() else {
            return Task::none();
        };
        step.kind = kind;
        step.code = Text::default();
        step.error = None;
        step.notice = None;
        let focus = first_code_field(kind).focus();
        if kind == TwoFactorKind::Email && !step.email_sent {
            return Task::batch([self.send_code(), focus]);
        }
        focus
    }

    pub(super) fn send_code(&mut self) -> Task<Msg> {
        let Some(step) = self.signin.two_step.as_mut() else {
            return Task::none();
        };
        step.notice = None;
        let kind = step.kind;
        self.ask(Ask::SendCode, Request::SendEmailCode { kind })
    }

    pub(super) fn on_code_sent(&mut self, result: Result<(), VaultError>) {
        if let Some(step) = self.signin.two_step.as_mut() {
            match result {
                Ok(()) => {
                    step.email_sent = true;
                    step.notice = Some(match &step.email_hint {
                        Some(hint) => format!("Sent to {hint}."),
                        None => "Sent. Check your email.".into(),
                    });
                }
                Err(e) => step.error = Some(e.message),
            }
        }
    }

    pub(super) fn do_unlock(&mut self) -> Task<Msg> {
        if self.unlock.busy {
            return Task::none();
        }
        if self.unlock.password.is_empty() {
            self.unlock.error = Some("Enter your master password.".into());
            return Task::none();
        }
        self.unlock.busy = true;
        self.unlock.error = None;
        let password = self.unlock.password.to_string();
        self.ask(Ask::Unlock, Request::Unlock { password })
    }

    pub(super) fn on_unlocked(&mut self, result: Result<(), VaultError>) -> Task<Msg> {
        self.unlock.busy = false;
        match result {
            Ok(()) => {
                self.unlock.password = Text::default();
                self.ask(Ask::Status, Request::Status)
            }
            Err(e) => {
                self.unlock.error = Some(match e.code {
                    ErrorCode::BadPassword => "That password didn't unlock this account.".into(),
                    _ => e.message,
                });
                self.unlock.password = Text::default();
                Field::Unlock.focus()
            }
        }
    }

    // ── Views ────────────────────────────────────────────────────────

    pub(super) fn view_gate(&self, phase: Phase) -> Element<'_, Msg> {
        let card = match phase {
            Phase::Connecting => return Space::new().into(),
            Phase::Unavailable => self.view_away(),
            Phase::Locked => self.view_unlock(),
            Phase::SignedOut => match &self.signin.two_step {
                Some(step) => self.view_two_step(step),
                None => self.view_sign_in(),
            },
            Phase::Unlocked => return Space::new().into(),
        };
        container(card)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into()
    }

    fn view_away(&self) -> Element<'_, Msg> {
        if let Some(why) = &self.refused {
            return sheet(
                head(
                    None,
                    Some(widgets::glyph_plate("lucide/lock", PlateSize::Small)),
                    "The vault service didn't let Vault in".into(),
                    "It lets in Sola's own programs only.".into(),
                ),
                lead_text(why.clone()),
            );
        }
        sheet(
            head(
                None,
                Some(widgets::glyph_plate("lucide/lock", PlateSize::Small)),
                "The vault service isn't running".into(),
                "Vault connects by itself once it starts.".into(),
            ),
            column![
                text("Your vault stays on the service, not in this window. Nothing is lost while it is away.")
                    .font(fonts::ui())
                    .size(12.5)
                    .style(|t: &Theme| text::Style { color: Some(style::ink2(t)) })
            ]
            .into(),
        )
    }

    fn view_sign_in(&self) -> Element<'_, Msg> {
        let s = &self.signin;
        let email = labeled_input(
            text_input("Email address", &s.email).size(13)
                .id(Field::Email.id())
                .on_input(|v| Msg::Input(Field::Email, Zeroizing::new(v)))
                .on_submit(Msg::Submit(Field::Email))
                .style(field_style(s.error_under(Under::Email).is_some())),
            s.error_under(Under::Email),
        );
        let password = labeled_input(
            secret_input(
                Field::Password,
                "Master password",
                &s.password,
                s.reveal,
                s.error_under(Under::Password).is_some(),
                Msg::RevealForm,
            ),
            s.error_under(Under::Password),
        );
        let options = [ServerChoice::Us, ServerChoice::Eu, ServerChoice::SelfHosted]
            .into_iter()
            .map(|c| SelectOption::new(c.label(), s.server == c, Msg::Server(c)));
        let open = self.popup == Some(Popup::Server);
        let server_select = select(
            s.server.label(),
            options,
            open,
            Msg::Popup(if open { None } else { Some(Popup::Server) }),
            Msg::Popup(None),
        );
        let mut server = column![
            row![
                kit_text::caption("Server").style(kit_text::muted).width(Length::Fill),
                container(server_select).width(Length::Fixed(160.0)),
            ]
            .align_y(Alignment::Center)
        ]
        .spacing(SPACE_SM);
        if s.server == ServerChoice::SelfHosted {
            server = server.push(
                text_input("https://vault.example.com", &s.url).size(13)
                    .id(Field::ServerUrl.id())
                    .on_input(|v| Msg::Input(Field::ServerUrl, Zeroizing::new(v)))
                    .on_submit(Msg::Submit(Field::ServerUrl))
                    .style(field_style(s.error_under(Under::Server).is_some())),
            );
        }
        if let Some(e) = s.error_under(Under::Server) {
            server = server.push(error_caption(e));
        }
        let mut body = column![email, password, server, primary(
            if s.busy { "Signing in…" } else { "Sign in" },
            (!s.busy).then_some(Msg::SignIn),
        )]
        .spacing(SPACE_LG);
        if let Some(e) = s.error_under(Under::Form) {
            body = body.push(error_caption(e));
        }
        body = body.push(
            text("Your account's settings, two-step setup and emergency access live on the web vault.")
                .font(fonts::ui())
                .size(11)
                .style(|t: &Theme| text::Style { color: Some(style::ink3(t)) }),
        );
        sheet(
            head(
                None,
                Some(widgets::glyph_plate("lucide/key-round", PlateSize::Small)),
                "Vault".into(),
                "Sign in to Bitwarden".into(),
            ),
            body.into(),
        )
    }

    fn view_two_step<'a>(&'a self, step: &'a TwoStep) -> Element<'a, Msg> {
        let email = self.signin.email.clone();
        let hint = step.email_hint.clone().unwrap_or_else(|| email.clone());
        let (title, lead) = match step.kind {
            TwoFactorKind::Authenticator => (
                "Two-step login",
                "Enter the 6-digit code from your authenticator app.".to_string(),
            ),
            TwoFactorKind::Email if step.email_sent => {
                ("Check your email", format!("Bitwarden sent a code to {hint}."))
            }
            TwoFactorKind::Email => ("Two-step login", format!("A code goes to {hint}.")),
            TwoFactorKind::NewDevice => (
                "Check your email",
                format!("This computer is new to Bitwarden. It sent a code to {hint}."),
            ),
        };
        let mut body = column![lead_text(lead)].spacing(SPACE_LG);
        if step.kinds.is_empty() {
            body = body.push(lead_text(format!(
                "This account signs in with {}, which Vault can't do yet. Add an authenticator \
                 or email on the web vault.",
                and_list(&step.unsupported)
            )));
        } else {
            let code: Element<'a, Msg> = match step.kind {
                TwoFactorKind::Authenticator => self.view_boxes(step),
                _ => text_input("Code", &step.code)
                    .id(Field::EmailCode.id())
                    .on_input(|v| Msg::Input(Field::EmailCode, Zeroizing::new(v)))
                    .on_submit(Msg::Submit(Field::EmailCode))
                    .font(fonts::mono())
                    .size(15)
                    .style(field_style(step.error.is_some()))
                    .into(),
            };
            let mut code_col = column![code].spacing(SPACE_XS + 1.0);
            if let Some(e) = &step.error {
                code_col = code_col.push(error_caption(e));
            } else if let Some(n) = &step.notice {
                code_col = code_col.push(kit_text::caption(n.as_str()).style(kit_text::muted));
            }
            body = body.push(code_col).push(primary(
                if step.busy { "Verifying…" } else { "Verify" },
                (!step.busy).then_some(Msg::VerifyTwoStep),
            ));
            if let Some(l) = self.two_step_link(step) {
                body = body.push(l);
            }
            if !step.unsupported.is_empty() {
                body = body.push(
                    kit_text::caption(format!(
                        "This account also offers {}, which Vault can't use.",
                        and_list(&step.unsupported)
                    ))
                    .style(kit_text::muted),
                );
            }
        }
        sheet(
            head(
                Some(Msg::BackToSignIn),
                None,
                title.into(),
                email,
            ),
            body.into(),
        )
    }

    /// Below Verify: another way, or the code again.
    fn two_step_link<'a>(&'a self, step: &'a TwoStep) -> Option<Element<'a, Msg>> {
        let has = |k| step.kinds.contains(&k);
        match step.kind {
            TwoFactorKind::Authenticator if has(TwoFactorKind::Email) => Some(link(
                "No app with you?",
                "Email a code instead",
                Msg::UseKind(TwoFactorKind::Email),
            )),
            TwoFactorKind::Email if has(TwoFactorKind::Authenticator) && step.email_sent => {
                Some(column![
                    link("Nothing came?", "Send it again", Msg::SendCode),
                    link("", "Use your authenticator app", Msg::UseKind(TwoFactorKind::Authenticator)),
                ].spacing(SPACE_SM).align_x(Alignment::Center).width(Length::Fill).into())
            }
            TwoFactorKind::Email if !step.email_sent => {
                Some(link("", "Send the code", Msg::SendCode))
            }
            TwoFactorKind::Email | TwoFactorKind::NewDevice => {
                Some(link("Nothing came?", "Send it again", Msg::SendCode))
            }
            _ => None,
        }
    }

    /// Six boxes over one field: the field takes the typing (Backspace, a
    /// paste, the Edit menu), the boxes show its digits.
    fn view_boxes<'a>(&'a self, step: &'a TwoStep) -> Element<'a, Msg> {
        let error = step.error.is_some();
        let next = otp::next_box(&step.code);
        let boxes = (0..otp::LEN).map(|i| {
            let digit = otp::digit(&step.code, i).map(String::from).unwrap_or_default();
            container(text(digit).font(fonts::mono()).size(20))
                .width(Length::Fixed(BOX_W))
                .height(Length::Fixed(BOX_H))
                .center_x(Length::Fixed(BOX_W))
                .center_y(Length::Fixed(BOX_H))
                .style(move |t: &Theme| style::code_box(t, next == Some(i), error))
                .into()
        });
        let shown = row(boxes).spacing(BOX_GAP);
        let typing = text_input("", &step.code)
            .id(Field::Otp.id())
            .on_input(|v| Msg::Input(Field::Otp, Zeroizing::new(v)))
            .on_submit(Msg::Submit(Field::Otp))
            .size(20)
            .padding(Padding::from([12.0, 0.0]))
            .width(Length::Fixed(BOX_W * otp::LEN as f32 + BOX_GAP * (otp::LEN - 1) as f32))
            .style(style::invisible_input);
        container(stack![shown, typing])
            .width(Length::Fill)
            .center_x(Length::Fill)
            .into()
    }

    fn view_unlock(&self) -> Element<'_, Msg> {
        let (email, server) = self
            .status
            .as_ref()
            .map(|s| (s.email.clone(), s.server.clone()))
            .unwrap_or_default();
        let sub = match ServerChoice::of(&server) {
            ServerChoice::Us => email,
            _ => format!("{email} · {}", server.label()),
        };
        let u = &self.unlock;
        let password = labeled_input(
            secret_input(
                Field::Unlock,
                "Master password",
                &u.password,
                u.reveal,
                u.error.is_some(),
                Msg::RevealForm,
            ),
            u.error.as_deref(),
        );
        let body = column![
            password,
            primary(
                if u.busy { "Unlocking…" } else { "Unlock" },
                (!u.busy).then_some(Msg::Unlock),
            ),
            link("Not you?", "Use another account", Msg::OtherAccount),
        ]
        .spacing(SPACE_LG);
        sheet(
            head(
                None,
                Some(widgets::glyph_plate("lucide/lock", PlateSize::Small)),
                "Bitwarden".into(),
                sub,
            ),
            body.into(),
        )
    }
}

/// The first field of a second step.
fn first_code_field(kind: TwoFactorKind) -> Field {
    match kind {
        TwoFactorKind::Authenticator => Field::Otp,
        _ => Field::EmailCode,
    }
}

// ── Parts shared with the sheets and the re-prompt ───────────────────

/// The card: a head, then its body.
pub fn sheet<'a>(head: Element<'a, Msg>, body: Element<'a, Msg>) -> Element<'a, Msg> {
    sheet_sized(head, body, CARD_W)
}

/// A card `width` wide.
pub fn sheet_sized<'a>(
    head: Element<'a, Msg>,
    body: Element<'a, Msg>,
    width: f32,
) -> Element<'a, Msg> {
    container(
        column![
            head,
            container(body).padding(Padding {
                top: SPACE_XS,
                right: 14.0,
                bottom: 14.0,
                left: 14.0,
            })
        ]
        .width(Length::Fill),
    )
    .width(Length::Fixed(width))
    .style(card_style)
    .into()
}

/// The card's look: raised, a 10% edge, 12 px corners, a popover shadow.
pub fn card_style(theme: &Theme) -> container::Style {
    use sola_kit::components::style::mix_white;
    let fill = style::raised_color(theme);
    container::Style {
        background: Some(iced::Background::Color(fill)),
        border: iced::Border {
            color: mix_white(fill, 0.10),
            width: 1.0,
            radius: sola_kit::components::window_frame::RADIUS.into(),
        },
        shadow: iced::Shadow {
            color: iced::Color::from_rgba(0.0, 0.0, 0.0, 0.28),
            offset: iced::Vector::new(0.0, 2.0),
            blur_radius: 10.0,
        },
        ..container::Style::default()
    }
}

/// A card's head: back, a plate, a title over a line.
pub fn head<'a>(
    back: Option<Msg>,
    plate: Option<Element<'a, Msg>>,
    title: String,
    sub: String,
) -> Element<'a, Msg> {
    let mut r = row![].spacing(10).align_y(Alignment::Center);
    let has_back = back.is_some();
    if let Some(b) = back {
        r = r.push(widgets::tool("lucide/chevron-left", "Back", Some(b)));
    }
    if let Some(p) = plate {
        r = r.push(p);
    }
    let mut who = column![
        text(title).font(fonts::ui_medium()).size(13).wrapping(text::Wrapping::None)
    ]
    .spacing(2)
    .width(Length::Fill);
    if !sub.is_empty() {
        who = who.push(kit_text::caption(sub).style(kit_text::muted).wrapping(text::Wrapping::None));
    }
    r = r.push(who);
    container(r)
        .height(Length::Fixed(44.0))
        .padding(Padding {
            top: 0.0,
            right: 8.0,
            bottom: 0.0,
            left: if has_back { 6.0 } else { 14.0 },
        })
        .align_y(Alignment::Center)
        .clip(true)
        .into()
}

/// The kit field, or with a danger edge.
pub fn field_style(
    error: bool,
) -> impl Fn(&Theme, kit_input::Status) -> kit_input::Style {
    move |t, s| {
        if error {
            style::input_error(t, s)
        } else {
            kit_input::style(t, s)
        }
    }
}

/// A password field with its eye inside, at the right.
pub fn secret_input<'a>(
    field: Field,
    placeholder: &str,
    value: &str,
    reveal: bool,
    error: bool,
    on_reveal: Msg,
) -> Element<'a, Msg> {
    let f = field.clone();
    let input = text_input(placeholder, value).size(13)
        .id(field.id())
        .secure(!reveal)
        .on_input(move |v| Msg::Input(f.clone(), Zeroizing::new(v)))
        .on_submit(Msg::Submit(field))
        .padding(Padding {
            top: 7.0,
            right: 40.0,
            bottom: 7.0,
            left: 12.0,
        })
        .style(field_style(error));
    let eye = container(widgets::tool_toggle(
        if reveal { "lucide/eye-off" } else { "lucide/eye" },
        if reveal { "Hide" } else { "Show" },
        reveal,
        on_reveal,
    ))
    .width(Length::Fill)
    .height(Length::Fill)
    .align_x(Alignment::End)
    .align_y(Alignment::Center)
    .padding(Padding::ZERO.right(3));
    stack![input, eye].height(Length::Shrink).into()
}

/// A field with its error under it.
pub fn labeled_input<'a>(
    input: impl Into<Element<'a, Msg>>,
    error: Option<&'a str>,
) -> Element<'a, Msg> {
    let mut c = column![input.into()].spacing(SPACE_XS + 1.0);
    if let Some(e) = error {
        c = c.push(error_caption(e));
    }
    c.into()
}

pub fn error_caption<'a>(message: &str) -> Element<'a, Msg> {
    kit_text::caption(message.to_string())
        .style(kit_text::danger)
        .into()
}

pub fn lead_text<'a>(s: String) -> Element<'a, Msg> {
    text(s)
        .font(fonts::ui())
        .size(12.5)
        .style(|t: &Theme| text::Style {
            color: Some(style::ink2(t)),
        })
        .into()
}

/// The one primary of a card, full width.
pub fn primary<'a>(label: &'a str, on_press: Option<Msg>) -> Element<'a, Msg> {
    button(
        text(label)
            .font(fonts::ui())
            .size(13)
            .width(Length::Fill)
            .align_x(iced::alignment::Horizontal::Center),
    )
    .padding(PAD_CONTROL)
    .width(Length::Fill)
    .style(kit_btn::primary)
    .on_press_maybe(on_press)
    .into()
}

/// "Not you? **Use another account**", centred.
pub fn link<'a>(lead: &'a str, action: &'a str, msg: Msg) -> Element<'a, Msg> {
    let mut r = row![].spacing(SPACE_XS).align_y(Alignment::Center);
    if !lead.is_empty() {
        r = r.push(kit_text::caption(lead).style(kit_text::muted));
    }
    r = r.push(
        button(text(action).font(fonts::ui_medium()).size(12))
            .padding(0)
            .style(style::link)
            .on_press(msg),
    );
    container(r).width(Length::Fill).center_x(Length::Fill).into()
}

/// "A, B and C".
pub fn and_list(parts: &[String]) -> String {
    match parts {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}
