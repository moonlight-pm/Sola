use sola_kit::{AppCtx, WindowConfig, WindowHandle, asset_bundle};

use crate::zoning;

/// Embedded web assets for the shell menubar window.
pub static MENUBAR_ASSETS: &sola_kit::AssetBundle = &asset_bundle! {
    "/menubar.tsx" => (include_bytes!("../../web/menubar.tsx"), Tsx),
    "/components/menubar/menubar.tsx" => (include_bytes!("../../web/components/menubar/menubar.tsx"), Tsx),
    "/components/menubar/system-menu.tsx" => (include_bytes!("../../web/components/menubar/system-menu.tsx"), Tsx),
    "/components/menubar/app-title.tsx" => (include_bytes!("../../web/components/menubar/app-title.tsx"), Tsx),
    "/components/menubar/menu-labels.tsx" => (include_bytes!("../../web/components/menubar/menu-labels.tsx"), Tsx),
    "/components/menubar/tray.tsx" => (include_bytes!("../../web/components/menubar/tray.tsx"), Tsx),
    "/components/menubar/menubar.css" => (include_bytes!("../../web/components/menubar/menubar.css"), Css),
    "/assets/pillars.svg" => (include_bytes!("../../web/assets/pillars.svg"), Svg),
    "/assets/flower.svg" => (include_bytes!("../../web/assets/flower.svg"), Svg),
};

/// Create and register the menubar window.
///
/// This is the keyboard target surface for Meta-driven shell key handling.
pub fn setup_menubar(ctx: &mut AppCtx, initial: serde_json::Value) -> WindowHandle {
    ctx.add_window(WindowConfig {
        title: "menubar".into(),
        size: (1920, zoning::MENUBAR_HEIGHT),
        position: Some((0, 0)),
        decorated: false,
        transparent: true,
        assets: MENUBAR_ASSETS,
        initial_state: Some(initial),
        zoned: false,
        keyboard_target: true,
        root_component: Some("/menubar.tsx"),
    })
}
