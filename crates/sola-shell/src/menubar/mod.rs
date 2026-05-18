use sola_kit::{AppCtx, WindowConfig, WindowHandle, asset_bundle};

use crate::zoning;

/// Embedded web assets for the shell menubar window.
pub static MENUBAR_ASSETS: &sola_kit::AssetBundle = &asset_bundle! {
    "/menubar.tsx" => (include_bytes!("../../web/menubar.tsx"), Tsx),
    "/assets/pillars.svg" => (include_bytes!("../../web/assets/pillars.svg"), Svg),
    "/assets/flower.svg" => (include_bytes!("../../web/assets/flower.svg"), Svg),
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
        root_component: Some("/menubar.tsx"),
    })
}
