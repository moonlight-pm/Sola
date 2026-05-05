//! CEF browser wrapper, one per window.
//!
//! `Browser::new` builds the CEF-side OSR browser bound to a Wayland
//! surface.  The render pipeline is:
//!
//!   CEF → `KitRenderHandler::on_accelerated_paint`
//!       → `Surface::present_dmabuf`
//!       → `zwp_linux_dmabuf_v1` wl_buffer → sola-river compositor

// `ImplBrowser` and `ImplFrame` are traits in `cef::*` — they must be in
// scope for `.main_frame()` and `.execute_java_script()` to be callable on
// the concrete `cef::Browser` / `cef::Frame` types.
#[allow(unused_imports)]
use cef::{rc::*, *};

use std::rc::Rc;

use crate::cef::handlers::{KitClient, KitRenderHandler};
use crate::wayland::Surface;

/// A CEF browser bound to a Wayland surface via OSR + dma-buf.
pub struct Browser {
    /// The underlying CEF browser handle.
    pub inner: cef::Browser,
}

impl Browser {
    /// Create a browser that renders into `surface` and loads `initial_url`.
    ///
    /// Flags set on `WindowInfo`:
    ///   - `windowless_rendering_enabled = 1` — OSR mode (no native window).
    ///   - `external_begin_frame_enabled = 1` — caller drives vsync via
    ///     Wayland frame callbacks; CEF will not self-tick.
    ///   - `shared_texture_enabled = 1` — dma-buf transport (Linux GBM).
    pub fn new(surface: Rc<Surface>, initial_url: &str) -> Self {
        // --- WindowInfo: OSR + external-begin-frame + shared-texture ---
        let mut window_info = cef::WindowInfo::default();
        window_info.windowless_rendering_enabled = 1;
        window_info.external_begin_frame_enabled = 1;
        window_info.shared_texture_enabled = 1;

        // --- BrowserSettings: opaque white background ---
        let mut browser_settings = cef::BrowserSettings::default();
        // Background colour is ARGB packed into u32: 0xFFFFFFFF = opaque white.
        browser_settings.background_color = 0xFFFF_FFFFu32;

        // --- Build the client with just a RenderHandler at this checkpoint ---
        // LoadHandler and IpcHandler land in D1/D5.
        let render_handler = KitRenderHandler::new(surface);
        let mut client = KitClient::new(render_handler);

        // --- URL ---
        let url = cef::CefString::from(initial_url);

        // --- CreateBrowserSync ---
        let inner = cef::browser_host_create_browser_sync(
            Some(&window_info),
            Some(&mut client),
            Some(&url),
            Some(&browser_settings),
            None, // extra_info
            None, // request_context — use default
        )
        .expect("cef::browser_host_create_browser_sync returned None");

        Self { inner }
    }

    /// Execute `script` in the browser's main frame.
    pub fn execute_js(&self, script: &str) {
        if let Some(frame) = self.inner.main_frame() {
            let code = cef::CefString::from(script);
            let url = cef::CefString::from("app:///inline.js");
            frame.execute_java_script(Some(&code), Some(&url), 0);
        }
    }

    /// Open DevTools for this browser.
    pub fn open_devtools(&self) {
        // TODO(taskE1)
    }
}
