//! Mail panel — IMAP/SMTP account credentials and per-rule routing
//! (smart mailbox / move). The whole `MailConfig` is the persistent
//! topic: any edit emits a new `Topic::MailConfig` and the bus replay
//! is what refreshes our canonical state.
//!
//! Account is explicit-Save. Rules are list + one detail (edit or
//! draft), matching Applications: dirty state locks selection until
//! Save or Discard.
//!
//! Chrome density inherits kit helpers (`button::labeled`, type roles,
//! `SPACE_*`, field + text_input + select) — no local pad/size snowflakes.

use iced::widget::{button, checkbox, column, container, row, scrollable};
use iced::{Alignment, Element, Length, Padding, Task};
use sola_kit::components::text_input::text_input;

use crate::mail_discover;
use sola_bus::topics::{
    MailAccount, MailConfig, MailRule, MailRuleCondition, Topic, is_catchall_addr, mail_addr_key,
};
use sola_core::Encrypted;
use sola_kit::app::bus;
use sola_kit::components::form::{checkbox_style, form_row};
use sola_kit::components::select::{SelectOption, select};
use sola_kit::components::style::{SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XL, SPACE_XS};
use sola_kit::components::text as kit_text;
use sola_kit::components::{button as kit_btn, card, field, text_input as kit_input};

/// Fixed width of the rules list (px). Detail takes the rest so the
/// condition editor has room.
const LIST_WIDTH: f32 = 280.0;
/// Vertical pad inside a compact rule row (graphite density).
const ROW_PAD: Padding = Padding {
    top: 8.0,
    bottom: 8.0,
    left: 12.0,
    right: 8.0,
};

const ACTION_MOVE: &str = "move";
const ACTION_SMART: &str = "smart_mailbox";
const ACTION_OPTIONS: [&str; 2] = [ACTION_MOVE, ACTION_SMART];
const FIELD_OPTIONS: [&str; 3] = ["from", "to", "subject"];
const MATCH_ADDR: [&str; 4] = ["contains", "equals", "address", "domain"];
const MATCH_TEXT: [&str; 2] = ["contains", "equals"];
/// Canonical IMAP mailboxes a move can target (same names the mail
/// client uses for Archive / Junk / Trash). A saved dest that is not
/// in this list is still offered so custom folders round-trip.
const DEST_OPTIONS: [&str; 5] = ["Archive", "Junk", "Trash", "Sent", "Drafts"];

// ── Drafts ────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct AccountDraft {
    pub email: String,
    pub imap_host: String,
    pub imap_port: String,
    pub smtp_host: String,
    pub smtp_port: String,
    pub username: String,
    pub password: String,
    pub aliases: Vec<String>,
    /// Parallel to `aliases`: shown in Mail's From picker.
    pub alias_show: Vec<bool>,
    pub imap_enabled: bool,
    pub smtp_enabled: bool,
}

impl AccountDraft {
    pub fn from_inbox(cfg: &MailConfig) -> Self {
        Self {
            email: cfg.email.clone(),
            imap_host: cfg.imap_host.clone(),
            imap_port: cfg.imap_port.to_string(),
            smtp_host: cfg.smtp_host.clone(),
            smtp_port: cfg.smtp_port.to_string(),
            username: cfg.username.clone(),
            password: cfg.password.0.clone(),
            aliases: cfg.aliases.clone(),
            alias_show: show_flags(&cfg.aliases, cfg),
            imap_enabled: cfg.imap_enabled,
            smtp_enabled: cfg.smtp_enabled,
        }
    }
    pub fn from_extra(acc: &MailAccount, cfg: &MailConfig) -> Self {
        Self {
            email: acc.email.clone(),
            imap_host: acc.imap_host.clone(),
            imap_port: acc.imap_port.to_string(),
            smtp_host: acc.smtp_host.clone(),
            smtp_port: acc.smtp_port.to_string(),
            username: acc.username.clone(),
            password: acc.password.0.clone(),
            aliases: acc.aliases.clone(),
            alias_show: show_flags(&acc.aliases, cfg),
            imap_enabled: acc.imap_enabled,
            smtp_enabled: acc.smtp_enabled,
        }
    }
    pub fn empty() -> Self {
        Self {
            email: String::new(),
            imap_host: String::new(),
            imap_port: "993".into(),
            smtp_host: String::new(),
            smtp_port: "587".into(),
            username: String::new(),
            password: String::new(),
            aliases: Vec::new(),
            alias_show: Vec::new(),
            imap_enabled: true,
            smtp_enabled: true,
        }
    }
    fn to_extra(&self) -> MailAccount {
        MailAccount {
            email: self.email.trim().to_string(),
            imap_host: self.imap_host.trim().to_string(),
            imap_port: parse_port(&self.imap_port, 993),
            smtp_host: self.smtp_host.trim().to_string(),
            smtp_port: parse_port(&self.smtp_port, 587),
            username: self.username.trim().to_string(),
            password: Encrypted(self.password.clone()),
            aliases: fold_aliases(&self.aliases, &self.alias_show).0,
            imap_enabled: self.imap_enabled,
            smtp_enabled: self.smtp_enabled,
        }
    }
    fn matches_inbox(&self, cfg: &MailConfig) -> bool {
        let (addrs, hidden) = fold_aliases(&self.aliases, &self.alias_show);
        self.email == cfg.email
            && self.imap_host == cfg.imap_host
            && self.imap_port == cfg.imap_port.to_string()
            && self.smtp_host == cfg.smtp_host
            && self.smtp_port == cfg.smtp_port.to_string()
            && self.username == cfg.username
            && self.password == cfg.password.0
            && addrs == clean_addrs(&cfg.aliases)
            && hidden_matches(cfg, &addrs, &hidden)
            && self.imap_enabled == cfg.imap_enabled
            && self.smtp_enabled == cfg.smtp_enabled
    }
    fn matches_extra(&self, acc: &MailAccount, cfg: &MailConfig) -> bool {
        let (addrs, hidden) = fold_aliases(&self.aliases, &self.alias_show);
        self.email == acc.email
            && self.imap_host == acc.imap_host
            && self.imap_port == acc.imap_port.to_string()
            && self.smtp_host == acc.smtp_host
            && self.smtp_port == acc.smtp_port.to_string()
            && self.username == acc.username
            && self.password == acc.password.0
            && addrs == clean_addrs(&acc.aliases)
            && hidden_matches(cfg, &addrs, &hidden)
            && self.imap_enabled == acc.imap_enabled
            && self.smtp_enabled == acc.smtp_enabled
    }
    fn apply_inbox(&self, cfg: &mut MailConfig) {
        cfg.email = self.email.trim().to_string();
        cfg.imap_host = self.imap_host.trim().to_string();
        cfg.imap_port = parse_port(&self.imap_port, 993);
        cfg.smtp_host = self.smtp_host.trim().to_string();
        cfg.smtp_port = parse_port(&self.smtp_port, 587);
        cfg.username = self.username.trim().to_string();
        cfg.password = Encrypted(self.password.clone());
        let (addrs, hidden) = fold_aliases(&self.aliases, &self.alias_show);
        apply_hidden(cfg, &addrs, &hidden);
        cfg.aliases = addrs;
        cfg.imap_enabled = self.imap_enabled;
        cfg.smtp_enabled = self.smtp_enabled;
    }
}

/// Fill empty server/username fields from a known provider. Never
/// overwrite a host the operator already typed.
fn apply_server_hint(d: &mut AccountDraft, prev_email: &str) {
    let Some(hint) = mail_discover::hint_for_email(&d.email) else {
        return;
    };
    apply_hint(d, hint, prev_email);
}

fn apply_hint(d: &mut AccountDraft, hint: mail_discover::ServerHint, prev_email: &str) {
    let email = d.email.trim().to_string();
    if d.username.trim().is_empty() || d.username.trim() == prev_email.trim() {
        d.username = email;
    }
    if d.imap_host.trim().is_empty() {
        d.imap_host = hint.imap_host.to_string();
        d.imap_port = hint.imap_port.to_string();
    }
    if d.smtp_host.trim().is_empty() {
        d.smtp_host = hint.smtp_host.to_string();
        d.smtp_port = hint.smtp_port.to_string();
    }
}

fn lookup_google_mx(domain: String) -> Task<MailMsg> {
    Task::perform(
        async move {
            let query = domain.clone();
            let is_google =
                tokio::task::spawn_blocking(move || mail_discover::domain_mx_is_google(&query))
                    .await
                    .unwrap_or(false);
            (domain, is_google)
        },
        |(domain, is_google)| MailMsg::GoogleMx { domain, is_google },
    )
}

fn clean_addrs(addrs: &[String]) -> Vec<String> {
    fold_aliases(addrs, &[]).0
}

fn show_flags(aliases: &[String], cfg: &MailConfig) -> Vec<bool> {
    aliases.iter().map(|a| !cfg.is_from_hidden(a)).collect()
}

/// Deduped, sorted aliases plus the subset that should stay hidden.
fn fold_aliases(aliases: &[String], show: &[bool]) -> (Vec<String>, Vec<String>) {
    let mut pairs: Vec<(String, bool)> = Vec::new();
    for (i, a) in aliases.iter().enumerate() {
        let t = a.trim();
        if t.is_empty() || is_catchall_addr(t) {
            continue;
        }
        let key = mail_addr_key(t);
        if key.is_empty() || pairs.iter().any(|(e, _)| mail_addr_key(e) == key) {
            continue;
        }
        let on = show.get(i).copied().unwrap_or(true);
        pairs.push((t.to_string(), on));
    }
    pairs.sort_by(|a, b| mail_addr_key(&a.0).cmp(&mail_addr_key(&b.0)));
    let hidden = pairs
        .iter()
        .filter(|(_, on)| !*on)
        .map(|(a, _)| a.clone())
        .collect();
    let addrs = pairs.into_iter().map(|(a, _)| a).collect();
    (addrs, hidden)
}

fn hidden_matches(cfg: &MailConfig, addrs: &[String], hidden: &[String]) -> bool {
    addrs.iter().all(|a| {
        let want = hidden.iter().any(|h| mail_addr_key(h) == mail_addr_key(a));
        cfg.is_from_hidden(a) == want
    })
}

fn apply_hidden(cfg: &mut MailConfig, addrs: &[String], hidden: &[String]) {
    for a in addrs {
        let hide = hidden.iter().any(|h| mail_addr_key(h) == mail_addr_key(a));
        cfg.set_from_hidden(a, hide);
    }
}

fn pad_alias_show(d: &mut AccountDraft) {
    d.alias_show.resize(d.aliases.len(), true);
}

fn uses_host_from_list(idx: usize, is_draft: bool) -> bool {
    idx == 0 && !is_draft
}

fn clear_host_emails(ui: &mut MailState) {
    ui.host_emails = None;
    ui.host_emails_error = None;
    ui.host_emails_loading = false;
    ui.host_emails_gen = ui.host_emails_gen.saturating_add(1);
}

fn begin_host_emails_fetch(ui: &mut MailState, draft: &AccountDraft) -> Task<MailMsg> {
    let host = draft.imap_host.trim();
    let user = draft.username.trim();
    if host.is_empty() || user.is_empty() || draft.password.is_empty() {
        ui.host_emails = None;
        ui.host_emails_loading = false;
        ui.host_emails_error =
            Some("IMAP host, username, and password are needed to load addresses.".into());
        return Task::none();
    }
    let host = host.to_string();
    let user = user.to_string();
    let pass = draft.password.clone();
    ui.host_emails_loading = true;
    ui.host_emails_error = None;
    ui.host_emails_gen = ui.host_emails_gen.saturating_add(1);
    let load_id = ui.host_emails_gen;
    Task::perform(
        async move {
            match tokio::task::spawn_blocking(move || {
                crate::mail_from_api::fetch(&host, &user, &pass)
            })
            .await
            {
                Ok(r) => r,
                Err(e) => Err(e.to_string()),
            }
        },
        move |result| MailMsg::HostEmailsLoaded { load_id, result },
    )
}

fn alias_checked(draft: &AccountDraft, addr: &str) -> bool {
    let key = mail_addr_key(addr);
    draft.aliases.iter().any(|a| mail_addr_key(a) == key)
}

fn account_count(cfg: &MailConfig) -> usize {
    1 + cfg.accounts.len()
}

fn account_at(cfg: &MailConfig, idx: usize) -> Option<AccountDraft> {
    if idx == 0 {
        Some(AccountDraft::from_inbox(cfg))
    } else {
        cfg.accounts
            .get(idx - 1)
            .map(|acc| AccountDraft::from_extra(acc, cfg))
    }
}

fn account_title(draft: &AccountDraft) -> String {
    let e = draft.email.trim();
    if e.is_empty() {
        "Untitled account".into()
    } else {
        e.to_string()
    }
}

fn account_caption(cfg: &MailConfig, idx: usize, draft: &AccountDraft) -> String {
    let mut bits: Vec<&str> = Vec::new();
    if idx == 0 && draft.imap_enabled {
        bits.push("Inbox");
    } else if draft.smtp_enabled && !draft.imap_enabled {
        bits.push("Send only");
    } else if draft.imap_enabled && !draft.smtp_enabled {
        bits.push("Receive only");
    }
    let primary = mail_addr_key(&cfg.primary_from_address());
    let is_primary = mail_addr_key(&draft.email) == primary
        || draft.aliases.iter().any(|a| mail_addr_key(a) == primary);
    if is_primary && !primary.is_empty() {
        bits.push("Default From");
    }
    if bits.is_empty() {
        "Account".into()
    } else {
        bits.join(" · ")
    }
}

#[derive(Debug, Clone)]
pub struct RuleDraft {
    pub name: String,
    pub action: String,
    pub dest: String,
    pub conditions: Vec<CondDraft>,
}

#[derive(Debug, Clone)]
pub struct CondDraft {
    pub field: String,
    pub match_type: String,
    pub value: String,
}

impl RuleDraft {
    fn from_rule(r: &MailRule) -> Self {
        Self {
            name: r.name.clone(),
            action: r.action.clone(),
            dest: r.dest.clone().unwrap_or_default(),
            conditions: r
                .conditions
                .iter()
                .map(|c| CondDraft {
                    field: c.field.clone(),
                    match_type: c.match_type.clone(),
                    value: c.value.clone(),
                })
                .collect(),
        }
    }
    fn matches(&self, r: &MailRule) -> bool {
        if self.name != r.name || self.action != r.action {
            return false;
        }
        let our_dest = dest_wire(&self.action, &self.dest);
        if our_dest.as_deref() != r.dest.as_deref() {
            return false;
        }
        if self.conditions.len() != r.conditions.len() {
            return false;
        }
        self.conditions
            .iter()
            .zip(r.conditions.iter())
            .all(|(a, b)| a.field == b.field && a.match_type == b.match_type && a.value == b.value)
    }
    fn to_rule(&self) -> MailRule {
        MailRule {
            name: self.name.trim().to_string(),
            action: self.action.clone(),
            dest: dest_wire(&self.action, &self.dest),
            conditions: self
                .conditions
                .iter()
                .map(|c| MailRuleCondition {
                    field: c.field.clone(),
                    match_type: c.match_type.clone(),
                    value: c.value.trim().to_string(),
                })
                .collect(),
        }
    }
}

fn dest_wire(action: &str, dest: &str) -> Option<String> {
    if action != ACTION_MOVE {
        return None;
    }
    let d = dest.trim();
    if d.is_empty() {
        None
    } else {
        Some(d.to_string())
    }
}

fn empty_rule() -> RuleDraft {
    RuleDraft {
        name: String::new(),
        action: ACTION_MOVE.into(),
        dest: String::new(),
        conditions: vec![empty_condition()],
    }
}

fn empty_condition() -> CondDraft {
    CondDraft {
        field: "from".into(),
        match_type: "address".into(),
        value: String::new(),
    }
}

fn dest_options(current: &str) -> Vec<String> {
    let mut v: Vec<String> = DEST_OPTIONS.iter().map(|s| (*s).to_string()).collect();
    let cur = current.trim();
    if !cur.is_empty() && !v.iter().any(|x| x.eq_ignore_ascii_case(cur)) {
        v.push(cur.to_string());
    }
    v
}

fn match_options_for(field: &str) -> &'static [&'static str] {
    if field == "subject" {
        &MATCH_TEXT
    } else {
        &MATCH_ADDR
    }
}

fn action_label(action: &str) -> String {
    match action {
        ACTION_MOVE => "Move to mailbox".into(),
        ACTION_SMART => "Smart mailbox".into(),
        other => other.to_string(),
    }
}

fn field_label(field: &str) -> String {
    match field {
        "from" => "From".into(),
        "to" => "To".into(),
        "subject" => "Subject".into(),
        other => other.to_string(),
    }
}

fn match_label(match_type: &str) -> String {
    match match_type {
        "contains" => "contains".into(),
        "equals" => "is".into(),
        "address" => "address is".into(),
        "domain" => "domain is".into(),
        other => other.to_string(),
    }
}

fn action_caption(draft: &RuleDraft) -> String {
    match draft.action.as_str() {
        ACTION_MOVE => {
            let dest = draft.dest.trim();
            if dest.is_empty() {
                "Move — choose a mailbox".into()
            } else {
                format!("Move to {dest}")
            }
        }
        ACTION_SMART => "Smart mailbox".into(),
        other => other.to_string(),
    }
}

fn display_title(draft: &RuleDraft) -> &str {
    let n = draft.name.trim();
    if n.is_empty() { "Untitled rule" } else { n }
}

fn draft_is_dirty(draft: &RuleDraft) -> bool {
    let blank = empty_rule();
    draft.name != blank.name
        || draft.action != blank.action
        || draft.dest.trim() != blank.dest.trim()
        || draft.conditions.len() != blank.conditions.len()
        || draft
            .conditions
            .iter()
            .zip(blank.conditions.iter())
            .any(|(a, b)| a.field != b.field || a.match_type != b.match_type || a.value != b.value)
}

// ── State ─────────────────────────────────────────────────────────

/// Single open editor for the rules list.
#[derive(Debug, Clone, Default)]
pub enum Detail {
    #[default]
    Closed,
    Edit {
        idx: usize,
        draft: RuleDraft,
    },
    Draft(RuleDraft),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenMenu {
    Action,
    Dest,
    CondField(usize),
    CondMatch(usize),
    PrimaryFrom,
}

/// Single open editor for the accounts list (same shape as rules).
#[derive(Debug, Clone, Default)]
pub enum AccountDetail {
    #[default]
    Closed,
    Edit {
        idx: usize,
        draft: AccountDraft,
    },
    Draft(AccountDraft),
}

pub struct MailState {
    pub account: AccountDetail,
    pub detail: Detail,
    pub open_menu: Option<OpenMenu>,
    /// Inline error for the open rule editor (`None` = account errors
    /// live in `account_error`).
    pub error: Option<String>,
    pub account_error: Option<String>,
    pub last_canonical: MailConfig,
    /// Inbox host `/api/auth/me` extras (Wicket). `None` = not loaded.
    host_emails: Option<Vec<String>>,
    host_emails_error: Option<String>,
    host_emails_loading: bool,
    host_emails_gen: u64,
}

impl Default for MailState {
    fn default() -> Self {
        let cfg = MailConfig::default();
        Self {
            account: AccountDetail::Closed,
            detail: Detail::Closed,
            open_menu: None,
            error: None,
            account_error: None,
            last_canonical: cfg,
            host_emails: None,
            host_emails_error: None,
            host_emails_loading: false,
            host_emails_gen: 0,
        }
    }
}

fn account_is_dirty(detail: &AccountDetail, cfg: &MailConfig) -> bool {
    match detail {
        AccountDetail::Closed => false,
        AccountDetail::Draft(d) => account_draft_started(d),
        AccountDetail::Edit { idx, draft } => match account_at(cfg, *idx) {
            Some(canonical) => {
                if *idx == 0 {
                    !draft.matches_inbox(cfg)
                } else {
                    cfg.accounts
                        .get(idx - 1)
                        .is_none_or(|acc| !draft.matches_extra(acc, cfg))
                        || draft.email != canonical.email
                }
            }
            None => true,
        },
    }
}

fn account_draft_started(d: &AccountDraft) -> bool {
    let blank = AccountDraft::empty();
    d.email != blank.email
        || d.imap_host != blank.imap_host
        || d.smtp_host != blank.smtp_host
        || d.username != blank.username
        || d.password != blank.password
        || d.aliases.iter().any(|a| !a.trim().is_empty())
        || d.imap_enabled != blank.imap_enabled
        || d.smtp_enabled != blank.smtp_enabled
}

fn can_leave_account(detail: &AccountDetail, cfg: &MailConfig) -> bool {
    !account_is_dirty(detail, cfg)
}

fn open_account_mut(detail: &mut AccountDetail) -> Option<&mut AccountDraft> {
    match detail {
        AccountDetail::Edit { draft, .. } | AccountDetail::Draft(draft) => Some(draft),
        AccountDetail::Closed => None,
    }
}

fn inbox_has_identity(cfg: &MailConfig) -> bool {
    !cfg.email.trim().is_empty()
        || !cfg.imap_host.trim().is_empty()
        || !cfg.username.trim().is_empty()
}

impl MailState {
    /// Refresh clean drafts to the latest canonical state; preserve
    /// dirty drafts as-is. Called from main.rs on every
    /// `Topic::MailConfig` delivery.
    pub fn sync_from_canonical(&mut self, next: &MailConfig) {
        let first_real = !inbox_has_identity(&self.last_canonical)
            && self.last_canonical.accounts.is_empty()
            && inbox_has_identity(next);
        if !account_is_dirty(&self.account, &self.last_canonical) {
            match &self.account {
                AccountDetail::Draft(_) => {}
                AccountDetail::Edit { idx, .. } => {
                    if let Some(d) = account_at(next, *idx) {
                        self.account = AccountDetail::Edit {
                            idx: *idx,
                            draft: d,
                        };
                    } else {
                        self.account = AccountDetail::Closed;
                    }
                }
                AccountDetail::Closed => {
                    if first_real {
                        if let Some(d) = account_at(next, 0) {
                            self.account = AccountDetail::Edit { idx: 0, draft: d };
                        }
                    }
                }
            }
        }
        match &self.detail {
            Detail::Closed | Detail::Draft(_) => {}
            Detail::Edit { idx, draft } => {
                let idx = *idx;
                if idx >= next.rules.len() {
                    self.detail = Detail::Closed;
                    self.error = None;
                    self.open_menu = None;
                } else if idx < self.last_canonical.rules.len()
                    && draft.matches(&self.last_canonical.rules[idx])
                {
                    self.detail = Detail::Edit {
                        idx,
                        draft: RuleDraft::from_rule(&next.rules[idx]),
                    };
                    self.open_menu = None;
                }
            }
        }
        self.last_canonical = next.clone();
    }
}

fn can_leave_detail(detail: &Detail, cfg: &MailConfig) -> bool {
    match detail {
        Detail::Closed => true,
        Detail::Draft(draft) => !draft_is_dirty(draft),
        Detail::Edit { idx, draft } => match cfg.rules.get(*idx) {
            Some(canonical) => draft.matches(canonical),
            None => true,
        },
    }
}

fn open_draft_mut(detail: &mut Detail) -> Option<&mut RuleDraft> {
    match detail {
        Detail::Edit { draft, .. } | Detail::Draft(draft) => Some(draft),
        Detail::Closed => None,
    }
}

// ── Messages ──────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum MailMsg {
    AccountField(AccountField, String),
    AliasField(usize, String),
    AliasShow(usize, bool),
    AddAlias,
    RemoveAlias(usize),
    HostEmailShow(String, bool),
    HostEmailsLoaded {
        load_id: u64,
        result: Result<Vec<String>, String>,
    },
    RetryHostEmails,
    GoogleMx {
        domain: String,
        is_google: bool,
    },
    SelectAccount(usize),
    StartNewAccount,
    AccountSave,
    AccountRevert,
    AccountRemove,
    CloseAccount,
    ToggleImap(bool),
    ToggleSmtp(bool),
    PickPrimary(String),
    SelectRule(usize),
    StartNew,
    Field(RuleField, String),
    PickAction(String),
    PickDest(String),
    CondValue(usize, String),
    PickCondField(usize, String),
    PickCondMatch(usize, String),
    AddCondition,
    RemoveCondition(usize),
    ToggleMenu(OpenMenu),
    DismissMenu,
    Save,
    Discard,
    CloseDetail,
    Remove,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountField {
    Email,
    ImapHost,
    ImapPort,
    SmtpHost,
    SmtpPort,
    Username,
    Password,
}

#[derive(Debug, Clone, Copy)]
pub enum RuleField {
    Name,
}

// ── Update ────────────────────────────────────────────────────────

pub fn update(msg: MailMsg, cfg: &mut MailConfig, ui: &mut MailState) -> Task<MailMsg> {
    match msg {
        MailMsg::AccountField(f, v) => {
            ui.account_error = None;
            let mut mx_domain = None;
            if let Some(d) = open_account_mut(&mut ui.account) {
                match f {
                    AccountField::Email => {
                        let prev = d.email.clone();
                        d.email = v;
                        apply_server_hint(d, &prev);
                        if mail_discover::hint_for_email(&d.email).is_none() {
                            mx_domain = mail_discover::domain(&d.email);
                        }
                    }
                    AccountField::ImapHost => d.imap_host = v,
                    AccountField::ImapPort => d.imap_port = v,
                    AccountField::SmtpHost => d.smtp_host = v,
                    AccountField::SmtpPort => d.smtp_port = v,
                    AccountField::Username => d.username = v,
                    AccountField::Password => d.password = v,
                }
            }
            if let Some(domain) = mx_domain {
                return lookup_google_mx(domain);
            }
        }
        MailMsg::AliasField(i, v) => {
            ui.account_error = None;
            if let Some(d) = open_account_mut(&mut ui.account) {
                pad_alias_show(d);
                if let Some(a) = d.aliases.get_mut(i) {
                    *a = v;
                }
            }
        }
        MailMsg::AliasShow(i, on) => {
            ui.account_error = None;
            if let Some(d) = open_account_mut(&mut ui.account) {
                pad_alias_show(d);
                if let Some(s) = d.alias_show.get_mut(i) {
                    *s = on;
                }
            }
        }
        MailMsg::AddAlias => {
            ui.account_error = None;
            if let Some(d) = open_account_mut(&mut ui.account) {
                d.aliases.push(String::new());
                d.alias_show.push(true);
            }
        }
        MailMsg::RemoveAlias(i) => {
            ui.account_error = None;
            if let Some(d) = open_account_mut(&mut ui.account) {
                pad_alias_show(d);
                if i < d.aliases.len() {
                    d.aliases.remove(i);
                    if i < d.alias_show.len() {
                        d.alias_show.remove(i);
                    }
                }
            }
        }
        MailMsg::HostEmailShow(addr, on) => {
            ui.account_error = None;
            if let Some(d) = open_account_mut(&mut ui.account) {
                let key = mail_addr_key(&addr);
                if on {
                    if !d.aliases.iter().any(|a| mail_addr_key(a) == key) {
                        d.aliases.push(addr);
                        d.alias_show.push(true);
                    }
                } else {
                    if let Some(i) = d.aliases.iter().position(|a| mail_addr_key(a) == key) {
                        d.aliases.remove(i);
                        if i < d.alias_show.len() {
                            d.alias_show.remove(i);
                        }
                    }
                }
            }
        }
        MailMsg::HostEmailsLoaded { load_id, result } => {
            if load_id != ui.host_emails_gen {
                return Task::none();
            }
            ui.host_emails_loading = false;
            match result {
                Ok(raw) => {
                    let email = match &ui.account {
                        AccountDetail::Edit { draft, .. } | AccountDetail::Draft(draft) => {
                            draft.email.clone()
                        }
                        AccountDetail::Closed => String::new(),
                    };
                    let list = crate::mail_from_api::prepare_from_list(&raw, &email);
                    if let Some(d) = open_account_mut(&mut ui.account) {
                        d.aliases
                            .retain(|a| list.iter().any(|e| mail_addr_key(e) == mail_addr_key(a)));
                        d.alias_show.resize(d.aliases.len(), true);
                    }
                    ui.host_emails = Some(list);
                    ui.host_emails_error = None;
                }
                Err(e) => {
                    ui.host_emails = None;
                    ui.host_emails_error = Some(e);
                }
            }
        }
        MailMsg::GoogleMx { domain, is_google } => {
            if !is_google {
                return Task::none();
            }
            if let Some(d) = open_account_mut(&mut ui.account) {
                if mail_discover::domain(&d.email).as_deref() == Some(domain.as_str()) {
                    let email = d.email.clone();
                    apply_hint(d, mail_discover::GMAIL, &email);
                }
            }
        }
        MailMsg::RetryHostEmails => {
            if let AccountDetail::Edit { idx, draft } = &ui.account {
                if uses_host_from_list(*idx, false) {
                    let draft = draft.clone();
                    return begin_host_emails_fetch(ui, &draft);
                }
            }
        }
        MailMsg::SelectAccount(idx) => {
            if !can_leave_account(&ui.account, cfg) {
                ui.account_error = Some("Save or revert this account first".into());
                return Task::none();
            }
            if let Some(d) = account_at(cfg, idx) {
                ui.account = AccountDetail::Edit {
                    idx,
                    draft: d.clone(),
                };
                ui.account_error = None;
                ui.open_menu = None;
                if uses_host_from_list(idx, false) {
                    return begin_host_emails_fetch(ui, &d);
                }
                clear_host_emails(ui);
            }
        }
        MailMsg::StartNewAccount => {
            if !can_leave_account(&ui.account, cfg) {
                ui.account_error = Some("Save or revert this account first".into());
                return Task::none();
            }
            ui.account = AccountDetail::Draft(AccountDraft::empty());
            ui.account_error = None;
            ui.open_menu = None;
            clear_host_emails(ui);
        }
        MailMsg::AccountSave => match &ui.account {
            AccountDetail::Closed => {}
            AccountDetail::Draft(draft) => {
                let draft = draft.clone();
                if draft.email.trim().is_empty() {
                    ui.account_error = Some("Email is required".into());
                    return Task::none();
                }
                if draft.aliases.iter().any(|a| is_catchall_addr(a)) {
                    ui.account_error =
                        Some("Catch-all addresses (*@…) can't be a From identity.".into());
                    return Task::none();
                }
                let extra = draft.to_extra();
                let (addrs, hidden) = fold_aliases(&draft.aliases, &draft.alias_show);
                apply_hidden(cfg, &addrs, &hidden);
                cfg.accounts.push(extra);
                if cfg.primary_from.trim().is_empty() {
                    cfg.primary_from = cfg.email.clone();
                }
                let idx = cfg.accounts.len();
                ui.account = AccountDetail::Edit {
                    idx,
                    draft: account_at(cfg, idx).unwrap_or(draft),
                };
                ui.last_canonical = cfg.clone();
                ui.account_error = None;
                emit(Topic::MailConfig(cfg.clone()));
            }
            AccountDetail::Edit { idx, draft } => {
                let idx = *idx;
                let draft = draft.clone();
                if draft.email.trim().is_empty() {
                    ui.account_error = Some("Email is required".into());
                    return Task::none();
                }
                if draft.aliases.iter().any(|a| is_catchall_addr(a)) {
                    ui.account_error =
                        Some("Catch-all addresses (*@…) can't be a From identity.".into());
                    return Task::none();
                }
                if idx == 0 {
                    draft.apply_inbox(cfg);
                } else if idx - 1 < cfg.accounts.len() {
                    let (addrs, hidden) = fold_aliases(&draft.aliases, &draft.alias_show);
                    apply_hidden(cfg, &addrs, &hidden);
                    cfg.accounts[idx - 1] = draft.to_extra();
                } else {
                    ui.account_error = Some("account is no longer in the list".into());
                    return Task::none();
                }
                if cfg.primary_from.trim().is_empty()
                    || !cfg
                        .from_addresses()
                        .iter()
                        .any(|a| mail_addr_key(a) == mail_addr_key(&cfg.primary_from))
                {
                    cfg.primary_from = cfg.email.clone();
                }
                ui.account = AccountDetail::Edit {
                    idx,
                    draft: account_at(cfg, idx).unwrap_or(draft.clone()),
                };
                ui.last_canonical = cfg.clone();
                ui.account_error = None;
                emit(Topic::MailConfig(cfg.clone()));
                if uses_host_from_list(idx, false) {
                    if let Some(d) = account_at(cfg, idx) {
                        return begin_host_emails_fetch(ui, &d);
                    }
                }
            }
        },
        MailMsg::AccountRevert => match &ui.account {
            AccountDetail::Closed => {}
            AccountDetail::Draft(_) => {
                ui.account = AccountDetail::Closed;
                ui.account_error = None;
            }
            AccountDetail::Edit { idx, .. } => {
                let idx = *idx;
                if let Some(d) = account_at(cfg, idx) {
                    ui.account = AccountDetail::Edit {
                        idx,
                        draft: d.clone(),
                    };
                    ui.account_error = None;
                    if uses_host_from_list(idx, false) {
                        return begin_host_emails_fetch(ui, &d);
                    }
                }
                ui.account_error = None;
            }
        },
        MailMsg::CloseAccount => {
            if !can_leave_account(&ui.account, cfg) {
                return Task::none();
            }
            ui.account = AccountDetail::Closed;
            ui.account_error = None;
            ui.open_menu = None;
            clear_host_emails(ui);
        }
        MailMsg::AccountRemove => match &ui.account {
            AccountDetail::Draft(_) => {
                ui.account = AccountDetail::Closed;
                ui.account_error = None;
            }
            AccountDetail::Edit { idx, .. } if *idx == 0 => {
                ui.account_error = Some("The inbox account cannot be removed".into());
            }
            AccountDetail::Edit { idx, .. } => {
                let extra_idx = idx - 1;
                if extra_idx < cfg.accounts.len() {
                    let removed = cfg.accounts.remove(extra_idx);
                    let gone = mail_addr_key(&removed.email);
                    if mail_addr_key(&cfg.primary_from) == gone
                        || removed
                            .aliases
                            .iter()
                            .any(|a| mail_addr_key(a) == mail_addr_key(&cfg.primary_from))
                    {
                        cfg.primary_from = cfg.email.clone();
                    }
                    ui.account = AccountDetail::Closed;
                    ui.last_canonical = cfg.clone();
                    ui.account_error = None;
                    emit(Topic::MailConfig(cfg.clone()));
                }
            }
            AccountDetail::Closed => {}
        },
        MailMsg::ToggleImap(on) => {
            ui.account_error = None;
            if let Some(d) = open_account_mut(&mut ui.account) {
                d.imap_enabled = on;
            }
        }
        MailMsg::ToggleSmtp(on) => {
            ui.account_error = None;
            if let Some(d) = open_account_mut(&mut ui.account) {
                d.smtp_enabled = on;
            }
        }
        MailMsg::PickPrimary(addr) => {
            ui.open_menu = None;
            cfg.primary_from = addr;
            ui.last_canonical = cfg.clone();
            emit(Topic::MailConfig(cfg.clone()));
        }
        MailMsg::SelectRule(idx) => {
            if !can_leave_detail(&ui.detail, cfg) {
                return Task::none();
            }
            if let Some(rule) = cfg.rules.get(idx) {
                ui.detail = Detail::Edit {
                    idx,
                    draft: RuleDraft::from_rule(rule),
                };
                ui.error = None;
                ui.open_menu = None;
            }
        }
        MailMsg::StartNew => {
            if !can_leave_detail(&ui.detail, cfg) {
                return Task::none();
            }
            ui.detail = Detail::Draft(empty_rule());
            ui.error = None;
            ui.open_menu = None;
        }
        MailMsg::Field(RuleField::Name, v) => {
            ui.error = None;
            if let Some(draft) = open_draft_mut(&mut ui.detail) {
                draft.name = v;
            }
        }
        MailMsg::PickAction(action) => {
            ui.error = None;
            ui.open_menu = None;
            if let Some(draft) = open_draft_mut(&mut ui.detail) {
                draft.action = action;
            }
        }
        MailMsg::PickDest(dest) => {
            ui.error = None;
            ui.open_menu = None;
            if let Some(draft) = open_draft_mut(&mut ui.detail) {
                draft.dest = dest;
            }
        }
        MailMsg::CondValue(i, v) => {
            ui.error = None;
            if let Some(draft) = open_draft_mut(&mut ui.detail) {
                if let Some(c) = draft.conditions.get_mut(i) {
                    c.value = v;
                }
            }
        }
        MailMsg::PickCondField(i, field) => {
            ui.error = None;
            ui.open_menu = None;
            if let Some(draft) = open_draft_mut(&mut ui.detail) {
                if let Some(c) = draft.conditions.get_mut(i) {
                    c.field = field;
                    let allowed = match_options_for(&c.field);
                    if !allowed.contains(&c.match_type.as_str()) {
                        c.match_type = allowed[0].to_string();
                    }
                }
            }
        }
        MailMsg::PickCondMatch(i, match_type) => {
            ui.error = None;
            ui.open_menu = None;
            if let Some(draft) = open_draft_mut(&mut ui.detail) {
                if let Some(c) = draft.conditions.get_mut(i) {
                    c.match_type = match_type;
                }
            }
        }
        MailMsg::AddCondition => {
            ui.error = None;
            if let Some(draft) = open_draft_mut(&mut ui.detail) {
                draft.conditions.push(empty_condition());
            }
        }
        MailMsg::RemoveCondition(i) => {
            ui.error = None;
            ui.open_menu = None;
            if let Some(draft) = open_draft_mut(&mut ui.detail) {
                if i < draft.conditions.len() {
                    draft.conditions.remove(i);
                }
            }
        }
        MailMsg::ToggleMenu(menu) => {
            ui.open_menu = if ui.open_menu == Some(menu) {
                None
            } else {
                Some(menu)
            };
        }
        MailMsg::DismissMenu => ui.open_menu = None,
        MailMsg::Save => match &ui.detail {
            Detail::Edit { idx, draft } => {
                let idx = *idx;
                let draft = draft.clone();
                if let Err(e) = validate(&draft) {
                    ui.error = Some(e);
                    return Task::none();
                }
                if idx >= cfg.rules.len() {
                    ui.error = Some("rule is no longer in the list".into());
                    return Task::none();
                }
                cfg.rules[idx] = draft.to_rule();
                ui.last_canonical = cfg.clone();
                ui.detail = Detail::Edit {
                    idx,
                    draft: RuleDraft::from_rule(&cfg.rules[idx]),
                };
                ui.error = None;
                ui.open_menu = None;
                emit(Topic::MailConfig(cfg.clone()));
            }
            Detail::Draft(draft) => {
                let draft = draft.clone();
                if let Err(e) = validate(&draft) {
                    ui.error = Some(e);
                    return Task::none();
                }
                cfg.rules.push(draft.to_rule());
                ui.last_canonical = cfg.clone();
                let idx = cfg.rules.len() - 1;
                ui.detail = Detail::Edit {
                    idx,
                    draft: RuleDraft::from_rule(&cfg.rules[idx]),
                };
                ui.error = None;
                ui.open_menu = None;
                emit(Topic::MailConfig(cfg.clone()));
            }
            Detail::Closed => {}
        },
        MailMsg::Discard => match &ui.detail {
            Detail::Edit { idx, .. } => {
                let idx = *idx;
                if let Some(rule) = cfg.rules.get(idx) {
                    ui.detail = Detail::Edit {
                        idx,
                        draft: RuleDraft::from_rule(rule),
                    };
                }
                ui.error = None;
                ui.open_menu = None;
            }
            Detail::Draft(_) => {
                ui.detail = Detail::Closed;
                ui.error = None;
                ui.open_menu = None;
            }
            Detail::Closed => {}
        },
        MailMsg::CloseDetail => {
            if !can_leave_detail(&ui.detail, cfg) {
                return Task::none();
            }
            ui.detail = Detail::Closed;
            ui.error = None;
            ui.open_menu = None;
        }
        MailMsg::Remove => {
            if let Detail::Edit { idx, .. } = ui.detail {
                if idx < cfg.rules.len() {
                    cfg.rules.remove(idx);
                    ui.last_canonical = cfg.clone();
                    ui.detail = Detail::Closed;
                    ui.error = None;
                    ui.open_menu = None;
                    emit(Topic::MailConfig(cfg.clone()));
                }
            }
        }
    }
    Task::none()
}

// ── View ──────────────────────────────────────────────────────────

pub fn view<'a>(cfg: &'a MailConfig, ui: &'a MailState) -> Element<'a, MailMsg> {
    column![accounts_section(cfg, ui), rules_section(cfg, ui)]
        .spacing(SPACE_XL + SPACE_MD)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn accounts_section<'a>(cfg: &'a MailConfig, ui: &'a MailState) -> Element<'a, MailMsg> {
    let primary_open = ui.open_menu == Some(OpenMenu::PrimaryFrom);
    let identities = cfg.from_addresses();
    let primary = cfg.primary_from_address();
    let primary_key = mail_addr_key(&primary);
    let primary_opts = identities.iter().map(|addr| {
        SelectOption::new(
            addr.clone(),
            mail_addr_key(addr) == primary_key,
            MailMsg::PickPrimary(addr.clone()),
        )
        .mark(addr.clone())
    });
    let primary_label = if primary.is_empty() {
        "Choose a default From".to_string()
    } else {
        primary.clone()
    };
    let primary_select = select(
        primary_label,
        primary_opts,
        primary_open,
        MailMsg::ToggleMenu(OpenMenu::PrimaryFrom),
        MailMsg::DismissMenu,
    );

    let header = column![
        kit_text::subheading("Accounts"),
        kit_text::caption("Inbox receives mail. Extra accounts are send identities for replies.")
            .style(kit_text::muted),
        field(
            "Default From",
            primary_select,
            Some("New messages use this address."),
            None,
        ),
    ]
    .spacing(SPACE_XS);

    let split = row![accounts_list(cfg, ui), accounts_detail(cfg, ui)]
        .spacing(SPACE_XL)
        .width(Length::Fill)
        .height(Length::Fill);

    column![header, split]
        .spacing(SPACE_LG)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn accounts_list<'a>(cfg: &'a MailConfig, ui: &'a MailState) -> Element<'a, MailMsg> {
    let n = account_count(cfg);
    let toolbar = row![
        kit_btn::labeled_sm("+ Add account", kit_btn::ghost).on_press(MailMsg::StartNewAccount),
        iced::widget::Space::new().width(Length::Fill),
        kit_text::caption(format!(
            "{n} {}",
            if n == 1 { "account" } else { "accounts" }
        ))
        .style(kit_text::muted),
    ]
    .spacing(SPACE_MD)
    .align_y(Alignment::Center)
    .width(Length::Fill);

    let mut rows = column![].spacing(0).width(Length::Fill);
    for idx in 0..n {
        if let Some(d) = account_at(cfg, idx) {
            let selected =
                matches!(ui.account, AccountDetail::Edit { idx: open, .. } if open == idx);
            rows = rows.push(account_row(
                idx,
                account_title(&d),
                account_caption(cfg, idx, &d),
                selected,
            ));
        }
    }
    if let AccountDetail::Draft(d) = &ui.account {
        let label = column![
            kit_text::body(account_title(d)),
            kit_text::caption("New account").style(kit_text::muted),
        ]
        .spacing(SPACE_XS);
        rows = rows.push(
            button(label)
                .on_press(MailMsg::StartNewAccount)
                .padding(ROW_PAD)
                .width(Length::Fill)
                .style(kit_btn::list_item(true)),
        );
    }

    let list_card = card(
        column![
            toolbar,
            scrollable(rows).height(Length::Fill).width(Length::Fill)
        ]
        .spacing(SPACE_MD)
        .width(Length::Fill)
        .height(Length::Fill),
    )
    .width(Length::Fill)
    .height(Length::Fill);

    container(list_card)
        .width(Length::Fixed(LIST_WIDTH))
        .height(Length::Fill)
        .into()
}

fn accounts_detail<'a>(cfg: &'a MailConfig, ui: &'a MailState) -> Element<'a, MailMsg> {
    let body: Element<'a, MailMsg> = match &ui.account {
        AccountDetail::Closed => container(
            column![
                kit_text::subheading("No selection").style(kit_text::muted),
                kit_text::caption("Select an account, or add one.").style(kit_text::muted),
            ]
            .spacing(SPACE_SM)
            .align_x(Alignment::Center),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into(),
        AccountDetail::Edit { idx, draft } => {
            let dirty = account_is_dirty(&ui.account, cfg);
            account_editor(draft, ui, dirty, /* is_draft */ false, *idx)
        }
        AccountDetail::Draft(draft) => {
            account_editor(draft, ui, true, /* is_draft */ true, 0)
        }
    };
    container(card(body).width(Length::Fill).height(Length::Fill))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn send_from_editor<'a>(
    draft: &'a AccountDraft,
    ui: &'a MailState,
    idx: usize,
    is_draft: bool,
) -> Element<'a, MailMsg> {
    if uses_host_from_list(idx, is_draft) {
        return host_from_editor(draft, ui);
    }
    extra_from_editor(draft)
}

fn host_from_editor<'a>(draft: &'a AccountDraft, ui: &'a MailState) -> Element<'a, MailMsg> {
    let mut col = column![
        kit_text::caption("Send from").style(kit_text::muted),
        kit_text::caption("Addresses on this server. Check the ones Mail should offer as From.")
            .style(kit_text::muted),
    ]
    .spacing(SPACE_SM)
    .width(Length::Fill);

    if ui.host_emails_loading {
        col = col.push(kit_text::caption("Loading addresses…").style(kit_text::muted));
        if ui.host_emails.is_none() {
            return col.into();
        }
    }
    if let Some(err) = ui.host_emails_error.as_deref() {
        col = col
            .push(kit_text::caption(err).style(kit_text::danger))
            .push(kit_btn::labeled_sm("Retry", kit_btn::ghost).on_press(MailMsg::RetryHostEmails));
    }

    let rows: Vec<String> = if let Some(list) = &ui.host_emails {
        list.clone()
    } else {
        crate::mail_from_api::prepare_from_list(&draft.aliases, &draft.email)
    };
    if rows.is_empty() && ui.host_emails_error.is_none() && !ui.host_emails_loading {
        col = col
            .push(kit_text::caption("No extra addresses on this server.").style(kit_text::muted));
        return col.into();
    }
    for addr in rows {
        let on = alias_checked(draft, &addr);
        let label = addr.clone();
        col = col.push(
            row![
                checkbox(on)
                    .on_toggle({
                        let addr = addr.clone();
                        move |v| MailMsg::HostEmailShow(addr.clone(), v)
                    })
                    .style(checkbox_style),
                kit_text::body(label),
            ]
            .spacing(SPACE_SM)
            .align_y(Alignment::Center),
        );
    }
    col.into()
}

fn extra_from_editor<'a>(draft: &'a AccountDraft) -> Element<'a, MailMsg> {
    let mut aliases = column![
        kit_text::caption("Send from — extra addresses on this SMTP").style(kit_text::muted),
        kit_text::caption(
            "Check the ones Mail should offer as From. Catch-alls like *@moonlight.pm are not allowed."
        )
        .style(kit_text::muted),
    ]
    .spacing(SPACE_SM)
    .width(Length::Fill);
    if draft.aliases.is_empty() {
        aliases = aliases.push(
            kit_text::caption("Same as Email unless you add another address.")
                .style(kit_text::muted),
        );
    }
    for (i, alias) in draft.aliases.iter().enumerate() {
        let shown = draft.alias_show.get(i).copied().unwrap_or(true);
        let input = text_input("alias@example.com", alias)
            .id(alias_id(i))
            .on_input(move |v| MailMsg::AliasField(i, v))
            .size(13)
            .style(kit_input::style)
            .width(Length::Fill);
        aliases = aliases.push(
            row![
                checkbox(shown)
                    .on_toggle(move |on| MailMsg::AliasShow(i, on))
                    .style(checkbox_style),
                input,
                kit_btn::labeled_sm("Remove", kit_btn::ghost).on_press(MailMsg::RemoveAlias(i)),
            ]
            .spacing(SPACE_SM)
            .align_y(Alignment::Center),
        );
    }
    aliases
        .push(kit_btn::labeled_sm("+ Add address", kit_btn::ghost).on_press(MailMsg::AddAlias))
        .into()
}

fn account_editor<'a>(
    draft: &'a AccountDraft,
    ui: &'a MailState,
    dirty: bool,
    is_draft: bool,
    idx: usize,
) -> Element<'a, MailMsg> {
    let title = if is_draft {
        "New account".to_string()
    } else {
        account_title(draft)
    };
    let subtitle = if is_draft || idx > 0 {
        "Uncheck IMAP for send-only (mail forwarded into another inbox)."
    } else {
        "IMAP receive + SMTP send. Extra addresses on this SMTP go under Send from."
    };

    let imap_row = form_row(
        "IMAP",
        checkbox(draft.imap_enabled)
            .on_toggle(MailMsg::ToggleImap)
            .style(checkbox_style),
    );
    let smtp_row = form_row(
        "SMTP",
        checkbox(draft.smtp_enabled)
            .on_toggle(MailMsg::ToggleSmtp)
            .style(checkbox_style),
    );

    let hosts = row![
        account_input("IMAP host", &draft.imap_host, AccountField::ImapHost),
        container(account_input(
            "IMAP port",
            &draft.imap_port,
            AccountField::ImapPort,
        ))
        .width(Length::Fixed(120.0)),
    ]
    .spacing(SPACE_MD);

    let smtp = row![
        account_input("SMTP host", &draft.smtp_host, AccountField::SmtpHost),
        container(account_input(
            "SMTP port",
            &draft.smtp_port,
            AccountField::SmtpPort,
        ))
        .width(Length::Fixed(120.0)),
    ]
    .spacing(SPACE_MD);

    let aliases = send_from_editor(draft, ui, idx, is_draft);

    let mut footer = row![].spacing(SPACE_SM).align_y(Alignment::Center);
    if is_draft {
        footer = footer
            .push(kit_btn::labeled_sm("Add", kit_btn::primary).on_press(MailMsg::AccountSave))
            .push(kit_btn::labeled_sm("Discard", kit_btn::ghost).on_press(MailMsg::AccountRevert));
    } else {
        if dirty {
            footer = footer
                .push(kit_btn::labeled_sm("Save", kit_btn::primary).on_press(MailMsg::AccountSave))
                .push(
                    kit_btn::labeled_sm("Discard", kit_btn::ghost).on_press(MailMsg::AccountRevert),
                );
        }
        if idx > 0 {
            footer = footer
                .push(iced::widget::Space::new().width(Length::Fill))
                .push(
                    kit_btn::labeled_sm("Remove", kit_btn::danger_outline)
                        .on_press(MailMsg::AccountRemove),
                );
        } else {
            footer = footer.push(iced::widget::Space::new().width(Length::Fill));
        }
        if !dirty {
            footer = footer
                .push(kit_btn::labeled_sm("Close", kit_btn::ghost).on_press(MailMsg::CloseAccount));
        }
    }

    let email_help = Some("Known providers fill IMAP and SMTP from this address.");
    let password_help = if mail_discover::needs_app_password(&draft.email)
        || mail_discover::uses_gmail_servers(&draft.imap_host)
    {
        Some("Use an app password, not the account password.")
    } else {
        None
    };

    let mut fields = column![
        account_input_help("Email", &draft.email, AccountField::Email, email_help),
        imap_row,
        hosts,
        smtp_row,
        smtp,
        account_input("Username", &draft.username, AccountField::Username),
        password_input("Password", &draft.password, password_help),
        aliases,
    ]
    .spacing(SPACE_LG);
    if let Some(err) = ui.account_error.as_deref() {
        fields = fields.push(kit_text::caption(err).style(kit_text::danger));
    }

    column![
        kit_text::subheading(title),
        kit_text::caption(subtitle).style(kit_text::muted),
        scrollable(fields).width(Length::Fill).height(Length::Fill),
        footer,
    ]
    .spacing(SPACE_MD)
    .height(Length::Fill)
    .into()
}

fn account_row<'a>(
    idx: usize,
    title: String,
    caption: String,
    selected: bool,
) -> Element<'a, MailMsg> {
    let label = column![
        kit_text::body(title),
        kit_text::caption(caption).style(kit_text::muted),
    ]
    .spacing(SPACE_XS);
    button(label)
        .on_press(MailMsg::SelectAccount(idx))
        .padding(ROW_PAD)
        .width(Length::Fill)
        .style(kit_btn::list_item(selected))
        .into()
}

fn rules_section<'a>(cfg: &'a MailConfig, ui: &'a MailState) -> Element<'a, MailMsg> {
    let header = column![
        kit_text::subheading("Rules"),
        kit_text::caption("Matching incoming mail is filed automatically.").style(kit_text::muted),
    ]
    .spacing(SPACE_XS);

    let split = row![rules_list(cfg, ui), rules_detail(cfg, ui)]
        .spacing(SPACE_XL)
        .width(Length::Fill)
        .height(Length::Fill);

    column![header, split]
        .spacing(SPACE_LG)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn rules_list<'a>(cfg: &'a MailConfig, ui: &'a MailState) -> Element<'a, MailMsg> {
    let toolbar = row![
        kit_btn::labeled_sm("+ Add rule", kit_btn::ghost).on_press(MailMsg::StartNew),
        iced::widget::Space::new().width(Length::Fill),
        kit_text::caption(format!(
            "{} {}",
            cfg.rules.len(),
            if cfg.rules.len() == 1 {
                "rule"
            } else {
                "rules"
            }
        ))
        .style(kit_text::muted),
    ]
    .spacing(SPACE_MD)
    .align_y(Alignment::Center)
    .width(Length::Fill);

    let list_body: Element<'a, MailMsg> =
        if cfg.rules.is_empty() && matches!(ui.detail, Detail::Closed) {
            container(
                column![
                    kit_text::body("No rules yet").style(kit_text::muted),
                    kit_text::caption("Add one to file incoming mail.").style(kit_text::muted),
                ]
                .spacing(SPACE_SM),
            )
            .padding(SPACE_LG)
            .width(Length::Fill)
            .into()
        } else if cfg.rules.is_empty() {
            container(iced::widget::Space::new())
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        } else {
            let mut rows = column![].spacing(0).width(Length::Fill);
            for (idx, rule) in cfg.rules.iter().enumerate() {
                rows = rows.push(rule_row(idx, rule, ui));
            }
            scrollable(rows)
                .height(Length::Fill)
                .width(Length::Fill)
                .into()
        };

    let list_card = card(
        column![toolbar, list_body]
            .spacing(SPACE_MD)
            .width(Length::Fill)
            .height(Length::Fill),
    )
    .width(Length::Fill)
    .height(Length::Fill);

    container(list_card)
        .width(Length::Fixed(LIST_WIDTH))
        .height(Length::Fill)
        .into()
}

fn rule_row<'a>(idx: usize, rule: &'a MailRule, ui: &'a MailState) -> Element<'a, MailMsg> {
    let selected = matches!(ui.detail, Detail::Edit { idx: open, .. } if open == idx);
    let draft = RuleDraft::from_rule(rule);
    let title = display_title(&draft);
    let caption = action_caption(&draft);

    let label = column![
        kit_text::body(title.to_string()),
        kit_text::caption(caption).style(kit_text::muted),
    ]
    .spacing(SPACE_XS);

    button(label)
        .on_press(MailMsg::SelectRule(idx))
        .padding(ROW_PAD)
        .width(Length::Fill)
        .style(kit_btn::list_item(selected))
        .into()
}

fn rules_detail<'a>(cfg: &'a MailConfig, ui: &'a MailState) -> Element<'a, MailMsg> {
    let body: Element<'a, MailMsg> = match &ui.detail {
        Detail::Closed => container(
            column![
                kit_text::subheading("No selection").style(kit_text::muted),
                kit_text::caption("Select a rule, or add one.").style(kit_text::muted),
            ]
            .spacing(SPACE_SM)
            .align_x(Alignment::Center),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into(),
        Detail::Edit { idx, draft } => {
            let dirty = cfg
                .rules
                .get(*idx)
                .map(|canonical| !draft.matches(canonical))
                .unwrap_or(true);
            editor_body(draft, ui, dirty, /* is_draft */ false)
        }
        Detail::Draft(draft) => editor_body(draft, ui, true, /* is_draft */ true),
    };

    container(card(body).width(Length::Fill).height(Length::Fill))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn editor_body<'a>(
    draft: &'a RuleDraft,
    ui: &'a MailState,
    dirty: bool,
    is_draft: bool,
) -> Element<'a, MailMsg> {
    let title = if is_draft {
        "New rule".to_string()
    } else {
        display_title(draft).to_string()
    };
    let subtitle = if is_draft {
        "Name the rule, pick an action, then the conditions."
    } else {
        "Every condition must match for the rule to fire."
    };

    let mut footer = row![].spacing(SPACE_SM).align_y(Alignment::Center);
    if is_draft {
        footer = footer
            .push(kit_btn::labeled_sm("Add", kit_btn::primary).on_press(MailMsg::Save))
            .push(kit_btn::labeled_sm("Discard", kit_btn::ghost).on_press(MailMsg::Discard));
    } else {
        if dirty {
            footer = footer
                .push(kit_btn::labeled_sm("Save", kit_btn::primary).on_press(MailMsg::Save))
                .push(kit_btn::labeled_sm("Discard", kit_btn::ghost).on_press(MailMsg::Discard));
        }
        footer = footer
            .push(iced::widget::Space::new().width(Length::Fill))
            .push(kit_btn::labeled_sm("Remove", kit_btn::danger_outline).on_press(MailMsg::Remove));
        if !dirty {
            footer = footer
                .push(kit_btn::labeled_sm("Close", kit_btn::ghost).on_press(MailMsg::CloseDetail));
        }
    }

    let mut col = column![
        kit_text::subheading(title),
        kit_text::caption(subtitle).style(kit_text::muted),
        editor_fields(draft, ui),
    ]
    .spacing(SPACE_MD);
    if let Some(err) = ui.error.as_deref() {
        col = col.push(kit_text::caption(err).style(kit_text::danger));
    }
    col = col
        .push(iced::widget::Space::new().height(Length::Fill))
        .push(footer);
    col.height(Length::Fill).into()
}

fn editor_fields<'a>(draft: &'a RuleDraft, ui: &'a MailState) -> Element<'a, MailMsg> {
    let name_input = text_input("News, receipts…", &draft.name)
        .id(rule_name_id())
        .on_input(|v| MailMsg::Field(RuleField::Name, v))
        .size(13)
        .style(kit_input::style);

    let action_open = ui.open_menu == Some(OpenMenu::Action);
    let action_opts = ACTION_OPTIONS.iter().map(|a| {
        SelectOption::new(
            action_label(a),
            draft.action == *a,
            MailMsg::PickAction((*a).to_string()),
        )
    });
    let action_select = select(
        action_label(&draft.action),
        action_opts,
        action_open,
        MailMsg::ToggleMenu(OpenMenu::Action),
        MailMsg::DismissMenu,
    );

    let mut col = column![
        field("Name", name_input, None, None),
        field("Action", action_select, None, None),
    ]
    .spacing(SPACE_LG);

    if draft.action == ACTION_MOVE {
        let dest_open = ui.open_menu == Some(OpenMenu::Dest);
        let options = dest_options(&draft.dest);
        let dest_opts = options.iter().map(|d| {
            SelectOption::new(
                d.clone(),
                draft.dest.eq_ignore_ascii_case(d),
                MailMsg::PickDest(d.clone()),
            )
        });
        let dest_label = if draft.dest.trim().is_empty() {
            "Choose a mailbox".to_string()
        } else {
            draft.dest.clone()
        };
        let dest_select = select(
            dest_label,
            dest_opts,
            dest_open,
            MailMsg::ToggleMenu(OpenMenu::Dest),
            MailMsg::DismissMenu,
        );
        col = col.push(field(
            "Mailbox",
            dest_select,
            Some("Matching messages leave the inbox for this mailbox."),
            None,
        ));
    }

    col = col.push(kit_text::caption("Conditions — all must match").style(kit_text::muted));

    let mut conds = column![].spacing(SPACE_MD);
    for (i, c) in draft.conditions.iter().enumerate() {
        conds = conds.push(condition_row(i, c, ui, draft.conditions.len() > 1));
    }
    conds = conds.push(
        kit_btn::labeled_sm("+ Add condition", kit_btn::ghost).on_press(MailMsg::AddCondition),
    );
    col = col.push(conds);

    scrollable(col)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn condition_row<'a>(
    i: usize,
    c: &'a CondDraft,
    ui: &'a MailState,
    can_remove: bool,
) -> Element<'a, MailMsg> {
    let field_open = ui.open_menu == Some(OpenMenu::CondField(i));
    let field_opts = FIELD_OPTIONS.iter().map(|f| {
        SelectOption::new(
            field_label(f),
            c.field == *f,
            MailMsg::PickCondField(i, (*f).to_string()),
        )
    });
    let field_select = container(select(
        field_label(&c.field),
        field_opts,
        field_open,
        MailMsg::ToggleMenu(OpenMenu::CondField(i)),
        MailMsg::DismissMenu,
    ))
    .width(Length::Fixed(120.0));

    let allowed = match_options_for(&c.field);
    let match_open = ui.open_menu == Some(OpenMenu::CondMatch(i));
    let match_opts = allowed.iter().map(|m| {
        SelectOption::new(
            match_label(m),
            c.match_type == *m,
            MailMsg::PickCondMatch(i, (*m).to_string()),
        )
    });
    let match_select = container(select(
        match_label(&c.match_type),
        match_opts,
        match_open,
        MailMsg::ToggleMenu(OpenMenu::CondMatch(i)),
        MailMsg::DismissMenu,
    ))
    .width(Length::Fixed(140.0));

    let value_owned = c.value.clone();
    let value_input = text_input("value", &value_owned)
        .id(cond_id(i))
        .on_input(move |v| MailMsg::CondValue(i, v))
        .size(13)
        .style(kit_input::style)
        .width(Length::Fill);

    let mut r = row![field_select, match_select, value_input]
        .spacing(SPACE_MD)
        .align_y(Alignment::Center)
        .width(Length::Fill);
    if can_remove {
        r = r.push(
            kit_btn::labeled_sm("Remove", kit_btn::ghost).on_press(MailMsg::RemoveCondition(i)),
        );
    }
    r.into()
}

fn account_input<'a>(label: &'a str, value: &'a str, f: AccountField) -> Element<'a, MailMsg> {
    account_input_help(label, value, f, None)
}

fn account_input_help<'a>(
    label: &'a str,
    value: &'a str,
    f: AccountField,
    help: Option<&'a str>,
) -> Element<'a, MailMsg> {
    let input = text_input("", value)
        .id(account_field_id(f))
        .on_input(move |v| MailMsg::AccountField(f, v))
        .size(13)
        .style(kit_input::style)
        .width(Length::Fill);
    field(label, input, help, None).into()
}

fn password_input<'a>(
    label: &'a str,
    value: &'a str,
    help: Option<&'a str>,
) -> Element<'a, MailMsg> {
    let input = text_input("", value)
        .id(account_field_id(AccountField::Password))
        .on_input(|v| MailMsg::AccountField(AccountField::Password, v))
        .secure(true)
        .size(13)
        .style(kit_input::style)
        .width(Length::Fill);
    field(label, input, help, None).into()
}

// ── Helpers ───────────────────────────────────────────────────────

fn validate(draft: &RuleDraft) -> Result<(), String> {
    if draft.name.trim().is_empty() {
        return Err("Name the rule".into());
    }
    if draft.action == ACTION_MOVE && draft.dest.trim().is_empty() {
        return Err("Choose a mailbox to move matching mail into".into());
    }
    if draft.conditions.is_empty() {
        return Err("Add at least one condition".into());
    }
    if draft.conditions.iter().any(|c| c.value.trim().is_empty()) {
        return Err("Each condition needs a value".into());
    }
    Ok(())
}

fn parse_port(raw: &str, fallback: u16) -> u16 {
    raw.trim()
        .parse::<u16>()
        .ok()
        .filter(|p| *p > 0)
        .unwrap_or(fallback)
}

fn account_field_id(f: AccountField) -> iced::widget::Id {
    iced::widget::Id::new(match f {
        AccountField::Email => "settings-mail-email",
        AccountField::ImapHost => "settings-mail-imap-host",
        AccountField::ImapPort => "settings-mail-imap-port",
        AccountField::SmtpHost => "settings-mail-smtp-host",
        AccountField::SmtpPort => "settings-mail-smtp-port",
        AccountField::Username => "settings-mail-username",
        AccountField::Password => "settings-mail-password",
    })
}

fn alias_id(i: usize) -> iced::widget::Id {
    iced::widget::Id::from(format!("settings-mail-alias-{i}"))
}

fn rule_name_id() -> iced::widget::Id {
    iced::widget::Id::new("settings-mail-rule-name")
}

fn cond_id(i: usize) -> iced::widget::Id {
    iced::widget::Id::from(format!("settings-mail-cond-{i}"))
}

/// Value of the focused mail field, if `id` is one of ours.
pub fn focused_value(ui: &MailState, id: &iced::widget::Id) -> Option<String> {
    if let Some(d) = match &ui.account {
        AccountDetail::Edit { draft, .. } | AccountDetail::Draft(draft) => Some(draft),
        AccountDetail::Closed => None,
    } {
        for f in [
            AccountField::Email,
            AccountField::ImapHost,
            AccountField::ImapPort,
            AccountField::SmtpHost,
            AccountField::SmtpPort,
            AccountField::Username,
            AccountField::Password,
        ] {
            if id == &account_field_id(f) {
                return Some(account_field_str(d, f).to_string());
            }
        }
        for (i, a) in d.aliases.iter().enumerate() {
            if id == &alias_id(i) {
                return Some(a.clone());
            }
        }
    }
    if let Some(draft) = match &ui.detail {
        Detail::Edit { draft, .. } | Detail::Draft(draft) => Some(draft),
        Detail::Closed => None,
    } {
        if id == &rule_name_id() {
            return Some(draft.name.clone());
        }
        for (i, c) in draft.conditions.iter().enumerate() {
            if id == &cond_id(i) {
                return Some(c.value.clone());
            }
        }
    }
    None
}

/// Replace the focused mail field. Returns whether `id` was ours.
pub fn set_focused_value(ui: &mut MailState, id: &iced::widget::Id, value: &str) -> bool {
    if let Some(d) = open_account_mut(&mut ui.account) {
        for f in [
            AccountField::Email,
            AccountField::ImapHost,
            AccountField::ImapPort,
            AccountField::SmtpHost,
            AccountField::SmtpPort,
            AccountField::Username,
            AccountField::Password,
        ] {
            if id == &account_field_id(f) {
                let prev = d.email.clone();
                set_account_field(d, f, value.to_string());
                if f == AccountField::Email {
                    apply_server_hint(d, &prev);
                }
                return true;
            }
        }
        for i in 0..d.aliases.len() {
            if id == &alias_id(i) {
                d.aliases[i] = value.to_string();
                return true;
            }
        }
    }
    if let Some(draft) = open_draft_mut(&mut ui.detail) {
        if id == &rule_name_id() {
            draft.name = value.to_string();
            return true;
        }
        for i in 0..draft.conditions.len() {
            if id == &cond_id(i) {
                draft.conditions[i].value = value.to_string();
                return true;
            }
        }
    }
    false
}

fn account_field_str(d: &AccountDraft, f: AccountField) -> &str {
    match f {
        AccountField::Email => &d.email,
        AccountField::ImapHost => &d.imap_host,
        AccountField::ImapPort => &d.imap_port,
        AccountField::SmtpHost => &d.smtp_host,
        AccountField::SmtpPort => &d.smtp_port,
        AccountField::Username => &d.username,
        AccountField::Password => &d.password,
    }
}

fn set_account_field(d: &mut AccountDraft, f: AccountField, value: String) {
    match f {
        AccountField::Email => d.email = value,
        AccountField::ImapHost => d.imap_host = value,
        AccountField::ImapPort => d.imap_port = value,
        AccountField::SmtpHost => d.smtp_host = value,
        AccountField::SmtpPort => d.smtp_port = value,
        AccountField::Username => d.username = value,
        AccountField::Password => d.password = value,
    }
}

fn emit(topic: Topic) {
    if let Err(e) = bus().lock().unwrap().emit(topic) {
        tracing::warn!("bus emit failed: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn move_rule() -> MailRule {
        MailRule {
            name: "Illuno spam".into(),
            action: ACTION_MOVE.into(),
            dest: Some("Trash".into()),
            conditions: vec![MailRuleCondition {
                field: "from".into(),
                match_type: "equals".into(),
                value: "no-reply@illuno.com".into(),
            }],
        }
    }

    #[test]
    fn dest_options_include_canonical_and_custom() {
        let base = dest_options("");
        assert!(base.iter().any(|d| d == "Trash"));
        assert!(!base.iter().any(|d| d == "Projects"));
        let with = dest_options("Projects");
        assert!(with.iter().any(|d| d == "Projects"));
    }

    #[test]
    fn validate_requires_dest_on_move() {
        let mut d = empty_rule();
        d.name = "x".into();
        d.conditions[0].value = "a@b.com".into();
        assert!(validate(&d).unwrap_err().to_lowercase().contains("mailbox"));
        d.dest = "Trash".into();
        assert!(validate(&d).is_ok());
    }

    #[test]
    fn validate_smart_mailbox_skips_dest() {
        let mut d = empty_rule();
        d.name = "GitHub".into();
        d.action = ACTION_SMART.into();
        d.conditions[0].value = "github.com".into();
        d.conditions[0].match_type = "domain".into();
        assert!(validate(&d).is_ok());
    }

    #[test]
    fn draft_round_trip_preserves_move() {
        let r = move_rule();
        let d = RuleDraft::from_rule(&r);
        assert!(d.matches(&r));
        let back = d.to_rule();
        assert_eq!(back.name, r.name);
        assert_eq!(back.action, r.action);
        assert_eq!(back.dest, r.dest);
        assert_eq!(back.conditions[0].value, r.conditions[0].value);
    }

    #[test]
    fn can_leave_clean_edit_but_not_dirty() {
        let mut cfg = MailConfig::default();
        cfg.rules.push(move_rule());
        let clean = RuleDraft::from_rule(&cfg.rules[0]);
        assert!(can_leave_detail(
            &Detail::Edit {
                idx: 0,
                draft: clean.clone()
            },
            &cfg
        ));
        let mut dirty = clean;
        dirty.dest = "Junk".into();
        assert!(!can_leave_detail(
            &Detail::Edit {
                idx: 0,
                draft: dirty
            },
            &cfg
        ));
        assert!(can_leave_detail(&Detail::Draft(empty_rule()), &cfg));
        let mut started = empty_rule();
        started.name = "x".into();
        assert!(!can_leave_detail(&Detail::Draft(started), &cfg));
    }

    #[test]
    fn action_caption_names_mailbox() {
        let d = RuleDraft::from_rule(&move_rule());
        assert_eq!(action_caption(&d), "Move to Trash");
    }

    #[test]
    fn subject_hides_address_matchers() {
        assert_eq!(match_options_for("subject"), &MATCH_TEXT);
        assert!(match_options_for("from").contains(&"address"));
    }

    #[test]
    fn extra_account_round_trips() {
        let mut cfg = MailConfig::default();
        cfg.email = "josh@wicket.example".into();
        let draft = AccountDraft {
            email: "me@gmail.com".into(),
            imap_host: String::new(),
            imap_port: "993".into(),
            smtp_host: "smtp.gmail.com".into(),
            smtp_port: "587".into(),
            username: "me@gmail.com".into(),
            password: "app-pass".into(),
            aliases: vec!["shop@gmail.com".into()],
            alias_show: vec![true],
            imap_enabled: false,
            smtp_enabled: true,
        };
        let extra = draft.to_extra();
        assert!(extra.owns_from("shop@gmail.com"));
        assert!(!extra.owns_from("josh@wicket.example"));
        cfg.accounts.push(extra);
        let loaded = account_at(&cfg, 1).expect("extra");
        assert!(loaded.matches_extra(&cfg.accounts[0], &cfg));
        assert_eq!(account_caption(&cfg, 1, &loaded), "Send only");
        assert_eq!(
            account_caption(&cfg, 0, &AccountDraft::from_inbox(&cfg)),
            "Inbox · Default From"
        );
    }

    #[test]
    fn clean_addrs_drops_blank_and_dupes() {
        assert_eq!(
            clean_addrs(&[
                " a@b.com ".into(),
                "".into(),
                "A@b.com".into(),
                "c@d.com".into()
            ]),
            vec!["a@b.com", "c@d.com"]
        );
        assert_eq!(
            clean_addrs(&["z@b.com".into(), "*@moonlight.pm".into(), "a@b.com".into()]),
            vec!["a@b.com", "z@b.com"]
        );
    }

    #[test]
    fn gmail_fills_imap_and_smtp() {
        let mut d = AccountDraft::empty();
        d.email = "me@gmail.com".into();
        apply_server_hint(&mut d, "");
        assert_eq!(d.smtp_host, "smtp.gmail.com");
        assert_eq!(d.smtp_port, "587");
        assert_eq!(d.imap_host, "imap.gmail.com");
        assert_eq!(d.imap_port, "993");
        assert_eq!(d.username, "me@gmail.com");
    }

    #[test]
    fn discovery_does_not_clobber_typed_smtp() {
        let mut d = AccountDraft::empty();
        d.smtp_host = "mail.wicket.example".into();
        d.email = "me@gmail.com".into();
        apply_server_hint(&mut d, "");
        assert_eq!(d.smtp_host, "mail.wicket.example");
        assert_eq!(d.username, "me@gmail.com");
        assert_eq!(d.imap_host, "imap.gmail.com");
    }
}
