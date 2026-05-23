use sola_kit::asset_bundle;

pub static MENU_ASSETS: &sola_kit::AssetBundle = &asset_bundle! {
    "/menu.tsx" => (include_bytes!("../../web/menu.tsx"), Tsx),
    "/components/menu/menu.tsx" => (include_bytes!("../../web/components/menu/menu.tsx"), Tsx),
    "/components/menu/menu-item.tsx" => (include_bytes!("../../web/components/menu/menu-item.tsx"), Tsx),
    "/components/menu/menu.css" => (include_bytes!("../../web/components/menu/menu.css"), Css),
};
