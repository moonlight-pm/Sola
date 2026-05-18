use sola_kit::asset_bundle;

pub static SWITCHER_ASSETS: &sola_kit::AssetBundle = &asset_bundle! {
    "/switcher.tsx" => (include_bytes!("../../web/switcher.tsx"), Tsx),
};
