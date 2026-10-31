//! The launcher's Projects chooser, as the shell runs it: entering it from
//! the launcher's Projects row, asking Projects which projects it has open,
//! and opening one (or a new one) through Projects' `project.open`. Its
//! Remote row asks for a host, lists that computer's `~/Projects` over ssh
//! and opens one there (`project.open --host`). The chooser's data and rows
//! are [`crate::launcher::projects`] and [`crate::launcher::remote`].

use std::path::PathBuf;
use std::time::Duration;

use sola_bus::topics::{LaunchAppPayload, Topic};

use super::{Msg, Shell, with_bus};
use crate::launcher::projects::{self, APP_ID, CALL_OWNER, Chooser, Known, Row};
use crate::launcher::remote::{self, Hosts, Listing};

/// `project.list` should answer at once; the chooser shows without it.
const LIST_TIMEOUT: Duration = Duration::from_secs(2);
/// `project.open` may add a project (Projects reads the folder).
const OPEN_TIMEOUT: Duration = Duration::from_secs(15);
/// How long a Projects just started waits for a project (its
/// `IDLE_QUIT_AFTER`), so how long to keep asking it.
const START_TIMEOUT: Duration = Duration::from_secs(20);
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
        let sessions = sola_core::project_sessions::list_local().unwrap_or_else(|e| {
            tracing::warn!("project chooser: {e}");
            Vec::new()
        });
        self.show_chooser(
            Chooser::new(projects::scan(&projects::projects_dir())).with_sessions(sessions),
        )
    }

    /// The chooser shows `chooser`'s folders; asks a running Projects
    /// which are open.
    fn show_chooser(&mut self, chooser: Chooser) -> iced::Task<Msg> {
        self.launcher.hosts = None;
        self.launcher.chooser = Some(chooser);
        self.launcher.query.clear();
        self.launcher.selected = 0;
        let focus = refocus_query();
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

    /// The chooser's Remote row: ask for a host, the recent ones offered.
    fn enter_hosts(&mut self) -> iced::Task<Msg> {
        self.launcher.chooser = None;
        self.launcher.hosts = Some(Hosts::new(remote::load_recent()));
        self.launcher.query.clear();
        self.launcher.selected = 0;
        refocus_query()
    }

    /// Act on host row `i`: list that computer's `~/Projects`.
    pub(crate) fn hosts_pick(&mut self, i: usize) -> iced::Task<Msg> {
        let Some(hosts) = self.launcher.hosts.as_mut() else {
            return iced::Task::none();
        };
        let Some(host) = hosts.rows().get(i).and_then(|&r| hosts.host(r)) else {
            return iced::Task::none();
        };
        let host = match sola_core::ssh::check_host(host) {
            Ok(h) => h.to_string(),
            Err(e) => {
                hosts.error = Some(e);
                return iced::Task::none();
            }
        };
        tracing::info!(%host, "project chooser: listing a remote host");
        hosts.error = None;
        hosts.connecting = Some(host.clone());
        self.launcher.selected = 0;
        iced::Task::perform(
            async move {
                let h = host.clone();
                let listed = tokio::task::spawn_blocking(move || remote::list(&h))
                    .await
                    .unwrap_or_else(|e| Err(format!("listing panicked: {e}")));
                (host, listed)
            },
            |(host, listed)| Msg::RemoteListed(host, listed),
        )
    }

    /// `host` listed its projects (or could not): show them, unless the
    /// launcher moved on meanwhile.
    pub(crate) fn on_remote_listed(
        &mut self,
        host: String,
        listed: Result<Listing, String>,
    ) -> iced::Task<Msg> {
        let Some(hosts) = self.launcher.hosts.as_mut() else {
            return iced::Task::none();
        };
        if !self.launcher.active || hosts.connecting.as_deref() != Some(host.as_str()) {
            return iced::Task::none();
        }
        hosts.connecting = None;
        match listed {
            Ok(listing) => {
                remote::remember(&host);
                self.show_chooser(
                    Chooser::remote(&host, &listing.root, listing.names)
                        .with_sessions(listing.sessions),
                )
            }
            Err(e) => {
                tracing::warn!(%host, error = %e, "project chooser: remote listing");
                hosts.error = Some(format!("{host}: {e}"));
                refocus_query()
            }
        }
    }

    /// Escape: out of naming, else back to the apps, else close. Remote:
    /// out of naming, else a host's projects back to the hosts, the hosts
    /// (or a listing that has not answered) back to this computer's
    /// projects.
    pub(crate) fn launcher_back(&mut self) -> iced::Task<Msg> {
        // The Browser field: back to the apps.
        if self.launcher.web.take().is_some() {
            let apps = self.applications.clone();
            self.launcher.apply_query(&apps, "");
            return refocus_query();
        }
        if let Some(hosts) = self.launcher.hosts.as_mut() {
            if hosts.connecting.take().is_some() {
                return refocus_query();
            }
            return self.enter_projects_chooser();
        }
        let Some(chooser) = self.launcher.chooser.as_mut() else {
            return iced::Task::done(Msg::CloseLauncher);
        };
        if chooser.naming {
            chooser.naming = false;
            chooser.apply_query("");
        } else if chooser.host.is_some() {
            return self.enter_hosts();
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
                let host = chooser.host.clone();
                self.open_project(host, path, None)
            }
            Some(Row::Session(f, s)) => {
                let path = chooser.folders[f].path.clone();
                let host = chooser.host.clone();
                let id = chooser.sessions[s].id.clone();
                self.open_project(host, path, Some(id))
            }
            Some(Row::Remote) => self.enter_hosts(),
            Some(Row::New) => {
                let name = self.launcher.query.trim().to_string();
                let remote = chooser.host.clone().zip(chooser.folders_root());
                if let (Some((host, root)), true) =
                    (remote, chooser.naming || chooser.new_name().is_some())
                {
                    return create_remote(host, root, name);
                }
                if chooser.naming || chooser.new_name().is_some() {
                    match projects::create(&projects::projects_dir(), &name) {
                        Ok(path) => {
                            tracing::info!(path = %path.display(), "new project folder");
                            self.open_project(None, path, None)
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

    /// Close the launcher and have Projects bring `path` (on `host`, else
    /// here) up: its sessions shown, or opened in the showing workspace —
    /// session `session` alone when given. Projects not running: start it,
    /// and ask until it answers (with nothing open it shows no window; it
    /// waits for this call).
    fn open_project(
        &mut self,
        host: Option<String>,
        path: PathBuf,
        session: Option<String>,
    ) -> iced::Task<Msg> {
        let close = iced::Task::done(Msg::CloseLauncher);
        if self.projects_running() {
            if self.is_app_hidden(APP_ID) {
                self.unhide_app(APP_ID);
            }
            return iced::Task::batch([close, ask_open(host, path, session)]);
        }
        let Some(app) = self.applications.get(APP_ID) else {
            return close;
        };
        let (command, label) = (app.command.clone(), app.label.clone());
        with_bus(|bus| {
            let _ = bus.emit(Topic::LaunchApp(LaunchAppPayload {
                app_id: APP_ID.to_string(),
                command,
            }));
        });
        let opening = self.begin_opening(APP_ID, &label);
        iced::Task::batch([close, opening, ask_open_starting(host, path, session)])
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

    /// A folder made on another computer for New: open it there.
    pub(crate) fn on_remote_created(
        &mut self,
        host: String,
        made: Result<PathBuf, String>,
    ) -> iced::Task<Msg> {
        match made {
            Ok(path) => {
                tracing::info!(%host, path = %path.display(), "new project folder there");
                self.open_project(Some(host), path, None)
            }
            Err(e) => self.chooser_toast(e),
        }
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

/// Make `root/name` on `host` off the UI thread ([`remote::create`]).
fn create_remote(host: String, root: PathBuf, name: String) -> iced::Task<Msg> {
    iced::Task::perform(
        async move {
            let h = host.clone();
            let made = tokio::task::spawn_blocking(move || remote::create(&h, &root, &name))
                .await
                .unwrap_or_else(|e| Err(e.to_string()));
            (host, made)
        },
        |(host, made)| Msg::RemoteCreated(host, made),
    )
}

/// `project.open --path [--host] [--session]`.
fn ask_open(host: Option<String>, path: PathBuf, session: Option<String>) -> iced::Task<Msg> {
    iced::Task::perform(call("project.open", open_params(host, path, session), OPEN_TIMEOUT), |r| {
        Msg::ProjectOpened(r.map(|_| ()))
    })
}

/// [`ask_open`] for a Projects just started: asked again while it is not
/// up yet ("projects is not running"), for as long as it waits for one.
fn ask_open_starting(host: Option<String>, path: PathBuf, session: Option<String>) -> iced::Task<Msg> {
    let params = open_params(host, path, session);
    iced::Task::perform(
        async move {
            let deadline = std::time::Instant::now() + START_TIMEOUT;
            loop {
                match call("project.open", params.clone(), OPEN_TIMEOUT).await {
                    Err(e) if e.contains("is not running") && std::time::Instant::now() < deadline => {
                        tokio::time::sleep(Duration::from_millis(300)).await
                    }
                    done => return done,
                }
            }
        },
        |r| Msg::ProjectOpened(r.map(|_| ())),
    )
}

fn open_params(host: Option<String>, path: PathBuf, session: Option<String>) -> serde_json::Value {
    let mut params = serde_json::json!({ "path": path.to_string_lossy() });
    if let Some(host) = host {
        params["host"] = serde_json::json!(host);
    }
    if let Some(session) = session {
        params["session"] = serde_json::json!(session);
    }
    params
}

/// Projects' known projects from a `project.list` reply: the computer
/// each is on (`None`: this one), and its root (canonical here).
fn known_from(reply: &serde_json::Value) -> Vec<(Option<String>, PathBuf, Known)> {
    reply
        .get("projects")
        .and_then(|p| p.as_array())
        .into_iter()
        .flatten()
        .filter_map(|p| {
            let host = p.get("host").and_then(|h| h.as_str()).map(str::to_string);
            let root = PathBuf::from(p.get("root")?.as_str()?);
            let root = match host {
                Some(_) => root,
                None => root.canonicalize().unwrap_or(root),
            };
            let known = Known {
                id: p.get("id")?.as_str()?.to_string(),
                open: p.get("open").and_then(|o| o.as_bool()).unwrap_or(false),
            };
            Some((host, root, known))
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
