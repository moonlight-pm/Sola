pub mod assets;
pub mod state;

pub use assets::LAUNCHER_ASSETS;
pub use state::LauncherState;

pub const WIDTH: i32 = 560;
pub const HEIGHT: i32 = 420;

use sola_kit::{AppCtx, SolaApp};
use sola_bus::topics::{FocusTarget, FrameUpdate, Topic};

use crate::app::ShellApp;
use crate::zoning;

impl ShellApp {
    pub fn open_launcher(&mut self, ctx: &mut AppCtx) {
        tracing::info!(
            already_active = self.launcher.active,
            prior_focus = ?self.focused_window_id,
            "open_launcher"
        );
        if self.launcher.active {
            return;
        }

        // Snapshot the focus target we'll restore on close.
        self.launcher.prior_focus = self.focused_window_id;

        self.launcher.active = true;
        self.emit_registered_chords(ctx);
        self.launcher.apply_query(&self.applications, "");

        // Launcher is a fullscreen-below-menubar overlay. The visible
        // panel is centered by the CSS; the rest is transparent and
        // absorbs pointer/scroll events so nothing beneath the launcher
        // can be interacted with while it's open.
        if let (Some((ow, oh)), Some(wid)) = (
            self.zoning.output_size,
            self.lookup_window_id(Self::APP_ID, "launcher"),
        ) {
            ctx.emit(Topic::Frame(FrameUpdate {
                window_id: wid,
                x: 0,
                y: zoning::MENUBAR_HEIGHT,
                width: ow,
                height: oh - zoning::MENUBAR_HEIGHT,
                fullscreen: false,
            }));
        }

        self.emit_composition(ctx);

        // Route keyboard to the launcher window.
        if let Some(wid) = self.lookup_window_id(Self::APP_ID, "launcher") {
            ctx.emit(Topic::Focus(FocusTarget { window_id: wid }));
        }

        self.windows.launcher.send_to_js(&serde_json::json!({"event": "reset"}));
        self.render_launcher();
    }

    pub fn close_launcher(&mut self, ctx: &mut AppCtx) {
        tracing::info!(
            was_active = self.launcher.active,
            prior_focus = ?self.launcher.prior_focus,
            "close_launcher"
        );
        if !self.launcher.active {
            return;
        }
        let prior_wid = self.launcher.prior_focus.take();
        self.launcher.active = false;
        self.emit_registered_chords(ctx);
        self.launcher.query.clear();
        self.launcher.filtered_ids.clear();
        self.launcher.selected = 0;

        self.emit_composition(ctx);

        if let Some(wid) = prior_wid {
            ctx.emit(Topic::Focus(FocusTarget { window_id: wid }));
        }
    }

    pub fn launch_and_close(&mut self, app_id: &str, ctx: &mut AppCtx) {
        tracing::info!(app_id, "launch_and_close");
        if let Some(app) = self.applications.get(app_id) {
            tracing::info!(app_id, command = %app.command, "emitting LaunchApp");
            ctx.emit(Topic::LaunchApp(sola_bus::topics::LaunchAppPayload {
                app_id: app_id.to_string(),
                command: app.command.clone(),
            }));
        } else {
            tracing::warn!(app_id, "launch requested for unknown application");
        }
        self.close_launcher(ctx);
    }

    pub(crate) fn render_launcher(&self) {
        let apps = state::render_value(&self.applications, &self.launcher.filtered_ids);
        self.windows.launcher.send_to_js(&serde_json::json!({
            "event": "render",
            "apps": apps,
            "selected": self.launcher.selected,
        }));
    }
}
