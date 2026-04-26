use std::cell::RefCell;
use std::rc::Rc;

use gtk4::glib::Propagation;
use gtk4::prelude::*;
use webkit6::prelude::*;

use sola_bus::BusClient;
use sola_bus::topics::{Topic, Window};

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
    /// Latest `Windows` sticky snapshot, used by the framework to correlate
    /// window_ids in bus topics (e.g. Copy/Paste) back to a `WindowHandle`.
    pub(crate) known_windows: Vec<Window>,
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
            known_windows: Vec::new(),
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
        let html = crate::inject_solarecv_bootstrap(&html);
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
        // Sola apps exit only via bus CloseApp → on_close_app → ctx.shutdown.
        // Swallowing xdg_toplevel.close here prevents the compositor's graceful-
        // close flow for external apps from also taking down sola apps.
        gtk_window.connect_close_request(|_win| Propagation::Stop);

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

        let loaded: Rc<RefCell<bool>> = Rc::new(RefCell::new(false));
        let pending: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        {
            let loaded = loaded.clone();
            let pending = pending.clone();
            let webview_for_drain = webview.clone();
            webview.connect_load_changed(move |_, event| {
                if event != webkit6::LoadEvent::Finished {
                    return;
                }
                *loaded.borrow_mut() = true;
                let queued: Vec<String> = std::mem::take(&mut *pending.borrow_mut());
                for script in queued {
                    crate::window::eval_js_now(&webview_for_drain, &script);
                }
            });
        }

        webview.load_uri("app:///index.html");

        let inner = WindowInner {
            title: cfg.title,
            webview,
            gtk_window: gtk_window.clone(),
            dispatcher: dispatcher_slot,
            loaded,
            pending,
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

    /// Emit a bus event. Sticky/persistent semantics are determined by
    /// the topic kind's `Behavior`.
    pub fn emit(&self, topic: Topic) {
        let _ = self.bus.borrow_mut().emit(topic);
    }

    /// Trigger a clean shutdown: calls `gtk::Application::quit` so the GTK
    /// main loop exits. The `on_shutdown` hook is called by the framework
    /// before this path is reached when coming from `Topic::Shutdown`; for
    /// `on_close_app`-initiated exits the hook is also invoked before quit.
    pub fn shutdown(&self) {
        self.gtk_app.quit();
    }

    /// Resolve a `window_id` (as seen on the bus) to one of *this process's*
    /// owned `WindowHandle`s. Returns `None` if the id doesn't belong to us.
    ///
    /// Matching is by `(app_id, title)` pulled from the latest `Apps` sticky
    /// snapshot. `app_id` guards against a coincidental title collision with
    /// another Sola app.
    pub(crate) fn find_window_by_id(&self, window_id: u32) -> Option<&WindowHandle> {
        let entry = self
            .known_windows
            .iter()
            .find(|a| a.window_id == window_id)?;
        if entry.app_id != self.app_id {
            return None;
        }
        self.windows.iter().find(|w| w.title() == entry.title)
    }
}
