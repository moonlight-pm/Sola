//! What the daemon answered, and what it pushed.

use std::time::{Duration, Instant};

use iced::Task;
use iced::widget::text_editor;
use sola_vault_proto::{ErrorCode, Event, LoginOutcome, Reply, Request, Status, VaultError};
use zeroize::Zeroize;

use super::view::id;
use super::{AfterFill, Ask, Generated, RepromptFor, Screen, TwoStep};
use crate::app::{App, Msg};
use crate::engine::{Cmd, Engine};
use crate::vault::{VaultEvent, log_host};

/// A new login's fill: how long to wait for the page to say whether it
/// found fields before the sheet just closes.
const FILL_REPORT_WAIT: Duration = Duration::from_secs(2);

impl<E: Engine> App<E> {
    /// The chrome's wake: take what the vault sent.
    pub(crate) fn vault_tick(&mut self) -> Task<Msg> {
        let mut tasks = Vec::new();
        while let Some(ev) = self.vault.client.try_recv() {
            tasks.push(match ev {
                VaultEvent::Answer(ask, answer) => self.vault_answer(ask, answer),
                VaultEvent::Daemon(ev) => self.vault_daemon(ev),
                VaultEvent::Connected(up) => self.vault_connected(up),
            });
        }
        let v = &mut self.vault;
        if v.awaiting_fill.is_some_and(|t| t.elapsed() >= FILL_REPORT_WAIT) {
            // The page never said: assume it filled.
            self.vault_created_filled(true);
        }
        if v_copied_done(&self.vault) {
            self.vault.record.copied = None;
        }
        // The page moved under an open sheet: its list follows.
        if self.vault.is_open() && self.vault.unlocked() && !self.vault.list.loading {
            let url = self.vault_page_url();
            if url != self.vault.list.url {
                self.vault.list.whole = false;
                self.vault.list.chosen = 0;
                self.vault_list();
            }
        }
        Task::batch(tasks)
    }

    fn vault_connected(&mut self, up: bool) -> Task<Msg> {
        let v = &mut self.vault;
        if up {
            v.away = None;
            v.ask(super::Ask::Status, Request::Status);
            v.ask(super::Ask::Settings, Request::Settings);
        } else {
            v.away = Some("Vault isn't running.".into());
            v.status = None;
            v.busy = false;
            v.forget_open_vault();
        }
        Task::none()
    }

    fn vault_daemon(&mut self, ev: Event) -> Task<Msg> {
        match ev {
            Event::Status(s) => self.vault_status(s),
            Event::ItemsChanged { ids } => {
                if self.vault.unlocked() {
                    self.vault_list();
                    let v = &self.vault;
                    let shown = v.record.id.clone().filter(|r| ids.contains(r));
                    if let Some(item) = shown.filter(|_| v.screen == Screen::Record) {
                        v.ask(Ask::Record { id: item.clone() }, Request::Get { id: item });
                    }
                }
                Task::none()
            }
            Event::Folders => Task::none(),
        }
    }

    /// The daemon's state: lock, sign-out and unlock change the sheet.
    fn vault_status(&mut self, s: Status) -> Task<Msg> {
        let v = &mut self.vault;
        let was_open = v.unlocked();
        let unlocked = s.unlocked;
        let signed_in = s.signed_in;
        v.away = None;
        // The account's email, unless one is being typed for a sign-in.
        if !s.email.is_empty() && (s.signed_in || v.forms.email.trim().is_empty()) {
            v.forms.email = s.email.clone();
        }
        v.status = Some(s);
        if signed_in {
            v.two_step = None;
        }
        if !unlocked {
            v.forget_open_vault();
            return Task::none();
        }
        if was_open {
            return Task::none();
        }
        // Just unlocked.
        v.forms.clear_secrets();
        v.busy = false;
        v.error = None;
        v.ask(Ask::Settings, sola_vault_proto::Request::Settings);
        self.vault_list();
        if self.vault.passkey.is_some() {
            return self.passkey_continue();
        }
        if self.vault.is_open() {
            return iced::widget::operation::focus(id("search"));
        }
        Task::none()
    }

    fn vault_answer(&mut self, ask: Ask, answer: Result<Reply, VaultError>) -> Task<Msg> {
        let reply = match answer {
            Ok(reply) => reply,
            Err(e) => return self.vault_failed(ask, e),
        };
        let v = &mut self.vault;
        match (ask, reply) {
            (Ask::Status, Reply::Status(s)) => return self.vault_status(s),
            (Ask::Settings, Reply::Settings(s)) => v.settings = s,
            (Ask::Login | Ask::TwoFactor, Reply::Login(outcome)) => {
                v.busy = false;
                v.forms.otp.zeroize();
                match outcome {
                    LoginOutcome::Unlocked => {
                        v.two_step = None;
                        v.forms.clear_secrets();
                        v.ask(Ask::Status, Request::Status);
                    }
                    LoginOutcome::NeedsTwoFactor {
                        kinds,
                        preferred,
                        email_hint,
                        email_sent,
                        unsupported,
                    } => {
                        tracing::info!(?preferred, email_sent, "vault: the login needs its second step");
                        v.error = None;
                        v.two_step = Some(TwoStep {
                            kinds,
                            kind: preferred,
                            email_hint,
                            unsupported,
                        });
                        v.ask(Ask::Status, Request::Status);
                        return iced::widget::operation::focus(id("otp"));
                    }
                }
            }
            (Ask::SendEmailCode, _) => {
                v.busy = false;
                v.error = None;
                v.forms.otp.zeroize();
            }
            (Ask::Unlock, _) => {
                v.busy = false;
                v.ask(Ask::Status, Request::Status);
            }
            (Ask::Logout, _) => {
                v.busy = false;
                v.two_step = None;
                v.forget_open_vault();
                v.ask(Ask::Status, Request::Status);
                return iced::widget::operation::focus(id("email"));
            }
            (Ask::Sync, _) => self.vault_list(),
            (Ask::Verify, Reply::Checked(ok)) => {
                v.busy = false;
                if !ok {
                    v.error = Some("That isn't your master password.".into());
                    return iced::widget::operation::focus(id("reprompt"));
                }
                let Some(then) = v.reprompt.take() else {
                    return Task::none();
                };
                v.reprompt_ok.insert(then.item().to_string());
                return match then {
                    RepromptFor::Open(item) => self.vault_open_item(item),
                    RepromptFor::Fill(item) => self.vault_fill_from_panel(item),
                    RepromptFor::PasskeyPick(item) => {
                        self.passkey_sign(item);
                        Task::none()
                    }
                };
            }
            (Ask::Items { url }, Reply::Items { items, unreadable }) => {
                if url != v.list.url {
                    return Task::none();
                }
                v.list.loading = false;
                v.list.unreadable = unreadable;
                tracing::debug!(n = items.len(), host = %log_host(&url), "vault: items");
                v.list.items = items;
                let mode = self.vault_mode();
                let v = &mut self.vault;
                let len = super::rows::rows(&v.list.items, mode, v.list.kind, &v.list.query).len();
                v.list.chosen = v.list.chosen.min(len.saturating_sub(1));
            }
            (Ask::Record { id: item }, Reply::Item(record)) => {
                if v.record.id.as_deref() == Some(item.as_str()) && v.screen == Screen::Record {
                    v.record.loading = false;
                    v.record.item = Some(record);
                }
            }
            (Ask::Draft { id: item }, Reply::Draft(draft)) => {
                v.busy = false;
                if v.record.id.as_deref() == Some(item.as_str()) {
                    v.notes = text_editor::Content::with_text(&draft.notes);
                    v.draft = Some(draft);
                    v.screen = Screen::Edit;
                    v.record.revealed.clear();
                    return iced::widget::operation::focus(id("edit-name"));
                }
            }
            (Ask::Save, Reply::Saved { id: item }) => {
                tracing::info!(id = %item, "vault: item saved");
                v.busy = false;
                v.draft = None;
                v.notes = text_editor::Content::new();
                v.screen = Screen::Record;
                v.record.id = Some(item.clone());
                v.record.loading = true;
                v.ask(Ask::Record { id: item.clone() }, Request::Get { id: item });
                self.vault_list();
            }
            (Ask::Create(aim), Reply::Saved { id: item }) => {
                tracing::info!(id = %item, "vault: login created");
                v.busy = false;
                self.vault_list();
                match aim {
                    Some(aim) => {
                        let url = aim.url.clone();
                        self.vault.ask(Ask::Fill(aim), Request::FillMaterial { id: item, url });
                    }
                    None => self.vault_created_filled(false),
                }
            }
            (Ask::Generate(Generated::NewLogin), Reply::Generated(mut password)) => {
                if let Some(form) = v.create.as_mut() {
                    form.password = std::mem::take(&mut password);
                    form.generated = true;
                }
                password.zeroize();
            }
            (Ask::Generate(Generated::Draft), Reply::Generated(mut password)) => {
                if let Some(d) = v.draft.as_mut() {
                    d.set_row("password", std::mem::take(&mut password));
                }
                password.zeroize();
            }
            (Ask::Fill(aim), Reply::Fill(material)) => return self.vault_filled(aim, material),
            (Ask::PasskeyList { req }, Reply::Passkeys(candidates)) => {
                self.passkey_candidates(req, candidates)
            }
            (Ask::PasskeyLogins { req }, Reply::Items { items, .. }) => {
                if v.passkey.as_ref().is_some_and(|p| p.req.id == req) {
                    v.site_logins_loading = false;
                    v.site_logins = items
                        .into_iter()
                        .filter(|i| !i.deleted && i.kind == sola_vault_proto::ItemKind::Login)
                        .collect();
                }
            }
            (Ask::PasskeySign { req }, Reply::Credential { json }) => {
                self.passkey_answered(req, Ok(json))
            }
            (Ask::Redeem { tab }, Reply::Value(mut value)) => {
                // At the caret of the page's focused field, never through
                // the clipboard.
                let text = std::mem::take(&mut value);
                let _ = self.cmd_tx.send(Cmd::Target {
                    tab,
                    cmd: Box::new(Cmd::PasteText(text)),
                });
                value.zeroize();
            }
            (ask, reply) => {
                tracing::warn!(ask = ask.name(), reply = reply_name(&reply), "vault: unexpected answer");
                self.vault.busy = false;
            }
        }
        Task::none()
    }

    fn vault_failed(&mut self, ask: Ask, e: VaultError) -> Task<Msg> {
        tracing::warn!(ask = ask.name(), code = ?e.code, "vault: request failed");
        let v = &mut self.vault;
        v.busy = false;
        match e.code {
            ErrorCode::Unavailable => {
                v.away = Some(e.message.clone());
                v.status = None;
            }
            ErrorCode::Locked | ErrorCode::SignedOut => {
                v.ask(Ask::Status, Request::Status);
            }
            _ => {}
        }
        match ask {
            Ask::Status | Ask::Settings | Ask::Sync => {}
            Ask::Items { url } if url == v.list.url => v.list.loading = false,
            Ask::Items { .. } => {}
            Ask::Record { .. } => {
                v.record.loading = false;
                v.error = Some(e.message);
            }
            Ask::Unlock if e.code == ErrorCode::BadPassword => {
                v.error = Some("That password didn't unlock this account.".into());
                return iced::widget::operation::focus(id("unlock"));
            }
            Ask::Verify if e.code == ErrorCode::BadPassword => {
                v.error = Some("That isn't your master password.".into());
            }
            Ask::SendEmailCode => v.error = Some(format!("Could not send the code: {}", e.message)),
            Ask::Fill(aim) if aim.then == AfterFill::Quiet => {}
            Ask::Fill(aim) if aim.then == AfterFill::Created => {
                // Saved; it just did not fill.
                self.vault_created_filled(false);
            }
            Ask::PasskeyList { req } => self.passkey_candidates_failed(req, e.message),
            Ask::PasskeyLogins { .. } => v.site_logins_loading = false,
            Ask::PasskeySign { req } => self.passkey_answered(req, Err(e.message)),
            Ask::Redeem { .. } => {}
            _ => v.error = Some(e.message),
        }
        Task::none()
    }

    /// A new login's fill is done, or found no fields (`found` false):
    /// close, or say it is saved.
    pub(super) fn vault_created_filled(&mut self, found: bool) {
        let v = &mut self.vault;
        v.awaiting_fill = None;
        v.busy = false;
        v.create = None;
        if found {
            v.screen = Screen::Browse;
            self.set_vault_open(false);
        } else {
            v.screen = Screen::CreateSaved;
        }
    }

    pub(super) fn vault_await_fill(&mut self) {
        self.vault.awaiting_fill = Some(Instant::now());
    }
}

fn v_copied_done(v: &super::Vault) -> bool {
    v.record
        .copied
        .as_ref()
        .is_some_and(|(_, t)| t.elapsed() >= super::view::COPIED_FOR)
}

/// A reply's kind, for a log line (never its contents).
fn reply_name(r: &Reply) -> &'static str {
    match r {
        Reply::Hello { .. } => "hello",
        Reply::Done => "done",
        Reply::Status(_) => "status",
        Reply::Settings(_) => "settings",
        Reply::Login(_) => "login",
        Reply::Checked(_) => "checked",
        Reply::Items { .. } => "items",
        Reply::Item(_) => "item",
        Reply::Draft(_) => "draft",
        Reply::Code { .. } => "code",
        Reply::Folders(_) => "folders",
        Reply::Organizations(_) => "organizations",
        Reply::Collections(_) => "collections",
        Reply::Saved { .. } => "saved",
        Reply::Generated(_) => "generated",
        Reply::Exported { .. } => "exported",
        Reply::Fill(_) => "fill",
        Reply::Passkeys(_) => "passkeys",
        Reply::Credential { .. } => "credential",
        Reply::Ticket { .. } => "ticket",
        Reply::Value(_) => "value",
    }
}
