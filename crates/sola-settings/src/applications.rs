//! Applications panel — configured launcher entries plus
//! "running, not configured" candidates suggested from
//! `Topic::Windows`. Persistence is bus-side: every save round-trips
//! through `Topic::Application` (a persistent topic keyed by
//! `app_id`), and the resulting replay is what updates our canonical
//! `ApplicationsConfig`.
//!
//! The legacy web panel committed edits 500 ms after the last
//! keystroke. The iced port uses explicit Save / Discard buttons
//! per row instead, matching the Mail panel's pattern and removing
//! the per-row debounce timer state.
//!
//! Builtin apps (defined in sola-shell) never appear here — they
//! are seeded by the shell directly and are not part of the
//! `Topic::Application` stream.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};

use iced::widget::{button, column, row, text, text_input};
use iced::{Element, Length, Padding, Task, Theme};

use sola_bus::topics::{Application, ApplicationsConfig, Topic, Window as BusWindow};
use sola_kit::app::bus;
use sola_kit::components::text as kit_text;
use sola_kit::components::{Tone, badge, button as kit_btn, card, field, text_input as kit_input};
use sola_kit::fonts;

use crate::procfs;

const FIELD_GAP: f32 = 12.0;
const CARD_GAP: f32 = 16.0;

#[derive(Debug, Clone, Copy)]
pub enum AppField {
    Id,
    Label,
    Command,
    Icon,
}

#[derive(Debug, Clone)]
pub struct DraftRow {
    pub key: u64,
    pub app_id: String,
    pub label: String,
    pub command: String,
    pub icon: String,
}

#[derive(Debug, Clone, Default)]
pub struct EditBuffer {
    pub app_id: String,
    pub label: String,
    pub command: String,
    pub icon: String,
}

impl EditBuffer {
    fn from_app(a: &Application) -> Self {
        Self {
            app_id: a.app_id.clone(),
            label: a.label.clone(),
            command: a.command.clone(),
            icon: a.icon.clone(),
        }
    }
    fn matches(&self, a: &Application) -> bool {
        self.app_id == a.app_id
            && self.label == a.label
            && self.command == a.command
            && self.icon == a.icon
    }
    fn to_application(&self) -> Application {
        let mut a = Application {
            app_id: self.app_id.trim().to_string(),
            label: self.label.trim().to_string(),
            command: self.command.trim().to_string(),
            icon: self.icon.trim().to_string(),
        };
        a.normalize();
        a
    }
}

#[derive(Default)]
pub struct AppsState {
    pub drafts: Vec<DraftRow>,
    /// Keyed by the canonical (original) app_id of the row being
    /// edited; stays stable across rename until commit clears it.
    pub edits: BTreeMap<String, EditBuffer>,
    /// Inline error messages, keyed by `draft-<key>` or
    /// `app-<original_app_id>`.
    pub errors: BTreeMap<String, String>,
}

static DRAFT_SEQ: AtomicU64 = AtomicU64::new(1);

fn next_key() -> u64 {
    DRAFT_SEQ.fetch_add(1, Ordering::Relaxed)
}

#[derive(Debug, Clone)]
pub enum AppsMsg {
    StartBlank,
    StartFromCandidate {
        app_id: String,
        command: Option<String>,
    },
    DraftField {
        key: u64,
        field: AppField,
        value: String,
    },
    DraftCommit(u64),
    DraftDiscard(u64),
    EditField {
        orig: String,
        field: AppField,
        value: String,
    },
    EditSave(String),
    EditDiscard(String),
    Remove(String),
}

pub fn update(
    msg: AppsMsg,
    apps: &mut ApplicationsConfig,
    ui: &mut AppsState,
) -> Task<AppsMsg> {
    match msg {
        AppsMsg::StartBlank => {
            ui.drafts.push(DraftRow {
                key: next_key(),
                app_id: String::new(),
                label: String::new(),
                command: String::new(),
                icon: String::new(),
            });
        }
        AppsMsg::StartFromCandidate { app_id, command } => {
            ui.drafts.insert(
                0,
                DraftRow {
                    key: next_key(),
                    app_id: app_id.clone(),
                    label: app_id,
                    command: command.unwrap_or_default(),
                    icon: String::new(),
                },
            );
        }
        AppsMsg::DraftField { key, field, value } => {
            if let Some(d) = ui.drafts.iter_mut().find(|d| d.key == key) {
                set_field(field, value, &mut d.app_id, &mut d.label, &mut d.command, &mut d.icon);
                ui.errors.remove(&draft_error_key(key));
            }
        }
        AppsMsg::DraftCommit(key) => {
            let Some(draft) = ui.drafts.iter().find(|d| d.key == key).cloned() else {
                return Task::none();
            };
            if draft.app_id.trim().is_empty()
                || draft.label.trim().is_empty()
                || draft.command.trim().is_empty()
            {
                ui.errors.insert(
                    draft_error_key(key),
                    "app_id, label, and command are required".into(),
                );
                return Task::none();
            }
            let mut new_app = Application {
                app_id: draft.app_id.trim().to_string(),
                label: draft.label.trim().to_string(),
                command: draft.command.trim().to_string(),
                icon: draft.icon.trim().to_string(),
            };
            new_app.normalize();
            match apps.add(new_app.clone()) {
                Ok(()) => {
                    emit(Topic::Application(new_app));
                    ui.drafts.retain(|d| d.key != key);
                    ui.errors.remove(&draft_error_key(key));
                }
                Err(e) => {
                    ui.errors.insert(draft_error_key(key), e.to_string());
                }
            }
        }
        AppsMsg::DraftDiscard(key) => {
            ui.drafts.retain(|d| d.key != key);
            ui.errors.remove(&draft_error_key(key));
        }
        AppsMsg::EditField { orig, field, value } => {
            // Lazy-create the edit buffer from current canonical state.
            let buf = ui
                .edits
                .entry(orig.clone())
                .or_insert_with(|| match apps.get(&orig) {
                    Some(a) => EditBuffer::from_app(a),
                    None => EditBuffer::default(),
                });
            set_field(field, value, &mut buf.app_id, &mut buf.label, &mut buf.command, &mut buf.icon);
            ui.errors.remove(&edit_error_key(&orig));
        }
        AppsMsg::EditSave(orig) => {
            let Some(buf) = ui.edits.get(&orig).cloned() else {
                return Task::none();
            };
            if buf.app_id.trim().is_empty()
                || buf.label.trim().is_empty()
                || buf.command.trim().is_empty()
            {
                ui.errors.insert(
                    edit_error_key(&orig),
                    "app_id, label, and command are required".into(),
                );
                return Task::none();
            }
            let new_app = buf.to_application();
            let id_changed = new_app.app_id != orig;
            let prev = apps.get(&orig).cloned();
            match apps.update(&orig, new_app.clone()) {
                Ok(()) => {
                    if id_changed
                        && let Some(old) = prev
                    {
                        retract(Topic::Application(old));
                    }
                    emit(Topic::Application(new_app));
                    ui.edits.remove(&orig);
                    ui.errors.remove(&edit_error_key(&orig));
                }
                Err(e) => {
                    ui.errors.insert(edit_error_key(&orig), e.to_string());
                }
            }
        }
        AppsMsg::EditDiscard(orig) => {
            ui.edits.remove(&orig);
            ui.errors.remove(&edit_error_key(&orig));
        }
        AppsMsg::Remove(app_id) => {
            if let Some(removed) = apps.get(&app_id).cloned() {
                apps.remove(&app_id);
                ui.edits.remove(&app_id);
                ui.errors.remove(&edit_error_key(&app_id));
                retract(Topic::Application(removed));
            }
        }
    }
    Task::none()
}

pub fn view<'a>(
    apps: &'a ApplicationsConfig,
    running: &'a [BusWindow],
    ui: &'a AppsState,
) -> Element<'a, AppsMsg> {
    let mut col = column![].spacing(CARD_GAP);

    if apps.apps.is_empty() && ui.drafts.is_empty() {
        col = col.push(
            text("No applications configured. Click \"+ Add application\" or pick from the candidates below.")
                .size(13)
                .style(kit_text::muted),
        );
    }

    for draft in &ui.drafts {
        col = col.push(draft_card(draft, ui));
    }
    for app in &apps.apps {
        col = col.push(configured_card(app, ui));
    }

    col = col.push(
        button(text("+ Add application").size(13))
            .style(kit_btn::ghost)
            .padding(Padding::new(6.0).left(10.0).right(10.0))
            .on_press(AppsMsg::StartBlank),
    );

    let candidates = collect_candidates(apps, running);
    if !candidates.is_empty() {
        col = col.push(candidates_card(candidates));
    }

    col.into()
}

// ── view helpers ───────────────────────────────────────────────────

fn configured_card<'a>(app: &'a Application, ui: &'a AppsState) -> Element<'a, AppsMsg> {
    let buf = ui.edits.get(&app.app_id);
    // Read straight from the edit buffer (if any) so the returned
    // widget borrows from data with the same `'a` lifetime as the
    // caller's `ui` — synthesizing a local `EditBuffer` here would
    // not outlive this function's stack frame.
    let app_id = buf.map(|b| b.app_id.as_str()).unwrap_or(&app.app_id);
    let label = buf.map(|b| b.label.as_str()).unwrap_or(&app.label);
    let command = buf.map(|b| b.command.as_str()).unwrap_or(&app.command);
    let icon = buf.map(|b| b.icon.as_str()).unwrap_or(&app.icon);
    let dirty = buf.map(|b| !b.matches(app)).unwrap_or(false);

    let missing = !sola_core::applications::command_exists(&app.command);
    let display_title: &str = if app.label.is_empty() {
        app.app_id.as_str()
    } else {
        app.label.as_str()
    };

    let mut header = row![
        text(display_title).font(fonts::ui_medium()).size(16),
    ]
    .spacing(8)
    .align_y(iced::Alignment::Center);
    if missing {
        header = header.push(badge("not found", Tone::Warning));
    }
    header = header.push(iced::widget::Space::new().width(Length::Fill));
    if dirty {
        let orig_save = app.app_id.clone();
        let orig_discard = app.app_id.clone();
        header = header
            .push(
                button(text("Save").size(13))
                    .style(kit_btn::primary)
                    .padding(Padding::new(6.0).left(12.0).right(12.0))
                    .on_press(AppsMsg::EditSave(orig_save)),
            )
            .push(
                button(text("Discard").size(13))
                    .style(kit_btn::ghost)
                    .padding(Padding::new(6.0).left(12.0).right(12.0))
                    .on_press(AppsMsg::EditDiscard(orig_discard)),
            );
    }
    header = header.push(
        button(text("Remove").size(13))
            .style(kit_btn::danger)
            .padding(Padding::new(6.0).left(12.0).right(12.0))
            .on_press(AppsMsg::Remove(app.app_id.clone())),
    );

    let orig_for_inputs = app.app_id.clone();
    let row1 = row![
        edit_text_input("app_id", app_id, orig_for_inputs.clone(), AppField::Id),
        edit_text_input("label", label, orig_for_inputs.clone(), AppField::Label),
        edit_text_input("icon", icon, orig_for_inputs.clone(), AppField::Icon),
    ]
    .spacing(FIELD_GAP);
    let row2 = edit_text_input("command", command, orig_for_inputs, AppField::Command);

    let mut body = column![header, row1, row2].spacing(FIELD_GAP);
    if let Some(err) = ui.errors.get(&edit_error_key(&app.app_id)) {
        body = body.push(text(err.as_str()).size(12).style(kit_text::muted));
    }

    card(body).width(Length::Fill).into()
}

fn draft_card<'a>(draft: &'a DraftRow, ui: &'a AppsState) -> Element<'a, AppsMsg> {
    let header = row![
        text("New application")
            .font(fonts::ui_medium())
            .size(16)
            .style(kit_text::muted),
        iced::widget::Space::new().width(Length::Fill),
        button(text("Add").size(13))
            .style(kit_btn::primary)
            .padding(Padding::new(6.0).left(12.0).right(12.0))
            .on_press(AppsMsg::DraftCommit(draft.key)),
        button(text("Discard").size(13))
            .style(kit_btn::ghost)
            .padding(Padding::new(6.0).left(12.0).right(12.0))
            .on_press(AppsMsg::DraftDiscard(draft.key)),
    ]
    .spacing(8)
    .align_y(iced::Alignment::Center);

    let row1 = row![
        draft_text_input("app_id", &draft.app_id, "firefox", draft.key, AppField::Id),
        draft_text_input("label", &draft.label, "Firefox", draft.key, AppField::Label),
        draft_text_input(
            "icon",
            &draft.icon,
            "simpleicons/firefox",
            draft.key,
            AppField::Icon,
        ),
    ]
    .spacing(FIELD_GAP);
    let row2 = draft_text_input(
        "command",
        &draft.command,
        "firefox",
        draft.key,
        AppField::Command,
    );

    let mut body = column![header, row1, row2].spacing(FIELD_GAP);
    if let Some(err) = ui.errors.get(&draft_error_key(draft.key)) {
        body = body.push(text(err.as_str()).size(12).style(kit_text::muted));
    }

    card(body).width(Length::Fill).into()
}

fn candidates_card<'a>(candidates: Vec<Candidate>) -> Element<'a, AppsMsg> {
    let mut col = column![
        text("Running, not configured")
            .font(fonts::ui_medium())
            .size(14),
        text("Pre-filled by what's currently running. One click drops a draft above.")
            .size(12)
            .style(kit_text::muted),
    ]
    .spacing(6);

    for c in candidates {
        let title_owned: String = if c.title.is_empty() {
            "(no title)".to_string()
        } else {
            c.title.clone()
        };
        let detail = if let Some(cmd) = &c.suggested_command {
            format!("{title_owned} · {cmd}")
        } else {
            format!("{title_owned} · command unknown — fill in manually")
        };
        let app_id_for_text = c.app_id.clone();
        let row = row![
            column![
                text(app_id_for_text).size(13),
                text(detail).size(12).style(kit_text::muted),
            ]
            .spacing(2)
            .width(Length::Fill),
            button(text("Configure").size(13))
                .style(kit_btn::ghost)
                .padding(Padding::new(6.0).left(12.0).right(12.0))
                .on_press(AppsMsg::StartFromCandidate {
                    app_id: c.app_id,
                    command: c.suggested_command,
                }),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center);
        col = col.push(row);
    }

    card(col.spacing(10)).width(Length::Fill).into()
}

fn draft_text_input<'a>(
    label: &'a str,
    value: &'a str,
    placeholder: &'a str,
    key: u64,
    f: AppField,
) -> Element<'a, AppsMsg> {
    let input = text_input(placeholder, value)
        .on_input(move |v| AppsMsg::DraftField { key, field: f, value: v })
        .padding(Padding::new(6.0).left(10.0).right(10.0))
        .size(13)
        .style(kit_input::style)
        .width(Length::Fill);
    field(label, input, None)
}

fn edit_text_input<'a>(
    label: &'a str,
    value: &'a str,
    orig: String,
    f: AppField,
) -> Element<'a, AppsMsg> {
    let input = text_input("", value)
        .on_input(move |v| AppsMsg::EditField {
            orig: orig.clone(),
            field: f,
            value: v,
        })
        .padding(Padding::new(6.0).left(10.0).right(10.0))
        .size(13)
        .style(kit_input::style)
        .width(Length::Fill);
    field(label, input, None)
}

// ── candidate derivation ───────────────────────────────────────────

struct Candidate {
    app_id: String,
    title: String,
    suggested_command: Option<String>,
}

fn collect_candidates(apps: &ApplicationsConfig, running: &[BusWindow]) -> Vec<Candidate> {
    use std::collections::HashSet;
    let configured: HashSet<&str> = apps.apps.iter().map(|a| a.app_id.as_str()).collect();
    let mut seen: HashSet<String> = HashSet::new();
    let mut out = Vec::new();
    for w in running {
        if configured.contains(w.app_id.as_str()) {
            continue;
        }
        if procfs::is_system_app(&w.app_id) {
            continue;
        }
        if !seen.insert(w.app_id.clone()) {
            continue;
        }
        let suggested = procfs::suggest_command(&w.app_id, w.pid);
        out.push(Candidate {
            app_id: w.app_id.clone(),
            title: w.title.clone(),
            suggested_command: suggested,
        });
    }
    out
}

// ── small helpers ──────────────────────────────────────────────────

fn set_field(
    f: AppField,
    value: String,
    id: &mut String,
    label: &mut String,
    command: &mut String,
    icon: &mut String,
) {
    match f {
        AppField::Id => *id = value,
        AppField::Label => *label = value,
        AppField::Command => *command = value,
        AppField::Icon => *icon = value,
    }
}

fn draft_error_key(key: u64) -> String {
    format!("draft-{key}")
}

fn edit_error_key(orig: &str) -> String {
    format!("app-{orig}")
}

fn emit(topic: Topic) {
    if let Err(e) = bus().lock().unwrap().emit(topic) {
        tracing::warn!("bus emit failed: {e}");
    }
}

fn retract(topic: Topic) {
    if let Err(e) = bus().lock().unwrap().retract(topic) {
        tracing::warn!("bus retract failed: {e}");
    }
}

// Used internally below — silences `Theme` unused if helpers don't need it.
const _: fn(&Theme) = |_| {};
