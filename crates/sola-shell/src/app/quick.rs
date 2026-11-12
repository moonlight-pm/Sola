//! The quick launch (`Topic::QuickLaunch`, persistent): the apps in the
//! middle of the top row (`crate::menubar::quick`). Settings →
//! Applications edits it, and so does an app's own menu (Add to / Remove
//! from Quick Launch, `sola_kit::menu`), which comes here.

use iced::Task;
use sola_bus::topics::{QuickLaunch, Topic};
use sola_kit::menu::QuickLaunchAction;

use super::{Msg, Shell};

impl Shell {
    /// `Topic::QuickLaunch` (`None`: retracted, empty).
    pub(crate) fn on_quick_launch(&mut self, quick: Option<QuickLaunch>) {
        self.quick_launch = quick.unwrap_or_default();
    }

    /// One of its apps clicked: as the launcher's row, it comes forward or
    /// starts.
    pub(crate) fn quick_launch_app(&mut self, app_id: &str) -> Task<Msg> {
        let _ = self.dismiss_open_menu();
        self.emit_overlay_frames();
        self.launcher.prior_focus = self.focused_window_id;
        self.launch_from_launcher(Some(app_id))
    }

    /// An app's menu → Add to / Remove from Quick Launch, for `window_app`
    /// (its windows' app id): the launcher's app for it goes on or off.
    pub(crate) fn on_quick_launch_menu(
        &mut self,
        window_app: &str,
        action: QuickLaunchAction,
    ) -> Task<Msg> {
        let Some(app) = self.applications.get_for_window(window_app) else {
            tracing::info!(window_app, "quick launch: not an app the launcher has");
            return self.projects_toast(format!("{window_app} is not in the launcher"));
        };
        let id = app.app_id.clone();
        let mut quick = self.quick_launch.clone();
        match action {
            QuickLaunchAction::Add if !quick.apps.contains(&id) => quick.apps.push(id),
            QuickLaunchAction::Remove => quick.apps.retain(|a| a != &id),
            QuickLaunchAction::Add => {}
        }
        if quick == self.quick_launch {
            return Task::none();
        }
        tracing::info!(apps = ?quick.apps, "quick launch");
        self.quick_launch = quick.clone();
        if let Some(Ok(mut bus)) = sola_kit::app::try_bus().map(|b| b.lock()) {
            if let Err(e) = bus.emit(Topic::QuickLaunch(quick)) {
                tracing::warn!("quick launch: not sent: {e}");
            }
        }
        Task::none()
    }
}

