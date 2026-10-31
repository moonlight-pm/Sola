//! What the sheet's controls and keys do.

use iced::Task;
use iced::widget::text_editor;
use sola_kit::components::text_input::{MenuEdit, MenuEdited};
use sola_vault_proto::{
    CustomDraft, GeneratorOptions, ItemDraft, ItemKind, Request, TwoFactorKind, UriDraft, UriMatch,
};
use zeroize::Zeroize;

use super::rows::{self, Mode};
use super::view::id;
use super::{AfterFill, Ask, Gate, Generated, Kind, NewLogin, RepromptFor, Screen, ServerPick};
use crate::app::{App, Msg};
use crate::engine::Engine;

/// Opening the sheet asks for a sync when the last one is older than this.
const STALE_SECS: u64 = 300;

#[derive(Debug, Clone)]
pub enum VaultMsg {
    /// The key button.
    Toggle,
    /// A click on the page, or the sheet's ×.
    Close,
    Escape,
    /// The daemon was away: ask again.
    Retry,
    Up,
    Down,
    Right,
    Enter,
    FocusNext,
    FocusPrev,

    // Signing in
    Email(String),
    Password(String),
    Server(ServerPick),
    ServerUrl(String),
    SignIn,
    Otp(String),
    Verify,
    SendCode,
    UseEmailCode,
    /// Two-step: back to the sign-in form.
    CancelSignIn,
    UnlockPassword(String),
    Unlock,
    /// Show the master password as typed.
    RevealPassword,
    /// "Not you?": sign this account out to use another.
    OtherAccount,

    // The list
    Search(String),
    Kind(Kind),
    /// The list scrolled: its offset and the height that shows.
    ListScrolled { y: f32, h: f32 },
    /// Past the site's logins: the whole vault.
    ShowAll,
    Fill(String),
    Open(String),

    // The head
    More,
    MoreClose,
    SyncNow,
    NewLogin,
    /// Show the item (or the vault) in Vault, the app.
    OpenInVault(Option<String>),
    LogOut,

    // A record
    Back,
    RevealField(String),
    Copy { key: String, value: String },
    OpenWebsite(String),
    Edit,

    // Editing it
    EditName(String),
    EditRow { key: String, value: String },
    EditUri { index: usize, value: String },
    UriMenu(Option<usize>),
    UriMatch { index: usize, value: Option<UriMatch> },
    AddUri,
    RemoveUri(usize),
    CustomName { index: usize, value: String },
    CustomValue { index: usize, value: String },
    AddCustom,
    RemoveCustom(usize),
    Notes(text_editor::Action),
    Regenerate,
    Save,
    CancelEdit,

    // A new login
    NewUsername(String),
    NewPassword(String),
    NewWebsite(String),
    NewRegenerate,
    NewSave,

    // Re-prompt
    RepromptPassword(String),
    RepromptSubmit,
    RepromptCancel,

    // A site asks for a passkey
    PasskeyPick(String),
    PasskeyCancel,
    PasskeyNew,
    PasskeyOnto(String),

    // The Edit menu on a sheet field
    MenuEdited(MenuEdit, Option<MenuEdited>),
    NotesMenuEdit(MenuEdit),
    ClipboardPaste(Option<String>),

    // A copied value's time on the clipboard
    ClipExpired(u64),
    ClipNow(u64, Option<String>),
}

impl<E: Engine> App<E> {
    pub(crate) fn on_vault(&mut self, m: VaultMsg) -> Task<Msg> {
        let v = &mut self.vault;
        match m {
            VaultMsg::Toggle => {
                let open = !v.is_open();
                self.set_vault_open(open);
                if open {
                    return self.vault_opened();
                }
            }
            VaultMsg::Close => {
                if v.passkey.is_some() {
                    self.cancel_passkey("User cancelled.");
                } else {
                    self.set_vault_open(false);
                }
            }
            VaultMsg::Escape => return self.vault_escape(),
            VaultMsg::Retry => {
                v.error = None;
                v.ask(Ask::Status, Request::Status);
            }
            VaultMsg::Up => return self.vault_step(false),
            VaultMsg::Down => return self.vault_step(true),
            VaultMsg::Right => {
                if v.gate() == Gate::Open && v.screen == Screen::Browse && v.list.query.is_empty() {
                    if let Some(id) = self.vault_chosen() {
                        return self.vault_open_item(id);
                    }
                }
            }
            VaultMsg::Enter => return self.vault_enter(),
            VaultMsg::FocusNext => return iced::widget::operation::focus_next(),
            VaultMsg::FocusPrev => return iced::widget::operation::focus_previous(),

            VaultMsg::Email(s) => v.forms.email = s,
            VaultMsg::Password(s) => v.forms.password = s,
            VaultMsg::Server(pick) => v.forms.server = pick,
            VaultMsg::ServerUrl(s) => v.forms.server_url = s,
            VaultMsg::SignIn => self.vault_sign_in(),
            VaultMsg::Otp(s) => {
                let six = v
                    .two_step
                    .as_ref()
                    .is_some_and(|t| t.kind == TwoFactorKind::Authenticator);
                if six {
                    // Six boxes: digits only; the sixth sends it.
                    let digits: String = s.chars().filter(char::is_ascii_digit).take(6).collect();
                    let full = digits.len() == 6 && v.forms.otp.len() < 6;
                    v.forms.otp = digits;
                    if full {
                        self.vault_verify();
                    }
                } else {
                    v.forms.otp = s;
                }
            }
            VaultMsg::Verify => self.vault_verify(),
            VaultMsg::SendCode => {
                let Some(kind) = v.two_step.as_ref().map(|t| t.kind) else {
                    return Task::none();
                };
                if !v.busy {
                    v.busy = true;
                    v.error = None;
                    v.ask(Ask::SendEmailCode, Request::SendEmailCode { kind });
                }
            }
            VaultMsg::UseEmailCode => {
                if let Some(t) = v.two_step.as_mut() {
                    t.kind = TwoFactorKind::Email;
                    v.forms.otp.zeroize();
                    return Task::batch([
                        self.on_vault(VaultMsg::SendCode),
                        iced::widget::operation::focus(id("otp")),
                    ]);
                }
            }
            VaultMsg::CancelSignIn => {
                v.two_step = None;
                v.forms.clear_secrets();
                v.error = None;
                v.busy = false;
                return iced::widget::operation::focus(id("password"));
            }
            VaultMsg::UnlockPassword(s) => v.forms.unlock = s,
            VaultMsg::Unlock => {
                if v.busy {
                    return Task::none();
                }
                if v.forms.unlock.is_empty() {
                    v.error = Some("Enter your master password.".into());
                    return Task::none();
                }
                v.busy = true;
                v.error = None;
                let password = std::mem::take(&mut v.forms.unlock);
                v.ask(Ask::Unlock, Request::Unlock { password });
            }
            VaultMsg::RevealPassword => v.forms.reveal = !v.forms.reveal,
            VaultMsg::OtherAccount => {
                v.forms.clear_secrets();
                v.forms.email.clear();
                v.busy = true;
                v.ask(Ask::Logout, Request::Logout);
            }

            VaultMsg::Search(s) => {
                v.list.query = s;
                v.list.chosen = 0;
            }
            VaultMsg::Kind(k) => {
                v.list.kind = k;
                v.list.chosen = 0;
            }
            VaultMsg::ListScrolled { y, h } => {
                v.list.scroll_y = y;
                v.list.view_h = h;
            }
            VaultMsg::ShowAll => {
                v.list.whole = true;
                v.list.chosen = 0;
                return iced::widget::operation::focus(id("search"));
            }
            VaultMsg::Fill(item) => return self.vault_fill_from_panel(item),
            VaultMsg::Open(item) => return self.vault_open_item(item),

            VaultMsg::More => v.more = !v.more,
            VaultMsg::MoreClose => v.more = false,
            VaultMsg::SyncNow => {
                v.more = false;
                v.ask(Ask::Sync, Request::Sync { force: true });
            }
            VaultMsg::NewLogin => {
                v.more = false;
                return self.vault_new_login();
            }
            VaultMsg::OpenInVault(item) => {
                self.set_vault_open(false);
                super::fill::open_in_vault(item);
            }
            VaultMsg::LogOut => {
                v.more = false;
                if v.passkey.is_some() {
                    self.cancel_passkey("Logged out.");
                }
                let v = &mut self.vault;
                v.busy = true;
                v.ask(Ask::Logout, Request::Logout);
            }

            VaultMsg::Back => {
                if v.screen == Screen::Edit {
                    return self.on_vault(VaultMsg::CancelEdit);
                }
                v.close_record();
                v.create = None;
                v.screen = Screen::Browse;
                v.error = None;
                v.busy = false;
                return iced::widget::operation::focus(id("search"));
            }
            VaultMsg::RevealField(key) => {
                if !v.record.revealed.remove(&key) {
                    v.record.revealed.insert(key);
                }
            }
            VaultMsg::Copy { key, value } => return self.vault_copy(key, value),
            VaultMsg::OpenWebsite(url) => {
                self.set_vault_open(false);
                self.open_tab(url, true);
            }
            VaultMsg::Edit => {
                let Some(item) = v.record.id.clone() else {
                    return Task::none();
                };
                if v.busy || !v.record.item.as_ref().is_some_and(|r| r.can_edit) {
                    return Task::none();
                }
                v.busy = true;
                v.error = None;
                v.ask(Ask::Draft { id: item.clone() }, Request::Draft { id: item });
            }

            VaultMsg::EditName(s) => {
                if let Some(d) = v.draft.as_mut() {
                    d.name = s;
                }
            }
            VaultMsg::EditRow { key, value } => {
                if let Some(d) = v.draft.as_mut() {
                    d.set_row(&key, value);
                }
            }
            VaultMsg::EditUri { index, value } => {
                if let Some(u) = v.draft.as_mut().and_then(|d| d.uris.get_mut(index)) {
                    u.value = value;
                }
            }
            VaultMsg::UriMenu(index) => v.uri_menu = index,
            VaultMsg::UriMatch { index, value } => {
                v.uri_menu = None;
                if let Some(u) = v.draft.as_mut().and_then(|d| d.uris.get_mut(index)) {
                    u.match_type = value;
                }
            }
            VaultMsg::AddUri => {
                if let Some(d) = v.draft.as_mut() {
                    d.uris.push(UriDraft {
                        index: None,
                        value: String::new(),
                        match_type: None,
                    });
                    let key = format!("uri:{}", d.uris.len() - 1);
                    return iced::widget::operation::focus(id(&key));
                }
            }
            VaultMsg::RemoveUri(index) => {
                if let Some(d) = v.draft.as_mut().filter(|d| index < d.uris.len()) {
                    d.uris.remove(index);
                }
            }
            VaultMsg::CustomName { index, value } => {
                if let Some(c) = v.draft.as_mut().and_then(|d| d.custom.get_mut(index)) {
                    c.name = value;
                }
            }
            VaultMsg::CustomValue { index, value } => {
                if let Some(c) = v.draft.as_mut().and_then(|d| d.custom.get_mut(index)) {
                    c.value = value;
                }
            }
            VaultMsg::AddCustom => {
                if let Some(d) = v.draft.as_mut() {
                    d.custom.push(CustomDraft {
                        index: None,
                        name: String::new(),
                        value: String::new(),
                        hidden: false,
                    });
                    let key = format!("custom-name:{}", d.custom.len() - 1);
                    return iced::widget::operation::focus(id(&key));
                }
            }
            VaultMsg::RemoveCustom(index) => {
                if let Some(d) = v.draft.as_mut().filter(|d| index < d.custom.len()) {
                    d.custom.remove(index);
                }
            }
            VaultMsg::Notes(action) => self.vault_edit_notes(action),
            VaultMsg::Regenerate => {
                if !v.busy && v.draft.is_some() {
                    v.ask(
                        Ask::Generate(Generated::Draft),
                        Request::Generate {
                            options: GeneratorOptions::default(),
                        },
                    );
                }
            }
            VaultMsg::Save => {
                if v.busy {
                    return Task::none();
                }
                let Some(draft) = v.draft.clone() else {
                    return Task::none();
                };
                if !draft.can_edit {
                    v.error = Some("This item can't be edited.".into());
                    return Task::none();
                }
                v.busy = true;
                v.error = None;
                v.uri_menu = None;
                v.ask(Ask::Save, Request::Update { draft });
            }
            VaultMsg::CancelEdit => {
                v.draft = None;
                v.notes = text_editor::Content::new();
                v.uri_menu = None;
                v.busy = false;
                v.error = None;
                v.screen = if v.record.item.is_some() {
                    Screen::Record
                } else {
                    Screen::Browse
                };
            }

            VaultMsg::NewUsername(s) => {
                if let Some(c) = v.create.as_mut() {
                    c.username = s;
                }
            }
            VaultMsg::NewPassword(s) => {
                if let Some(c) = v.create.as_mut() {
                    c.password = s;
                    c.generated = false;
                }
            }
            VaultMsg::NewWebsite(s) => {
                if let Some(c) = v.create.as_mut() {
                    c.website = s;
                }
            }
            VaultMsg::NewRegenerate => {
                if !v.busy {
                    v.ask(
                        Ask::Generate(Generated::NewLogin),
                        Request::Generate {
                            options: GeneratorOptions::default(),
                        },
                    );
                }
            }
            VaultMsg::NewSave => self.vault_save_new_login(),

            VaultMsg::RepromptPassword(s) => v.reprompt_password = s,
            VaultMsg::RepromptSubmit => {
                if v.busy || v.reprompt.is_none() {
                    return Task::none();
                }
                if v.reprompt_password.is_empty() {
                    v.error = Some("Enter your master password.".into());
                    return Task::none();
                }
                v.busy = true;
                v.error = None;
                let password = std::mem::take(&mut v.reprompt_password);
                v.ask(Ask::Verify, Request::VerifyMasterPassword { password });
            }
            VaultMsg::RepromptCancel => {
                v.reprompt = None;
                v.reprompt_password.zeroize();
                v.error = None;
                v.busy = false;
            }

            VaultMsg::PasskeyPick(item) => {
                if self.vault_needs_reprompt(&item) {
                    return self.vault_ask_reprompt(RepromptFor::PasskeyPick(item));
                }
                self.passkey_sign(item);
            }
            VaultMsg::PasskeyCancel => self.cancel_passkey("User cancelled."),
            VaultMsg::PasskeyNew => self.passkey_create(None),
            VaultMsg::PasskeyOnto(item) => self.passkey_create(Some(item)),

            VaultMsg::MenuEdited(edit, done) => return self.vault_menu_edited(edit, done),
            VaultMsg::NotesMenuEdit(edit) => return self.vault_notes_menu_edit(edit),
            VaultMsg::ClipboardPaste(text) => {
                let Some(text) = crate::util::usable_clipboard_text(text) else {
                    return Task::none();
                };
                // Restore: a smithay receive can drop the original offer.
                return Task::batch([
                    self.vault_menu_edit(MenuEdit::Paste(text.clone())),
                    iced::clipboard::write(text),
                ]);
            }
            VaultMsg::ClipExpired(generation) => return self.vault_clip_expired(generation),
            VaultMsg::ClipNow(generation, now) => return self.vault_clip_now(generation, now),
        }
        Task::none()
    }

    /// Open or close the sheet (the downloads panel gives way).
    pub(crate) fn set_vault_open(&mut self, open: bool) {
        self.vault.set_open(open);
        if open {
            self.set_downloads_panel_open(false);
        }
    }

    /// The sheet just opened: ask what it needs and put the caret where
    /// the first key belongs.
    pub(super) fn vault_opened(&mut self) -> Task<Msg> {
        let v = &mut self.vault;
        if v.screen != Screen::Edit {
            v.error = None;
        }
        match v.gate() {
            Gate::Away | Gate::Connecting => {
                v.ask(Ask::Status, Request::Status);
                Task::none()
            }
            Gate::SignIn => {
                if v.forms.email.trim().is_empty() {
                    iced::widget::operation::focus(id("email"))
                } else {
                    iced::widget::operation::focus(id("password"))
                }
            }
            Gate::TwoStep => iced::widget::operation::focus(id("otp")),
            Gate::Locked => iced::widget::operation::focus(id("unlock")),
            Gate::Open => {
                let stale = v.status.as_ref().is_some_and(|s| {
                    !s.syncing && s.synced_at.is_none_or(|t| now_secs().saturating_sub(t) >= STALE_SECS)
                });
                if stale {
                    v.ask(Ask::Sync, Request::Sync { force: false });
                }
                let keep = match v.screen {
                    Screen::PasskeyPick | Screen::PasskeyCreate => v.passkey.is_some(),
                    Screen::Edit | Screen::Create => true,
                    _ => false,
                };
                if !keep {
                    v.screen = Screen::Browse;
                }
                self.vault_list();
                iced::widget::operation::focus(id("search"))
            }
        }
    }

    fn vault_escape(&mut self) -> Task<Msg> {
        let v = &mut self.vault;
        if v.more {
            v.more = false;
            return Task::none();
        }
        if v.uri_menu.is_some() {
            v.uri_menu = None;
            return Task::none();
        }
        if v.reprompt.is_some() {
            return self.on_vault(VaultMsg::RepromptCancel);
        }
        if v.passkey.is_some() {
            self.cancel_passkey("User cancelled.");
            return Task::none();
        }
        match v.screen {
            Screen::Edit => self.on_vault(VaultMsg::CancelEdit),
            Screen::Record | Screen::Create if v.is_open() => self.on_vault(VaultMsg::Back),
            _ => {
                self.set_vault_open(false);
                Task::none()
            }
        }
    }

    /// The page the sheet is for: the active page's URL (empty on none).
    pub(super) fn vault_page_url(&self) -> String {
        self.active_tab_info()
            .map(|t| t.url.clone())
            .filter(|u| !u.is_empty() && u != crate::app::BLANK_URL)
            .unwrap_or_default()
    }

    /// Ask for the list, for the active page.
    pub(super) fn vault_list(&mut self) {
        if !self.vault.unlocked() {
            return;
        }
        let url = self.vault_page_url();
        self.vault.ask_list(url);
    }

    /// The list as the sheet shows it now.
    pub(super) fn vault_mode(&self) -> Mode {
        let v = &self.vault;
        let has_site = super::page_host(&v.list.url).is_some();
        rows::mode(&v.list.items, has_site, &v.list.query, v.list.whole)
    }

    fn vault_chosen(&self) -> Option<String> {
        let v = &self.vault;
        let mode = self.vault_mode();
        rows::rows(&v.list.items, mode, v.list.kind, &v.list.query)
            .get(v.list.chosen)
            .map(|(_, i)| i.id.clone())
    }

    /// ↑ / ↓: move the chosen row; past the site's last, the whole vault.
    /// The list scrolls to keep it in view.
    fn vault_step(&mut self, down: bool) -> Task<Msg> {
        let mode = self.vault_mode();
        let v = &mut self.vault;
        let len = match v.screen {
            Screen::Browse if v.gate() == Gate::Open => {
                rows::rows(&v.list.items, mode, v.list.kind, &v.list.query).len()
            }
            Screen::PasskeyPick => v.passkey.as_ref().map_or(0, |p| p.candidates.len()),
            Screen::PasskeyCreate => v.site_logins.len(),
            _ => return Task::none(),
        };
        let chosen = &mut v.list.chosen;
        if down {
            if *chosen + 1 < len {
                *chosen += 1;
            } else if v.screen == Screen::Browse && matches!(mode, Mode::Site | Mode::NoneHere) {
                v.list.whole = true;
                v.list.chosen = 0;
                return iced::widget::operation::scroll_to(id("list"), offset(0.0));
            }
        } else if *chosen > 0 {
            *chosen -= 1;
        } else if v.screen == Screen::Browse && mode == Mode::Whole && v.list.whole {
            v.list.whole = false;
            let site = rows::site(&v.list.items).len();
            v.list.chosen = site.saturating_sub(1);
        }
        if v.screen != Screen::Browse {
            return Task::none();
        }
        let mode = self.vault_mode();
        let v = &self.vault;
        let listed = rows::rows(&v.list.items, mode, v.list.kind, &v.list.query);
        let (top, bottom) = rows::span(&listed, v.list.chosen);
        let (y, h) = (v.list.scroll_y, v.list.view_h);
        if h <= 0.0 {
            return Task::none();
        }
        let to = if top < y {
            top
        } else if bottom > y + h {
            bottom - h
        } else {
            return Task::none();
        };
        iced::widget::operation::scroll_to(id("list"), offset(to.max(0.0)))
    }

    /// Enter with no field taking it: the sheet's one action.
    fn vault_enter(&mut self) -> Task<Msg> {
        let v = &self.vault;
        if v.gate() != Gate::Open || v.reprompt.is_some() {
            return Task::none();
        }
        let chosen = v.list.chosen;
        match v.screen {
            Screen::Browse => self.vault_act_on_chosen(),
            Screen::Record => match v.record.item.as_ref() {
                Some(r) if r.can_fill() => {
                    let item = r.id.clone();
                    self.vault_fill_from_panel(item)
                }
                _ => Task::none(),
            },
            Screen::PasskeyPick => {
                let pick = v
                    .passkey
                    .as_ref()
                    .and_then(|p| p.candidates.get(chosen))
                    .map(|c| c.cipher_id.clone());
                match pick {
                    Some(item) => self.on_vault(VaultMsg::PasskeyPick(item)),
                    None => Task::none(),
                }
            }
            Screen::PasskeyCreate => {
                let onto = v.site_logins.get(chosen).map(|i| i.id.clone());
                match onto {
                    Some(item) => self.on_vault(VaultMsg::PasskeyOnto(item)),
                    None => self.on_vault(VaultMsg::PasskeyNew),
                }
            }
            _ => Task::none(),
        }
    }

    /// Enter in search, or with the list chosen: fill a site's login,
    /// open anything else.
    pub(super) fn vault_act_on_chosen(&mut self) -> Task<Msg> {
        let Some(item) = self.vault_chosen() else {
            return Task::none();
        };
        if self.vault_mode() == Mode::Site {
            self.vault_fill_from_panel(item)
        } else {
            self.vault_open_item(item)
        }
    }

    pub(super) fn vault_open_item(&mut self, item: String) -> Task<Msg> {
        if !self.vault.unlocked() {
            return Task::none();
        }
        if self.vault_needs_reprompt(&item) {
            return self.vault_ask_reprompt(RepromptFor::Open(item));
        }
        let v = &mut self.vault;
        v.screen = Screen::Record;
        v.record = super::Record {
            id: Some(item.clone()),
            loading: true,
            ..super::Record::default()
        };
        v.error = None;
        v.more = false;
        v.ask_record(item);
        Task::none()
    }

    /// The item asks for the master password, and it has not been given
    /// while the sheet has been up.
    pub(super) fn vault_needs_reprompt(&self, item: &str) -> bool {
        let v = &self.vault;
        if v.reprompt_ok.contains(item) {
            return false;
        }
        v.list.items.iter().any(|i| i.id == item && i.reprompt)
            || v
                .passkey
                .as_ref()
                .is_some_and(|p| p.candidates.iter().any(|c| c.cipher_id == item && c.reprompt))
    }

    pub(super) fn vault_ask_reprompt(&mut self, then: RepromptFor) -> Task<Msg> {
        let v = &mut self.vault;
        v.reprompt = Some(then);
        v.reprompt_password.zeroize();
        v.error = None;
        if !v.is_open() {
            self.set_vault_open(true);
        }
        iced::widget::operation::focus(id("reprompt"))
    }

    fn vault_sign_in(&mut self) {
        let v = &mut self.vault;
        if v.busy {
            return;
        }
        let email = v.forms.email.trim().to_string();
        if email.is_empty() || v.forms.password.is_empty() {
            v.error = Some("Enter your email and master password.".into());
            return;
        }
        // Under the server's field: the service refuses all but https.
        if let Some(why) = v.forms.server().address_problem() {
            v.error = Some(why.into());
            return;
        }
        v.busy = true;
        v.error = None;
        let password = std::mem::take(&mut v.forms.password);
        let server = v.forms.server();
        v.ask(
            Ask::Login,
            Request::Login {
                email,
                password,
                server,
            },
        );
    }

    fn vault_verify(&mut self) {
        let v = &mut self.vault;
        if v.busy {
            return;
        }
        let Some(kind) = v.two_step.as_ref().map(|t| t.kind) else {
            return;
        };
        // Spaces and line breaks come in with a paste.
        let code: String = v.forms.otp.chars().filter(|c| !c.is_whitespace()).collect();
        if code.is_empty() {
            v.error = Some("Enter the code.".into());
            return;
        }
        v.busy = true;
        v.error = None;
        v.ask(Ask::TwoFactor, Request::TwoFactor { kind, code });
    }

    /// + or ··· New login: the form, prefilled for this page, with a
    /// generated password on its way.
    fn vault_new_login(&mut self) -> Task<Msg> {
        if !self.vault.unlocked() {
            return Task::none();
        }
        let url = self.vault_page_url();
        let v = &mut self.vault;
        v.close_record();
        v.create = Some(NewLogin {
            username: String::new(),
            password: String::new(),
            website: super::page_host(&url).unwrap_or_default(),
            generated: false,
        });
        v.screen = Screen::Create;
        v.error = None;
        v.busy = false;
        v.ask(
            Ask::Generate(Generated::NewLogin),
            Request::Generate {
                options: GeneratorOptions::default(),
            },
        );
        iced::widget::operation::focus(id("new-username"))
    }

    fn vault_save_new_login(&mut self) {
        let aim = self.vault_aim(self.cached_active, AfterFill::Created);
        let v = &mut self.vault;
        if v.busy {
            return;
        }
        let Some(form) = v.create.as_ref() else {
            return;
        };
        if form.password.trim().is_empty() {
            v.error = Some("A login needs a password.".into());
            return;
        }
        let website = form.website.trim().to_string();
        let mut draft = ItemDraft::blank(ItemKind::Login);
        draft.name = match crate::vault::apex_domain(&website) {
            apex if !apex.is_empty() => apex,
            _ if !website.is_empty() => website.clone(),
            _ => "Login".into(),
        };
        draft.set_row("username", form.username.trim().to_string());
        draft.set_row("password", form.password.clone());
        if !website.is_empty() {
            draft.uris.push(UriDraft {
                index: None,
                value: website,
                match_type: None,
            });
        }
        v.busy = true;
        v.error = None;
        v.ask(Ask::Create(aim), Request::Create { draft });
    }
}

pub(super) fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// For the select over a website's match type.
pub(super) const MATCHES: [Option<UriMatch>; 7] = [
    None,
    Some(UriMatch::Domain),
    Some(UriMatch::Host),
    Some(UriMatch::StartsWith),
    Some(UriMatch::Exact),
    Some(UriMatch::RegularExpression),
    Some(UriMatch::Never),
];

/// `None` is the account's default, Domain.
pub(super) fn match_label(m: Option<UriMatch>) -> &'static str {
    m.map_or("Default", UriMatch::label)
}

/// A vertical scroll position for the list.
fn offset(y: f32) -> iced::widget::scrollable::AbsoluteOffset {
    iced::widget::scrollable::AbsoluteOffset { x: 0.0, y }
}
