//! Mail panel — IMAP/SMTP account credentials and per-rule routing
//! (smart mailbox / move). The whole `MailConfig` is the persistent
//! topic: any edit emits a new `Topic::MailConfig` and the bus replay
//! is what refreshes our canonical state.
//!
//! Edits are explicit-Save: an account or rule shows Save / Discard
//! buttons once dirty; a new rule has Add / Discard until committed.
//! Dirty drafts survive external replays — only clean drafts get
//! refreshed when the bus pushes a new canonical config.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};

use iced::widget::{button, column, pick_list, row, text};
use sola_kit::components::text_input::text_input;
use iced::{Element, Length, Padding, Task};

use sola_bus::topics::{MailConfig, MailRule, MailRuleCondition, Topic};
use sola_core::Encrypted;
use sola_kit::app::bus;
use sola_kit::components::text as kit_text;
use sola_kit::components::{button as kit_btn, card, field, text_input as kit_input};
use sola_kit::fonts;

const ACTION_OPTIONS: [&str; 2] = ["smart_mailbox", "move"];
const FIELD_OPTIONS: [&str; 3] = ["from", "to", "subject"];
const MATCH_OPTIONS: [&str; 4] = ["contains", "equals", "address", "domain"];

const FIELD_GAP: f32 = 12.0;

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
        let our_dest = if self.action == "move" {
            Some(self.dest.trim().to_string())
        } else {
            None
        };
        if our_dest.as_deref() != r.dest.as_deref() {
            return false;
        }
        if self.conditions.len() != r.conditions.len() {
            return false;
        }
        for (a, b) in self.conditions.iter().zip(r.conditions.iter()) {
            if a.field != b.field || a.match_type != b.match_type || a.value != b.value {
                return false;
            }
        }
        true
    }
    fn to_rule(&self) -> MailRule {
        MailRule {
            name: self.name.trim().to_string(),
            action: self.action.clone(),
            dest: if self.action == "move" {
                let d = self.dest.trim();
                if d.is_empty() { None } else { Some(d.to_string()) }
            } else {
                None
            },
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

fn empty_rule() -> RuleDraft {
    RuleDraft {
        name: String::new(),
        action: "smart_mailbox".into(),
        dest: String::new(),
        conditions: Vec::new(),
    }
}

fn empty_condition() -> CondDraft {
    CondDraft {
        field: "from".into(),
        match_type: "contains".into(),
        value: String::new(),
    }
}

// ── State ─────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct NewRule {
    pub key: u64,
    pub draft: RuleDraft,
}

pub struct MailState {
    pub account: AccountDraft,
    /// Edits to existing rules keyed by canonical index. Lazy-created
    /// on first edit; cleared on Save or Discard. Survives external
    /// replays only while dirty.
    pub existing: BTreeMap<usize, RuleDraft>,
    /// Unsaved new rules, ordered by author time.
    pub new_rules: Vec<NewRule>,
    /// Error messages keyed by `account`, `rule-<idx>`, or `new-<key>`.
    pub errors: BTreeMap<String, String>,
    /// Most recent canonical mail config — used to decide whether a
    /// draft is "clean" (matches canonical) and can be refreshed in
    /// place when an external replay arrives.
    pub last_canonical: MailConfig,
}

impl Default for MailState {
    fn default() -> Self {
        let cfg = MailConfig::default();
        Self {
            account: AccountDraft::from_config(&cfg),
            existing: BTreeMap::new(),
            new_rules: Vec::new(),
            errors: BTreeMap::new(),
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
        let mut carry = BTreeMap::new();
        for (idx, draft) in &self.existing {
            if *idx >= next.rules.len() {
                continue;
            }
            if !draft.matches(&self.last_canonical.rules[*idx]) {
                carry.insert(*idx, draft.clone());
            }
        }
        self.existing = carry;
        self.last_canonical = next.clone();
    }
}

static NEW_RULE_SEQ: AtomicU64 = AtomicU64::new(1);

fn next_new_key() -> u64 {
    NEW_RULE_SEQ.fetch_add(1, Ordering::Relaxed)
}

// ── Messages ──────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum MailMsg {
    AccountField(AccountField, String),
    AccountSave,
    AccountRevert,
    ExistingField(usize, RuleField, String),
    ExistingAction(usize, String),
    ExistingCondField(usize, usize, CondField, String),
    ExistingAddCondition(usize),
    ExistingRemoveCondition(usize, usize),
    ExistingSave(usize),
    ExistingDiscard(usize),
    ExistingRemove(usize),
    NewStart,
    NewField(u64, RuleField, String),
    NewAction(u64, String),
    NewCondField(u64, usize, CondField, String),
    NewAddCondition(u64),
    NewRemoveCondition(u64, usize),
    NewSave(u64),
    NewDiscard(u64),
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
    Dest,
}

#[derive(Debug, Clone, Copy)]
pub enum CondField {
    Field,
    Match,
    Value,
}

// ── Update ────────────────────────────────────────────────────────

pub fn update(msg: MailMsg, cfg: &mut MailConfig, ui: &mut MailState) -> Task<MailMsg> {
    match msg {
        MailMsg::AccountField(f, v) => {
            ui.errors.remove("account");
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
            emit(Topic::MailConfig(cfg.clone()));
        }
        MailMsg::AccountRevert => {
            ui.account = AccountDraft::from_config(cfg);
            ui.errors.remove("account");
        }
        MailMsg::ExistingField(idx, f, v) => {
            ui.errors.remove(&existing_err_key(idx));
            let draft = ui
                .existing
                .entry(idx)
                .or_insert_with(|| RuleDraft::from_rule(&cfg.rules[idx]));
            match f {
                RuleField::Name => draft.name = v,
                RuleField::Dest => draft.dest = v,
            }
        }
        MailMsg::ExistingAction(idx, action) => {
            ui.errors.remove(&existing_err_key(idx));
            let draft = ui
                .existing
                .entry(idx)
                .or_insert_with(|| RuleDraft::from_rule(&cfg.rules[idx]));
            draft.action = action;
        }
        MailMsg::ExistingCondField(idx, cond_idx, f, v) => {
            ui.errors.remove(&existing_err_key(idx));
            let draft = ui
                .existing
                .entry(idx)
                .or_insert_with(|| RuleDraft::from_rule(&cfg.rules[idx]));
            if let Some(c) = draft.conditions.get_mut(cond_idx) {
                match f {
                    CondField::Field => c.field = v,
                    CondField::Match => c.match_type = v,
                    CondField::Value => c.value = v,
                }
            }
        }
        MailMsg::ExistingAddCondition(idx) => {
            ui.errors.remove(&existing_err_key(idx));
            let draft = ui
                .existing
                .entry(idx)
                .or_insert_with(|| RuleDraft::from_rule(&cfg.rules[idx]));
            draft.conditions.push(empty_condition());
        }
        MailMsg::ExistingRemoveCondition(idx, cond_idx) => {
            ui.errors.remove(&existing_err_key(idx));
            let draft = ui
                .existing
                .entry(idx)
                .or_insert_with(|| RuleDraft::from_rule(&cfg.rules[idx]));
            if cond_idx < draft.conditions.len() {
                draft.conditions.remove(cond_idx);
            }
        }
        MailMsg::ExistingSave(idx) => {
            let Some(draft) = ui.existing.get(&idx).cloned() else {
                return Task::none();
            };
            if let Err(e) = validate(&draft) {
                ui.errors.insert(existing_err_key(idx), e);
                return Task::none();
            }
            if idx >= cfg.rules.len() {
                ui.errors.insert(existing_err_key(idx), "rule index out of range".into());
                return Task::none();
            }
            cfg.rules[idx] = draft.to_rule();
            ui.last_canonical = cfg.clone();
            ui.existing.remove(&idx);
            ui.errors.remove(&existing_err_key(idx));
            emit(Topic::MailConfig(cfg.clone()));
        }
        MailMsg::ExistingDiscard(idx) => {
            ui.existing.remove(&idx);
            ui.errors.remove(&existing_err_key(idx));
        }
        MailMsg::ExistingRemove(idx) => {
            if idx < cfg.rules.len() {
                cfg.rules.remove(idx);
                ui.last_canonical = cfg.clone();
                // Surviving existing-drafts at later indices shift; the
                // simplest correct thing is to drop them — the user
                // edits a stale row that no longer exists otherwise.
                ui.existing.clear();
                ui.errors.remove(&existing_err_key(idx));
                emit(Topic::MailConfig(cfg.clone()));
            }
        }
        MailMsg::NewStart => {
            ui.new_rules.push(NewRule {
                key: next_new_key(),
                draft: empty_rule(),
            });
        }
        MailMsg::NewField(key, f, v) => {
            ui.errors.remove(&new_err_key(key));
            if let Some(nr) = ui.new_rules.iter_mut().find(|r| r.key == key) {
                match f {
                    RuleField::Name => nr.draft.name = v,
                    RuleField::Dest => nr.draft.dest = v,
                }
            }
        }
        MailMsg::NewAction(key, action) => {
            ui.errors.remove(&new_err_key(key));
            if let Some(nr) = ui.new_rules.iter_mut().find(|r| r.key == key) {
                nr.draft.action = action;
            }
        }
        MailMsg::NewCondField(key, cond_idx, f, v) => {
            ui.errors.remove(&new_err_key(key));
            if let Some(nr) = ui.new_rules.iter_mut().find(|r| r.key == key) {
                if let Some(c) = nr.draft.conditions.get_mut(cond_idx) {
                    match f {
                        CondField::Field => c.field = v,
                        CondField::Match => c.match_type = v,
                        CondField::Value => c.value = v,
                    }
                }
            }
        }
        MailMsg::NewAddCondition(key) => {
            ui.errors.remove(&new_err_key(key));
            if let Some(nr) = ui.new_rules.iter_mut().find(|r| r.key == key) {
                nr.draft.conditions.push(empty_condition());
            }
        }
        MailMsg::NewRemoveCondition(key, cond_idx) => {
            ui.errors.remove(&new_err_key(key));
            if let Some(nr) = ui.new_rules.iter_mut().find(|r| r.key == key) {
                if cond_idx < nr.draft.conditions.len() {
                    nr.draft.conditions.remove(cond_idx);
                }
            }
        }
        MailMsg::NewSave(key) => {
            let Some(nr) = ui.new_rules.iter().find(|r| r.key == key).cloned() else {
                return Task::none();
            };
            if let Err(e) = validate(&nr.draft) {
                ui.errors.insert(new_err_key(key), e);
                return Task::none();
            }
            cfg.rules.push(nr.draft.to_rule());
            ui.last_canonical = cfg.clone();
            ui.new_rules.retain(|r| r.key != key);
            ui.errors.remove(&new_err_key(key));
            emit(Topic::MailConfig(cfg.clone()));
        }
        MailMsg::NewDiscard(key) => {
            ui.new_rules.retain(|r| r.key != key);
            ui.errors.remove(&new_err_key(key));
        }
    }
    Task::none()
}

// ── View ──────────────────────────────────────────────────────────

pub fn view<'a>(cfg: &'a MailConfig, ui: &'a MailState) -> Element<'a, MailMsg> {
    column![account_card(cfg, ui), rules_card(cfg, ui)]
        .spacing(24)
        .into()
}

fn account_card<'a>(cfg: &'a MailConfig, ui: &'a MailState) -> Element<'a, MailMsg> {
    let dirty = !ui.account.matches(cfg);

    let header = column![
        text("Account").font(fonts::ui_medium()).size(16),
        text("IMAP receive + SMTP send credentials.")
            .size(12)
            .style(kit_text::muted),
    ]
    .spacing(2);

    let body = column![
        header,
        account_input("Email", &ui.account.email, AccountField::Email),
        account_input("IMAP host", &ui.account.imap_host, AccountField::ImapHost),
        account_input("IMAP port", &ui.account.imap_port, AccountField::ImapPort),
        account_input("SMTP host", &ui.account.smtp_host, AccountField::SmtpHost),
        account_input("SMTP port", &ui.account.smtp_port, AccountField::SmtpPort),
        account_input("Username", &ui.account.username, AccountField::Username),
        password_input("Password", &ui.account.password),
        row![
            button(text("Save account").size(13))
                .style(if dirty { kit_btn::primary } else { kit_btn::secondary })
                .padding(Padding::new(6.0).left(12.0).right(12.0))
                .on_press_maybe(dirty.then_some(MailMsg::AccountSave)),
            button(text("Revert").size(13))
                .style(kit_btn::ghost)
                .padding(Padding::new(6.0).left(12.0).right(12.0))
                .on_press_maybe(dirty.then_some(MailMsg::AccountRevert)),
        ]
        .spacing(8),
    ]
    .spacing(FIELD_GAP);

    let body: Element<'_, MailMsg> = if let Some(err) = ui.errors.get("account") {
        column![body, text(err.as_str()).size(12).style(kit_text::muted)]
            .spacing(FIELD_GAP)
            .into()
    } else {
        body.into()
    };

    card(body).width(Length::Fill).into()
}

fn rules_card<'a>(cfg: &'a MailConfig, ui: &'a MailState) -> Element<'a, MailMsg> {
    let header = column![
        text("Rules").font(fonts::ui_medium()).size(16),
        text("Each condition row must match for the rule to fire.")
            .size(12)
            .style(kit_text::muted),
    ]
    .spacing(2);

    let mut col = column![header].spacing(FIELD_GAP);

    if cfg.rules.is_empty() && ui.new_rules.is_empty() {
        col = col.push(text("No rules configured.").size(13).style(kit_text::muted));
    }

    for (idx, rule) in cfg.rules.iter().enumerate() {
        let draft = ui.existing.get(&idx);
        let working = draft.cloned().unwrap_or_else(|| RuleDraft::from_rule(rule));
        let dirty = !working.matches(rule);
        col = col.push(existing_rule_card(idx, working, dirty, ui));
    }
    for nr in &ui.new_rules {
        col = col.push(new_rule_card(nr.clone(), ui));
    }

    col = col.push(
        button(text("+ Add rule").size(13))
            .style(kit_btn::ghost)
            .padding(Padding::new(6.0).left(10.0).right(10.0))
            .on_press(MailMsg::NewStart),
    );

    card(col).width(Length::Fill).into()
}

fn existing_rule_card<'a>(
    idx: usize,
    draft: RuleDraft,
    dirty: bool,
    ui: &'a MailState,
) -> Element<'a, MailMsg> {
    let title = if draft.name.is_empty() {
        "(unnamed rule)".to_string()
    } else {
        draft.name.clone()
    };

    let actions = row![
        button(text("Save").size(13))
            .style(if dirty { kit_btn::primary } else { kit_btn::secondary })
            .padding(Padding::new(6.0).left(12.0).right(12.0))
            .on_press_maybe(dirty.then_some(MailMsg::ExistingSave(idx))),
        button(text("Discard").size(13))
            .style(kit_btn::ghost)
            .padding(Padding::new(6.0).left(12.0).right(12.0))
            .on_press_maybe(dirty.then_some(MailMsg::ExistingDiscard(idx))),
        iced::widget::Space::new().width(Length::Fill),
        button(text("Remove rule").size(13))
            .style(kit_btn::danger)
            .padding(Padding::new(6.0).left(12.0).right(12.0))
            .on_press(MailMsg::ExistingRemove(idx)),
    ]
    .spacing(8)
    .align_y(iced::Alignment::Center);

    let body_input = rule_body(
        draft,
        move |f, v| MailMsg::ExistingField(idx, f, v),
        move |action| MailMsg::ExistingAction(idx, action),
        move |cond_idx, f, v| MailMsg::ExistingCondField(idx, cond_idx, f, v),
        move || MailMsg::ExistingAddCondition(idx),
        move |cond_idx| MailMsg::ExistingRemoveCondition(idx, cond_idx),
    );

    let mut inner = column![
        text(title).font(fonts::ui_medium()).size(14),
        body_input,
        actions,
    ]
    .spacing(FIELD_GAP);
    if let Some(err) = ui.errors.get(&existing_err_key(idx)) {
        inner = inner.push(text(err.as_str()).size(12).style(kit_text::muted));
    }
    card(inner).width(Length::Fill).into()
}

fn new_rule_card<'a>(nr: NewRule, ui: &'a MailState) -> Element<'a, MailMsg> {
    let key = nr.key;
    let title = if nr.draft.name.is_empty() {
        "(new rule)".to_string()
    } else {
        nr.draft.name.clone()
    };

    let actions = row![
        button(text("Save").size(13))
            .style(kit_btn::primary)
            .padding(Padding::new(6.0).left(12.0).right(12.0))
            .on_press(MailMsg::NewSave(key)),
        button(text("Discard").size(13))
            .style(kit_btn::ghost)
            .padding(Padding::new(6.0).left(12.0).right(12.0))
            .on_press(MailMsg::NewDiscard(key)),
    ]
    .spacing(8)
    .align_y(iced::Alignment::Center);

    let body_input = rule_body(
        nr.draft,
        move |f, v| MailMsg::NewField(key, f, v),
        move |action| MailMsg::NewAction(key, action),
        move |cond_idx, f, v| MailMsg::NewCondField(key, cond_idx, f, v),
        move || MailMsg::NewAddCondition(key),
        move |cond_idx| MailMsg::NewRemoveCondition(key, cond_idx),
    );

    let mut inner = column![
        text(title).font(fonts::ui_medium()).size(14).style(kit_text::muted),
        body_input,
        actions,
    ]
    .spacing(FIELD_GAP);
    if let Some(err) = ui.errors.get(&new_err_key(key)) {
        inner = inner.push(text(err.as_str()).size(12).style(kit_text::muted));
    }
    card(inner).width(Length::Fill).into()
}

/// Shared body renderer for both existing and new rule cards. Takes
/// the draft by value so the returned widget owns the strings it
/// reads — the caller's `RuleDraft` is per-render scratch state, so
/// borrowing it would tangle the returned `Element`'s lifetime with
/// the caller's stack frame.
fn rule_body<'a, FName, FAction, FCond, FAdd, FRm>(
    draft: RuleDraft,
    on_field: FName,
    on_action: FAction,
    on_cond: FCond,
    on_add_cond: FAdd,
    on_rm_cond: FRm,
) -> Element<'a, MailMsg>
where
    FName: Fn(RuleField, String) -> MailMsg + Clone + 'a,
    FAction: Fn(String) -> MailMsg + Clone + 'a,
    FCond: Fn(usize, CondField, String) -> MailMsg + Clone + 'a,
    FAdd: Fn() -> MailMsg + Clone + 'a,
    FRm: Fn(usize) -> MailMsg + Clone + 'a,
{
    let on_name = on_field.clone();
    let name_input = text_input("rule name", &draft.name)
        .on_input(move |v| on_name(RuleField::Name, v))
        .padding(Padding::new(6.0).left(10.0).right(10.0))
        .size(13)
        .style(kit_input::style);

    let on_act = on_action.clone();
    let action_picker = pick_list(
        as_string_vec(&ACTION_OPTIONS),
        Some(draft.action.clone()),
        move |a: String| on_act(a),
    )
    .text_size(13)
    .padding(Padding::new(6.0).left(10.0).right(10.0));

    let dest_block: Element<'_, MailMsg> = if draft.action == "move" {
        let on_dest = on_field.clone();
        let dest_input = text_input("mailbox (e.g. Trash)", &draft.dest)
            .on_input(move |v| on_dest(RuleField::Dest, v))
            .padding(Padding::new(6.0).left(10.0).right(10.0))
            .size(13)
            .style(kit_input::style);
        field("Destination", dest_input, None).into()
    } else {
        iced::widget::Space::new().height(Length::Fixed(0.0)).into()
    };

    let conditions_header = text("Conditions (all must match)")
        .font(fonts::ui_medium())
        .size(12);

    let mut cond_col = column![].spacing(8);
    for (i, c) in draft.conditions.iter().enumerate() {
        let on_field_pick = on_cond.clone();
        let field_pick = pick_list(
            as_string_vec(&FIELD_OPTIONS),
            Some(c.field.clone()),
            move |v: String| on_field_pick(i, CondField::Field, v),
        )
        .text_size(13)
        .padding(Padding::new(6.0).left(10.0).right(10.0));

        let on_match_pick = on_cond.clone();
        let match_pick = pick_list(
            as_string_vec(&MATCH_OPTIONS),
            Some(c.match_type.clone()),
            move |v: String| on_match_pick(i, CondField::Match, v),
        )
        .text_size(13)
        .padding(Padding::new(6.0).left(10.0).right(10.0));

        let on_val = on_cond.clone();
        let value_owned = c.value.clone();
        let value_input = text_input("value", &value_owned)
            .on_input(move |v| on_val(i, CondField::Value, v))
            .padding(Padding::new(6.0).left(10.0).right(10.0))
            .size(13)
            .style(kit_input::style)
            .width(Length::Fill);

        let on_rm = on_rm_cond.clone();
        let row = row![
            field_pick,
            match_pick,
            value_input,
            button(text("Remove").size(12))
                .style(kit_btn::ghost)
                .padding(Padding::new(4.0).left(8.0).right(8.0))
                .on_press(on_rm(i)),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center);
        cond_col = cond_col.push(row);
    }
    cond_col = cond_col.push(
        button(text("+ Add condition").size(13))
            .style(kit_btn::ghost)
            .padding(Padding::new(4.0).left(8.0).right(8.0))
            .on_press(on_add_cond()),
    );

    column![
        field("Name", name_input, None),
        field("Action", action_picker, None),
        dest_block,
        conditions_header,
        cond_col,
    ]
    .spacing(FIELD_GAP)
    .into()
}

fn as_string_vec(slice: &[&str]) -> Vec<String> {
    slice.iter().map(|s| (*s).to_string()).collect()
}

fn account_input<'a>(
    label: &'a str,
    value: &'a str,
    f: AccountField,
) -> Element<'a, MailMsg> {
    let input = text_input("", value)
        .on_input(move |v| MailMsg::AccountField(f, v))
        .padding(Padding::new(6.0).left(10.0).right(10.0))
        .size(13)
        .style(kit_input::style)
        .width(Length::Fill);
    field(label, input, None).into()
}

fn password_input<'a>(label: &'a str, value: &'a str) -> Element<'a, MailMsg> {
    let input = text_input("", value)
        .on_input(|v| MailMsg::AccountField(AccountField::Password, v))
        .secure(true)
        .padding(Padding::new(6.0).left(10.0).right(10.0))
        .size(13)
        .style(kit_input::style)
        .width(Length::Fill);
    field(label, input, None).into()
}

// ── Helpers ───────────────────────────────────────────────────────

fn validate(draft: &RuleDraft) -> Result<(), String> {
    if draft.name.trim().is_empty() {
        return Err("rule name is required".into());
    }
    if draft.conditions.is_empty() {
        return Err("at least one condition is required".into());
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

fn existing_err_key(idx: usize) -> String {
    format!("rule-{idx}")
}

fn new_err_key(key: u64) -> String {
    format!("new-{key}")
}

fn emit(topic: Topic) {
    if let Err(e) = bus().lock().unwrap().emit(topic) {
        tracing::warn!("bus emit failed: {e}");
    }
}
