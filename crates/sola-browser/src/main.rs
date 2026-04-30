mod app;
mod chrome;
mod migrate;
mod state;
mod tabs;

use app::BrowserApp;
use sola_app::asset_bundle;

pub static APP_ASSETS: &sola_app::AssetBundle = &asset_bundle! {
    "/index.html" => (include_str!("../web/index.html"), Html),
    "/src/main.ts" => (include_str!("../web/src/main.ts"), TypeScript),
    "/src/app.ts" => (include_str!("../web/src/app.ts"), TypeScript),
    "/src/tabs.ts" => (include_str!("../web/src/tabs.ts"), TypeScript),
    "/src/address.ts" => (include_str!("../web/src/address.ts"), TypeScript),
    "/src/toasts.ts" => (include_str!("../web/src/toasts.ts"), TypeScript),
    "/src/icons.ts" => (include_str!("../web/src/icons.ts"), TypeScript),
    "/src/theme.css" => (include_str!("../web/src/theme.css"), Css),
};

fn main() {
    sola_app::run::<BrowserApp>();
}
