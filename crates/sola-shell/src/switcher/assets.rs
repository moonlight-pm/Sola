use sola_kit::asset_bundle;

pub static SWITCHER_ASSETS: &sola_kit::AssetBundle = &asset_bundle! {
    "/switcher.tsx" => (include_bytes!("../../web/switcher.tsx"), Tsx),
    "/components/switcher/switcher.tsx" => (include_bytes!("../../web/components/switcher/switcher.tsx"), Tsx),
    "/components/switcher/switcher-card.tsx" => (include_bytes!("../../web/components/switcher/switcher-card.tsx"), Tsx),
    "/components/switcher/switcher.css" => (include_bytes!("../../web/components/switcher/switcher.css"), Css),
};
