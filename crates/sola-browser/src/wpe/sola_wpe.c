/* WPE Platform integration helpers used by sola-browser.
 *
 * `WPEDisplayHeadless` / `WPEViewHeadless` are `G_DECLARE_FINAL_TYPE` —
 * no subclass path. We hijack class vmethods + an emission hook:
 *
 * 1. **Display** `get_preferred_buffer_formats` (LINEAR ARGB hint).
 * 2. **View** `set_cursor_from_name` → iced cursor.
 * 3. **ViewHeadless** `render_buffer` — **critical for paint quality**:
 *    stock headless 60 Hz timer does release(previous) then
 *    buffer_rendered — that auto-release races iced import/blit
 *    (YouTube homepage black swaths / nav flicker). Our hijack:
 *      latest-wins pending + 60 Hz timer → buffer_rendered only
 *      never auto-release presented frames (sola after blit+Wait)
 * 4. **Signal emission hook** on `"buffer-rendered"` for frame delivery
 *    to Rust (claim + mailbox).
 *
 * Class patches are process-wide (one display + engine in sola-browser).
 * `g_type_class_ref` runs class_init first, then we overwrite slots. */

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

WebKitWebView *sola_wpe_web_view_new(WebKitNetworkSession *session) {
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

/* ---- headless render_buffer: sola owns loan, 60 Hz pace ---------- */

/*
 * Stock WPEViewHeadless (WebKit 2.52):
 *   pending = buffer;
 *   60 Hz timer → release(committed); committed = pending;
 *                 wpe_view_buffer_rendered(committed);
 *
 * Auto-release races iced import/blit → black swaths / nav flicker.
 * Immediate buffer_rendered uncapped FrameDone → ~200 Hz present
 * storm + live_buffers cap drops.
 *
 * Our path:
 *   - latest-wins pending per view (superseded: release without paint)
 *   - 60 Hz timer emits buffer_rendered only (FrameDone + sola claim)
 *   - never buffer_released of the presented frame here — sola after blit
 */

typedef struct {
    WPEView *view;
    WPEBuffer *pending; /* strong while waiting for tick */
} SolaViewPending;

static GHashTable *s_pending_by_view = NULL;
static GSource *s_frame_source = NULL;
static gint64 s_last_frame_time = 0;

static void sola_pending_free(gpointer data) {
    SolaViewPending *p = (SolaViewPending *)data;
    if (p->pending) {
        if (p->view && WPE_IS_VIEW(p->view) && WPE_IS_BUFFER(p->pending))
            wpe_view_buffer_released(p->view, p->pending);
        g_object_unref(p->pending);
        p->pending = NULL;
    }
    g_free(p);
}

static gboolean sola_frame_source_dispatch(GSource *source,
                                           GSourceFunc callback,
                                           gpointer user_data) {
    if (g_source_get_ready_time(source) == -1)
        return G_SOURCE_CONTINUE;
    g_source_set_ready_time(source, -1);
    return callback(user_data);
}

static GSourceFuncs s_frame_source_funcs = {
    NULL, /* prepare */
    NULL, /* check */
    sola_frame_source_dispatch,
    NULL, /* finalize */
    NULL,
    NULL,
};

static gboolean sola_frame_tick(gpointer user_data) {
    (void)user_data;
    if (!s_pending_by_view)
        return G_SOURCE_CONTINUE;

    GHashTableIter iter;
    gpointer key, value;
    g_hash_table_iter_init(&iter, s_pending_by_view);
    while (g_hash_table_iter_next(&iter, &key, &value)) {
        SolaViewPending *p = (SolaViewPending *)value;
        if (!p->pending || !p->view)
            continue;
        WPEBuffer *buf = p->pending;
        p->pending = NULL;
        /* FrameDone + emission hook → sola claim. Sola owns the loan. */
        wpe_view_buffer_rendered(p->view, buf);
        g_object_unref(buf);
    }
    return G_SOURCE_CONTINUE;
}

static void sola_arm_frame_timer(void) {
    if (!s_frame_source)
        return;
    gint64 now = g_get_monotonic_time();
    if (!s_last_frame_time)
        s_last_frame_time = now;
    gint64 next = s_last_frame_time + (G_USEC_PER_SEC / 60);
    s_last_frame_time = now;
    if (next <= now)
        g_source_set_ready_time(s_frame_source, 0);
    else
        g_source_set_ready_time(s_frame_source, next);
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

    /* Latest-wins: release superseded frame that never painted. */
    if (p->pending && p->pending != buffer) {
        if (WPE_IS_BUFFER(p->pending))
            wpe_view_buffer_released(view, p->pending);
        g_object_unref(p->pending);
        p->pending = NULL;
    }
    if (p->pending != buffer)
        p->pending = g_object_ref(buffer);

    sola_arm_frame_timer();
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

    /* Hijack the headless view's set_cursor_from_name slot so we
     * see CSS cursor changes. Same pattern as the display class:
     * g_type_class_ref triggers class_init (installing the default
     * which is a no-op for headless), then we overwrite the slot.
     * The ref is intentionally leaked — the vtable must outlive
     * any view that might still dispatch through it. */
    WPEViewClass *view_class =
        (WPEViewClass *)g_type_class_ref(WPE_TYPE_VIEW_HEADLESS);
    view_class->set_cursor_from_name = sola_view_set_cursor_from_name;
    /* Own loan + 60 Hz pace; no stock auto-release (see above). */
    view_class->render_buffer = sola_view_render_buffer;

    /* Process-wide frame timer (ready-time driven, like stock headless). */
    s_frame_source = g_source_new(&s_frame_source_funcs, sizeof(GSource));
    g_source_set_priority(s_frame_source, G_PRIORITY_DEFAULT);
    g_source_set_name(s_frame_source, "sola headless frame timer");
    g_source_set_callback(s_frame_source, sola_frame_tick, NULL, NULL);
    g_source_attach(s_frame_source, NULL);
    g_source_set_ready_time(s_frame_source, -1);

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
