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

use iced::widget::{button, column, container, row, scrollable};
use iced::{Alignment, Element, Length, Padding, Task};
use sola_kit::components::text_input::text_input;

use sola_bus::topics::{MailConfig, MailRule, MailRuleCondition, Topic};
use sola_core::Encrypted;
use sola_kit::app::bus;
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
}

impl AccountDraft {
    pub fn from_config(cfg: &MailConfig) -> Self {
        Self {
            email: cfg.email.clone(),
            imap_host: cfg.imap_host.clone(),
            imap_port: cfg.imap_port.to_string(),
            smtp_host: cfg.smtp_host.clone(),
            smtp_port: cfg.smtp_port.to_string(),
            username: cfg.username.clone(),
            password: cfg.password.0.clone(),
        }
    }
    pub fn matches(&self, cfg: &MailConfig) -> bool {
        self.email == cfg.email
            && self.imap_host == cfg.imap_host
            && self.imap_port == cfg.imap_port.to_string()
            && self.smtp_host == cfg.smtp_host
            && self.smtp_port == cfg.smtp_port.to_string()
            && self.username == cfg.username
            && self.password == cfg.password.0
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
}

pub struct MailState {
    pub account: AccountDraft,
    pub detail: Detail,
    pub open_menu: Option<OpenMenu>,
    /// Inline error for the open rule editor (`None` = account errors
    /// live in `account_error`).
    pub error: Option<String>,
    pub account_error: Option<String>,
    pub last_canonical: MailConfig,
}

impl Default for MailState {
    fn default() -> Self {
        let cfg = MailConfig::default();
        Self {
            account: AccountDraft::from_config(&cfg),
            detail: Detail::Closed,
            open_menu: None,
            error: None,
            account_error: None,
            last_canonical: cfg,
        }
    }
}

impl MailState {
    /// Refresh clean drafts to the latest canonical state; preserve
    /// dirty drafts as-is. Called from main.rs on every
    /// `Topic::MailConfig` delivery.
    pub fn sync_from_canonical(&mut self, next: &MailConfig) {
        if self.account.matches(&self.last_canonical) {
            self.account = AccountDraft::from_config(next);
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
    AccountSave,
    AccountRevert,
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

#[derive(Debug, Clone, Copy)]
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
            match f {
                AccountField::Email => ui.account.email = v,
                AccountField::ImapHost => ui.account.imap_host = v,
                AccountField::ImapPort => ui.account.imap_port = v,
                AccountField::SmtpHost => ui.account.smtp_host = v,
                AccountField::SmtpPort => ui.account.smtp_port = v,
                AccountField::Username => ui.account.username = v,
                AccountField::Password => ui.account.password = v,
            }
        }
        MailMsg::AccountSave => {
            cfg.email = ui.account.email.trim().to_string();
            cfg.imap_host = ui.account.imap_host.trim().to_string();
            cfg.imap_port = parse_port(&ui.account.imap_port, 993);
            cfg.smtp_host = ui.account.smtp_host.trim().to_string();
            cfg.smtp_port = parse_port(&ui.account.smtp_port, 587);
            cfg.username = ui.account.username.trim().to_string();
            cfg.password = Encrypted(ui.account.password.clone());
            ui.account = AccountDraft::from_config(cfg);
            ui.last_canonical = cfg.clone();
            ui.account_error = None;
            emit(Topic::MailConfig(cfg.clone()));
        }
        MailMsg::AccountRevert => {
            ui.account = AccountDraft::from_config(cfg);
            ui.account_error = None;
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
    column![account_card(cfg, ui), rules_section(cfg, ui)]
        .spacing(SPACE_XL + SPACE_MD)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn account_card<'a>(cfg: &'a MailConfig, ui: &'a MailState) -> Element<'a, MailMsg> {
    let dirty = !ui.account.matches(cfg);

    let header = column![
        kit_text::subheading("Account"),
        kit_text::caption("IMAP receive + SMTP send.").style(kit_text::muted),
    ]
    .spacing(SPACE_XS);

    let hosts = row![
        account_input("IMAP host", &ui.account.imap_host, AccountField::ImapHost),
        container(account_input(
            "IMAP port",
            &ui.account.imap_port,
            AccountField::ImapPort,
        ))
        .width(Length::Fixed(120.0)),
    ]
    .spacing(SPACE_MD);

    let smtp = row![
        account_input("SMTP host", &ui.account.smtp_host, AccountField::SmtpHost),
        container(account_input(
            "SMTP port",
            &ui.account.smtp_port,
            AccountField::SmtpPort,
        ))
        .width(Length::Fixed(120.0)),
    ]
    .spacing(SPACE_MD);

    let body = column![
        header,
        account_input("Email", &ui.account.email, AccountField::Email),
        hosts,
        smtp,
        account_input("Username", &ui.account.username, AccountField::Username),
        password_input("Password", &ui.account.password),
        row![
            kit_btn::labeled(
                "Save account",
                if dirty {
                    kit_btn::primary
                } else {
                    kit_btn::secondary
                },
            )
            .on_press_maybe(dirty.then_some(MailMsg::AccountSave)),
            kit_btn::labeled("Revert", kit_btn::ghost)
                .on_press_maybe(dirty.then_some(MailMsg::AccountRevert)),
        ]
        .spacing(SPACE_MD),
    ]
    .spacing(SPACE_LG);

    let body: Element<'_, MailMsg> = if let Some(err) = ui.account_error.as_deref() {
        column![body, kit_text::caption(err).style(kit_text::danger)]
            .spacing(SPACE_LG)
            .into()
    } else {
        body.into()
    };

    card(body).width(Length::Fill).into()
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
    let input = text_input("", value)
        .on_input(move |v| MailMsg::AccountField(f, v))
        .size(13)
        .style(kit_input::style)
        .width(Length::Fill);
    field(label, input, None, None).into()
}

fn password_input<'a>(label: &'a str, value: &'a str) -> Element<'a, MailMsg> {
    let input = text_input("", value)
        .on_input(|v| MailMsg::AccountField(AccountField::Password, v))
        .secure(true)
        .size(13)
        .style(kit_input::style)
        .width(Length::Fill);
    field(label, input, None, None).into()
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
}
