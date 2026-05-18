use sola_kit::asset_bundle;

pub static LAUNCHER_ASSETS: &sola_kit::AssetBundle = &asset_bundle! {
    "/launcher.tsx" => (include_bytes!("../../web/launcher.tsx"), Tsx),
};
