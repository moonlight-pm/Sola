//! Applications panel — configured launcher entries plus
//! "running, not configured" candidates suggested from
//! `Topic::Windows`. Persistence is bus-side: every save round-trips
//! through `Topic::Application` (a persistent topic keyed by
//! `app_id`), and the resulting replay is what updates our canonical
//! `ApplicationsConfig`.
//!
//! One A→Z list; the open app's editor sits under its row
//! ([`crate::page`]). Single open editor (edit or draft); dirty state
//! locks selection / blank draft / candidate configure until Save or
//! Discard.
//!
//! Builtin apps (defined in sola-shell) never appear here — they
//! are seeded by the shell directly and are not part of the
//! `Topic::Application` stream. Nor do web apps: the browser makes them
//! (Wrap This Page…) and each removes itself from its own menu.

use iced::widget::{column, row};
use iced::{Element, Length, Task};
use sola_kit::components::text_input::text_input;

use sola_bus::topics::{AppKind, Application, ApplicationsConfig, Topic, Window as BusWindow};
use sola_kit::app::bus;
use sola_kit::components::style::{SPACE_LG, SPACE_MD};
use sola_kit::components::text as kit_text;
use sola_kit::components::{Tone, badge, button as kit_btn, field, text_input as kit_input};

use crate::{page, procfs};

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
            kind: AppKind::Command,
            url: None,
            profile: None,
        };
        a.finalize();
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

/// Configured command apps (not web apps) sorted by [`sort_key`], then
/// `app_id`.
pub fn sorted_apps(apps: &ApplicationsConfig) -> Vec<&Application> {
    let mut v: Vec<&Application> = apps.apps.iter().filter(|a| !a.is_web()).collect();
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

/// Reconcile open Edit detail after the apps list changes (bus replay).
///
/// - Retracted app while editing → close detail.
/// - Clean edit still present → refresh buffer from canonical (external update).
/// - Dirty edit → keep local buffer.
/// - Closed / Draft → unchanged.
pub fn on_apps_changed(apps: &ApplicationsConfig, ui: &mut AppsState) {
    match &ui.detail {
        Detail::Closed | Detail::Draft(_) => {}
        Detail::Edit { orig, buffer } => {
            match apps.get(orig) {
                None => {
                    ui.detail = Detail::Closed;
                    ui.error = None;
                }
                Some(canonical) if !edit_is_dirty(buffer, canonical) => {
                    // Refresh clean buffer from canonical (external update).
                    let refreshed = EditBuffer::from_app(canonical);
                    ui.detail = Detail::Edit {
                        orig: orig.clone(),
                        buffer: refreshed,
                    };
                }
                Some(_) => {
                    // Dirty — keep local buffer.
                }
            }
        }
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

pub fn update(msg: AppsMsg, apps: &mut ApplicationsConfig, ui: &mut AppsState) -> Task<AppsMsg> {
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
                ..Default::default()
            });
            ui.error = None;
        }
        AppsMsg::Field { field, value } => {
            if let Some(buf) = open_buffer_mut(&mut ui.detail) {
                set_field(field, value, buf);
                ui.error = None;
            }
        }
        AppsMsg::Save => match &ui.detail {
            Detail::Edit { orig, buffer } => {
                let orig = orig.clone();
                let buf = buffer.clone();
                if let Some(err) = required_error(&buf) {
                    ui.error = Some(err);
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
                if let Some(err) = required_error(&buf) {
                    ui.error = Some(err);
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
    let mut sections = vec![catalog(apps, ui)];
    let candidates = collect_candidates(apps, running);
    if !candidates.is_empty() {
        sections.push(candidates_section(candidates));
    }
    page::page(sections)
}

// ── view helpers ───────────────────────────────────────────────────

/// The launcher's apps, A→Z, the open one with its editor under it.
fn catalog<'a>(apps: &'a ApplicationsConfig, ui: &'a AppsState) -> Element<'a, AppsMsg> {
    let mut rows: Vec<Element<'a, AppsMsg>> = Vec::new();
    if let Detail::Draft(buffer) = &ui.detail {
        rows.push(
            page::item("New application")
                .subtitle("Not in the launcher until you add it")
                .leading(page::glyph(page::icon("lucide/app-window"), 18, 0.7))
                .open(true)
                .on_press(AppsMsg::Discard)
                .into(),
        );
        rows.push(page::editor(draft_editor(buffer, ui)));
    }
    for app in sorted_apps(apps) {
        let open = matches!(&ui.detail, Detail::Edit { orig, .. } if orig == &app.app_id);
        let mut row = page::item(display_title(app))
            .subtitle(format!("{} · {}", app.app_id, app.command))
            .leading(app_icon(app))
            .open(open)
            .on_press(if open {
                AppsMsg::CloseDetail
            } else {
                AppsMsg::Select(app.app_id.clone())
            });
        if !sola_core::applications::command_exists(&app.command) {
            row = row.trailing(badge("not found", Tone::Warning));
        }
        rows.push(row.into());
        if let (true, Detail::Edit { orig, buffer }) = (open, &ui.detail) {
            rows.push(page::editor(edit_editor(apps, ui, orig, buffer)));
        }
    }
    if rows.is_empty() {
        rows.push(page::empty("No applications yet."));
    }
    page::section(
        "Launcher",
        Some(page::add("Add application", AppsMsg::StartBlank)),
        page::group(rows),
        Some(
            "Apps you added to the launcher, beside Sola's own. An icon is a pack \
             name (lucide/…, simpleicons/…) or a path to a picture."
                .into(),
        ),
    )
}

/// The app's own icon, or a plain window mark.
fn app_icon<'a>(app: &Application) -> Element<'a, AppsMsg> {
    if app.icon.trim().is_empty() {
        return page::glyph(page::icon("lucide/app-window"), 18, 0.7);
    }
    sola_kit::components::icon::icon(&app.icon, 18)
}

fn edit_editor<'a>(
    apps: &'a ApplicationsConfig,
    ui: &'a AppsState,
    orig: &'a str,
    buffer: &'a EditBuffer,
) -> Element<'a, AppsMsg> {
    let dirty = apps
        .get(orig)
        .map(|canonical| edit_is_dirty(buffer, canonical))
        .unwrap_or(true);
    let mut left: Vec<Element<'a, AppsMsg>> = Vec::new();
    if dirty {
        left.push(page::action("Save", kit_btn::primary, AppsMsg::Save));
        left.push(page::action("Discard", kit_btn::ghost, AppsMsg::Discard));
    }
    let right = vec![
        kit_btn::labeled_sm("Remove", kit_btn::danger_outline)
            .on_press(AppsMsg::Remove(orig.to_string()))
            .into(),
    ];
    let mut col = column![detail_fields(buffer, /* draft placeholders */ false)].spacing(SPACE_LG);
    if let Some(err) = ui.error.as_deref() {
        col = col.push(kit_text::caption(err).style(kit_text::danger));
    }
    col.push(page::footer(left, right)).into()
}

fn draft_editor<'a>(buffer: &'a EditBuffer, ui: &'a AppsState) -> Element<'a, AppsMsg> {
    let left = vec![
        page::action("Add", kit_btn::primary, AppsMsg::Save),
        page::action("Discard", kit_btn::ghost, AppsMsg::Discard),
    ];
    let mut col = column![detail_fields(buffer, /* draft placeholders */ true)].spacing(SPACE_LG);
    if let Some(err) = ui.error.as_deref() {
        col = col.push(kit_text::caption(err).style(kit_text::danger));
    }
    col.push(page::footer(left, Vec::new())).into()
}

fn detail_fields<'a>(buf: &'a EditBuffer, draft_placeholders: bool) -> Element<'a, AppsMsg> {
    let (ph_id, ph_label, ph_icon, ph_cmd) = if draft_placeholders {
        ("firefox", "Firefox", "simpleicons/firefox", "firefox")
    } else {
        ("", "", "", "")
    };
    column![
        row![
            field_input("Label", &buf.label, ph_label, AppField::Label),
            field_input("App id", &buf.app_id, ph_id, AppField::Id),
        ]
        .spacing(SPACE_MD),
        field_input("Command", &buf.command, ph_cmd, AppField::Command),
        field_input("Icon", &buf.icon, ph_icon, AppField::Icon),
    ]
    .spacing(SPACE_MD)
    .into()
}

fn field_input<'a>(
    label: &'a str,
    value: &'a str,
    placeholder: &'a str,
    f: AppField,
) -> Element<'a, AppsMsg> {
    let input = text_input(placeholder, value)
        .id(app_field_id(f))
        .on_input(move |v| AppsMsg::Field { field: f, value: v })
        .size(13)
        .style(kit_input::style)
        .width(Length::Fill);
    field(label, input, None, None).into()
}

/// Apps open on the desktop that the launcher does not have: a press
/// starts one from what is known of it.
fn candidates_section<'a>(candidates: Vec<Candidate>) -> Element<'a, AppsMsg> {
    let rows = candidates
        .into_iter()
        .map(|c| {
            let title = if c.title.is_empty() {
                "(no title)".to_string()
            } else {
                c.title
            };
            let subtitle = match &c.suggested_command {
                Some(cmd) => format!("{title} · {cmd}"),
                None => format!("{title} · command unknown"),
            };
            page::item(c.app_id.clone())
                .subtitle(subtitle)
                .leading(page::glyph(page::icon("lucide/app-window"), 18, 0.5))
                .trailing(kit_text::caption("Add").style(kit_text::accent))
                .on_press(AppsMsg::StartFromCandidate {
                    app_id: c.app_id,
                    command: c.suggested_command,
                })
                .into()
        })
        .collect();
    page::section(
        "Running, not in the launcher",
        None,
        page::group(rows),
        None,
    )
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

#[cfg(test)]
fn required_fields_missing(buf: &EditBuffer) -> bool {
    required_error(buf).is_some()
}

fn required_error(buf: &EditBuffer) -> Option<String> {
    if buf.app_id.trim().is_empty()
        || buf.label.trim().is_empty()
        || buf.command.trim().is_empty()
    {
        return Some("app_id, label, and command are required".into());
    }
    None
}

fn app_field_id(f: AppField) -> iced::widget::Id {
    iced::widget::Id::new(match f {
        AppField::Id => "settings-apps-id",
        AppField::Label => "settings-apps-label",
        AppField::Command => "settings-apps-command",
        AppField::Icon => "settings-apps-icon",
    })
}

const APP_FIELDS: [AppField; 4] = [
    AppField::Id,
    AppField::Label,
    AppField::Command,
    AppField::Icon,
];

pub fn focused_value(ui: &AppsState, id: &iced::widget::Id) -> Option<String> {
    let buf = match &ui.detail {
        Detail::Edit { buffer, .. } | Detail::Draft(buffer) => buffer,
        Detail::Closed => return None,
    };
    for f in APP_FIELDS {
        if id == &app_field_id(f) {
            return Some(app_field_str(buf, f).to_string());
        }
    }
    None
}

pub fn set_focused_value(ui: &mut AppsState, id: &iced::widget::Id, value: &str) -> bool {
    let Some(buf) = open_buffer_mut(&mut ui.detail) else {
        return false;
    };
    for f in APP_FIELDS {
        if id == &app_field_id(f) {
            set_field(f, value.to_string(), buf);
            return true;
        }
    }
    false
}

fn app_field_str(buf: &EditBuffer, f: AppField) -> &str {
    match f {
        AppField::Id => &buf.app_id,
        AppField::Label => &buf.label,
        AppField::Command => &buf.command,
        AppField::Icon => &buf.icon,
    }
}

fn set_field(f: AppField, value: String, buf: &mut EditBuffer) {
    match f {
        AppField::Id => buf.app_id = value,
        AppField::Label => buf.label = value,
        AppField::Command => buf.command = value,
        AppField::Icon => buf.icon = value,
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
            ..Default::default()
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
        let ids: Vec<&str> = sorted_apps(&cfg)
            .iter()
            .map(|a| a.app_id.as_str())
            .collect();
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

    #[test]
    fn on_apps_changed_closes_edit_when_app_removed() {
        let mut cfg = ApplicationsConfig::default();
        let a = app("chrome", "Chrome");
        cfg.apps.push(a.clone());
        let mut ui = AppsState {
            detail: Detail::Edit {
                orig: "chrome".into(),
                buffer: EditBuffer::from_app(&a),
            },
            error: None,
        };
        cfg.remove("chrome");
        on_apps_changed(&cfg, &mut ui);
        assert!(matches!(ui.detail, Detail::Closed));
    }

    #[test]
    fn on_apps_changed_keeps_clean_edit_when_app_still_present() {
        let mut cfg = ApplicationsConfig::default();
        let a = app("chrome", "Chrome");
        cfg.apps.push(a.clone());
        let mut ui = AppsState {
            detail: Detail::Edit {
                orig: "chrome".into(),
                buffer: EditBuffer::from_app(&a),
            },
            error: None,
        };
        // Bus replay re-upserts the same sticky entry — still clean.
        on_apps_changed(&cfg, &mut ui);
        match &ui.detail {
            Detail::Edit { orig, buffer } => {
                assert_eq!(orig, "chrome");
                assert!(buffer.matches(&a));
            }
            other => panic!("expected Edit, got {other:?}"),
        }
    }

    #[test]
    fn on_apps_changed_keeps_dirty_edit_buffer() {
        let mut cfg = ApplicationsConfig::default();
        let a = app("chrome", "Chrome");
        cfg.apps.push(a.clone());
        let mut dirty = EditBuffer::from_app(&a);
        dirty.label = "Local draft".into();
        let mut ui = AppsState {
            detail: Detail::Edit {
                orig: "chrome".into(),
                buffer: dirty,
            },
            error: None,
        };
        // External sticky upsert with different fields — buffer ≠ canonical,
        // so treat as dirty and keep local edits.
        cfg.remove("chrome");
        cfg.apps.push(Application {
            app_id: "chrome".into(),
            label: "Google Chrome".into(),
            command: "true".into(),
            icon: String::new(),
            ..Default::default()
        });
        on_apps_changed(&cfg, &mut ui);
        match &ui.detail {
            Detail::Edit { buffer, .. } => assert_eq!(buffer.label, "Local draft"),
            other => panic!("expected Edit, got {other:?}"),
        }
    }

    #[test]
    fn on_apps_changed_leaves_draft_and_closed_alone() {
        let cfg = ApplicationsConfig::default();
        let mut closed = AppsState::default();
        on_apps_changed(&cfg, &mut closed);
        assert!(matches!(closed.detail, Detail::Closed));

        let mut draft = AppsState {
            detail: Detail::Draft(EditBuffer {
                app_id: "x".into(),
                ..Default::default()
            }),
            error: Some("err".into()),
        };
        on_apps_changed(&cfg, &mut draft);
        assert!(matches!(draft.detail, Detail::Draft(_)));
        assert_eq!(draft.error.as_deref(), Some("err"));
    }

    #[test]
    fn an_app_needs_an_id_a_label_and_a_command() {
        let mut buf = EditBuffer {
            app_id: "firefox".into(),
            label: "Firefox".into(),
            ..Default::default()
        };
        assert!(required_fields_missing(&buf));
        buf.command = "firefox".into();
        assert!(!required_fields_missing(&buf));
        assert_eq!(buf.to_application().kind, AppKind::Command);
    }

    #[test]
    fn web_apps_are_not_listed() {
        let mut cfg = ApplicationsConfig::default();
        cfg.apps = vec![
            app("firefox", "Firefox"),
            Application {
                kind: AppKind::Web,
                ..app("web-1", "Slack")
            },
        ];
        let ids: Vec<&str> = sorted_apps(&cfg).iter().map(|a| a.app_id.as_str()).collect();
        assert_eq!(ids, ["firefox"]);
    }
}
