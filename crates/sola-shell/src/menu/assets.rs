use sola_app::asset_bundle;

pub static MENU_ASSETS: &sola_app::AssetBundle = &asset_bundle! {
    "/index.html" => (include_str!("../../web/menu.html"), Html),
    "/src/menu.ts" => (include_str!("../../web/src/menu.ts"), TypeScript),
};
