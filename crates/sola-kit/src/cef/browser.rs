//! CEF browser wrapper, one per window.

#[allow(unused_imports)]
use std::rc::Rc;
// TODO(taskB8): the Surface type lives in crate::wayland::Surface (added
// in B8). For B4 we keep the import path so it compiles once B8 lands.
// Until then this won't resolve — accepted at the B4 checkpoint.
//use crate::wayland::Surface;

/// A CEF browser bound to a Wayland surface.
pub struct Browser {
    // TODO(taskB10): wrap the binding crate's Browser handle.
}

impl Browser {
    /// Create a browser that paints into `surface` and loads `initial_url`.
    pub fn new(/* _surface: Rc<crate::wayland::Surface>, */ _initial_url: &str) -> Self {
        // TODO(taskB10): build CefBrowserSettings + CefWindowInfo + RenderHandler
        // and call CreateBrowserSync.
        Self {}
    }

    /// Execute JS in the main frame.
    pub fn execute_js(&self, _script: &str) {
        // TODO(taskD4)
    }

    /// Open DevTools for this browser in a new OSR-managed Surface.
    pub fn open_devtools(&self) {
        // TODO(taskE1)
    }
}
