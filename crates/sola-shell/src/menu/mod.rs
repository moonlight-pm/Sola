pub mod assets;
pub mod state;

pub use assets::MENU_ASSETS;
pub use state::{MenuCache, SYNTHESIZED_CLOSE_ACTION, synthesized_menu};

use serde_json::Value;
use sola_kit::{AppCtx, SolaApp};
use sola_bus::topics::{FrameUpdate, MenuItem, Topic};

use crate::app::ShellApp;
use crate::zoning;

impl ShellApp {
    pub fn open_menu(&mut self, source: &str, menu_index: usize, anchor_x: f64, ctx: &mut AppCtx) {
        let app_id = if source == "system" {
            Self::APP_ID.to_string()
        } else {
            self.focused_app_id.clone().unwrap_or_default()
        };

        let synthesized;
        let menu = match self.menus.get_menu(&app_id) {
            Some(m) => m,
            None if app_id != Self::APP_ID && !app_id.is_empty() => {
                synthesized = synthesized_menu(&app_id, &self.display_label(&app_id));
                &synthesized
            }
            None => return,
        };
        let Some(menu_def) = menu.menus.get(menu_index) else {
            return;
        };

        let items: Vec<Value> = menu_def
            .items
            .iter()
            .map(|item| match item {
                MenuItem::Action {
                    id,
                    label,
                    shortcut,
                    disabled,
                    ..
                } => serde_json::json!({
                    "type": "action",
                    "id": id,
                    "app_id": app_id,
                    "label": label,
                    "shortcut": shortcut.as_ref().map(|c| c.display()),
                    "disabled": disabled,
                }),
                MenuItem::Divider => serde_json::json!({ "type": "divider" }),
            })
            .collect();

        self.windows.menu.send_to_js(&serde_json::json!({
            "event": "show",
            "items": items,
            "anchor_x": anchor_x,
        }));

        // Full-screen overlay below the menubar — transparent except the dropdown.
        if let (Some((ow, oh)), Some(wid)) = (
            self.zoning.output_size,
            self.lookup_window_id(Self::APP_ID, "menu"),
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

        self.menu_open = true;
        self.emit_registered_chords(ctx);
        self.emit_composition(ctx);
    }

    pub fn close_menu(&mut self, ctx: &mut AppCtx) {
        if !self.menu_open {
            return;
        }
        self.menu_open = false;
        self.emit_registered_chords(ctx);
        self.windows.menu.send_to_js(&serde_json::json!({"event": "clear"}));
        self.windows
            .menubar
            .send_to_js(&serde_json::json!({"event": "close_menu"}));
        self.emit_composition(ctx);
    }
}
