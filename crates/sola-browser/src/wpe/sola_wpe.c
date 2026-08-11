/* WPE Platform integration helpers used by sola-browser.
 *
 * `WPEDisplayHeadless` / `WPEViewHeadless` are `G_DECLARE_FINAL_TYPE` —
 * no subclass path. We hijack class vmethods + an emission hook:
 *
 * 1. **Display** `get_preferred_buffer_formats` (LINEAR ARGB hint).
 * 2. **View** `set_cursor_from_name` → iced cursor.
 * 3. **ViewHeadless** `render_buffer` — **critical for paint quality**:
 *    deliver buffers to Rust for claim; FrameDone only after present
 *    (`sola_wpe_view_buffer_rendered_safe`); never auto-release.
 *
 * Class patches are process-wide (one display + engine in sola-browser).
 * `g_type_class_ref` runs class_init first, then we overwrite slots. */

#include "sola_wpe.h"

#include <gio/gio.h>
#include <stdlib.h>
#include <string.h>
#include <wpe/headless/wpe-headless.h>
#include <wpe/wayland/wpe-wayland.h>

/* Wire app_id for Option A content companion (must be GLib-valid:
 * reverse-DNS with at least one '.'). Matches sola_bus::BROWSER_CONTENT_APP_ID. */
#define SOLA_CONTENT_APP_ID "sola.browser-content"

void sola_wpe_prepare_wayland_identity(void) {
    static gsize once = 0;
    if (!g_once_init_enter(&once))
        return;

    /* WTF::applicationID() (WPEToplevelWayland) reads g_application_get_default()
     * then valid g_get_prgname(). Set both before any xdg_toplevel is created. */
    g_set_prgname(SOLA_CONTENT_APP_ID);
    g_set_application_name("Sola Browser Content");

    if (!g_application_get_default()) {
        GApplication *app = g_application_new(
            SOLA_CONTENT_APP_ID,
            G_APPLICATION_NON_UNIQUE);
        if (app) {
            /* Hold as process default for WTF::applicationID(); do not run. */
            g_application_set_default(app);
            /* Intentionally leak the ref for process lifetime. */
        } else {
            g_warning("sola: failed to create GApplication for content app_id");
        }
    }

    g_message("sola: wayland content identity app_id=%s", SOLA_CONTENT_APP_ID);
    g_once_init_leave(&once, 1);
}

void sola_wpe_view_set_toplevel_title(WPEView *view, const char *title) {
    if (!view || !WPE_IS_VIEW(view))
        return;
    WPEToplevel *toplevel = wpe_view_get_toplevel(view);
    if (!toplevel)
        return;
    wpe_toplevel_set_title(toplevel, title ? title : "");
}

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

static sola_wpe_cursor_cb s_cursor_cb = NULL;
static void              *s_cursor_ud = NULL;

void sola_wpe_set_cursor_callback(sola_wpe_cursor_cb cb, void *user_data) {
    s_cursor_cb = cb;
    s_cursor_ud = user_data;
}

static sola_wpe_selection_cb s_selection_cb = NULL;
static void                 *s_selection_ud = NULL;

void sola_wpe_set_selection_callback(sola_wpe_selection_cb cb, void *user_data) {
    s_selection_cb = cb;
    s_selection_ud = user_data;
}

/* ---- copy bridge: page selection -> Rust -> iced clipboard ------ */

/* WebKit's own "Copy" editing command writes to WebKit's internal
 * clipboard only — the custom headless WPEDisplay has no Wayland
 * clipboard backend, so the copy never reaches other apps. Instead we
 * pull the selected text out via JS and hand it to the Rust side, which
 * writes it to the system clipboard through iced (Wayland-backed). */
static void sola_on_js_selection(GObject *source, GAsyncResult *res,
                                 gpointer user_data) {
    (void)user_data;
    WebKitWebView *view = WEBKIT_WEB_VIEW(source);
    GError *error = NULL;
    JSCValue *value =
        webkit_web_view_evaluate_javascript_finish(view, res, &error);
    if (!value) {
        if (error) g_error_free(error);
        return;
    }
    if (jsc_value_is_string(value)) {
        char *text = jsc_value_to_string(value);
        if (s_selection_cb && text) {
            s_selection_cb(s_selection_ud, text);
        }
        if (text) g_free(text);
    }
    g_object_unref(value);
}

void sola_wpe_copy_selection(WebKitWebView *view) {
    if (!view) return;
    webkit_web_view_evaluate_javascript(
        view,
        "window.getSelection().toString()",
        -1,    /* length: -1 = NUL-terminated */
        NULL,  /* world_name */
        NULL,  /* source_uri */
        NULL,  /* cancellable */
        sola_on_js_selection,
        NULL); /* user_data — the result callback uses the global slot */
}

void sola_wpe_evaluate_js(WebKitWebView *view, const char *script) {
    if (!view || !script) return;
    /* No finish callback — fire-and-forget fill / helper scripts. */
    webkit_web_view_evaluate_javascript(
        view,
        script,
        -1,
        NULL,
        NULL,
        NULL,
        NULL,
        NULL);
}

WebKitNetworkSession *sola_wpe_network_session_new(const char *data_dir,
                                                   const char *cache_dir) {
    if (!data_dir || !cache_dir) return NULL;

    /* Network/Web processes run under bubblewrap. Custom cookie + storage
     * paths under XDG must be explicitly allowed or opens fail with
     * "Failed to load cookie file … Read-only file system" — cookies.db
     * then looks full on disk but HTTP requests never carry SID/LOGIN_INFO
     * after restart (YouTube keeps asking to sign in). */
    WebKitWebContext *ctx = webkit_web_context_get_default();
    if (ctx) {
        webkit_web_context_add_path_to_sandbox(ctx, data_dir, FALSE);
        webkit_web_context_add_path_to_sandbox(ctx, cache_dir, FALSE);
    }

    WebKitNetworkSession *session =
        webkit_network_session_new(data_dir, cache_dir);
    if (!session) return NULL;

    /* ITP / resource-load statistics rewrites SameSite and blocks
     * third-party Google cookies that YouTube's SSO still needs. Personal
     * browser: prefer "stay signed in" over tracking prevention. */
    webkit_network_session_set_itp_enabled(session, FALSE);

    /* Explicit SQLite cookie jar under data_dir. NetworkSession alone
     * does not always create a durable jar we can inspect/migrate; the
     * old GTK sola-browser path is cookies.db. data_dir is already on
     * the sandbox allowlist above so NetworkProcess can open it RW.
     *
     * ACCEPT_ALWAYS: Google/YouTube SSO sets cookies across google.* /
     * youtube.* redirects. */
    WebKitCookieManager *cookies =
        webkit_network_session_get_cookie_manager(session);
    if (cookies) {
        char *cookie_path = g_build_filename(data_dir, "cookies.db", NULL);
        /* Canonical absolute path — relative names become "cookie" inside
         * bwrap cwd and fail with EROFS. */
        char *abs_cookie = g_canonicalize_filename(cookie_path, NULL);
        webkit_cookie_manager_set_persistent_storage(
            cookies,
            abs_cookie ? abs_cookie : cookie_path,
            WEBKIT_COOKIE_PERSISTENT_STORAGE_SQLITE);
        webkit_cookie_manager_set_accept_policy(
            cookies, WEBKIT_COOKIE_POLICY_ACCEPT_ALWAYS);
        g_free(abs_cookie);
        g_free(cookie_path);
    }

    /* Keep HTTP auth / password manager credentials on disk too. */
    webkit_network_session_set_persistent_credential_storage_enabled(session,
                                                                     TRUE);
    return session;
}

WebKitWebView *sola_wpe_web_view_new(WebKitNetworkSession *session,
                                     WPEDisplay *display) {
    /* Always pass display when we have one. WebKit defaults to
     * wpe_display_get_default(), which is *not* the same as primary and will
     * invent a headless connection once WAYLAND_DISPLAY is cleared. */
    if (session && display) {
        return WEBKIT_WEB_VIEW(g_object_new(WEBKIT_TYPE_WEB_VIEW,
                                           "network-session", session,
                                           "display", display,
                                           NULL));
    }
    if (display) {
        return WEBKIT_WEB_VIEW(g_object_new(WEBKIT_TYPE_WEB_VIEW,
                                           "display", display,
                                           NULL));
    }
    if (session) {
        return WEBKIT_WEB_VIEW(g_object_new(WEBKIT_TYPE_WEB_VIEW,
                                           "network-session", session, NULL));
    }
    return webkit_web_view_new(NULL);
}

void sola_wpe_buffer_ref(WPEBuffer *buffer) {
    if (buffer)
        g_object_ref(buffer);
}

void sola_wpe_buffer_unref(WPEBuffer *buffer) {
    if (buffer)
        g_object_unref(buffer);
}

void sola_wpe_view_buffer_released_safe(WPEView *view, WPEBuffer *buffer) {
    if (!view || !buffer)
        return;
    /* Our claim path holds a ref, so the GObject should still be a WPEBuffer.
     * If not, skip — calling into freed memory SEGV'd (YouTube 19:54). */
    if (!WPE_IS_VIEW(view) || !WPE_IS_BUFFER(buffer)) {
        g_warning("sola: skip buffer_released (stale view/buffer %p %p)",
                  (void *)view, (void *)buffer);
        return;
    }
    wpe_view_buffer_released(view, buffer);
}

/* ---- vmethod hijack for set_cursor_from_name ------------------- */

/* WebKit calls wpe_view_set_cursor_from_name(view, name) whenever
 * the CSS cursor under the pointer changes. Standard FDO cursor
 * names: "default", "pointer", "text", "wait", "grab", "grabbing",
 * "ew-resize", "ns-resize", "crosshair", "move", "not-allowed", …
 * The headless backend has no native cursor surface, so by default
 * this slot is a no-op. We override it to forward the name to the
 * Rust side, where iced's `mouse_interaction` reads it and returns
 * the matching `iced::mouse::Interaction`. */
static void sola_view_set_cursor_from_name(WPEView *view, const char *name) {
    (void)view;
    if (s_cursor_cb) {
        s_cursor_cb(s_cursor_ud, name ? name : "default");
    }
}

/* ---- vmethod hijack for get_preferred_buffer_formats ------------ */

/* NOTE: as of WebKit 2.52.3 this vmethod is never invoked by the
 * engine — the GPU process picks modifiers independently of any
 * UI-process preference. The override stays in place so that we
 * benefit automatically once upstream wires the hint through.
 * Today the modifier story has to be solved on the import side
 * (see crates/sola-browser/src/wpe/wgpu_import.rs). */
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

/* ---- headless render_buffer: claim now, FrameDone after present ---- */

/*
 * Stock WPEViewHeadless (WebKit 2.52):
 *   pending = buffer;
 *   60 Hz timer → release(committed); committed = pending;
 *                 wpe_view_buffer_rendered(committed);
 *
 * Problems we hit:
 *   - Auto-release races present → black swaths / nav flicker
 *   - Early buffer_rendered (= FrameDone) lets WebKit recycle tiles
 *     while the buffer is still on the Wayland path → residual black
 *
 * Protocol (matches freeze + DRM/GTK present):
 *   1. render_buffer → latest-wins pending; deliver to sola (claim) when
 *      no buffer is in-flight awaiting FrameDone
 *   2. sola presents (plane frame cb / import blit)
 *   3. sola_wpe_view_buffer_rendered_safe → FrameDone (WebKit paces)
 *   4. sola_wpe_view_buffer_released_safe → return buffer after compositor
 *
 * Delivery is direct via s_buffer_cb — NOT via buffer-rendered emission
 * (that would re-enter claim when we signal FrameDone).
 */

typedef struct {
    WPEView *view;
    WPEBuffer *pending;   /* strong; waiting to deliver to sola */
    WPEBuffer *in_flight; /* strong; delivered; awaiting FrameDone */
    gboolean framedone;   /* FrameDone already called for in_flight */
} SolaViewPending;

static GHashTable *s_pending_by_view = NULL;

static void sola_pending_free(gpointer data) {
    SolaViewPending *p = (SolaViewPending *)data;
    if (p->pending) {
        if (p->view && WPE_IS_VIEW(p->view) && WPE_IS_BUFFER(p->pending))
            wpe_view_buffer_released(p->view, p->pending);
        g_object_unref(p->pending);
        p->pending = NULL;
    }
    if (p->in_flight) {
        if (p->view && WPE_IS_VIEW(p->view) && WPE_IS_BUFFER(p->in_flight)) {
            if (!p->framedone)
                wpe_view_buffer_rendered(p->view, p->in_flight);
            wpe_view_buffer_released(p->view, p->in_flight);
        }
        g_object_unref(p->in_flight);
        p->in_flight = NULL;
    }
    g_free(p);
}

static SolaViewPending *sola_pending_get(WPEView *view) {
    if (!s_pending_by_view) {
        s_pending_by_view = g_hash_table_new_full(
            g_direct_hash, g_direct_equal, NULL, sola_pending_free);
    }
    SolaViewPending *p =
        (SolaViewPending *)g_hash_table_lookup(s_pending_by_view, view);
    if (!p) {
        p = g_new0(SolaViewPending, 1);
        p->view = view;
        g_hash_table_insert(s_pending_by_view, view, p);
    }
    return p;
}

static void sola_try_deliver(SolaViewPending *p) {
    if (!p || p->in_flight || !p->pending || !s_buffer_cb)
        return;
    if (!WPE_IS_BUFFER_DMA_BUF(p->pending)) {
        /* Non-dmabuf: release without claim (nothing sola can present). */
        if (p->view && WPE_IS_VIEW(p->view) && WPE_IS_BUFFER(p->pending))
            wpe_view_buffer_released(p->view, p->pending);
        g_object_unref(p->pending);
        p->pending = NULL;
        return;
    }
    WPEBuffer *buf = p->pending;
    p->pending = NULL;
    p->in_flight = buf; /* transfers pending's strong ref */
    p->framedone = FALSE;
    s_buffer_cb(s_buffer_ud, p->view, WPE_BUFFER_DMA_BUF(buf));
}

void sola_wpe_view_buffer_rendered_safe(WPEView *view, WPEBuffer *buffer) {
    if (!view || !buffer)
        return;
    if (!WPE_IS_VIEW(view) || !WPE_IS_BUFFER(buffer)) {
        g_warning("sola: skip buffer_rendered (stale view/buffer %p %p)",
                  (void *)view, (void *)buffer);
        return;
    }

    SolaViewPending *p = NULL;
    if (s_pending_by_view)
        p = (SolaViewPending *)g_hash_table_lookup(s_pending_by_view, view);

    if (p && p->in_flight == buffer) {
        if (!p->framedone) {
            wpe_view_buffer_rendered(view, buffer);
            p->framedone = TRUE;
        }
        /* Drop deliver ref; sola claim still holds its own ref. */
        p->in_flight = NULL;
        g_object_unref(buffer);
        sola_try_deliver(p);
        return;
    }

    /* Orphan / already cleared — still emit FrameDone if WebKit needs it. */
    wpe_view_buffer_rendered(view, buffer);
    if (p)
        sola_try_deliver(p);
}

static gboolean
sola_view_render_buffer(WPEView *view,
                        WPEBuffer *buffer,
                        const WPERectangle *damage_rects,
                        guint n_damage_rects,
                        GError **error) {
    (void)damage_rects;
    (void)n_damage_rects;
    (void)error;
    if (!view || !buffer)
        return FALSE;

    SolaViewPending *p = sola_pending_get(view);

    /* Latest-wins for not-yet-delivered frames. */
    if (p->pending && p->pending != buffer) {
        if (WPE_IS_BUFFER(p->pending))
            wpe_view_buffer_released(view, p->pending);
        g_object_unref(p->pending);
        p->pending = NULL;
    }
    if (p->pending != buffer)
        p->pending = g_object_ref(buffer);

    /* Deliver immediately when previous FrameDone cleared in_flight.
     * WebKit paces further frames on our FrameDone — no 60 Hz timer. */
    sola_try_deliver(p);
    return TRUE;
}

/* ---- one-time setup --------------------------------------------- */

static void sola_wpe_init_headless_hijacks(void) {
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

    /* Hijack the headless view's set_cursor_from_name slot so we
     * see CSS cursor changes. Same pattern as the display class:
     * g_type_class_ref triggers class_init (installing the default
     * which is a no-op for headless), then we overwrite the slot.
     * The ref is intentionally leaked — the vtable must outlive
     * any view that might still dispatch through it. */
    WPEViewClass *view_class =
        (WPEViewClass *)g_type_class_ref(WPE_TYPE_VIEW_HEADLESS);
    view_class->set_cursor_from_name = sola_view_set_cursor_from_name;
    /* Own loan; FrameDone only after sola present (see above). */
    view_class->render_buffer = sola_view_render_buffer;

    /* No emission hook on buffer-rendered: FrameDone must not re-claim. */

    g_once_init_leave(&once_init, 1);
}

WPEDisplay *sola_wpe_display_new(int use_wayland) {
    if (use_wayland) {
        /* Stock WPEViewWayland present — no headless hijacks.
         * Connect with wpe_display_wayland_connect(display, name, err)
         * from Rust (pass WAYLAND_DISPLAY socket name). */
        sola_wpe_prepare_wayland_identity();
        g_message("sola: WPEDisplayWayland (stock present path, app_id=%s)",
                  SOLA_CONTENT_APP_ID);
        return wpe_display_wayland_new();
    }
    sola_wpe_init_headless_hijacks();
    return wpe_display_headless_new();
}
