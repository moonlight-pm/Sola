/* sola_wpe.h — public surface of the WPE Platform subclasses we
 * compile from sola_wpe.c. Rust includes this header through
 * bindgen so the FFI matches the C ABI exactly.
 *
 * The subclasses exist for one purpose: telling WPE "render into
 * ARGB8888 with DRM_FORMAT_MOD_LINEAR modifier" so the DMA-BUF the
 * WebProcess produces is row-major rather than NVIDIA block-linear.
 * Without that, sampling the imported buffer through wgpu yields
 * tile-pattern artifacts (because wgpu-hal doesn't enable
 * VK_EXT_image_drm_format_modifier).
 *
 * Doing the GObject subclass plumbing in C avoids the GType /
 * vmethod-table machinery we'd otherwise need to thread through
 * `gobject-sys` / `glib-sys` on the Rust side. */

#pragma once

/* wpe-platform.h is the umbrella for the new (modifier-negotiating)
 * API — pulls in WPEDisplay, WPEView, WPEBuffer + the subclasses
 * we need. Including the sub-headers directly trips their
 * "Only <wpe/wpe-platform.h> can be included directly" guards. */
#include <wpe/wpe-platform.h>

/* The WebKit API (WebKitWebView, webkit_web_view_evaluate_javascript,
 * JSCValue) — needed for the copy bridge below. Include guards make the
 * duplicate include from wpe_wrapper.h harmless. */
#include <wpe/webkit.h>

/* Callback the SolaView::render_buffer vmethod invokes for every
 * inbound frame. The user_data is the cookie passed to
 * sola_wpe_set_buffer_callback. `buffer` is borrowed; the C side
 * acks the engine via wpe_view_buffer_rendered immediately after
 * the callback returns. */
typedef void (*sola_wpe_buffer_cb)(void *user_data,
                                   WPEView *view,
                                   WPEBufferDMABuf *buffer);

/* Install the buffer callback (single global slot — we only have
 * one display in flight). Pass NULL to clear. */
void sola_wpe_set_buffer_callback(sola_wpe_buffer_cb cb, void *user_data);

/* Callback fired when WebKit changes the CSS cursor for the view.
 * `name` is a freedesktop cursor name like "default", "pointer",
 * "text", "grab", "ew-resize", etc. Borrowed for the call only —
 * dup it if you need to keep it. */
typedef void (*sola_wpe_cursor_cb)(void *user_data, const char *name);

/* Install the CSS-cursor callback. NULL clears. */
void sola_wpe_set_cursor_callback(sola_wpe_cursor_cb cb, void *user_data);

/* Callback delivering the page's current text selection (extracted via
 * window.getSelection().toString()). `text` is borrowed for the call
 * only — copy it if you need to keep it. Fired asynchronously after
 * sola_wpe_copy_selection() resolves; never fired for an empty result. */
typedef void (*sola_wpe_selection_cb)(void *user_data, const char *text);

/* Install the selection callback (single global slot). NULL clears. */
void sola_wpe_set_selection_callback(sola_wpe_selection_cb cb, void *user_data);

/* Asynchronously extract `view`'s active text selection and deliver it
 * to the selection callback. Used to bridge page copy to the system
 * clipboard, since the headless display has no Wayland clipboard. */
void sola_wpe_copy_selection(WebKitWebView *view);

/* Construct a new WPEDisplay (subclass of WPEDisplayHeadless) that
 * advertises LINEAR-only buffer formats. Caller owns the reference
 * (g_object_unref to free, though the engine usually keeps it alive
 * via wpe_display_set_primary). */
WPEDisplay *sola_wpe_display_new(void);
