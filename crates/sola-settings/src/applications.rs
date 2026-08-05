//! Applications panel — configured launcher entries plus
//! "running, not configured" candidates suggested from
//! `Topic::Windows`. Persistence is bus-side: every save round-trips
//! through `Topic::Application` (a persistent topic keyed by
//! `app_id`), and the resulting replay is what updates our canonical
//! `ApplicationsConfig`.
//!
//! Compact A→Z list (left) + fixed-width detail panel (right). Single
//! open editor (edit or draft); dirty state locks selection / blank
//! draft / candidate configure until Save or Discard.
//!
//! Builtin apps (defined in sola-shell) never appear here — they
//! are seeded by the shell directly and are not part of the
//! `Topic::Application` stream.

use iced::widget::{button, column, container, row};
use iced::{Element, Length, Task};
use sola_kit::components::text_input::text_input;

use sola_bus::topics::{Application, ApplicationsConfig, Topic, Window as BusWindow};
use sola_kit::app::bus;
use sola_kit::components::style::{PAD_CONTROL, SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XL, SPACE_XS};
use sola_kit::components::text as kit_text;
use sola_kit::components::{Tone, badge, button as kit_btn, card, field, text_input as kit_input};

use crate::procfs;

/// Fixed width of the right-hand detail panel (px).
const DETAIL_WIDTH: f32 = 400.0;

#[derive(Debug, Clone, Copy)]
pub enum AppField {
    Id,
    Label,
    Command,
    Icon,
}

#[derive(Debug, Clone, Default)]
pub struct EditBuffer {
    pub app_id: String,
    pub label: String,
    pub command: String,
    pub icon: String,
}

impl EditBuffer {
    pub fn from_app(a: &Application) -> Self {
        Self {
            app_id: a.app_id.clone(),
            label: a.label.clone(),
            command: a.command.clone(),
            icon: a.icon.clone(),
        }
    }
    pub fn matches(&self, a: &Application) -> bool {
        self.app_id == a.app_id
            && self.label == a.label
            && self.command == a.command
            && self.icon == a.icon
    }
    pub fn to_application(&self) -> Application {
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

/// Single detail panel selection for the list+detail Applications UI.
#[derive(Debug, Clone, Default)]
pub enum Detail {
    #[default]
    Closed,
    Edit {
        orig: String,
        buffer: EditBuffer,
    },
    Draft(EditBuffer),
}

#[derive(Default)]
pub struct AppsState {
    pub detail: Detail,
    pub error: Option<String>,
}

/// Label if non-empty after trim, otherwise `app_id`.
pub fn display_title(app: &Application) -> &str {
    if app.label.trim().is_empty() {
        app.app_id.as_str()
    } else {
        app.label.as_str()
    }
}

/// Case-insensitive sort key from [`display_title`].
pub fn sort_key(app: &Application) -> String {
    display_title(app).to_ascii_lowercase()
}

/// Configured apps sorted by [`sort_key`], then `app_id`.
pub fn sorted_apps(apps: &ApplicationsConfig) -> Vec<&Application> {
    let mut v: Vec<&Application> = apps.apps.iter().collect();
    v.sort_by(|a, b| {
        sort_key(a)
            .cmp(&sort_key(b))
            .then_with(|| a.app_id.cmp(&b.app_id))
    });
    v
}

/// Draft is dirty when any field has non-whitespace content.
pub fn draft_is_dirty(buf: &EditBuffer) -> bool {
    !buf.app_id.trim().is_empty()
        || !buf.label.trim().is_empty()
        || !buf.command.trim().is_empty()
        || !buf.icon.trim().is_empty()
}

/// Edit is dirty when the buffer differs from the canonical app.
pub fn edit_is_dirty(buf: &EditBuffer, canonical: &Application) -> bool {
    !buf.matches(canonical)
}

/// Whether the user may leave the current detail without discard/save.
/// Blank drafts and clean edits may leave; dirty draft/edit may not.
pub fn can_leave_detail(detail: &Detail, apps: &ApplicationsConfig) -> bool {
    match detail {
        Detail::Closed => true,
        Detail::Draft(buf) => !draft_is_dirty(buf),
        Detail::Edit { orig, buffer } => match apps.get(orig) {
            Some(canonical) => !edit_is_dirty(buffer, canonical),
            // Canonical gone — treat as free to leave (on_apps_changed will close).
            None => true,
        },
    }
}

#[derive(Debug, Clone)]
pub enum AppsMsg {
    Select(String),
    StartBlank,
    StartFromCandidate {
        app_id: String,
        command: Option<String>,
    },
    Field {
        field: AppField,
        value: String,
    },
    /// Edit: commit; draft: add.
    Save,
    /// Edit: reset buffer; draft: close.
    Discard,
    CloseDetail,
    Remove(String),
}

pub fn update(
    msg: AppsMsg,
    apps: &mut ApplicationsConfig,
    ui: &mut AppsState,
) -> Task<AppsMsg> {
    match msg {
        AppsMsg::Select(id) => {
            if !can_leave_detail(&ui.detail, apps) {
                return Task::none();
            }
            if let Some(a) = apps.get(&id) {
                ui.detail = Detail::Edit {
                    orig: id,
                    buffer: EditBuffer::from_app(a),
                };
                ui.error = None;
            }
        }
        AppsMsg::StartBlank => {
            if !can_leave_detail(&ui.detail, apps) {
                return Task::none();
            }
            ui.detail = Detail::Draft(EditBuffer::default());
            ui.error = None;
        }
        AppsMsg::StartFromCandidate { app_id, command } => {
            if !can_leave_detail(&ui.detail, apps) {
                return Task::none();
            }
            ui.detail = Detail::Draft(EditBuffer {
                app_id: app_id.clone(),
                label: app_id,
                command: command.unwrap_or_default(),
                icon: String::new(),
            });
            ui.error = None;
        }
        AppsMsg::Field { field, value } => {
            if let Some(buf) = open_buffer_mut(&mut ui.detail) {
                set_field(
                    field,
                    value,
                    &mut buf.app_id,
                    &mut buf.label,
                    &mut buf.command,
                    &mut buf.icon,
                );
                ui.error = None;
            }
        }
        AppsMsg::Save => match &ui.detail {
            Detail::Edit { orig, buffer } => {
                let orig = orig.clone();
                let buf = buffer.clone();
                if required_fields_missing(&buf) {
                    ui.error = Some("app_id, label, and command are required".into());
                    return Task::none();
                }
                let new_app = buf.to_application();
                let id_changed = new_app.app_id != orig;
                let prev = apps.get(&orig).cloned();
                match apps.update(&orig, new_app.clone()) {
                    Ok(()) => {
                        if id_changed && let Some(old) = prev {
                            retract(Topic::Application(old));
                        }
                        emit(Topic::Application(new_app.clone()));
                        ui.detail = Detail::Edit {
                            orig: new_app.app_id.clone(),
                            buffer: EditBuffer::from_app(&new_app),
                        };
                        ui.error = None;
                    }
                    Err(e) => {
                        ui.error = Some(e.to_string());
                    }
                }
            }
            Detail::Draft(buf) => {
                let buf = buf.clone();
                if required_fields_missing(&buf) {
                    ui.error = Some("app_id, label, and command are required".into());
                    return Task::none();
                }
                let new_app = buf.to_application();
                match apps.add(new_app.clone()) {
                    Ok(()) => {
                        emit(Topic::Application(new_app.clone()));
                        ui.detail = Detail::Edit {
                            orig: new_app.app_id.clone(),
                            buffer: EditBuffer::from_app(&new_app),
                        };
                        ui.error = None;
                    }
                    Err(e) => {
                        ui.error = Some(e.to_string());
                    }
                }
            }
            Detail::Closed => {}
        },
        AppsMsg::Discard => match &ui.detail {
            Detail::Edit { orig, .. } => {
                let orig = orig.clone();
                if let Some(a) = apps.get(&orig) {
                    ui.detail = Detail::Edit {
                        orig,
                        buffer: EditBuffer::from_app(a),
                    };
                }
                ui.error = None;
            }
            Detail::Draft(_) => {
                ui.detail = Detail::Closed;
                ui.error = None;
            }
            Detail::Closed => {}
        },
        AppsMsg::CloseDetail => {
            if !can_leave_detail(&ui.detail, apps) {
                return Task::none();
            }
            match &ui.detail {
                Detail::Edit { .. } | Detail::Draft(_) => {
                    ui.detail = Detail::Closed;
                    ui.error = None;
                }
                Detail::Closed => {}
            }
        }
        AppsMsg::Remove(app_id) => {
            if let Some(removed) = apps.get(&app_id).cloned() {
                apps.remove(&app_id);
                if matches!(&ui.detail, Detail::Edit { orig, .. } if orig == &app_id) {
                    ui.detail = Detail::Closed;
                    ui.error = None;
                }
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
    let list = list_column(apps, running, ui);
    let detail = detail_panel(apps, ui);
    row![list, detail]
        .spacing(SPACE_XL)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

// ── view helpers ───────────────────────────────────────────────────

fn list_column<'a>(
    apps: &'a ApplicationsConfig,
    running: &'a [BusWindow],
    ui: &'a AppsState,
) -> Element<'a, AppsMsg> {
    let mut col = column![
        kit_btn::labeled("+ Add application", kit_btn::ghost).on_press(AppsMsg::StartBlank),
    ]
    .spacing(SPACE_MD)
    .width(Length::Fill);

    if apps.apps.is_empty() {
        col = col.push(
            kit_text::body(
                "No applications configured. Click \"+ Add application\" or pick from the candidates below.",
            )
            .style(kit_text::muted),
        );
    }

    let mut rows = column![].spacing(SPACE_XS).width(Length::Fill);
    for app in sorted_apps(apps) {
        rows = rows.push(app_row(app, ui));
    }
    col = col.push(rows);

    let candidates = collect_candidates(apps, running);
    if !candidates.is_empty() {
        col = col.push(candidates_card(candidates));
    }

    col.height(Length::Fill).into()
}

fn app_row<'a>(app: &'a Application, ui: &'a AppsState) -> Element<'a, AppsMsg> {
    let selected = matches!(
        &ui.detail,
        Detail::Edit { orig, .. } if orig == &app.app_id
    );
    let missing = !sola_core::applications::command_exists(&app.command);

    let mut hit = row![kit_text::body(display_title(app))]
        .spacing(SPACE_MD)
        .align_y(iced::Alignment::Center)
        .width(Length::Fill);
    if missing {
        hit = hit.push(badge("not found", Tone::Warning));
    }

    row![
        button(hit)
            .on_press(AppsMsg::Select(app.app_id.clone()))
            .padding(PAD_CONTROL)
            .width(Length::Fill)
            .style(kit_btn::list_item(selected)),
        kit_btn::labeled("Remove", kit_btn::danger)
            .on_press(AppsMsg::Remove(app.app_id.clone())),
    ]
    .spacing(SPACE_MD)
    .align_y(iced::Alignment::Center)
    .width(Length::Fill)
    .into()
}

fn detail_panel<'a>(apps: &'a ApplicationsConfig, ui: &'a AppsState) -> Element<'a, AppsMsg> {
    let body: Element<'a, AppsMsg> = match &ui.detail {
        Detail::Closed => kit_text::body("Select an app or add one")
            .style(kit_text::muted)
            .into(),
        Detail::Edit { orig, buffer } => {
            let dirty = apps
                .get(orig)
                .map(|canonical| edit_is_dirty(buffer, canonical))
                .unwrap_or(true);
            let title = if buffer.label.trim().is_empty() {
                buffer.app_id.as_str()
            } else {
                buffer.label.as_str()
            };
            let title = if title.is_empty() {
                orig.as_str()
            } else {
                title
            };

            let mut footer = row![].spacing(SPACE_MD).align_y(iced::Alignment::Center);
            if dirty {
                footer = footer
                    .push(kit_btn::labeled("Save", kit_btn::primary).on_press(AppsMsg::Save))
                    .push(
                        kit_btn::labeled("Discard", kit_btn::ghost).on_press(AppsMsg::Discard),
                    );
            }
            footer = footer
                .push(iced::widget::Space::new().width(Length::Fill))
                .push(kit_btn::labeled("Close", kit_btn::ghost).on_press(AppsMsg::CloseDetail));

            let mut col = column![
                kit_text::subheading(title),
                detail_fields(buffer, /* draft placeholders */ false),
            ]
            .spacing(SPACE_LG);
            if let Some(err) = ui.error.as_deref() {
                col = col.push(kit_text::caption(err).style(kit_text::danger));
            }
            col = col.push(footer);
            col.into()
        }
        Detail::Draft(buffer) => {
            let footer = row![
                kit_btn::labeled("Add", kit_btn::primary).on_press(AppsMsg::Save),
                kit_btn::labeled("Discard", kit_btn::ghost).on_press(AppsMsg::Discard),
            ]
            .spacing(SPACE_MD)
            .align_y(iced::Alignment::Center);

            let mut col = column![
                kit_text::subheading("New application").style(kit_text::muted),
                detail_fields(buffer, /* draft placeholders */ true),
            ]
            .spacing(SPACE_LG);
            if let Some(err) = ui.error.as_deref() {
                col = col.push(kit_text::caption(err).style(kit_text::danger));
            }
            col = col.push(footer);
            col.into()
        }
    };

    container(card(body).width(Length::Fill).height(Length::Fill))
        .width(Length::Fixed(DETAIL_WIDTH))
        .height(Length::Fill)
        .into()
}

fn detail_fields<'a>(buf: &'a EditBuffer, draft_placeholders: bool) -> Element<'a, AppsMsg> {
    let (ph_id, ph_label, ph_icon, ph_cmd) = if draft_placeholders {
        ("firefox", "Firefox", "simpleicons/firefox", "firefox")
    } else {
        ("", "", "", "")
    };
    column![
        field_input("app_id", &buf.app_id, ph_id, AppField::Id),
        field_input("label", &buf.label, ph_label, AppField::Label),
        field_input("icon", &buf.icon, ph_icon, AppField::Icon),
        field_input("command", &buf.command, ph_cmd, AppField::Command),
    ]
    .spacing(SPACE_LG)
    .into()
}

fn field_input<'a>(
    label: &'a str,
    value: &'a str,
    placeholder: &'a str,
    f: AppField,
) -> Element<'a, AppsMsg> {
    let input = text_input(placeholder, value)
        .on_input(move |v| AppsMsg::Field { field: f, value: v })
        .size(13)
        .style(kit_input::style)
        .width(Length::Fill);
    field(label, input, None, None).into()
}

fn candidates_card<'a>(candidates: Vec<Candidate>) -> Element<'a, AppsMsg> {
    let mut col = column![
        kit_text::subheading("Running, not configured"),
        kit_text::caption(
            "Pre-filled by what's currently running. Configure opens the detail panel.",
        )
        .style(kit_text::muted),
    ]
    .spacing(SPACE_SM);

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
        let row_el = row![
            column![
                kit_text::body(app_id_for_text),
                kit_text::caption(detail).style(kit_text::muted),
            ]
            .spacing(SPACE_XS)
            .width(Length::Fill),
            kit_btn::labeled("Configure", kit_btn::ghost).on_press(AppsMsg::StartFromCandidate {
                app_id: c.app_id,
                command: c.suggested_command,
            }),
        ]
        .spacing(SPACE_MD)
        .align_y(iced::Alignment::Center);
        col = col.push(row_el);
    }

    card(col.spacing(SPACE_LG)).width(Length::Fill).into()
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

fn open_buffer_mut(detail: &mut Detail) -> Option<&mut EditBuffer> {
    match detail {
        Detail::Edit { buffer, .. } | Detail::Draft(buffer) => Some(buffer),
        Detail::Closed => None,
    }
}

fn required_fields_missing(buf: &EditBuffer) -> bool {
    buf.app_id.trim().is_empty() || buf.label.trim().is_empty() || buf.command.trim().is_empty()
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use sola_bus::topics::Application;

    fn app(id: &str, label: &str) -> Application {
        Application {
            app_id: id.into(),
            label: label.into(),
            command: "true".into(),
            icon: String::new(),
        }
    }

    #[test]
    fn display_title_prefers_nonempty_label() {
        assert_eq!(display_title(&app("x", "Chrome")), "Chrome");
        assert_eq!(display_title(&app("x", "  ")), "x");
        assert_eq!(display_title(&app("x", "")), "x");
    }

    #[test]
    fn sorted_apps_orders_case_insensitive_by_label() {
        let mut cfg = ApplicationsConfig::default();
        cfg.apps = vec![
            app("z", "chrome"),
            app("a", "Bitwarden"),
            app("m", "Signal"),
        ];
        let ids: Vec<&str> = sorted_apps(&cfg).iter().map(|a| a.app_id.as_str()).collect();
        assert_eq!(ids, vec!["a", "z", "m"]); // Bitwarden, chrome, Signal
    }

    #[test]
    fn draft_dirty_when_any_field_nonempty() {
        assert!(!draft_is_dirty(&EditBuffer::default()));
        assert!(draft_is_dirty(&EditBuffer {
            label: "x".into(),
            ..Default::default()
        }));
    }

    #[test]
    fn can_leave_blank_draft_but_not_dirty_edit() {
        let mut cfg = ApplicationsConfig::default();
        let a = app("chrome", "Chrome");
        cfg.apps.push(a.clone());

        assert!(can_leave_detail(&Detail::Closed, &cfg));
        assert!(can_leave_detail(
            &Detail::Draft(EditBuffer::default()),
            &cfg
        ));
        assert!(!can_leave_detail(
            &Detail::Draft(EditBuffer {
                command: "x".into(),
                ..Default::default()
            }),
            &cfg
        ));

        let clean = EditBuffer::from_app(&a);
        assert!(can_leave_detail(
            &Detail::Edit {
                orig: "chrome".into(),
                buffer: clean.clone(),
            },
            &cfg
        ));
        let mut dirty = clean;
        dirty.label = "Nope".into();
        assert!(!can_leave_detail(
            &Detail::Edit {
                orig: "chrome".into(),
                buffer: dirty,
            },
            &cfg
        ));
    }
}
