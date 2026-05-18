use sola_kit::asset_bundle;

pub static MENU_ASSETS: &sola_kit::AssetBundle = &asset_bundle! {
    "/menu.tsx" => (include_bytes!("../../web/menu.tsx"), Tsx),
};
