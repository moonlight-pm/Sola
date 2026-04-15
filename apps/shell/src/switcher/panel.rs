use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;
use webkit6::prelude::*;

use crate::state::ShellState;

static OVERLAY_HTML: &str = include_str!("../../web/overlay.html");
static OVERLAY_JS: &str = include_str!("../../web/src/overlay.ts");

pub fn setup_switcher_panel(
    window: &gtk4::ApplicationWindow,
    state: &Rc<RefCell<ShellState>>,
    _bus: &Rc<RefCell<sola_bus::BusClient>>,
) {
    let app = window.application().unwrap();

    let switcher_window = gtk4::ApplicationWindow::new(&app);
    switcher_window.set_decorated(false);
    switcher_window.set_default_size(800, 400);
    switcher_window.set_title(Some("switcher"));

    let css = gtk4::CssProvider::new();
    css.load_from_data(
        "window.switcher-window, window.switcher-window.background { background: transparent; }",
    );
    gtk4::style_context_add_provider_for_display(
        &gdk4::Display::default().unwrap(),
        &css,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
    switcher_window.add_css_class("switcher-window");

    let switcher_webview = webkit6::WebView::new();
    switcher_webview.set_background_color(&gdk4::RGBA::new(0.0, 0.0, 0.0, 0.0));
    if let Some(settings) = webkit6::prelude::WebViewExt::settings(&switcher_webview) {
        settings.set_enable_developer_extras(true);
        settings.set_enable_write_console_messages_to_stdout(true);
    }
    switcher_webview.connect_context_menu(|_, _, _| true);

    let html = OVERLAY_HTML.replace("__OVERLAY_JS__", OVERLAY_JS);
    switcher_webview.load_html(&html, None);

    switcher_window.set_child(Some(&switcher_webview));
    switcher_window.present();

    state.borrow_mut().switcher_webview = Some(switcher_webview);
}
