/* bindgen entry point — includes every WPE / GLib symbol our probe
 * and (later) the main browser need. Keep this minimal; each new
 * header is more declarations bindgen has to walk. */

#include <glib.h>

#include <wpe/wpe.h>
/* wpe-fdo headers refuse direct includes of sub-files — `fdo.h` is
 * the only allowed entry point. It pulls in view-backend-exportable
 * + the rest. */
#include <wpe/fdo.h>
/* `wpe_fdo_initialize_for_egl_display` lives in this header — the
 * one-time backend init the engine needs before any view-backend
 * gets created. */
#include <wpe/fdo-egl.h>
/* wpe_fdo_initialize_dmabuf() — initializes the backend in pure
 * DMA-BUF mode without needing an EGL display. */
#include <wpe/unstable/fdo-dmabuf.h>

#include <EGL/egl.h>
#include <EGL/eglext.h>
#include <gbm.h>
#include <fcntl.h>

/* WPE Platform API (new, modifier-negotiating). Headless + Wayland
 * concrete displays (stock present vs sola content plane). */
#include <wpe/headless/wpe-headless.h>
#include <wpe/wayland/wpe-wayland.h>

/* Our local GObject subclasses. Built into the binary via build.rs
 * (cc-rs). */
#include "sola_wpe.h"

#include <wpe/webkit.h>
