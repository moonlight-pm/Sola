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

use iced::widget::{button, checkbox, column, container, row, scrollable};
use iced::{Alignment, Element, Length, Padding, Task};
use sola_kit::components::text_input::text_input;

use sola_bus::topics::{AppKind, Application, ApplicationsConfig, Topic, Window as BusWindow};
use sola_core::applications::{is_wrapper_url, wrapper_command};
use sola_kit::app::bus;
use sola_kit::components::form::{checkbox_style, form_row};
use sola_kit::components::style::{SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XL, SPACE_XS};
use sola_kit::components::text as kit_text;
use sola_kit::components::{Tone, badge, button as kit_btn, card, field, text_input as kit_input};

use crate::procfs;

/// Fixed width of the right-hand detail panel (px).
const DETAIL_WIDTH: f32 = 380.0;
/// Vertical pad inside a compact app row (graphite density).
const ROW_PAD: Padding = Padding {
    top: 8.0,
    bottom: 8.0,
    left: 12.0,
    right: 8.0,
};

#[derive(Debug, Clone, Copy)]
pub enum AppField {
    Id,
    Label,
    Command,
    Icon,
    Url,
}

#[derive(Debug, Clone, Default)]
pub struct EditBuffer {
    pub app_id: String,
    pub label: String,
    pub command: String,
    pub icon: String,
    pub kind: AppKind,
    pub url: String,
}

impl EditBuffer {
    pub fn from_app(a: &Application) -> Self {
        Self {
            app_id: a.app_id.clone(),
            label: a.label.clone(),
            command: a.command.clone(),
            icon: a.icon.clone(),
            kind: a.kind,
            url: a.url.clone().unwrap_or_default(),
        }
    }
    pub fn matches(&self, a: &Application) -> bool {
        self.app_id == a.app_id
            && self.label == a.label
            && self.command == a.command
            && self.icon == a.icon
            && self.kind == a.kind
            && self.url.trim() == a.url.as_deref().unwrap_or("").trim()
    }
    pub fn to_application(&self) -> Application {
        let mut a = Application {
            app_id: self.app_id.trim().to_string(),
            label: self.label.trim().to_string(),
            command: self.command.trim().to_string(),
            icon: self.icon.trim().to_string(),
            kind: self.kind,
            url: {
                let u = self.url.trim();
                if u.is_empty() {
                    None
                } else {
                    Some(u.to_string())
                }
            },
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
        || !buf.url.trim().is_empty()
        || buf.kind != AppKind::Command
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
    SetWrapper(bool),
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
        AppsMsg::SetWrapper(on) => {
            if let Some(buf) = open_buffer_mut(&mut ui.detail) {
                buf.kind = if on {
                    AppKind::Wrapper
                } else {
                    AppKind::Command
                };
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
    // Toolbar: one quiet add control, not a second chrome layer.
    let toolbar = row![
        kit_btn::labeled_sm("+ Add application", kit_btn::ghost).on_press(AppsMsg::StartBlank),
        iced::widget::Space::new().width(Length::Fill),
        kit_text::caption(format!("{} apps", apps.apps.len())).style(kit_text::muted),
    ]
    .spacing(SPACE_MD)
    .align_y(Alignment::Center)
    .width(Length::Fill);

    let list_body: Element<'a, AppsMsg> = if apps.apps.is_empty()
        && matches!(ui.detail, Detail::Closed)
    {
        container(
            column![
                kit_text::body("No applications yet").style(kit_text::muted),
                kit_text::caption("Add one above, or configure a running app below.")
                    .style(kit_text::muted),
            ]
            .spacing(SPACE_SM),
        )
        .padding(SPACE_LG)
        .width(Length::Fill)
        .into()
    } else if apps.apps.is_empty() {
        // Draft open while catalog empty — no noisy empty copy.
        container(iced::widget::Space::new())
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    } else {
        let mut rows = column![].spacing(0).width(Length::Fill);
        for app in sorted_apps(apps) {
            rows = rows.push(app_row(app, ui));
        }
        scrollable(rows)
            .height(Length::Fill)
            .width(Length::Fill)
            .into()
    };

    // One raised surface holds the catalog — rows share the card, not
    // a void of free-floating pink buttons across the pane.
    let list_card = card(
        column![toolbar, list_body]
            .spacing(SPACE_MD)
            .width(Length::Fill)
            .height(Length::Fill),
    )
    .width(Length::Fill)
    .height(Length::Fill);

    let mut col = column![list_card]
        .spacing(SPACE_LG)
        .width(Length::Fill)
        .height(Length::Fill);

    let candidates = collect_candidates(apps, running);
    if !candidates.is_empty() {
        col = col.push(candidates_card(candidates));
    }

    col.into()
}

fn app_row<'a>(app: &'a Application, ui: &'a AppsState) -> Element<'a, AppsMsg> {
    let selected = matches!(
        &ui.detail,
        Detail::Edit { orig, .. } if orig == &app.app_id
    );
    let missing = !sola_core::applications::command_exists(&app.command);

    // Title + optional status chip — primary Select hit target.
    let mut title_row = row![kit_text::body(display_title(app))]
        .spacing(SPACE_SM)
        .align_y(Alignment::Center);
    if app.is_wrapper() {
        title_row = title_row.push(badge("web", Tone::Neutral));
    }
    if missing {
        title_row = title_row.push(badge("not found", Tone::Warning));
    }

    // Sibling controls (not nested buttons — iced hit-testing is reliable
    // this way). Title strip is list_item; Remove is compact outline so it
    // does not form a pink wall down the pane.
    row![
        button(title_row)
            .on_press(AppsMsg::Select(app.app_id.clone()))
            .padding(ROW_PAD)
            .width(Length::Fill)
            .style(kit_btn::list_item(selected)),
        kit_btn::labeled_sm("Remove", kit_btn::danger_outline)
            .on_press(AppsMsg::Remove(app.app_id.clone())),
    ]
    .spacing(SPACE_SM)
    .align_y(Alignment::Center)
    .width(Length::Fill)
    .padding(Padding {
        top: 2.0,
        bottom: 2.0,
        left: 4.0,
        right: 4.0,
    })
    .into()
}

fn detail_panel<'a>(apps: &'a ApplicationsConfig, ui: &'a AppsState) -> Element<'a, AppsMsg> {
    let body: Element<'a, AppsMsg> = match &ui.detail {
        Detail::Closed => {
            // Centered empty state — no lonely caption glued to the top of a slab.
            container(
                column![
                    kit_text::subheading("No selection").style(kit_text::muted),
                    kit_text::caption("Select an app from the list, or add a new one.")
                        .style(kit_text::muted),
                ]
                .spacing(SPACE_SM)
                .align_x(Alignment::Center),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into()
        }
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

            let mut footer = row![].spacing(SPACE_SM).align_y(Alignment::Center);
            if dirty {
                footer = footer
                    .push(kit_btn::labeled_sm("Save", kit_btn::primary).on_press(AppsMsg::Save))
                    .push(
                        kit_btn::labeled_sm("Discard", kit_btn::ghost).on_press(AppsMsg::Discard),
                    );
            }
            footer = footer
                .push(iced::widget::Space::new().width(Length::Fill))
                .push(kit_btn::labeled_sm("Close", kit_btn::ghost).on_press(AppsMsg::CloseDetail));

            let mut col = column![
                kit_text::subheading(title),
                kit_text::caption(orig.as_str()).style(kit_text::muted),
                detail_fields(buffer, /* draft placeholders */ false),
            ]
            .spacing(SPACE_MD);
            if let Some(err) = ui.error.as_deref() {
                col = col.push(kit_text::caption(err).style(kit_text::danger));
            }
            // Footer pinned below form with breathing room.
            col = col
                .push(iced::widget::Space::new().height(Length::Fill))
                .push(footer);
            col.height(Length::Fill).into()
        }
        Detail::Draft(buffer) => {
            let footer = row![
                kit_btn::labeled_sm("Add", kit_btn::primary).on_press(AppsMsg::Save),
                kit_btn::labeled_sm("Discard", kit_btn::ghost).on_press(AppsMsg::Discard),
            ]
            .spacing(SPACE_SM)
            .align_y(Alignment::Center);

            let mut col = column![
                kit_text::subheading("New application"),
                kit_text::caption("Fill in identity and launch command.")
                    .style(kit_text::muted),
                detail_fields(buffer, /* draft placeholders */ true),
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
    };

    container(card(body).width(Length::Fill).height(Length::Fill))
        .width(Length::Fixed(DETAIL_WIDTH))
        .height(Length::Fill)
        .into()
}

fn detail_fields<'a>(buf: &'a EditBuffer, draft_placeholders: bool) -> Element<'a, AppsMsg> {
    let (ph_id, ph_label, ph_icon, ph_cmd, ph_url) = if draft_placeholders {
        (
            "slack",
            "Slack",
            "simpleicons/slack",
            "firefox",
            "https://app.slack.com",
        )
    } else {
        ("", "", "", "", "")
    };
    let wrapper = buf.kind == AppKind::Wrapper;
    let mut col = column![
        field_input("app_id", &buf.app_id, ph_id, AppField::Id),
        field_input("label", &buf.label, ph_label, AppField::Label),
        field_input("icon", &buf.icon, ph_icon, AppField::Icon),
        form_row(
            "Web wrapper",
            checkbox(wrapper)
                .on_toggle(AppsMsg::SetWrapper)
                .style(checkbox_style),
        ),
    ]
    .spacing(SPACE_LG);
    if wrapper {
        let id = buf.app_id.trim();
        let synthesized = if id.is_empty() {
            wrapper_command("<id>")
        } else {
            wrapper_command(id)
        };
        col = col.push(field_input("url", &buf.url, ph_url, AppField::Url));
        col = col.push(
            kit_text::caption(format!("Launches as {synthesized}")).style(kit_text::muted),
        );
    } else {
        col = col.push(field_input("command", &buf.command, ph_cmd, AppField::Command));
    }
    col.into()
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
        kit_text::caption("Apps open on the desktop but not in the catalog yet.")
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
            kit_btn::labeled_sm("Configure", kit_btn::ghost).on_press(
                AppsMsg::StartFromCandidate {
                    app_id: c.app_id,
                    command: c.suggested_command,
                },
            ),
        ]
        .spacing(SPACE_MD)
        .align_y(Alignment::Center)
        .padding(Padding {
            top: 4.0,
            bottom: 4.0,
            left: 0.0,
            right: 0.0,
        });
        col = col.push(row_el);
    }

    card(col.spacing(SPACE_MD)).width(Length::Fill).into()
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
    if buf.app_id.trim().is_empty() || buf.label.trim().is_empty() {
        return Some("app_id and label are required".into());
    }
    match buf.kind {
        AppKind::Command if buf.command.trim().is_empty() => {
            Some("app_id, label, and command are required".into())
        }
        AppKind::Wrapper if !is_wrapper_url(&buf.url) => {
            Some("wrapper needs an http(s) URL".into())
        }
        _ => None,
    }
}

fn set_field(f: AppField, value: String, buf: &mut EditBuffer) {
    match f {
        AppField::Id => buf.app_id = value,
        AppField::Label => buf.label = value,
        AppField::Command => buf.command = value,
        AppField::Icon => buf.icon = value,
        AppField::Url => buf.url = value,
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
    fn wrapper_to_application_synthesizes_command() {
        let buf = EditBuffer {
            app_id: " slack ".into(),
            label: "Slack".into(),
            command: "ignored".into(),
            icon: "simpleicons/slack".into(),
            kind: AppKind::Wrapper,
            url: " https://app.slack.com ".into(),
        };
        let a = buf.to_application();
        assert_eq!(a.kind, AppKind::Wrapper);
        assert_eq!(a.app_id, "slack");
        assert_eq!(a.url.as_deref(), Some("https://app.slack.com"));
        assert_eq!(a.command, wrapper_command("slack"));
    }

    #[test]
    fn wrapper_draft_is_dirty_when_kind_set() {
        assert!(!draft_is_dirty(&EditBuffer::default()));
        assert!(draft_is_dirty(&EditBuffer {
            kind: AppKind::Wrapper,
            ..Default::default()
        }));
    }

    #[test]
    fn wrapper_requires_http_url() {
        let mut buf = EditBuffer {
            app_id: "slack".into(),
            label: "Slack".into(),
            kind: AppKind::Wrapper,
            url: "app.slack.com".into(),
            ..Default::default()
        };
        assert!(required_fields_missing(&buf));
        buf.url = "https://app.slack.com".into();
        assert!(!required_fields_missing(&buf));
    }
}
