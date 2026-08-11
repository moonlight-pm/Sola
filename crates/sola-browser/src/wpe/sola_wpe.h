/* sola_wpe.h — public surface of the WPE Platform hooks we compile
 * from sola_wpe.c. Rust includes this header through bindgen so the
 * FFI matches the C ABI exactly.
 *
 * WPEDisplayHeadless / WPEViewHeadless are G_DECLARE_FINAL_TYPE — there
 * is no subclass path. We hijack vmethods on the concrete classes to:
 *   1. Install a buffer-rendered emission hook (frame delivery)
 *   2. Install cursor-from-name passthrough (CSS cursors)
 *   3. Bridge page selection → Rust for system clipboard copy
 *
 * A get_preferred_buffer_formats override remains as upstream-
 * aspirational (WebKit 2.52.3 never invokes it); LINEAR dma-buf import
 * is solved on the wgpu-hal import side via
 * VK_EXT_image_drm_format_modifier.
 *
 * Doing the GObject plumbing in C avoids threading GType / vmethod
 * tables through gobject-sys on the Rust side. */

#pragma once

#include <wpe/wpe-platform.h>
#include <wpe/webkit.h>

/* Fired for every inbound headless buffer. `buffer` is borrowed;
 * the Rust side dups the fd and later acks via wpe_view_buffer_released
 * (token-on-Drop). */
typedef void (*sola_wpe_buffer_cb)(void *user_data,
                                   WPEView *view,
                                   WPEBufferDMABuf *buffer);

void sola_wpe_set_buffer_callback(sola_wpe_buffer_cb cb, void *user_data);

/* CSS cursor name (freedesktop) when WebKit changes the cursor. */
typedef void (*sola_wpe_cursor_cb)(void *user_data, const char *name);

void sola_wpe_set_cursor_callback(sola_wpe_cursor_cb cb, void *user_data);

/* Page text selection (async after sola_wpe_copy_selection). */
typedef void (*sola_wpe_selection_cb)(void *user_data, const char *text);

void sola_wpe_set_selection_callback(sola_wpe_selection_cb cb, void *user_data);

/* Async extract selection → selection callback (system clipboard bridge). */
void sola_wpe_copy_selection(WebKitWebView *view);

/* Fire-and-forget JS on the page (password fill, etc.). No result callback. */
void sola_wpe_evaluate_js(WebKitWebView *view, const char *script);

/* Persistent profile: data + cache dirs under XDG (cookies, storage, service
 * workers). Caller owns the returned session ref. */
WebKitNetworkSession *sola_wpe_network_session_new(const char *data_dir,
                                                   const char *cache_dir);

/* WebView bound to a network session (or default if session is NULL). */
WebKitWebView *sola_wpe_web_view_new(WebKitNetworkSession *session);

/* New headless WPEDisplay. Caller owns the reference. */
WPEDisplay *sola_wpe_display_new(void);
