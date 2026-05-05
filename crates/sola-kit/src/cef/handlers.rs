//! CEF callback handler implementations. Each handler trait that the
//! binding crate exposes gets implemented here and wired in
//! `browser::Browser::new`. Most handlers run on CEF's UI thread, which
//! is our main thread — so dispatch to surface methods is direct.

// TODO(taskB10/B11): RenderHandler with on_accelerated_paint forwarding to Surface.
// TODO(taskD): LoadHandler, IpcHandler.
