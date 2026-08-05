//! Applications panel — configured launcher entries plus
//! "running, not configured" candidates suggested from
//! `Topic::Windows`. Persistence is bus-side: every save round-trips
//! through `Topic::Application` (a persistent topic keyed by
//! `app_id`), and the resulting replay is what updates our canonical
//! `ApplicationsConfig`.
//!
//! Single open detail editor (edit or draft). Dirty state locks
//! selection / blank draft / candidate configure until Save or Discard.
//!
//! Builtin apps (defined in sola-shell) never appear here — they
//! are seeded by the shell directly and are not part of the
//! `Topic::Application` stream.

use iced::{Element, Task};

use sola_bus::topics::{Application, ApplicationsConfig, Topic, Window as BusWindow};
use sola_kit::app::bus;
use sola_kit::components::text as kit_text;

// Constructed by the master–detail view (Task 3); update already handles Field.
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
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

// Pure helpers for list+detail (sort used once master list lands in Task 3).
#[allow(dead_code)]
/// Label if non-empty after trim, otherwise `app_id`.
pub fn display_title(app: &Application) -> &str {
    if app.label.trim().is_empty() {
        app.app_id.as_str()
    } else {
        app.label.as_str()
    }
}

#[allow(dead_code)]
/// Case-insensitive sort key from [`display_title`].
pub fn sort_key(app: &Application) -> String {
    display_title(app).to_ascii_lowercase()
}

#[allow(dead_code)]
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

// Variants are produced by the master–detail view (Task 3); update is wired now.
#[derive(Debug, Clone)]
#[allow(dead_code)]
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

/// Temporary stub until Task 3 master–detail view lands.
pub fn view<'a>(
    _apps: &'a ApplicationsConfig,
    _running: &'a [BusWindow],
    _ui: &'a AppsState,
) -> Element<'a, AppsMsg> {
    kit_text::body("Applications UI rebuild in progress").into()
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
