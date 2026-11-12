//! Apps kept in the sidebar (`Topic::KeptApps`, persistent): a kept app
//! keeps its row in the bar while it is not running, and a tap on that row
//! starts it (`crate::stacks`, [`crate::stacks::Block::Kept`]). Settings →
//! Applications keeps any app, and so does an app's own menu (Keep in
//! Sidebar / Remove from Sidebar, `sola_kit::menu`), which comes here. The
//! same topic says whether the bar has its Sleep button.
//!
//! The shell also tells Settings which apps ship with Sola
//! (`Topic::BuiltinApps`), so it can list them to keep.

use iced::Task;
use sola_bus::topics::{BuiltinApp, BuiltinApps, KeptApps, Topic};
use sola_kit::menu::KeepAction;

use super::{Msg, Shell};

impl Shell {
    /// `Topic::KeptApps` (`None`: retracted, none).
    pub(crate) fn on_kept_apps(&mut self, kept: Option<KeptApps>) {
        let kept = kept.unwrap_or_default();
        if self.tabs.set_kept(&kept.apps) {
            self.persist_tabs();
        }
        self.kept = kept;
    }

    /// A kept app's row tapped: it starts, as from the launcher (its window
    /// takes the row).
    pub(crate) fn open_kept(&mut self, app_id: &str) -> Task<Msg> {
        let _ = self.dismiss_open_menu();
        self.emit_overlay_frames();
        self.launcher.prior_focus = self.focused_window_id;
        self.launch_from_launcher(Some(app_id))
    }

    /// An app's menu → Keep in Sidebar / Remove from Sidebar, for
    /// `window_app` (its windows' app id): the launcher's app for it is
    /// kept or let go.
    pub(crate) fn on_keep_menu(&mut self, window_app: &str, action: KeepAction) -> Task<Msg> {
        let Some(app) = self.applications.get_for_window(window_app) else {
            tracing::info!(window_app, "keep in sidebar: not an app the launcher has");
            return self.projects_toast(format!("{window_app} is not in the launcher"));
        };
        let id = app.app_id.clone();
        let mut kept = self.kept.clone();
        match action {
            KeepAction::Keep if !kept.apps.contains(&id) => kept.apps.push(id),
            KeepAction::Unkeep => kept.apps.retain(|a| a != &id),
            KeepAction::Keep => {}
        }
        if kept == self.kept {
            return Task::none();
        }
        tracing::info!(apps = ?kept.apps, "kept in the sidebar");
        self.on_kept_apps(Some(kept.clone()));
        emit(Topic::KeptApps(kept));
        Task::none()
    }

    /// Tell Settings which apps ship with Sola (sticky; the list is the
    /// shell's own, `crate::builtins`).
    pub(crate) fn publish_builtins(&self) {
        let apps = crate::builtins::builtin_apps()
            .into_iter()
            .map(|a| BuiltinApp {
                app_id: a.app_id,
                label: a.label,
                icon: a.icon,
            })
            .collect();
        emit(Topic::BuiltinApps(BuiltinApps { apps }));
    }
}

fn emit(topic: Topic) {
    if let Some(Ok(mut bus)) = sola_kit::app::try_bus().map(|b| b.lock()) {
        if let Err(e) = bus.emit(topic) {
            tracing::warn!("kept apps: not sent: {e}");
        }
    }
}
