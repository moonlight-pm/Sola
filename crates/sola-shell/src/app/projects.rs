//! The launcher's Projects row (Joshua, 2026-10-31: "the launcher should
//! just open a project window (and load the projects app by doing so, if
//! it wasn't already).. then in that new projects window, we'll navigate to
//! the project to open"): Projects opens a window to pick a project in
//! (`window.navigate`, its navigator). Not running: start it, and ask until
//! it answers — with nothing to bring back, it waits for this call.
//! Design's row does the same since 2026-11-10
//! ([`crate::launcher::projects::navigating`]).
//!
//! Until 2026-10-31 the launcher listed `~/Projects` itself (the chooser);
//! that list is the navigator's now.

use std::time::Duration;

use sola_bus::topics::{LaunchAppPayload, Topic};
use sola_core::window_label::WindowLabel;

use super::{Msg, Shell, with_bus};
use crate::launcher::projects::APP_ID;

/// `window.navigate` answers at once once the app is up.
const CALL_TIMEOUT: Duration = Duration::from_secs(5);
/// How long a Projects just started waits for a call (its
/// `IDLE_QUIT_AFTER`), so how long to keep asking an app just started.
const START_TIMEOUT: Duration = Duration::from_secs(20);
/// How long an error stays up.
const TOAST_SECS: u64 = 5;

impl Shell {
    /// The windows a new Projects window belongs beside in the bar: the
    /// other windows of its project (read from the titles, `WindowLabel`),
    /// so a new session lands after its project's last row. None for
    /// another app, or a window without its title yet.
    pub(crate) fn project_kin(&self, window_id: u32, app_id: &str) -> Vec<u32> {
        let project = |title: &str| WindowLabel::parse(title).map(|l| l.project);
        if app_id != APP_ID {
            return Vec::new();
        }
        let Some(mine) = self
            .known_windows
            .iter()
            .find(|w| w.window_id == window_id)
            .and_then(|w| project(&w.title))
        else {
            return Vec::new();
        };
        self.known_windows
            .iter()
            .filter(|w| w.window_id != window_id && w.app_id == APP_ID)
            .filter(|w| project(&w.title).as_ref() == Some(&mine))
            .map(|w| w.window_id)
            .collect()
    }

    fn app_running(&self, app_id: &str) -> bool {
        self.known_windows
            .iter()
            .any(|w| w.app_id.eq_ignore_ascii_case(app_id))
    }

    /// Close the launcher; app `app_id` (call owner `owner`) opens a window
    /// to pick in.
    pub(crate) fn open_navigator_window(&mut self, app_id: &str, owner: &'static str) -> iced::Task<Msg> {
        let close = iced::Task::done(Msg::CloseLauncher);
        let Some(app) = self.applications.get(app_id) else {
            return close;
        };
        let (command, label) = (app.command.clone(), app.label.clone());
        if self.app_running(app_id) {
            if self.is_app_hidden(app_id) {
                self.unhide_app(app_id);
            }
            return iced::Task::batch([close, ask_navigate(label, owner, Duration::ZERO)]);
        }
        with_bus(|bus| {
            let _ = bus.emit(Topic::LaunchApp(LaunchAppPayload {
                app_id: app_id.to_string(),
                command,
            }));
        });
        let opening = self.begin_opening(app_id, &label);
        iced::Task::batch([close, opening, ask_navigate(label, owner, START_TIMEOUT)])
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

/// `window.navigate` of `owner` (the app `label`), asked again while it is
/// not up yet, for as long as `starting` allows.
fn ask_navigate(label: String, owner: &'static str, starting: Duration) -> iced::Task<Msg> {
    iced::Task::perform(
        async move {
            let deadline = std::time::Instant::now() + starting;
            loop {
                match call(owner, "window.navigate", CALL_TIMEOUT).await {
                    Err(e) if e.contains("is not running") && std::time::Instant::now() < deadline => {
                        tokio::time::sleep(Duration::from_millis(300)).await
                    }
                    done => return done.map(|_| ()),
                }
            }
        },
        move |done| Msg::NavigatorWindow(label.clone(), done),
    )
}

/// A call to `owner` off the iced thread.
async fn call(owner: &'static str, method: &'static str, timeout: Duration) -> Result<serde_json::Value, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    std::thread::spawn(move || {
        let r = sola_call::invoke(owner, method, serde_json::json!({}), timeout)
            .map_err(|e| e.to_string());
        let _ = tx.send(r);
    });
    rx.await
        .unwrap_or_else(|_| Err(format!("{method}: the call worker dropped")))
}
