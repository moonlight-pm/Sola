use sola_kit::asset_bundle;

pub static LAUNCHER_ASSETS: &sola_kit::AssetBundle = &asset_bundle! {
    "/launcher.tsx" => (include_bytes!("../../web/launcher.tsx"), Tsx),
    "/components/launcher/launcher.tsx" => (include_bytes!("../../web/components/launcher/launcher.tsx"), Tsx),
    "/components/launcher/app-row.tsx" => (include_bytes!("../../web/components/launcher/app-row.tsx"), Tsx),
    "/components/launcher/launcher.css" => (include_bytes!("../../web/components/launcher/launcher.css"), Css),
};
