use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;
use webkit6::prelude::*;

use sola_bus::topics::{FrameUpdate, MenuItem, Topic};

use crate::state::ShellState;
use crate::util::eval_js;
use crate::zoning;

static MENU_HTML: &str = include_str!("../../web/menu.html");

pub fn setup_menu_panel(
    window: &gtk4::ApplicationWindow,
    state: &Rc<RefCell<ShellState>>,
    bus: &Rc<RefCell<sola_bus::BusClient>>,
) {
    let app = window.application().unwrap();

    let menu_window = gtk4::ApplicationWindow::new(&app);
    menu_window.set_decorated(false);
    menu_window.set_default_size(1920, 1052);
    menu_window.set_title(Some("menu"));

    let css = gtk4::CssProvider::new();
    css.load_from_data(
        "window.menu-window, window.menu-window.background { background: transparent; }",
    );
    gtk4::style_context_add_provider_for_display(
        &gdk4::Display::default().unwrap(),
        &css,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
    menu_window.add_css_class("menu-window");

    let menu_webview = webkit6::WebView::new();
    menu_webview.set_background_color(&gdk4::RGBA::new(0.0, 0.0, 0.0, 0.0));
    if let Some(settings) = webkit6::prelude::WebViewExt::settings(&menu_webview) {
        settings.set_enable_developer_extras(true);
        settings.set_enable_write_console_messages_to_stdout(true);
    }
    menu_webview.connect_context_menu(|_, _, _| true);
    menu_webview.load_html(MENU_HTML, None);

    menu_webview.connect_notify_local(Some("title"), {
        let state = state.clone();
        let bus = bus.clone();
        move |webview, _| {
            let Some(title) = webview.title() else { return };
            let title = title.to_string();

            if title == "dismiss" {
                let mut s = state.borrow_mut();
                close_menu(&mut s, &|topic: Topic| {
                    let _ = bus.borrow_mut().emit(topic);
                });
                return;
            }

            if let Some(rest) = title.strip_prefix("action:") {
                if let Some((app_id, action_id)) = rest.split_once(':') {
                    tracing::info!(app_id, action_id, "menu action");

                    if app_id == "sola-shell" && action_id == "exit" {
                        let _ = bus.borrow_mut().emit(Topic::Shutdown);
                    } else {
                        let _ = bus.borrow_mut().emit(Topic::MenuAction(
                            sola_bus::topics::MenuActionPayload {
                                app_id: app_id.to_string(),
                                action_id: action_id.to_string(),
                            },
                        ));
                    }

                    let mut s = state.borrow_mut();
                    close_menu(&mut s, &|topic: Topic| {
                        let _ = bus.borrow_mut().emit(topic);
                    });
                }
            }
        }
    });

    menu_window.set_child(Some(&menu_webview));
    menu_window.present();

    state.borrow_mut().menu_webview = Some(menu_webview);
}

pub fn open_menu(s: &mut ShellState, source: &str, menu_index: usize, anchor_x: f64) {
    let app_id = if source == "system" {
        "sola-shell".to_string()
    } else {
        s.focused_app_id.clone().unwrap_or_default()
    };

    let menu = s.menus.get_menu(&app_id);
    let menu_def = menu.and_then(|m| m.menus.get(menu_index));
    let Some(menu_def) = menu_def else { return };

    let items: Vec<serde_json::Value> = menu_def
        .items
        .iter()
        .map(|item| match item {
            MenuItem::Action {
                id,
                label,
                shortcut,
                disabled,
                ..
            } => serde_json::json!({
                "type": "action",
                "id": id,
                "app_id": app_id,
                "label": label,
                "shortcut": shortcut,
                "disabled": disabled,
            }),
            MenuItem::Divider => serde_json::json!({ "type": "divider" }),
        })
        .collect();

    if let Some(ref wv) = s.menu_webview {
        let json = serde_json::to_string(&items).unwrap_or_default();
        eval_js(wv, &format!("showMenu({}, {})", json, anchor_x));
    }

    // Full-screen overlay below the menubar — transparent except for the dropdown.
    if let Some((ow, oh)) = s.zoning.output_size {
        if let Some(ref bus) = s.bus {
            let _ = bus.borrow_mut().emit(Topic::Frame(FrameUpdate {
                app_id: "sola-shell".into(),
                title: Some("menu".into()),
                x: 0,
                y: zoning::MENUBAR_HEIGHT,
                width: ow,
                height: oh - zoning::MENUBAR_HEIGHT,
            }));
        }
    }

    s.menu_open = true;
    if let Some(ref bus) = s.bus {
        let emit = |topic: Topic| {
            let _ = bus.borrow_mut().emit(topic);
        };
        s.emit_composition(&emit);
    }
}

pub fn close_menu(s: &mut ShellState, emit: &dyn Fn(Topic)) {
    if !s.menu_open {
        return;
    }
    s.menu_open = false;
    if let Some(ref wv) = s.menu_webview {
        eval_js(wv, "clearMenu()");
    }
    if let Some(ref wv) = s.menubar_webview {
        let msg = serde_json::json!({"event": "close_menu"}).to_string();
        let js_str = serde_json::to_string(&msg).unwrap_or_default();
        eval_js(wv, &format!("window.__solaRecv({js_str})"));
    }
    s.emit_composition(emit);
}
