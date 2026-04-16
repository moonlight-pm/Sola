use sola_app::asset_bundle;

pub static LAUNCHER_ASSETS: &sola_app::AssetBundle = &asset_bundle! {
    "/index.html" => (include_str!("../../web/launcher.html"), Html),
    "/src/launcher.ts" => (include_str!("../../web/src/launcher.ts"), TypeScript),
};
