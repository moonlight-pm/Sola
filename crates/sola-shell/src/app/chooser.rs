//! The launcher's Projects chooser, as the shell runs it: entering it from
//! the launcher's Projects row, asking Projects which projects it has open,
//! and opening one (or a new one) through Projects' `project.open`. The
//! chooser's data and rows are [`crate::launcher::projects`].

use std::path::PathBuf;
use std::time::Duration;

use sola_bus::topics::{LaunchAppPayload, Topic};

use super::{Msg, Shell, with_bus};
use crate::launcher::projects::{self, APP_ID, CALL_OWNER, Chooser, Known, Row};

/// `project.list` should answer at once; the chooser shows without it.
const LIST_TIMEOUT: Duration = Duration::from_secs(2);
/// `project.open` may add a project (Projects reads the folder).
const OPEN_TIMEOUT: Duration = Duration::from_secs(15);
/// How long an error from the chooser stays up.
const TOAST_SECS: u64 = 5;

impl Shell {
    pub(crate) fn projects_running(&self) -> bool {
        self.known_windows
            .iter()
            .any(|w| w.app_id.eq_ignore_ascii_case(APP_ID))
    }

    /// The launcher's Projects row: show `~/Projects` instead of starting
    /// another Projects. Asks a running Projects which are open.
    pub(crate) fn enter_projects_chooser(&mut self) -> iced::Task<Msg> {
        self.launcher.chooser = Some(Chooser::new(projects::scan(&projects::projects_dir())));
        self.launcher.query.clear();
        self.launcher.selected = 0;
        let focus = iced::widget::operation::focus::<Msg>(iced::widget::Id::new(
            crate::launcher::view::QUERY_INPUT_ID,
        ));
        if !self.projects_running() {
            return focus;
        }
        let list = iced::Task::perform(
            call(
                "project.list",
                serde_json::json!({}),
                LIST_TIMEOUT,
            ),
            |r| Msg::ProjectsKnown(r.map(|v| known_from(&v))),
        );
        iced::Task::batch([focus, list])
    }

    /// Escape: out of naming, else back to the apps, else close.
    pub(crate) fn launcher_back(&mut self) -> iced::Task<Msg> {
        let Some(chooser) = self.launcher.chooser.as_mut() else {
            return iced::Task::done(Msg::CloseLauncher);
        };
        if chooser.naming {
            chooser.naming = false;
            chooser.apply_query("");
        } else {
            self.launcher.chooser = None;
            let apps = self.applications.clone();
            self.launcher.apply_query(&apps, "");
            return refocus_query();
        }
        self.launcher.query.clear();
        self.launcher.selected = 0;
        refocus_query()
    }

    /// Act on chooser row `i` (Enter on it, or a click).
    pub(crate) fn chooser_pick(&mut self, i: usize) -> iced::Task<Msg> {
        let Some(chooser) = self.launcher.chooser.as_mut() else {
            return iced::Task::none();
        };
        match chooser.rows().get(i).copied() {
            Some(Row::Folder(f)) => {
                let path = chooser.folders[f].path.clone();
                self.open_project(path)
            }
            Some(Row::New) => {
                let name = self.launcher.query.trim().to_string();
                if chooser.naming || chooser.new_name().is_some() {
                    match projects::create(&projects::projects_dir(), &name) {
                        Ok(path) => {
                            tracing::info!(path = %path.display(), "new project folder");
                            self.open_project(path)
                        }
                        Err(e) => self.chooser_toast(e),
                    }
                } else {
                    // New with nothing typed: ask for the name.
                    chooser.naming = true;
                    self.launcher.query.clear();
                    self.launcher.selected = 0;
                    refocus_query()
                }
            }
            None => iced::Task::none(),
        }
    }

    /// Close the launcher and have Projects bring `path` up: shown on its
    /// workspace if open, else opened in the showing one. Projects not
    /// running: start it, and ask once its window is up
    /// ([`Self::flush_project_pending`]).
    fn open_project(&mut self, path: PathBuf) -> iced::Task<Msg> {
        let close = iced::Task::done(Msg::CloseLauncher);
        if self.projects_running() {
            if self.is_app_hidden(APP_ID) {
                self.unhide_app(APP_ID);
            }
            return iced::Task::batch([close, ask_open(path)]);
        }
        let Some(app) = self.applications.get(APP_ID) else {
            return close;
        };
        let (command, label) = (app.command.clone(), app.label.clone());
        self.project_pending = Some(path);
        with_bus(|bus| {
            let _ = bus.emit(Topic::LaunchApp(LaunchAppPayload {
                app_id: APP_ID.to_string(),
                command,
            }));
        });
        let opening = self.begin_opening(APP_ID, &label);
        iced::Task::batch([close, opening])
    }

    /// The project the chooser asked for, once Projects' first window is
    /// up (its call methods are advertised before it maps).
    pub(crate) fn flush_project_pending(&mut self) -> iced::Task<Msg> {
        if self.project_pending.is_none() || !self.projects_running() {
            return iced::Task::none();
        }
        match self.project_pending.take() {
            Some(path) => ask_open(path),
            None => iced::Task::none(),
        }
    }

    /// The workspace project `id`'s window is on, by name, if it has one.
    pub(crate) fn project_workspace(&self, id: &str) -> Option<String> {
        let wid = self.pairing.window_of(&format!("{APP_ID}/{id}"))?;
        let ws = self.tabs.workspace_of(wid)?;
        self.tabs
            .workspaces()
            .into_iter()
            .find(|w| w.id == ws)
            .map(|w| w.name)
    }

    pub(crate) fn chooser_toast(&mut self, message: String) -> iced::Task<Msg> {
        self.menubar.push_toast(message);
        let generation = self.menubar.toast_generation;
        iced::Task::perform(
            tokio::time::sleep(Duration::from_secs(TOAST_SECS)),
            move |_| Msg::ToastExpire(generation),
        )
    }
}

/// `project.open --path`.
fn ask_open(path: PathBuf) -> iced::Task<Msg> {
    let params = serde_json::json!({ "path": path.to_string_lossy() });
    iced::Task::perform(call("project.open", params, OPEN_TIMEOUT), |r| {
        Msg::ProjectOpened(r.map(|_| ()))
    })
}

/// Projects' known projects from a `project.list` reply, by canonical root.
fn known_from(reply: &serde_json::Value) -> Vec<(PathBuf, Known)> {
    reply
        .get("projects")
        .and_then(|p| p.as_array())
        .into_iter()
        .flatten()
        .filter_map(|p| {
            let root = PathBuf::from(p.get("root")?.as_str()?);
            let root = root.canonicalize().unwrap_or(root);
            let known = Known {
                id: p.get("id")?.as_str()?.to_string(),
                open: p.get("open").and_then(|o| o.as_bool()).unwrap_or(false),
            };
            Some((root, known))
        })
        .collect()
}

/// A call to Projects off the iced thread.
async fn call(
    method: &'static str,
    params: serde_json::Value,
    timeout: Duration,
) -> Result<serde_json::Value, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    std::thread::spawn(move || {
        let r = sola_call::invoke(CALL_OWNER, method, params, timeout).map_err(|e| e.to_string());
        let _ = tx.send(r);
    });
    rx.await
        .unwrap_or_else(|_| Err(format!("{method}: the call worker dropped")))
}

fn refocus_query() -> iced::Task<Msg> {
    iced::widget::operation::focus::<Msg>(iced::widget::Id::new(
        crate::launcher::view::QUERY_INPUT_ID,
    ))
}
