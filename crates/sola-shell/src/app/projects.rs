//! The launcher's Projects row (Joshua, 2026-10-31: "the launcher should
//! just open a project window (and load the projects app by doing so, if
//! it wasn't already).. then in that new projects window, we'll navigate to
//! the project to open"): Projects opens a window to pick a project in
//! (`window.navigate`, its navigator). Not running: start it, and ask until
//! it answers — with nothing to bring back, it waits for this call.
//!
//! Until 2026-10-31 the launcher listed `~/Projects` itself (the chooser);
//! that list is the navigator's now.

use std::time::Duration;

use sola_bus::topics::{LaunchAppPayload, Topic};

use super::{Msg, Shell, with_bus};
use crate::launcher::projects::{APP_ID, CALL_OWNER};

/// `window.navigate` answers at once once Projects is up.
const CALL_TIMEOUT: Duration = Duration::from_secs(5);
/// How long a Projects just started waits for a call (its
/// `IDLE_QUIT_AFTER`), so how long to keep asking it.
const START_TIMEOUT: Duration = Duration::from_secs(20);
/// How long an error stays up.
const TOAST_SECS: u64 = 5;

impl Shell {
    pub(crate) fn projects_running(&self) -> bool {
        self.known_windows
            .iter()
            .any(|w| w.app_id.eq_ignore_ascii_case(APP_ID))
    }

    /// Close the launcher; Projects opens a window to pick a project in.
    pub(crate) fn open_projects_window(&mut self) -> iced::Task<Msg> {
        let close = iced::Task::done(Msg::CloseLauncher);
        if self.projects_running() {
            if self.is_app_hidden(APP_ID) {
                self.unhide_app(APP_ID);
            }
            return iced::Task::batch([close, ask_navigate(Duration::ZERO)]);
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
        iced::Task::batch([close, opening, ask_navigate(START_TIMEOUT)])
    }

    /// Escape: the Browser field back to the apps, else close.
    pub(crate) fn launcher_back(&mut self) -> iced::Task<Msg> {
        if self.launcher.web.take().is_some() {
            let apps = self.applications.clone();
            self.launcher.apply_query(&apps, "");
            return iced::widget::operation::focus::<Msg>(iced::widget::Id::new(
                crate::launcher::view::QUERY_INPUT_ID,
            ));
        }
        iced::Task::done(Msg::CloseLauncher)
    }

    pub(crate) fn projects_toast(&mut self, message: String) -> iced::Task<Msg> {
        self.menubar.push_toast(message);
        let generation = self.menubar.toast_generation;
        iced::Task::perform(
            tokio::time::sleep(Duration::from_secs(TOAST_SECS)),
            move |_| Msg::ToastExpire(generation),
        )
    }
}

/// `window.navigate`, asked again while Projects is not up yet, for as
/// long as `starting` allows.
fn ask_navigate(starting: Duration) -> iced::Task<Msg> {
    iced::Task::perform(
        async move {
            let deadline = std::time::Instant::now() + starting;
            loop {
                match call("window.navigate", CALL_TIMEOUT).await {
                    Err(e) if e.contains("is not running") && std::time::Instant::now() < deadline => {
                        tokio::time::sleep(Duration::from_millis(300)).await
                    }
                    done => return done.map(|_| ()),
                }
            }
        },
        Msg::ProjectsWindow,
    )
}

/// A call to Projects off the iced thread.
async fn call(method: &'static str, timeout: Duration) -> Result<serde_json::Value, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    std::thread::spawn(move || {
        let r = sola_call::invoke(CALL_OWNER, method, serde_json::json!({}), timeout)
            .map_err(|e| e.to_string());
        let _ = tx.send(r);
    });
    rx.await
        .unwrap_or_else(|_| Err(format!("{method}: the call worker dropped")))
}
