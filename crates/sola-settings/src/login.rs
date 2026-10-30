//! Login: the desk's password and auto-login, which Oath's login service
//! keeps (`sola_core::login`, `svc:login`). Super+L locks (the flower menu's
//! Lock Screen); Lock when idle is under Appearance → Idle. See
//! `docs/specs/2026-10-29-secure-access-design.md`.

use iced::widget::{row, toggler};
use iced::{Element, Length, Task};
use sola_core::login::{self, Status};
use sola_kit::components::button as kit_btn;
use sola_kit::components::form::toggle_style;
use sola_kit::components::style::SPACE_SM;
use sola_kit::components::text_input::text_input;

use crate::page;

#[derive(Debug, Clone)]
pub enum LoginMsg {
    /// Ask the service again (the page opened).
    Refresh,
    Status(Result<Status, String>),
    Current(String),
    New(String),
    Confirm(String),
    /// Set (or change) the password to the new one.
    Set,
    Remove,
    AutoLogin(bool),
    /// A change finished: what to say, or why it failed.
    Done(Result<&'static str, String>),
}

#[derive(Default)]
pub struct LoginState {
    /// `None` until the service answers; `Err` when it is not here.
    status: Option<Result<Status, String>>,
    current: String,
    new: String,
    confirm: String,
    busy: bool,
    /// The last change's outcome: (is an error, words).
    note: Option<(bool, String)>,
}

pub fn update(msg: LoginMsg, ui: &mut LoginState) -> Task<LoginMsg> {
    match msg {
        LoginMsg::Refresh => return ask(|| login::status(), LoginMsg::Status),
        LoginMsg::Status(s) => ui.status = Some(s),
        LoginMsg::Current(v) => ui.current = v,
        LoginMsg::New(v) => ui.new = v,
        LoginMsg::Confirm(v) => ui.confirm = v,
        LoginMsg::Set => {
            let (old, new) = (ui.current.clone(), ui.new.clone());
            return change(ui, move || login::set(&old, &new).map(|()| "Password set."));
        }
        LoginMsg::Remove => {
            let old = ui.current.clone();
            return change(ui, move || login::set(&old, "").map(|()| "Password removed."));
        }
        LoginMsg::AutoLogin(on) => {
            let password = ui.current.clone();
            let said = if on {
                "The desk will start signed in."
            } else {
                "The desk will start locked."
            };
            return change(ui, move || login::autologin(on, &password).map(|()| said));
        }
        LoginMsg::Done(result) => {
            ui.busy = false;
            ui.note = Some(match result {
                Ok(said) => {
                    ui.current.clear();
                    ui.new.clear();
                    ui.confirm.clear();
                    (false, said.to_string())
                }
                Err(e) => (true, e),
            });
            return ask(|| login::status(), LoginMsg::Status);
        }
    }
    Task::none()
}

/// A call to the service off the UI thread (a wrong password is answered
/// late).
fn ask<T: Send + 'static>(
    call: impl FnOnce() -> Result<T, login::Error> + Send + 'static,
    then: impl Fn(Result<T, String>) -> LoginMsg + Send + 'static,
) -> Task<LoginMsg> {
    Task::perform(
        async move {
            tokio::task::spawn_blocking(move || call().map_err(|e| e.to_string()))
                .await
                .map_err(|e| e.to_string())
                .and_then(|r| r)
        },
        then,
    )
}

fn change(
    ui: &mut LoginState,
    call: impl FnOnce() -> Result<&'static str, login::Error> + Send + 'static,
) -> Task<LoginMsg> {
    ui.busy = true;
    ui.note = None;
    ask(call, LoginMsg::Done)
}

pub fn view(ui: &LoginState) -> Element<'_, LoginMsg> {
    let status = match &ui.status {
        None => return page::page(vec![page::empty("Asking Oath's login service…")]),
        Some(Err(e)) => {
            return page::page(vec![page::section(
                "Password",
                None,
                page::group(vec![page::empty(e.clone())]),
                Some(
                    "Without Oath's login service there is no password: the desk starts \
                     signed in and nothing locks."
                        .into(),
                ),
            )]);
        }
        Some(Ok(s)) => s,
    };
    let secure = |placeholder, value: &str, on: fn(String) -> LoginMsg| {
        text_input(placeholder, value)
            .secure(true)
            .on_input_maybe((!ui.busy).then_some(on))
            .width(Length::Fixed(240.0))
    };
    let mut rows = Vec::new();
    if status.password {
        rows.push(page::setting(
            "Current password",
            secure("Current password", &ui.current, LoginMsg::Current),
        ));
    }
    rows.push(page::setting(
        if status.password { "New password" } else { "Password" },
        secure("New password", &ui.new, LoginMsg::New),
    ));
    rows.push(page::setting(
        "Again",
        secure("Type it again", &ui.confirm, LoginMsg::Confirm),
    ));
    let can_set = !ui.busy
        && !ui.new.is_empty()
        && ui.new == ui.confirm
        && (!status.password || !ui.current.is_empty());
    let set_label = if status.password {
        "Change Password"
    } else {
        "Set Password"
    };
    let mut buttons = row![
        kit_btn::labeled_sm(set_label, kit_btn::primary).on_press_maybe(can_set.then_some(LoginMsg::Set))
    ]
    .spacing(SPACE_SM);
    if status.password {
        let can_remove = !ui.busy && !ui.current.is_empty();
        buttons = buttons.push(
            kit_btn::labeled_sm("Remove Password", kit_btn::danger_outline)
                .on_press_maybe(can_remove.then_some(LoginMsg::Remove)),
        );
    }
    rows.push(page::setting("", buttons));
    if let Some((error, words)) = &ui.note {
        let text = sola_kit::components::text::body(words.clone());
        rows.push(page::plain(if *error {
            text.style(sola_kit::components::text::danger)
        } else {
            text.style(sola_kit::components::text::muted)
        }));
    }
    let password = page::section(
        "Password",
        None,
        page::group(rows),
        Some(if status.password {
            "A password is set. Changing or removing it needs the current one.".into()
        } else {
            "No password is set: the desk never asks for one, and Super+L does not lock.".into()
        }),
    );

    let auto = toggler(status.autologin)
        .on_toggle_maybe(
            (!ui.busy && (!status.password || !ui.current.is_empty())).then_some(LoginMsg::AutoLogin),
        )
        .style(toggle_style);
    let signin = page::section(
        "Signing in",
        None,
        page::group(vec![page::setting("Sign in automatically", auto)]),
        Some(
            "On: the desk starts signed in, and Exit Sola opens a console without asking. \
             Off: the desk starts locked and the console asks for the password. With a \
             password set, type the current one above to change this."
                .into(),
        ),
    );
    let lock = page::section(
        "Locking",
        None,
        page::group(vec![page::empty(
            "Super+L locks the screen (the flower menu's Lock Screen). To lock when the \
             windows have faded, turn on Lock when idle in Appearance → Idle.",
        )]),
        None,
    );
    page::page(vec![password, signin, lock])
}
