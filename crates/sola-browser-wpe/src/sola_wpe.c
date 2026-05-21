/* WPE Platform integration helpers used by sola-browser-wpe.
 *
 * `WPEDisplayHeadless` (the concrete class we want to use) is
 * declared with `G_DECLARE_FINAL_TYPE` — its instance/class struct
 * is private, so the standard "subclass it" approach is closed off.
 * Instead we use two GObject tricks:
 *
 * 1. **Class vmethod hijack** to swap in our own
 *    `get_preferred_buffer_formats` on the headless display class.
 *    `g_type_class_ref` returns the live class struct; we write to
 *    its `get_preferred_buffer_formats` slot and keep the ref so
 *    the override stays in place for the process lifetime.
 *
 * 2. **Signal emission hook** to receive buffer-rendered events on
 *    every WPEView the engine creates, without needing per-instance
 *    `g_signal_connect`. The hook fires on every emission of
 *    "buffer-rendered" against the WPEView base type and any subclass.
 *
 * Both tricks affect the *class* (so the whole process) — fine
 * because sola-browser-wpe hosts one display + one engine.
 *
 * `WEBKIT_FORCE_*` env vars or anything else that depends on the
 * subclassed display picking up our overrides at construction time
 * works because `g_type_class_ref` runs class_init before we patch
 * the vtable — i.e. WPE's own defaults are installed first, then
 * ours replace them. Existing displays would see the new vtable
 * too. */

#include "sola_wpe.h"

#include <wpe/headless/wpe-headless.h>

/* DRM format / modifier constants — keeping these inline avoids a
 * libdrm header dependency in the build graph. */
#define DRM_FORMAT_ARGB8888  0x34325241u
#define DRM_FORMAT_MOD_LINEAR 0ULL

static sola_wpe_buffer_cb s_buffer_cb = NULL;
static void              *s_buffer_ud = NULL;

void sola_wpe_set_buffer_callback(sola_wpe_buffer_cb cb, void *user_data) {
    s_buffer_cb = cb;
    s_buffer_ud = user_data;
}

/* ---- vmethod hijack for get_preferred_buffer_formats ------------ */

/* NOTE: as of WebKit 2.52.3 this vmethod is never invoked by the
 * engine — the GPU process picks modifiers independently of any
 * UI-process preference. The override stays in place so that we
 * benefit automatically once upstream wires the hint through.
 * Today the modifier story has to be solved on the import side
 * (see crates/sola-browser-wpe/src/wgpu_import.rs). */
static WPEBufferFormats *
sola_get_preferred_buffer_formats(WPEDisplay *display) {
    WPEDRMDevice *drm = wpe_display_get_drm_device(display);
    WPEBufferFormatsBuilder *builder = wpe_buffer_formats_builder_new(drm);
    wpe_buffer_formats_builder_append_group(
        builder, drm, WPE_BUFFER_FORMAT_USAGE_RENDERING);
    wpe_buffer_formats_builder_append_format(
        builder, DRM_FORMAT_ARGB8888, DRM_FORMAT_MOD_LINEAR);
    return wpe_buffer_formats_builder_end(builder);
}

/* ---- emission hook on WPEView::buffer-rendered ------------------ */

static gboolean
on_buffer_rendered_emission(GSignalInvocationHint *hint,
                            guint n_param_values,
                            const GValue *param_values,
                            gpointer user_data) {
    (void)hint;
    (void)user_data;
    if (n_param_values < 2) {
        return TRUE; /* keep the hook installed */
    }
    WPEView *view = (WPEView *)g_value_get_object(&param_values[0]);
    GObject *buf_obj = g_value_get_object(&param_values[1]);
    if (s_buffer_cb && buf_obj && WPE_IS_BUFFER_DMA_BUF(buf_obj)) {
        s_buffer_cb(s_buffer_ud, view, WPE_BUFFER_DMA_BUF(buf_obj));
    }
    return TRUE;
}

/* ---- one-time setup --------------------------------------------- */

static void sola_wpe_init_once(void) {
    static gsize once_init = 0;
    if (!g_once_init_enter(&once_init)) {
        return;
    }

    /* class_init fires inside g_type_class_ref the first time it's
     * called — WPE's own defaults are installed there. Our override
     * goes in *after* that, so we genuinely replace the default
     * rather than racing it. We intentionally never unref: the
     * class struct must stay alive (and our patched vtable along
     * with it) for the lifetime of the process. */
    WPEDisplayClass *display_class =
        (WPEDisplayClass *)g_type_class_ref(WPE_TYPE_DISPLAY_HEADLESS);
    display_class->get_preferred_buffer_formats =
        sola_get_preferred_buffer_formats;

    /* Ref WPEView so signals are registered, then look up the
     * signal id we want and install our emission hook. The hook
     * fires for every emission against any WPEView (or subclass),
     * which is the whole point of using one. */
    g_type_class_ref(WPE_TYPE_VIEW);
    guint sig = g_signal_lookup("buffer-rendered", WPE_TYPE_VIEW);
    if (sig != 0) {
        g_signal_add_emission_hook(
            sig, 0, on_buffer_rendered_emission, NULL, NULL);
    }

    g_once_init_leave(&once_init, 1);
}

WPEDisplay *sola_wpe_display_new(void) {
    sola_wpe_init_once();
    return wpe_display_headless_new();
}
