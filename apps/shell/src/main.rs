mod config;
mod keys;
mod menu;
mod state;
mod switcher;
mod util;
mod zoning;

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;

use sola_app::{SolaApp, asset_bundle};
use sola_bus::topics::Topic;

use state::ShellState;

static MENUBAR_ASSETS: &sola_app::AssetBundle = &asset_bundle! {
    "/index.html" => (include_str!("../web/index.html"), Html),
    "/src/menubar.ts" => (include_str!("../web/src/menubar.ts"), TypeScript),
};

fn main() {
    let state: Rc<RefCell<ShellState>> = Rc::new(RefCell::new(ShellState::new()));

    SolaApp::builder()
        .app_id("sola-shell")
        .window_size(1920, zoning::MENUBAR_HEIGHT)
        .decorated(false)
        .transparent(true)
        .web_assets(MENUBAR_ASSETS)
        .on_js_command({
            let state = state.clone();
            move |cmd, args| match cmd {
                "open_menu" => {
                    let source = args.get("source").and_then(|v| v.as_str()).unwrap_or("app");
                    let index = args.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                    let anchor_x = args.get("anchor_x").and_then(|v| v.as_f64()).unwrap_or(0.0);
                    let mut s = state.borrow_mut();
                    menu::open_menu(&mut s, source, index, anchor_x);
                }
                "close_menu" => {
                    let mut s = state.borrow_mut();
                    if let Some(ref bus) = s.bus {
                        let bus = bus.clone();
                        let emit = move |topic: Topic| {
                            let _ = bus.borrow_mut().emit(topic);
                        };
                        menu::close_menu(&mut s, &emit);
                    }
                }
                _ => {}
            }
        })
        .on_bus_event({
            let state = state.clone();
            move |topic, send_to_js, emit| {
                let mut s = state.borrow_mut();
                match topic {
                    Topic::Apps(apps) => {
                        s.handle_apps_update(apps.clone(), emit);

                        if s.switcher.active {
                            let json = serde_json::to_string(&s.switcher.apps).unwrap_or_default();
                            let script =
                                format!("renderSwitcher({}, {})", json, s.switcher.selected);
                            if let Some(ref wv) = s.switcher_webview {
                                util::eval_js(wv, &script);
                            }
                        }
                    }
                    Topic::SetAppMenu(payload) => {
                        s.menus.set_menu(payload.clone());

                        if s.focused_app_id.as_deref() == Some(&payload.app_id) {
                            let app_name = payload
                                .menus
                                .first()
                                .map(|d| d.label.as_str())
                                .unwrap_or(&payload.app_id);
                            let menu_labels: Vec<String> =
                                payload.menus.iter().map(|d| d.label.clone()).collect();
                            send_to_js(serde_json::json!({
                                "event": "focus",
                                "app_name": app_name,
                                "menu_labels": menu_labels,
                            }));
                        }
                    }
                    Topic::OutputGeometry(geo) => {
                        s.zoning.set_output_size(geo);
                        s.emit_all_frames(emit);
                        s.emit_composition(emit);
                    }
                    _ => {}
                }
            }
        })
        .on_activate({
            let state = state.clone();
            move |window, _webview, bus| {
                use sola_bus::topics::{
                    AppMenuPayload, MenuDefinition, MenuItem, WindowPolicy, WindowPolicyPayload,
                };

                let _ = bus
                    .borrow_mut()
                    .emit_sticky(Topic::SetWindowPolicy(WindowPolicyPayload {
                        app_id: "sola-shell".into(),
                        windows: vec![
                            WindowPolicy {
                                title: "menubar".into(),
                                zoned: false,
                                keyboard_target: true,
                                size: Some((1920, zoning::MENUBAR_HEIGHT)),
                                position: Some((0, 0)),
                            },
                            WindowPolicy {
                                title: "switcher".into(),
                                zoned: false,
                                keyboard_target: false,
                                size: Some((800, 400)),
                                position: Some((560, 340)),
                            },
                            WindowPolicy {
                                title: "menu".into(),
                                zoned: false,
                                keyboard_target: false,
                                size: Some((220, 300)),
                                position: Some((0, zoning::MENUBAR_HEIGHT)),
                            },
                        ],
                    }));

                // Register the shell's system menu (for shortcut lookup).
                state.borrow_mut().menus.set_menu(AppMenuPayload {
                    app_id: "sola-shell".into(),
                    menus: vec![MenuDefinition {
                        label: "Sola".into(),
                        items: vec![MenuItem::Action {
                            id: "exit".into(),
                            label: "Exit Sola".into(),
                            shortcut: Some("Super+Shift+Backspace".into()),
                            disabled: false,
                            checked: false,
                        }],
                    }],
                });

                window.set_title(Some("menubar"));

                let webview_ref = {
                    let child = window.child().unwrap();
                    child.downcast::<webkit6::WebView>().unwrap()
                };
                state.borrow_mut().menubar_webview = Some(webview_ref);

                state.borrow_mut().bus = Some(bus.clone());

                switcher::setup_switcher_panel(window, &state, &bus);
                menu::setup_menu_panel(window, &state, &bus);
                keys::setup_key_controller(window, &state, &bus);

                tracing::info!("shell ready");
            }
        })
        .run();
}
