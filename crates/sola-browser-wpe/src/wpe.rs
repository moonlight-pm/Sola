//! WPE engine wrapper used by the main browser binary.
//!
//! Migrated from the libwpe + libwpe-fdo path to the WPE Platform
//! API (wpe-platform-2.0 + wpe-platform-headless-2.0). The Platform
//! API lets the consumer advertise modifier preferences via
//! `WPEDisplay::get_preferred_buffer_formats`, which is the
//! mechanism we need to ask for ARGB8888 + LINEAR so wgpu (without
//! VK_EXT_image_drm_format_modifier) can sample buffers correctly.
//!
//! See `src/sola_wpe.c` for the GObject vmethod hijack that makes
//! this work without subclassing the FINAL `WPEDisplayHeadless`.
//!
//! WebKit silently switches its internal renderer path to the
//! Platform API the first time any WPEDisplay subclass instance is
//! constructed (it checks via `g_type_class_peek(WPE_TYPE_DISPLAY)
//! != NULL`). Past that point `webkit_web_view_new(NULL)` builds a
//! WebView using the primary WPEDisplay.

#![allow(non_upper_case_globals, non_camel_case_types, non_snake_case)]
#![allow(unsafe_op_in_unsafe_fn)]

use std::ffi::{CString, c_void};
use std::os::fd::{FromRawFd, OwnedFd};
use std::ptr;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::thread::{self, JoinHandle};

use crate::wpe_sys as sys;

/// One frame as it crosses thread boundaries. The FD is dup'd by
/// the worker before sending so iced can own the lifetime
/// independent of WPE's buffer-recycle cycle.
pub struct WpeFrame {
    pub fd: OwnedFd,
    pub width: u32,
    pub height: u32,
    /// DRM fourcc (e.g. `0x34325241` = ARGB8888).
    pub format: u32,
    pub modifier: u64,
    pub stride: u32,
    pub offset: u32,
    /// Opaque tokens the consumer hands back via `Cmd::Release`
    /// when it's done with the frame. The pair (view, buffer) is
    /// what `wpe_view_buffer_released` needs.
    pub token: ResourceToken,
}

/// `Send + Sync`-safe wrapper around the raw `WPEView*` +
/// `WPEBuffer*` pair we get from the buffer-arrival callback.
/// Always treated as opaque off the worker thread.
#[derive(Clone, Copy, Debug)]
pub struct ResourceToken {
    pub view: *mut c_void,
    pub buffer: *mut c_void,
}

unsafe impl Send for ResourceToken {}
unsafe impl Sync for ResourceToken {}

pub enum Cmd {
    /// Request a new viewport size. Applied to the WPEView's
    /// toplevel via `wpe_toplevel_resize`; sent by the shader
    /// Program whenever the iced widget bounds change.
    Resize { width: u32, height: u32 },
    Release { token: ResourceToken },
    /// Forward a user input event into WPE. Materialized to a
    /// WPEEvent GObject on the worker thread and dispatched via
    /// `wpe_view_event`.
    Input(InputEvent),
    /// Toggle view focus. Iced delivers keyboard events only to
    /// focused widgets, but WPE has its own focus state which
    /// needs explicit `wpe_view_focus_in` / `wpe_view_focus_out`.
    Focus(bool),
    Quit,
}

/// A user input event in a thread-safe shape. Sent over the cmd
/// channel from iced (main thread) to the WPE worker, which turns
/// it into a `WPEEvent` and dispatches via `wpe_view_event`.
#[derive(Debug, Clone)]
pub enum InputEvent {
    /// Pointer motion. `x`/`y` are in WPE view-local pixels;
    /// `delta_x`/`delta_y` are the change since the previous
    /// PointerMove (0.0 for the first move). `modifiers` MUST
    /// include `WPE_MODIFIER_POINTER_BUTTON*` bits for any
    /// currently-held buttons — that's how WebKit distinguishes
    /// a drag from a plain hover.
    PointerMove {
        x: f64,
        y: f64,
        delta_x: f64,
        delta_y: f64,
        modifiers: u32,
        time_ms: u32,
    },
    /// Pointer button down/up. `button` is the X11/WPE convention:
    /// 1 = left, 2 = middle, 3 = right. `press_count` is filled in
    /// by the worker via `wpe_view_compute_press_count`.
    PointerButton {
        down: bool,
        x: f64,
        y: f64,
        button: u32,
        modifiers: u32,
        time_ms: u32,
    },
    /// Scroll. `delta_x` / `delta_y` are in CSS pixels (or wheel
    /// "ticks" if `precise` is false).
    Scroll {
        x: f64,
        y: f64,
        delta_x: f64,
        delta_y: f64,
        precise: bool,
        modifiers: u32,
        time_ms: u32,
    },
    /// Keyboard key down/up. `keyval` is the X11 keysym
    /// (`XK_*`); `keycode` is the hardware scancode (0 if we
    /// can't determine it — WebKit primarily uses `keyval`).
    Key {
        down: bool,
        keyval: u32,
        keycode: u32,
        modifiers: u32,
        time_ms: u32,
    },
}

pub struct WpeEngine {
    worker: Option<JoinHandle<()>>,
    cmd_tx: Sender<Cmd>,
    frames: Arc<Mutex<Receiver<WpeFrame>>>,
    /// Latest CSS cursor name (encoded as `CursorKind`) WebKit
    /// asked us to display. Written from the worker thread when
    /// `wpe_view_set_cursor_from_name` fires, read from iced's
    /// render thread by `mouse_interaction`.
    cursor: Arc<std::sync::atomic::AtomicU32>,
}

impl WpeEngine {
    /// Spawn the WPE worker. **Blocks** until the worker has
    /// finished the parts of WPE init that consult
    /// `WAYLAND_DISPLAY` (display creation + view creation + URL
    /// load kick-off). After this returns, the worker thread is
    /// running the GMainLoop and the caller can safely restore
    /// `WAYLAND_DISPLAY` if it manipulated it (see main.rs).
    pub fn spawn(url: &str, width: u32, height: u32) -> Self {
        let (cmd_tx, cmd_rx) = channel::<Cmd>();
        let (frame_tx, frame_rx) = channel::<WpeFrame>();
        let (ready_tx, ready_rx) = channel::<()>();
        let cursor = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let cursor_worker = cursor.clone();
        // Queue the initial size before the worker starts. The pump
        // stashes it in `pending_resize` until the first frame
        // latches `WorkerCtx::view`, then replays it. Without this
        // WPE's headless default (1024x768) leaks through as the
        // first frame's dimensions.
        let _ = cmd_tx.send(Cmd::Resize { width, height });
        let url = url.to_string();
        let worker = thread::Builder::new()
            .name("wpe-engine".into())
            .spawn(move || unsafe {
                worker_main(
                    url, width, height, frame_tx, cmd_rx, ready_tx, cursor_worker,
                )
            })
            .expect("spawn wpe-engine thread");
        // Block until WPE init is done. Without this, the env-var
        // dance in main.rs wouldn't have a sync point and iced
        // could observe WAYLAND_DISPLAY in either state.
        let _ = ready_rx.recv();
        Self {
            worker: Some(worker),
            cmd_tx,
            frames: Arc::new(Mutex::new(frame_rx)),
            cursor,
        }
    }

    /// Shared handle to the current cursor shape. Reads are
    /// non-blocking; safe to call from iced's render thread.
    pub fn cursor_handle(&self) -> Arc<std::sync::atomic::AtomicU32> {
        self.cursor.clone()
    }

    pub fn cmd_sender(&self) -> Sender<Cmd> {
        self.cmd_tx.clone()
    }

    pub fn frames(&self) -> Arc<Mutex<Receiver<WpeFrame>>> {
        self.frames.clone()
    }

    pub fn shutdown(mut self) {
        let _ = self.cmd_tx.send(Cmd::Quit);
        if let Some(h) = self.worker.take() {
            let _ = h.join();
        }
    }
}

// ---- worker thread ------------------------------------------------

struct WorkerCtx {
    main_loop: *mut sys::GMainLoop,
    frame_tx: Sender<WpeFrame>,
    cmd_rx: Receiver<Cmd>,
    /// Latched on the first `buffer-rendered` we observe so the
    /// command pump can resolve `wpe_view_get_toplevel` for resize
    /// without going through WebKit APIs. Null until the first
    /// frame arrives.
    view: *mut sys::WPEView,
    /// Resize commands that arrived before `view` was latched.
    /// Replayed once the first frame populates `view`. Only the
    /// most recent size is kept — older requests are obsolete.
    pending_resize: Option<(u32, u32)>,
    /// Mirror of `WpeEngine::cursor` — the worker writes into this
    /// from the `sola_wpe_set_cursor_callback` callback. Same Arc,
    /// just held here so the callback's user_data resolves to it.
    cursor: Arc<std::sync::atomic::AtomicU32>,
}

unsafe fn worker_main(
    url: String,
    _width: u32,
    _height: u32,
    frame_tx: Sender<WpeFrame>,
    cmd_rx: Receiver<Cmd>,
    ready_tx: Sender<()>,
    cursor: Arc<std::sync::atomic::AtomicU32>,
) {
    // 1. Construct our subclass-hijacked WPEDisplayHeadless. This
    //    is what flips WebKit's internal path to the Platform API
    //    (see `isUsingWPEPlatformAPI` in libWPEWebKit, which just
    //    checks `g_type_class_peek(WPE_TYPE_DISPLAY) != NULL`).
    let display = sys::sola_wpe_display_new();
    if display.is_null() {
        panic!("sola_wpe_display_new returned null");
    }
    let mut err: *mut sys::GError = ptr::null_mut();
    if sys::wpe_display_connect(display, &mut err) == 0 {
        let msg = if !err.is_null() {
            std::ffi::CStr::from_ptr((*err).message)
                .to_string_lossy()
                .into_owned()
        } else {
            "(no error)".into()
        };
        panic!("wpe_display_connect failed: {msg}");
    }
    sys::wpe_display_set_primary(display);
    tracing::info!("WPE platform display ready (subclassed for LINEAR-only modifier)");

    // Bump font-hinting from default SLIGHT to FULL for crisper
    // text. WPE's WPESettings defaults are registered with
    // SLIGHT hinting + RGB subpixel + antialias on (see
    // `WPESettings.cpp` defaultSettings table). FULL hinting
    // snaps glyph features to the pixel grid more aggressively
    // and gives sharper edges at the expense of slight
    // distortion of the glyph's true shape — usually the right
    // tradeoff for body text on a desktop browser.
    let settings = sys::wpe_display_get_settings(display);
    if !settings.is_null() {
        let key = std::ffi::CString::new("/wpe-platform/font-hinting-style").unwrap();
        let mut err: *mut sys::GError = ptr::null_mut();
        let ok = sys::wpe_settings_set_byte(
            settings,
            key.as_ptr(),
            sys::WPESettingsHintingStyle_WPE_SETTINGS_HINTING_STYLE_FULL as u8,
            sys::WPESettingsSource_WPE_SETTINGS_SOURCE_APPLICATION,
            &mut err,
        );
        if ok == 0 {
            let msg = if !err.is_null() {
                std::ffi::CStr::from_ptr((*err).message)
                    .to_string_lossy()
                    .into_owned()
            } else {
                "(no error)".into()
            };
            tracing::warn!("font-hinting-style=FULL rejected: {msg}");
        } else {
            tracing::info!("WPE font hinting set to FULL");
        }
    }

    // 2. Install our buffer-rendered + cursor callbacks. The C
    //    side installs a GObject emission hook on the WPEView
    //    signal (for buffers) and overrides the
    //    `set_cursor_from_name` vmethod on WPEViewHeadlessClass
    //    (for cursor). Both fire on this same worker thread.
    let ctx = Box::into_raw(Box::new(WorkerCtx {
        main_loop: ptr::null_mut(),
        frame_tx,
        cmd_rx,
        view: ptr::null_mut(),
        pending_resize: None,
        cursor,
    }));
    sys::sola_wpe_set_buffer_callback(Some(on_buffer_rendered), ctx as *mut c_void);
    sys::sola_wpe_set_cursor_callback(Some(on_cursor_changed), ctx as *mut c_void);

    // 3. Build a WebKitWebView. With `isUsingWPEPlatformAPI()` true
    //    (we forced that by constructing a display above), passing
    //    NULL as the backend tells WebKit to create one from the
    //    primary WPEDisplay.
    let view = sys::webkit_web_view_new(ptr::null_mut());
    if view.is_null() {
        panic!("webkit_web_view_new(NULL) returned null");
    }
    tracing::info!("created WebKitWebView via Platform API path");

    let url_c = CString::new(url.as_str()).unwrap();
    sys::webkit_web_view_load_uri(view as *mut _, url_c.as_ptr());
    tracing::info!(url = %url, "kicked off URL load");

    // Tell the spawner we're done with the parts of WPE init that
    // examine WAYLAND_DISPLAY. The main thread is blocked in
    // `spawn`, holding WAYLAND_DISPLAY unset, and will restore it
    // and start iced once we signal here. After this point WPE
    // shouldn't be opening new Wayland connections — it has its
    // headless WPEDisplay and renders to DMA-BUFs.
    let _ = ready_tx.send(());

    // 4. GMain loop. Poll the command channel from a timeout source
    //    so Release / Quit are handled alongside WPE's own events.
    let main_loop = sys::g_main_loop_new(ptr::null_mut(), 0);
    (*ctx).main_loop = main_loop;
    sys::g_timeout_add(16, Some(cb_pump_cmds), ctx as *mut c_void);

    tracing::info!("WPE engine entering GMainLoop");
    sys::g_main_loop_run(main_loop);
    tracing::info!("WPE engine GMainLoop exited");

    sys::sola_wpe_set_buffer_callback(None, ptr::null_mut());
    sys::sola_wpe_set_cursor_callback(None, ptr::null_mut());
    let _ = Box::from_raw(ctx);
}

/// Called from C when WebKit changes the CSS cursor. `name` is
/// a borrowed UTF-8 cstr; we copy the bytes briefly to translate,
/// then store the discriminant in the shared atomic. iced's
/// `mouse_interaction` polls that atomic each frame.
unsafe extern "C" fn on_cursor_changed(
    user_data: *mut c_void,
    name: *const std::os::raw::c_char,
) {
    let ctx = &*(user_data as *mut WorkerCtx);
    let kind = if name.is_null() {
        crate::input::CursorKind::Default
    } else {
        let s = std::ffi::CStr::from_ptr(name).to_string_lossy();
        crate::input::parse_cursor_name(&s)
    };
    ctx.cursor.store(
        kind as u32,
        std::sync::atomic::Ordering::Relaxed,
    );
}

unsafe extern "C" fn on_buffer_rendered(
    user_data: *mut c_void,
    view: *mut sys::WPEView,
    buffer: *mut sys::WPEBufferDMABuf,
) {
    let ctx = &mut *(user_data as *mut WorkerCtx);

    // Latch the view ptr on the first frame so cmd_pump can target
    // it for resize. The WebKitWebView wraps a WPEView internally
    // but doesn't expose it via public API — observing buffer
    // emissions is how we capture it.
    if ctx.view.is_null() {
        ctx.view = view;
        if let Some((w, h)) = ctx.pending_resize.take() {
            apply_resize(view, w, h);
        }
    }

    let buffer_base = buffer as *mut sys::WPEBuffer;
    let width = sys::wpe_buffer_get_width(buffer_base);
    let height = sys::wpe_buffer_get_height(buffer_base);
    let n_planes = sys::wpe_buffer_dma_buf_get_n_planes(buffer);
    if n_planes != 1 {
        tracing::warn!(n_planes, "ignoring multi-plane frame");
        return;
    }
    let format = sys::wpe_buffer_dma_buf_get_format(buffer);
    let modifier = sys::wpe_buffer_dma_buf_get_modifier(buffer);
    let stride = sys::wpe_buffer_dma_buf_get_stride(buffer, 0);
    let offset = sys::wpe_buffer_dma_buf_get_offset(buffer, 0);
    let raw_fd = sys::wpe_buffer_dma_buf_get_fd(buffer, 0);

    tracing::trace!(
        w = width,
        h = height,
        format = format!("{:#x}", format),
        modifier = format!("{:#x}", modifier),
        stride,
        fd = raw_fd,
        "WPE produced DMA-BUF frame",
    );

    let dup_fd = libc::fcntl(raw_fd, libc::F_DUPFD_CLOEXEC, 0);
    if dup_fd < 0 {
        tracing::warn!(err = ?std::io::Error::last_os_error(), "dup of dmabuf fd failed");
        return;
    }

    let frame = WpeFrame {
        fd: OwnedFd::from_raw_fd(dup_fd),
        width: width as u32,
        height: height as u32,
        format,
        modifier,
        stride,
        offset,
        token: ResourceToken {
            view: view as *mut c_void,
            buffer: buffer as *mut c_void,
        },
    };
    if ctx.frame_tx.send(frame).is_err() {
        tracing::info!("frame channel closed, quitting GMainLoop");
        sys::g_main_loop_quit(ctx.main_loop);
    }
}

unsafe extern "C" fn cb_pump_cmds(data: *mut c_void) -> sys::gboolean {
    let ctx = &mut *(data as *mut WorkerCtx);
    loop {
        match ctx.cmd_rx.try_recv() {
            Ok(Cmd::Resize { width, height }) => {
                if ctx.view.is_null() {
                    // View not yet observed via buffer-rendered;
                    // remember the most recent size and replay
                    // once we have a view ptr.
                    ctx.pending_resize = Some((width, height));
                } else {
                    apply_resize(ctx.view, width, height);
                }
            }
            Ok(Cmd::Release { token }) => {
                // Tell WPE we're done with this buffer; it may now
                // recycle the underlying DMA-BUF.
                sys::wpe_view_buffer_released(
                    token.view as *mut sys::WPEView,
                    token.buffer as *mut sys::WPEBuffer,
                );
            }
            Ok(Cmd::Input(ev)) => {
                if !ctx.view.is_null() {
                    dispatch_input(ctx.view, ev);
                }
            }
            Ok(Cmd::Focus(focused)) => {
                if !ctx.view.is_null() {
                    if focused {
                        sys::wpe_view_focus_in(ctx.view);
                    } else {
                        sys::wpe_view_focus_out(ctx.view);
                    }
                }
            }
            Ok(Cmd::Quit) => {
                sys::g_main_loop_quit(ctx.main_loop);
                return 0; /* G_SOURCE_REMOVE */
            }
            Err(TryRecvError::Empty) => break,
            Err(TryRecvError::Disconnected) => {
                sys::g_main_loop_quit(ctx.main_loop);
                return 0;
            }
        }
    }
    1 /* G_SOURCE_CONTINUE */
}

/// Materialize an `InputEvent` as a WPEEvent GObject and dispatch
/// to the view. Each event constructor takes ownership of the
/// new event ref; `wpe_view_event` consumes it (we don't need to
/// `wpe_event_unref` ourselves).
unsafe fn dispatch_input(view: *mut sys::WPEView, ev: InputEvent) {
    let mouse_src = sys::WPEInputSource_WPE_INPUT_SOURCE_MOUSE;
    let kbd_src = sys::WPEInputSource_WPE_INPUT_SOURCE_KEYBOARD;
    let event = match ev {
        InputEvent::PointerMove { x, y, delta_x, delta_y, modifiers, time_ms } => {
            sys::wpe_event_pointer_move_new(
                sys::WPEEventType_WPE_EVENT_POINTER_MOVE,
                view,
                mouse_src,
                time_ms,
                modifiers,
                x,
                y,
                delta_x,
                delta_y,
            )
        }
        InputEvent::PointerButton { down, x, y, button, modifiers, time_ms } => {
            let type_ = if down {
                sys::WPEEventType_WPE_EVENT_POINTER_DOWN
            } else {
                sys::WPEEventType_WPE_EVENT_POINTER_UP
            };
            // press_count drives single/double/triple-click on the
            // web side. WPE keeps the per-view bookkeeping for us.
            // UP events MUST use press_count=0; otherwise WebKit's
            // click-synthesis path (mousedown+mouseup → click) won't
            // fire, which breaks link navigation and form submit.
            // See `WPEWaylandSeat.cpp:178` for the upstream reference.
            let press_count = if down {
                sys::wpe_view_compute_press_count(view, x, y, button, time_ms)
            } else {
                0
            };
            sys::wpe_event_pointer_button_new(
                type_,
                view,
                mouse_src,
                time_ms,
                modifiers,
                button,
                x,
                y,
                press_count,
            )
        }
        InputEvent::Scroll { x, y, delta_x, delta_y, precise, modifiers, time_ms } => {
            sys::wpe_event_scroll_new(
                view,
                mouse_src,
                time_ms,
                modifiers,
                delta_x,
                delta_y,
                if precise { 1 } else { 0 },
                0, /* is_stop — only relevant for kinetic/touchpad scroll */
                x,
                y,
            )
        }
        InputEvent::Key { down, keyval, keycode, modifiers, time_ms } => {
            let type_ = if down {
                sys::WPEEventType_WPE_EVENT_KEYBOARD_KEY_DOWN
            } else {
                sys::WPEEventType_WPE_EVENT_KEYBOARD_KEY_UP
            };
            sys::wpe_event_keyboard_new(
                type_,
                view,
                kbd_src,
                time_ms,
                modifiers,
                keycode,
                keyval,
            )
        }
    };
    if event.is_null() {
        tracing::warn!("wpe_event_*_new returned null; dropping input");
        return;
    }
    sys::wpe_view_event(view, event);
}

/// Resize the view's toplevel. WPE's WebProcess picks this up and
/// produces subsequent buffers at the new size. Idempotent — calling
/// with the same size as before is a no-op inside WPE.
unsafe fn apply_resize(view: *mut sys::WPEView, width: u32, height: u32) {
    if view.is_null() {
        return;
    }
    let toplevel = sys::wpe_view_get_toplevel(view);
    if toplevel.is_null() {
        tracing::warn!("wpe_view_get_toplevel returned null; cannot resize");
        return;
    }
    let ok = sys::wpe_toplevel_resize(toplevel, width as i32, height as i32);
    if ok == 0 {
        tracing::warn!(
            width,
            height,
            "wpe_toplevel_resize returned FALSE — backend rejected the size",
        );
        return;
    }
    // On Wayland backends `wpe_toplevel_resize` requests a size from
    // the compositor and the actual size lands later via a configure
    // event, which then triggers `wpe_toplevel_resized` /
    // `wpe_view_resized` internally. The headless backend has no
    // compositor round-trip — without calling the resized notifiers
    // ourselves, the size sticks at the WPEView level but the
    // WebProcess never gets told to re-render at the new size, so
    // frames keep coming at the headless default (1024x768).
    sys::wpe_toplevel_resized(toplevel, width as i32, height as i32);
    sys::wpe_view_resized(view, width as i32, height as i32);
    tracing::info!(width, height, "wpe_toplevel_resize accepted + notified");
}
