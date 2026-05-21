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

/* Construct a new WPEDisplay (subclass of WPEDisplayHeadless) that
 * advertises LINEAR-only buffer formats. Caller owns the reference
 * (g_object_unref to free, though the engine usually keeps it alive
 * via wpe_display_set_primary). */
WPEDisplay *sola_wpe_display_new(void);
