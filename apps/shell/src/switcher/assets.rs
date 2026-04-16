use sola_app::asset_bundle;

pub static SWITCHER_ASSETS: &sola_app::AssetBundle = &asset_bundle! {
    "/index.html" => (include_str!("../../web/overlay.html"), Html),
    "/src/overlay.ts" => (include_str!("../../web/src/overlay.ts"), TypeScript),
};
