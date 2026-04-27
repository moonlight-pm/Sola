use sola_app::{AppCtx, WindowConfig, WindowHandle, asset_bundle};

use crate::zoning;

/// Embedded web assets for the shell menubar window.
pub static MENUBAR_ASSETS: &sola_app::AssetBundle = &asset_bundle! {
    "/index.html" => (include_str!("../../web/index.html"), Html),
    "/src/menubar.ts" => (include_str!("../../web/src/menubar.ts"), TypeScript),
    "/assets/pillars.svg" => (include_str!("../../web/assets/pillars.svg"), Svg),
    "/assets/flower.svg" => (include_str!("../../web/assets/flower.svg"), Svg),
};

/// Create and register the menubar window.
///
/// This is the keyboard target surface for Meta-driven shell key handling.
pub fn setup_menubar(ctx: &mut AppCtx) -> WindowHandle {
    ctx.add_window(WindowConfig {
        title: "menubar".into(),
        size: (1920, zoning::MENUBAR_HEIGHT),
        position: Some((0, 0)),
        decorated: false,
        transparent: true,
        assets: MENUBAR_ASSETS,
        initial_state: None,
        zoned: false,
        keyboard_target: true,
    })
}
