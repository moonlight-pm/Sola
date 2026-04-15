use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;
use webkit6::prelude::*;

use sola_bus::BusClient;
use sola_bus::topics::{Topic, WindowPolicy, WindowPolicyPayload};

use crate::assets;
use crate::webview;
use crate::window::{JsDispatcher, WindowConfig, WindowHandle, WindowInner};

/// Effect handle passed to every `SolaApp` trait method.
/// Holds the bus, the GTK application, and the list of live windows.
pub struct AppCtx {
    pub(crate) bus: Rc<RefCell<BusClient>>,
    pub(crate) gtk_app: gtk4::Application,
    pub(crate) windows: Vec<WindowHandle>,
    pub(crate) app_id: &'static str,
}

impl AppCtx {
    pub(crate) fn new(
        bus: Rc<RefCell<BusClient>>,
        gtk_app: gtk4::Application,
        app_id: &'static str,
    ) -> Self {
        Self {
            bus,
            gtk_app,
            windows: Vec::new(),
            app_id,
        }
    }

    /// Create a new window. The returned handle can be stored as a field
    /// and used later to `eval_js`, `send_to_js`, etc.
    pub fn add_window(&mut self, cfg: WindowConfig) -> WindowHandle {
        let platform = Box::leak(Box::new(assets::platform_assets()));
        let html_raw = cfg
            .assets
            .find("/index.html")
            .map(|a| a.content.to_string())
            .unwrap_or_else(|| "<html><body>No index.html</body></html>".to_string());

        let html = if let Some(state_json) = cfg.initial_state.as_ref() {
            html_raw.replace("__RESTORED_STATE__", state_json)
        } else {
            html_raw
        };
        let html = crate::inject_import_map(&html);

        let web_context = webview::create_web_context(cfg.assets, platform, html);

        let dispatcher_slot: Rc<RefCell<Option<JsDispatcher>>> = Rc::new(RefCell::new(None));
        let ucm = webview::create_ucm_for_window(dispatcher_slot.clone());

        if cfg.transparent {
            let css = gtk4::CssProvider::new();
            css.load_from_data("window, window.background { background: transparent; }");
            gtk4::style_context_add_provider_for_display(
                &gdk4::Display::default().unwrap(),
                &css,
                gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }

        let gtk_window = gtk4::ApplicationWindow::new(&self.gtk_app);
        gtk_window.set_decorated(cfg.decorated);
        gtk_window.set_default_size(cfg.size.0, cfg.size.1);
        gtk_window.set_title(Some(&cfg.title));

        let webview = webkit6::WebView::builder()
            .web_context(&web_context)
            .user_content_manager(&ucm)
            .build();
        if let Some(settings) = webkit6::prelude::WebViewExt::settings(&webview) {
            settings.set_enable_developer_extras(true);
            settings.set_enable_write_console_messages_to_stdout(true);
        }
        // Suppress WebKit's default right-click menu.
        webview.connect_context_menu(|_, _, _| true);
        if cfg.transparent {
            webview.set_background_color(&gdk4::RGBA::new(0.0, 0.0, 0.0, 0.0));
        }
        gtk_window.set_child(Some(&webview));
        webview.load_uri("app:///index.html");

        let inner = WindowInner {
            title: cfg.title,
            webview,
            gtk_window: gtk_window.clone(),
            dispatcher: dispatcher_slot,
            zoned: cfg.zoned,
            keyboard_target: cfg.keyboard_target,
            size: cfg.size,
            position: cfg.position,
        };

        gtk_window.present();

        let handle = WindowHandle {
            inner: Rc::new(inner),
        };
        self.windows.push(handle.clone());
        handle
    }

    /// Close and remove a window.
    pub fn remove_window(&mut self, handle: &WindowHandle) {
        handle.inner.gtk_window.close();
        self.windows.retain(|w| w != handle);
    }

    /// Emit a bus event.
    pub fn emit(&self, topic: Topic) {
        let _ = self.bus.borrow_mut().emit(topic);
    }

    /// Emit a sticky bus event.
    pub fn emit_sticky(&self, topic: Topic) {
        let _ = self.bus.borrow_mut().emit_sticky(topic);
    }

    /// Build a `SetWindowPolicy` sticky from the windows collected during
    /// `A::new` and emit it. Called once by `run::<A>()`.
    pub(crate) fn emit_window_policy(&self) {
        let windows: Vec<WindowPolicy> = self
            .windows
            .iter()
            .map(|h| WindowPolicy {
                title: h.inner.title.clone(),
                zoned: h.inner.zoned,
                keyboard_target: h.inner.keyboard_target,
                // Zoned windows are sized by the compositor's zone system,
                // so skip the hint to match the pre-migration behavior.
                size: if h.inner.zoned {
                    None
                } else {
                    Some(h.inner.size)
                },
                position: h.inner.position,
            })
            .collect();
        self.emit_sticky(Topic::SetWindowPolicy(WindowPolicyPayload {
            app_id: self.app_id.to_string(),
            windows,
        }));
    }
}
