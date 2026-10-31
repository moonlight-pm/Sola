//! The vault panel: one sheet under the key button, on `sola-vaultd`
//! (`docs/specs/2026-10-31-sola-vault-design.md`, canvas `design/vault`).
//!
//! The daemon holds the keys and answers on its socket; this module keeps
//! what the sheet shows, asks the daemon ([`Ask`] tags each request so its
//! answer lands where it was meant), and runs the page side: fill
//! scripts, passkey answers, inserting a value the desk menu delivered.

mod answers;
mod edit;
mod fill;
mod keys;
mod passkeys;
mod rows;
mod update;
mod view;

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use iced::widget::text_editor;
use sola_vault_proto::{ItemDraft, ItemRecord, ItemSummary, Server, Settings, Status, TwoFactorKind};
use zeroize::Zeroize;

use crate::engine::TabId;
use crate::vault::{PasskeyPageRequest, VaultHandle};

pub(crate) use keys::{keys_open, panel_key};
pub(crate) use rows::Kind;
pub(crate) use update::VaultMsg;

/// Why chrome asked the daemon; its answer is routed by this.
#[derive(Debug, Clone)]
pub(crate) enum Ask {
    Status,
    Settings,
    Login,
    TwoFactor,
    SendEmailCode,
    Unlock,
    Logout,
    Sync,
    /// The re-prompt's master password.
    Verify,
    /// The list, for the page at `url`; only the latest (`seq`) counts.
    Items { url: String, seq: u64 },
    Record { id: String, seq: u64 },
    Draft { id: String },
    Save,
    /// A new login, filled into `Aim`'s page once saved.
    Create(Option<Aim>),
    Generate(Generated),
    Fill(Aim),
    /// Passkeys for a page's `get()`.
    PasskeyList { req: u64 },
    /// The site's logins a `create()` could go onto.
    PasskeyLogins { req: u64 },
    PasskeySign { req: u64 },
}

impl Ask {
    /// For a log line: never a URL or an id's secret.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Status => "status",
            Self::Settings => "settings",
            Self::Login => "login",
            Self::TwoFactor => "two_factor",
            Self::SendEmailCode => "send_email_code",
            Self::Unlock => "unlock",
            Self::Logout => "logout",
            Self::Sync => "sync",
            Self::Verify => "verify",
            Self::Items { .. } => "items",
            Self::Record { .. } => "record",
            Self::Draft { .. } => "draft",
            Self::Save => "save",
            Self::Create(_) => "create",
            Self::Generate(_) => "generate",
            Self::Fill(_) => "fill",
            Self::PasskeyList { .. } => "passkey_list",
            Self::PasskeyLogins { .. } => "passkey_logins",
            Self::PasskeySign { .. } => "passkey_sign",
        }
    }
}

/// Where a generated password goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Generated {
    NewLogin,
    Draft,
}

/// Where a fill goes: the tab it was asked from, while that tab stays on
/// the origin it had then.
#[derive(Debug, Clone)]
pub(crate) struct Aim {
    pub tab: TabId,
    pub origin: String,
    pub url: String,
    pub then: AfterFill,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AfterFill {
    /// The panel closes so the form can be sent.
    Close,
    /// A login just made: wait for the page to say whether it had fields.
    Created,
    /// From the desk menu: the panel is not involved.
    Quiet,
}

/// What the sheet shows while the vault is unlocked.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Screen {
    #[default]
    Browse,
    Record,
    Edit,
    Create,
    /// A new login saved; the page had nowhere to fill it.
    CreateSaved,
    PasskeyPick,
    PasskeyCreate,
}

/// What the sheet can show at all, from the daemon's word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Gate {
    /// The daemon is not running.
    Away,
    /// Asked, no answer yet.
    Connecting,
    SignIn,
    TwoStep,
    Locked,
    Open,
}

/// Which Bitwarden the sign-in form is for (V5).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ServerPick {
    #[default]
    Us,
    Eu,
    SelfHosted,
}

/// The sign-in, two-step and unlock fields.
#[derive(Default)]
pub(crate) struct Forms {
    pub email: String,
    pub password: String,
    pub server: ServerPick,
    pub server_url: String,
    pub otp: String,
    pub unlock: String,
    /// The master password shows as typed.
    pub reveal: bool,
}

impl Forms {
    fn clear_secrets(&mut self) {
        self.password.zeroize();
        self.otp.zeroize();
        self.unlock.zeroize();
        self.reveal = false;
    }

    pub fn server(&self) -> Server {
        match self.server {
            ServerPick::Us => Server::Us,
            ServerPick::Eu => Server::Eu,
            ServerPick::SelfHosted => Server::SelfHosted {
                url: self.server_url.trim().to_string(),
            },
        }
    }
}

/// A login waiting for its second step.
#[derive(Debug, Clone)]
pub(crate) struct TwoStep {
    pub kinds: Vec<TwoFactorKind>,
    pub kind: TwoFactorKind,
    pub email_hint: Option<String>,
    pub unsupported: Vec<String>,
}

/// What a master-password re-prompt unlocks once it checks out.
#[derive(Debug, Clone)]
pub(crate) enum RepromptFor {
    Open(String),
    Fill(String),
    PasskeyPick(String),
}

impl RepromptFor {
    pub fn item(&self) -> &str {
        match self {
            Self::Open(id) | Self::Fill(id) | Self::PasskeyPick(id) => id,
        }
    }
}

/// The list: everything the daemon listed for the page, and how the
/// sheet looks at it.
#[derive(Default)]
pub(crate) struct List {
    pub items: Vec<ItemSummary>,
    pub unreadable: u32,
    pub loading: bool,
    /// The page the list was asked for.
    pub url: String,
    pub query: String,
    pub kind: Kind,
    /// The chosen row (arrows move it, Enter acts on it).
    pub chosen: usize,
    /// The whole vault, though the page has logins of its own.
    pub whole: bool,
    /// Where the list is scrolled to, and how much of it shows (arrows
    /// keep the chosen row in view).
    pub scroll_y: f32,
    pub view_h: f32,
}

/// The record on the sheet.
#[derive(Default)]
pub(crate) struct Record {
    pub id: Option<String>,
    pub item: Option<ItemRecord>,
    pub loading: bool,
    /// Secret fields shown as typed, by key.
    pub revealed: HashSet<String>,
    /// The field just copied, and when (the row says so a moment).
    pub copied: Option<(String, Instant)>,
}

/// The new-login form.
#[derive(Default)]
pub(crate) struct NewLogin {
    pub username: String,
    pub password: String,
    pub website: String,
    /// The password is the one the generator made.
    pub generated: bool,
}

impl Drop for NewLogin {
    fn drop(&mut self) {
        self.password.zeroize();
    }
}

/// A page's WebAuthn `get()` or `create()` waiting for the user.
#[derive(Debug, Clone)]
pub(crate) struct PendingPasskey {
    pub req: PasskeyPageRequest,
    /// The same site asking again while the sheet is up (some retry);
    /// answered with `req`, so the page does not fail mid-pick.
    pub extra: Vec<PasskeyPageRequest>,
    /// The asking tab's origin then; the request ends if it leaves it.
    pub page_origin: Option<String>,
    pub candidates: Vec<sola_vault_proto::PasskeyCandidate>,
    pub loading: bool,
    pub error: Option<String>,
    /// How long the site said it waits (`publicKey.timeout`).
    pub timeout: Option<std::time::Duration>,
}

impl PendingPasskey {
    /// The foot's line: how much longer the site waits.
    pub fn wait_line(&self) -> String {
        match self.timeout {
            Some(t) => rows::wait_words(t.saturating_sub(self.req.received.elapsed()).as_secs()),
            None => "The site is waiting".into(),
        }
    }

    pub fn requests(&self) -> impl Iterator<Item = &PasskeyPageRequest> {
        std::iter::once(&self.req).chain(&self.extra)
    }

    pub fn is_create(&self) -> bool {
        self.req.is_create()
    }
}

/// A value the vault put on the clipboard, to take back off it later.
pub(crate) struct Clip {
    pub generation: u64,
    pub value: String,
}

impl Drop for Clip {
    fn drop(&mut self) {
        self.value.zeroize();
    }
}

/// The panel's state and its client.
pub(crate) struct Vault {
    client: VaultHandle<Ask>,
    open: bool,
    /// The daemon's word; `None` until it answers.
    pub status: Option<Status>,
    /// The daemon is away: why, for the sheet.
    pub away: Option<String>,
    pub settings: Settings,
    pub screen: Screen,
    pub busy: bool,
    pub error: Option<String>,
    /// The ··· menu is open.
    pub more: bool,
    pub forms: Forms,
    pub two_step: Option<TwoStep>,
    pub reprompt: Option<RepromptFor>,
    pub reprompt_password: String,
    /// Items whose re-prompt was answered while the sheet stayed up.
    pub reprompt_ok: HashSet<String>,
    pub list: List,
    pub record: Record,
    pub draft: Option<ItemDraft>,
    pub notes: text_editor::Content,
    /// The website whose match-type menu is open (edit).
    pub uri_menu: Option<usize>,
    pub create: Option<NewLogin>,
    pub passkey: Option<PendingPasskey>,
    passkey_seq: u64,
    /// The site's logins, for a passkey `create()`.
    pub site_logins: Vec<ItemSummary>,
    pub site_logins_loading: bool,
    /// A new login's fill waits for the page's word since then.
    pub awaiting_fill: Option<Instant>,
    pub clip: Option<Clip>,
    clip_generation: u64,
    /// The latest list and record asked for: an older answer that comes
    /// in late is dropped.
    list_seq: u64,
    record_seq: u64,
}

/// For `listen_with` (a fn pointer): the sheet is up and has the keys.
static PANEL_OPEN: AtomicBool = AtomicBool::new(false);

impl Vault {
    pub fn new() -> Self {
        Self {
            client: VaultHandle::spawn(),
            open: false,
            status: None,
            away: None,
            settings: Settings::default(),
            screen: Screen::Browse,
            busy: false,
            error: None,
            more: false,
            forms: Forms::default(),
            two_step: None,
            reprompt: None,
            reprompt_password: String::new(),
            reprompt_ok: HashSet::new(),
            list: List::default(),
            record: Record::default(),
            draft: None,
            notes: text_editor::Content::new(),
            uri_menu: None,
            create: None,
            passkey: None,
            passkey_seq: 0,
            site_logins: Vec::new(),
            site_logins_loading: false,
            awaiting_fill: None,
            clip: None,
            clip_generation: 0,
            list_seq: 0,
            record_seq: 0,
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn passkey_pending(&self) -> bool {
        self.passkey.is_some()
    }

    pub fn unlocked(&self) -> bool {
        self.status.as_ref().is_some_and(|s| s.unlocked)
    }

    pub fn gate(&self) -> Gate {
        if self.away.is_some() {
            return Gate::Away;
        }
        match &self.status {
            None => Gate::Connecting,
            Some(s) if s.unlocked => Gate::Open,
            Some(s) if s.signed_in => Gate::Locked,
            Some(_) if self.two_step.is_some() => Gate::TwoStep,
            Some(_) => Gate::SignIn,
        }
    }

    pub(crate) fn ask(&self, ask: Ask, req: sola_vault_proto::Request) {
        self.client.ask(ask, req);
    }

    /// Open or close the sheet. Closing keeps a sign-in, a two-step code
    /// and an unsaved edit for the next open (a click on the page to copy
    /// from it must not lose them); it forgets the record and re-prompts.
    pub fn set_open(&mut self, open: bool) {
        self.open = open;
        PANEL_OPEN.store(open, Ordering::Relaxed);
        self.more = false;
        self.uri_menu = None;
        if open {
            return;
        }
        if self.screen != Screen::Edit {
            self.close_record();
        }
        if self.screen == Screen::CreateSaved {
            self.screen = Screen::Browse;
        }
        self.reprompt = None;
        self.reprompt_password.zeroize();
        self.reprompt_ok.clear();
        self.list.query.clear();
        self.list.whole = false;
        self.list.chosen = 0;
    }

    pub fn close_record(&mut self) {
        self.record = Record::default();
        self.draft = None;
        self.notes = text_editor::Content::new();
        if matches!(self.screen, Screen::Record | Screen::Edit) {
            self.screen = Screen::Browse;
        }
    }

    /// Ask for the list for `url` (empty: no page).
    pub(crate) fn ask_list(&mut self, url: String) {
        self.list_seq += 1;
        self.list.url = url.clone();
        self.list.loading = true;
        let page = (!url.is_empty()).then(|| url.clone());
        let seq = self.list_seq;
        self.ask(Ask::Items { url, seq }, sola_vault_proto::Request::List { url: page });
    }

    /// Ask for record `id`.
    pub(crate) fn ask_record(&mut self, id: String) {
        self.record_seq += 1;
        let seq = self.record_seq;
        self.ask(Ask::Record { id: id.clone(), seq }, sola_vault_proto::Request::Get { id });
    }

    pub fn latest_list(&self, seq: u64) -> bool {
        seq == self.list_seq
    }

    pub fn latest_record(&self, seq: u64) -> bool {
        seq == self.record_seq
    }

    pub fn next_passkey_id(&mut self) -> u64 {
        self.passkey_seq += 1;
        self.passkey_seq
    }

    pub fn next_clip_generation(&mut self) -> u64 {
        self.clip_generation += 1;
        self.clip_generation
    }

    /// The vault locked, signed out or went away: forget what it showed.
    fn forget_open_vault(&mut self) {
        self.list.items.clear();
        self.list.unreadable = 0;
        self.list.loading = false;
        self.list.url.clear();
        self.record = Record::default();
        self.draft = None;
        self.notes = text_editor::Content::new();
        self.create = None;
        self.reprompt = None;
        self.reprompt_password.zeroize();
        self.reprompt_ok.clear();
        self.site_logins.clear();
        if !matches!(self.screen, Screen::PasskeyPick | Screen::PasskeyCreate) {
            self.screen = Screen::Browse;
        }
    }

    /// A 250 ms clock while something on the sheet moves (a code's ring,
    /// a copied mark, a new login's fill).
    pub fn needs_clock(&self) -> bool {
        if self.awaiting_fill.is_some() || self.record.copied.is_some() {
            return true;
        }
        if self.open && self.passkey.is_some() {
            return true;
        }
        self.open
            && self.screen == Screen::Record
            && self
                .record
                .item
                .as_ref()
                .is_some_and(|i| i.totp_secret.is_some() && i.unreadable_totp().is_none())
    }
}

impl Drop for Vault {
    fn drop(&mut self) {
        self.forms.clear_secrets();
        self.reprompt_password.zeroize();
    }
}

/// The page's registrable site for the head and new logins: its host,
/// `None` on a page with no web address (blank, internal).
pub(crate) fn page_host(url: &str) -> Option<String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return None;
    }
    let host = crate::vault::log_host(url);
    (!host.is_empty()).then(|| host.trim_start_matches("www.").to_string())
}
