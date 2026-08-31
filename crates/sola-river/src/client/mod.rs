//! Wayland client state and dispatch.
//!
//! A single `AppData` owns every cross-cutting state: bus handle, registries,
//! pending update, proxy caches, and the `QueueHandle`. Each interface
//! implements `Dispatch<T, _>` on `AppData`.

use std::collections::HashMap;

use tracing::{info, warn};
use wayland_client::{
    Connection, Dispatch, EventQueue, Proxy, QueueHandle,
    backend::ObjectId,
    protocol::{wl_output, wl_pointer, wl_registry, wl_seat, wl_shm},
};
use wayland_protocols::wp::cursor_shape::v1::client::{
    wp_cursor_shape_device_v1::WpCursorShapeDeviceV1,
    wp_cursor_shape_manager_v1::WpCursorShapeManagerV1,
};

use crate::bus::BusClient;
use crate::pending::PendingUpdate;
use crate::protocol::river_libinput_config_v1::river_libinput_config_v1::RiverLibinputConfigV1;
use crate::protocol::river_window_management_v1::{
    river_node_v1::RiverNodeV1, river_output_v1::RiverOutputV1, river_seat_v1::RiverSeatV1,
    river_window_manager_v1::RiverWindowManagerV1, river_window_v1::RiverWindowV1,
};
use crate::protocol::river_xkb_bindings_v1::{
    river_xkb_bindings_seat_v1::RiverXkbBindingsSeatV1, river_xkb_bindings_v1::RiverXkbBindingsV1,
};
use crate::protocol::virtual_keyboard_unstable_v1::zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1;
use crate::protocol::wlr_output_management_unstable_v1::zwlr_output_manager_v1::ZwlrOutputManagerV1;
use crate::protocol::wlr_virtual_pointer_unstable_v1::zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1;
use crate::registry::{ChordRegistry, WindowRegistry};

pub mod binding;
pub mod input;
pub mod layer_shell;
pub mod manage;
pub mod op;
pub mod output_config;
pub mod screenshot;
pub mod seat;
pub mod shadow;
pub mod virtual_keyboard;
pub mod virtual_pointer;
pub mod window;

pub struct AppData {
    pub wm: Option<RiverWindowManagerV1>,
    pub xkb_bindings: Option<RiverXkbBindingsV1>,
    pub seat: Option<RiverSeatV1>,
    pub wl_seat: Option<wl_seat::WlSeat>,
    pub registry: WindowRegistry,
    pub pending: PendingUpdate,
    pub chords: ChordRegistry,
    pub bus: BusClient,
    /// Map from the Wayland object id of a `river_window_v1` to our u32.
    pub windows_by_object: HashMap<ObjectId, u32>,
    /// Map from our u32 to the live proxy. Needed for `propose_dimensions`,
    /// `set_borders`, and `focus_window`.
    pub windows_by_id: HashMap<u32, RiverWindowV1>,
    /// Map from our u32 to the per-window `river_node_v1`, created eagerly
    /// after `river_window_manager_v1::Event::Window` fires.
    pub nodes_by_window: HashMap<u32, RiverNodeV1>,
    /// Live `river_output_v1` proxies. Stored so their Dispatch impl
    /// keeps receiving dimensions events (hotplug-safe).
    pub outputs: Vec<RiverOutputV1>,
    /// Last-known dimensions of the primary output, used to center
    /// unzoned windows.
    pub output_size: Option<(i32, i32)>,
    /// Layout origin of the first logical output (`river_output_v1.position`).
    /// Screencopy regions are output-local; `pointer_position` is global.
    pub output_origin: Option<(i32, i32)>,
    /// Windows we have already positioned at least once, either via an
    /// explicit shell Frame or our own centered default. Prevents the
    /// default from re-firing across subsequent render passes.
    pub placed: std::collections::HashSet<u32>,
    /// Windows that have received their first `river_window_v1.dimensions`
    /// event. Until a window is in this set, we never send it a sizing
    /// configure — only `propose_dimensions(0, 0)` (self-size) — so a
    /// GPU/Vulkan client can build its swapchain against its own size
    /// before any resize arrives. See `manage::size_decision`.
    pub first_dimensions: std::collections::HashSet<u32>,
    /// Sizes (zone or restore) requested for a window before it was
    /// initialized, held back until its first `dimensions` event. Applied
    /// as a normal runtime resize on the next manage cycle. See
    /// `manage::note_dimensions`.
    pub deferred_size: HashMap<u32, (i32, i32)>,
    /// Last dimensions we sent River via `propose_dimensions` for each
    /// window. We skip re-proposing an unchanged size so an identical
    /// configure isn't re-sent to the client every time the shell
    /// re-broadcasts its frames. See `manage::should_send`.
    pub last_proposed: HashMap<u32, (i32, i32)>,
    /// Instant of first `dimensions` for gamescope hosts — size hold/debounce.
    pub gamescope_first_dim_at: HashMap<u32, std::time::Instant>,
    /// Instant of last size propose for gamescope hosts — size debounce.
    pub gamescope_last_size_at: HashMap<u32, std::time::Instant>,
    /// Last position we sent River via `node.set_position` for each window.
    /// We skip repositioning a window that has not moved. See
    /// `manage::should_send`.
    pub last_position: HashMap<u32, (i32, i32)>,
    /// Windows currently in compositor-fullscreen state because we
    /// called `proxy.fullscreen`. Used by the focus-change handler to
    /// auto-exit fullscreen when the user Alt-Tabs away — many games
    /// (Enshrouded) toggle to "Windowed" mode internally without ever
    /// sending `xdg_toplevel.unset_fullscreen`, so the surface stays
    /// z-stacked above everything and Alt-Tab gives no visual feedback.
    pub currently_fullscreen: std::collections::HashSet<u32>,
    /// Mode selection state for `zwlr_output_manager_v1` — we use this
    /// protocol to pick the highest resolution ≥60Hz on startup.
    pub output_config: output_config::OutputConfigState,
    /// `zwp_virtual_keyboard_v1` state — used to synthesize Ctrl+C / Ctrl+V
    /// into non-Sola clients when the shell's Meta+C/V chords fire.
    pub virtual_keyboard: virtual_keyboard::VirtualKeyboardState,
    /// `zwlr_virtual_pointer_v1` state — used by solactl to script
    /// pointer movement and clicks.
    pub virtual_pointer: virtual_pointer::VirtualPointerState,
    /// Held so its `device` events keep firing; preferences (natural
    /// scroll) are applied in `client/input.rs`.
    pub libinput_config: Option<RiverLibinputConfigV1>,
    pub qh: Option<QueueHandle<Self>>,
    /// Cloned from the wayland `Connection` so bus_tick (running on the
    /// calloop timer source) can flush outgoing wayland requests. Without
    /// this, `manage_dirty` and friends queue forever.
    pub conn: Option<Connection>,
    /// Window we last told River to focus via `seat.focus_window`. River
    /// does not auto-clear `seat.focused` when a window is destroyed, and
    /// it asserts in `Window.destroy` that no seat is still focused on the
    /// dying window — so we must clear focus ourselves when this window
    /// receives a `closed` event.
    pub focused_window: Option<u32>,
    /// Windows the shell has marked floating (`Topic::WindowFloating`). Gates
    /// CSD move/resize. Dropped when the window closes.
    pub floating: std::collections::HashSet<u32>,
    /// Window currently under the pointer, tracked from `river_seat_v1`
    /// `pointer_enter`/`pointer_leave`. A move/resize op targets this window at
    /// button-press time.
    pub pointer_window: Option<u32>,
    /// Latest pointer position in compositor logical coords (`pointer_position`
    /// event). Used to pick the grabbed corner when a resize starts.
    pub pointer_pos: Option<(i32, i32)>,
    /// The in-flight interactive move/resize, if any. See `client::op`.
    /// Started only from CSD (`pointer_move_requested` / resize).
    pub op: Option<op::OpState>,
    /// `wp_cursor_shape_manager_v1`, bound from the registry. Used to make a
    /// cursor-shape device for the seat's pointer.
    pub cursor_shape_manager: Option<WpCursorShapeManagerV1>,
    /// The seat's `wl_pointer`, obtained from `wl_seat`. Held alive so the
    /// cursor-shape device stays valid; its own events are ignored.
    pub wl_pointer: Option<wl_pointer::WlPointer>,
    /// Cursor-shape device for `wl_pointer`. During an op river uses the WM's
    /// pointer cursor (no client has focus), so `set_shape` here drives the
    /// move/resize cursor.
    pub cursor_device: Option<WpCursorShapeDeviceV1>,
    /// `wlr-screencopy` + `wl_shm` + `wl_output` state for screenshots.
    pub screenshot: screenshot::ScreenshotState,
    /// Incoming sola-call invokes (`compositor.*`).
    pub call_rx: Option<std::sync::mpsc::Receiver<sola_call::Incoming>>,
    /// Floating-window drop shadows (`get_decoration_below` + SHM silhouette).
    pub shadow: shadow::ShadowState,
    /// `river_layer_shell_v1` — enables wlr-layer-shell for clients
    /// (sola-kvm edge capture, panels, etc.).
    pub layer_shell: layer_shell::LayerShellState,
    /// Last composition stack (bottom→top). Re-applied every render so a
    /// newly mapped window that is not yet in the list stays **hidden**
    /// (shell overlays used to flash at default-center before Frame).
    pub last_composition: Vec<u32>,
}

/// Operator hook (`compositor.cursor`). River honors the magic xcursor
/// names `sola-cursor-hidden` / `sola-cursor-visible` when patched.
pub fn set_pointer_visible(state: &AppData, visible: bool) {
    let Some(seat) = state.seat.as_ref() else {
        return;
    };
    let name = if visible {
        "sola-cursor-visible"
    } else {
        "sola-cursor-hidden"
    };
    seat.set_xcursor_theme(name.to_string(), 24);
    if let Some(conn) = state.conn.as_ref() {
        if let Err(e) = conn.flush() {
            warn!(%e, "wayland flush after cursor visibility failed");
        }
    }
    info!(visible, "compositor pointer visibility");
}

impl AppData {
    pub fn new(bus: BusClient) -> Self {
        Self {
            wm: None,
            xkb_bindings: None,
            seat: None,
            wl_seat: None,
            registry: WindowRegistry::new(),
            pending: PendingUpdate::default(),
            chords: ChordRegistry::default(),
            bus,
            windows_by_object: HashMap::new(),
            windows_by_id: HashMap::new(),
            nodes_by_window: HashMap::new(),
            outputs: Vec::new(),
            output_size: None,
            output_origin: None,
            placed: std::collections::HashSet::new(),
            first_dimensions: std::collections::HashSet::new(),
            deferred_size: HashMap::new(),
            last_proposed: HashMap::new(),
            gamescope_first_dim_at: HashMap::new(),
            gamescope_last_size_at: HashMap::new(),
            last_position: HashMap::new(),
            currently_fullscreen: std::collections::HashSet::new(),
            output_config: output_config::OutputConfigState::default(),
            virtual_keyboard: virtual_keyboard::VirtualKeyboardState::default(),
            libinput_config: None,
            virtual_pointer: virtual_pointer::VirtualPointerState::default(),
            qh: None,
            conn: None,
            focused_window: None,
            floating: std::collections::HashSet::new(),
            pointer_window: None,
            pointer_pos: None,
            op: None,
            cursor_shape_manager: None,
            wl_pointer: None,
            cursor_device: None,
            screenshot: screenshot::ScreenshotState::default(),
            call_rx: None,
            shadow: shadow::ShadowState::default(),
            layer_shell: layer_shell::LayerShellState::default(),
            last_composition: Vec::new(),
        }
    }
}

/// Connect to River, bind globals, and return the conn/queue/data triple.
pub fn connect(
    bus: BusClient,
) -> Result<(Connection, EventQueue<AppData>, AppData), Box<dyn std::error::Error>> {
    let conn = Connection::connect_to_env()?;
    let display = conn.display();
    let mut queue = conn.new_event_queue::<AppData>();
    let qh = queue.handle();
    let _registry = display.get_registry(&qh, ());

    let mut data = AppData::new(bus);
    queue.roundtrip(&mut data)?;
    queue.roundtrip(&mut data)?;

    if data.wm.is_none() {
        return Err("river_window_manager_v1 not advertised; is River 0.4.2+ running?".into());
    }
    data.qh = Some(qh.clone());
    data.conn = Some(conn.clone());
    info!("bound river_window_manager_v1");

    // Virtual keyboard init needs both wl_seat and the vk manager bound.
    // Both arrive during the initial roundtrips above; if either is
    // missing, init_if_ready is a no-op and we'll never be able to
    // synthesize (logged on first attempt). In practice wlroots
    // advertises both by default.
    virtual_keyboard::init_if_ready(&mut data, &qh);
    virtual_pointer::init_if_ready(&mut data, &qh);

    Ok((conn, queue, data))
}

/// Called on every bus tick. Drains bus messages, folds them into `pending`,
/// and if pending is dirty, asks River to start a new manage cycle via
/// `manage_dirty`.
pub fn bus_tick(state: &mut AppData) {
    // sola-bus holds sticky topics only in memory. A bus restart wipes
    // OutputGeometry / Windows; if we stay connected without re-emitting,
    // a late shell (or a shell that also restarted for the install) never
    // frames the menubar and river's default placement centers the 28px
    // bar in the middle of the screen.
    if state.bus.ensure_connected() {
        republish_after_bus_reconnect(state);
    }
    // Hard-killed clients (lag spike + SIGKILL) never send `closed`.
    // Zombie sola-shell surfaces then occupy composition and the
    // replacement iced process does not map — menubar gone, process up.
    let pruned = crate::client::window::prune_dead_pid_windows(state);
    if pruned > 0 {
        tracing::info!(count = pruned, "pruned windows with dead pids");
        crate::translator::emit_windows(state);
        state.pending.manage_dirty = true;
        state.pending.render_dirty = true;
    }
    state.bus.drain_notify();
    // Screenshot PNG encode runs off-thread; deliver results here.
    screenshot::poll_results(state);
    crate::call::poll(state);
    while let Some(msg) = state.bus.try_recv() {
        let Some(topic) = sola_bus::topics::Topic::parse(&msg) else {
            continue;
        };
        match topic {
            sola_bus::topics::Topic::Composition(entries) => {
                tracing::debug!(count = entries.len(), "got Composition");
                let ids: Vec<u32> = entries.into_iter().map(|e| e.window_id).collect();
                state.pending.set_composition(ids);
            }
            sola_bus::topics::Topic::WindowFloating(wf) => {
                if wf.floating {
                    state.floating.insert(wf.window_id);
                } else {
                    state.floating.remove(&wf.window_id);
                }
                // Kick a manage/render cycle so decoration-below shadows
                // attach or tear down promptly (not only on the next frame).
                state.pending.manage_dirty = true;
            }
            sola_bus::topics::Topic::Frame(f) => {
                let app_id = state.registry.app_id_for(f.window_id).unwrap_or("?");
                tracing::info!(
                    window_id = f.window_id,
                    app_id,
                    x = f.x,
                    y = f.y,
                    w = f.width,
                    h = f.height,
                    fullscreen = f.fullscreen,
                    "got Frame"
                );
                // Ignore non-positive frames (Float zone sentinel / poisoned
                // FloatGeometry restore). Applying them would stick a 0×0
                // rect on the registry and break window-region screenshots
                // even when the surface later self-sizes correctly.
                if f.width <= 0 || f.height <= 0 {
                    tracing::warn!(
                        window_id = f.window_id,
                        app_id,
                        x = f.x,
                        y = f.y,
                        w = f.width,
                        h = f.height,
                        "ignoring non-positive Frame"
                    );
                } else {
                    state
                        .pending
                        .frame(f.window_id, f.x, f.y, f.width, f.height);
                    if f.fullscreen {
                        // Shell-initiated fullscreen (Cinema zone). Same
                        // path as a client-initiated request — manage_finish
                        // enters the surface into true xdg-shell fullscreen.
                        state.pending.queue_fullscreen(f.window_id);
                    } else if state.currently_fullscreen.contains(&f.window_id) {
                        // Leaving Cinema (or any true-fullscreen) for a normal
                        // zone/float Frame. Without this, the surface stays in
                        // xdg fullscreen (above everything); zone keys appear
                        // to "stop working" until focus leaves the window.
                        tracing::info!(
                            window_id = f.window_id,
                            app_id,
                            "Frame without fullscreen — exit compositor fullscreen"
                        );
                        state.pending.queue_exit_fullscreen(f.window_id);
                    }
                    state
                        .registry
                        .set_frame(f.window_id, f.x, f.y, f.width, f.height);
                }
            }
            sola_bus::topics::Topic::Focus(t) => {
                // If focus is moving AWAY from a window we put in
                // compositor-fullscreen state, exit fullscreen for the
                // previously focused window first. Otherwise the
                // surface keeps its "above everything" z-stack and
                // Alt-Tab gives no visual feedback, even though focus
                // technically changed (music drops, but the game is
                // still painted on top of the new focus target).
                if let Some(prev) = state.focused_window {
                    if prev != t.window_id && state.currently_fullscreen.contains(&prev) {
                        tracing::info!(
                            prev_window_id = prev,
                            new_window_id = t.window_id,
                            "focus moved away from fullscreen window — auto-exit"
                        );
                        state.pending.queue_exit_fullscreen(prev);
                    }
                }
                state
                    .pending
                    .set_focus(crate::pending::FocusAction::Window(t.window_id));
            }
            sola_bus::topics::Topic::RegisteredChords(chords) => {
                let pairs: Vec<(u32, u32)> = chords
                    .into_iter()
                    .map(|c| (c.keysym, c.modifiers))
                    .collect();
                state.pending.set_chords(pairs);
            }
            // Screenshot and synthetic input are sola-call methods now.
            sola_bus::topics::Topic::CloseApp(app_id) => {
                let to_close: Vec<u32> = state
                    .windows_by_id
                    .keys()
                    .copied()
                    .filter(|&id| {
                        state
                            .registry
                            .app_id_for(id)
                            .map(|a| a == app_id)
                            .unwrap_or(false)
                    })
                    .collect();
                tracing::info!(%app_id, count = to_close.len(), "CloseApp: queuing close");
                state.pending.queue_close(to_close);
            }
            sola_bus::topics::Topic::Shutdown => {
                info!("shutdown requested via bus");
                std::process::exit(0);
            }
            _ => {}
        }
    }

    if (state.pending.manage_dirty || state.pending.render_dirty) && state.wm.is_some() {
        tracing::debug!(
            manage_items = state.pending.manage.len(),
            render_pos = state.pending.render_positions.len(),
            composition = state
                .pending
                .composition
                .as_ref()
                .map(|c| c.len())
                .unwrap_or(0),
            "requesting manage_dirty"
        );
        state.wm.as_ref().unwrap().manage_dirty();
        if let Some(conn) = state.conn.as_ref() {
            if let Err(e) = conn.flush() {
                tracing::warn!(%e, "wayland flush failed");
            }
        }
    }
}

/// Re-publish sticky bus state after sola-bus restarts.
///
/// sola-bus keeps stickies in memory only. When the bus process is replaced
/// (e.g. `cargo make install bus shell river` mid-session), every sticky is
/// wiped. River still has live Wayland state; without re-emitting, a shell
/// that comes up (or reconnects) never sees `OutputGeometry` and cannot
/// frame the menubar — river then default-centers the 28px bar mid-screen.
fn republish_after_bus_reconnect(state: &mut AppData) {
    use sola_bus::topics::{OutputGeometry, Topic, WindowFloating};

    if let Some((width, height)) = state.output_size {
        info!(
            width,
            height, "re-emitting OutputGeometry after bus reconnect"
        );
        state
            .bus
            .emit(Topic::OutputGeometry(OutputGeometry { width, height }));
    } else {
        tracing::warn!("bus reconnected but output_size unknown; OutputGeometry not re-emitted");
    }

    // Windows list so shell can look up menubar/launcher/… by title again.
    crate::translator::emit_windows(state);

    // Live geometry for every window we already placed/sized — late
    // subscribers (and shell float restore) need the sticky map rebuilt.
    let ids: Vec<u32> = state
        .registry
        .as_windows()
        .iter()
        .map(|w| w.window_id)
        .collect();
    for window_id in ids {
        crate::translator::emit_geometry(state, window_id);
    }

    // Re-assert floating bits we still track locally. Shell is the usual
    // publisher, but after a bus wipe our local set is the authority until
    // shell re-syncs via WindowFloating on its own restart.
    let floating: Vec<u32> = state.floating.iter().copied().collect();
    for window_id in floating {
        state.bus.emit(Topic::WindowFloating(WindowFloating {
            window_id,
            floating: true,
        }));
    }
}

// ---------- Registry dispatch — the only place globals are bound ----------

impl Dispatch<wl_registry::WlRegistry, ()> for AppData {
    fn event(
        state: &mut Self,
        proxy: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        {
            match interface.as_str() {
                "river_window_manager_v1" => {
                    let wm: RiverWindowManagerV1 = proxy.bind(name, version.min(4), qh, ());
                    info!(%version, "bound river_window_manager_v1");
                    state.wm = Some(wm);
                }
                "river_xkb_bindings_v1" => {
                    let xb: RiverXkbBindingsV1 = proxy.bind(name, version.min(2), qh, ());
                    info!(%version, "bound river_xkb_bindings_v1");
                    state.xkb_bindings = Some(xb);
                }
                "wl_seat" => {
                    let s: wl_seat::WlSeat = proxy.bind(name, version.min(7), qh, ());
                    state.wl_seat = Some(s);
                }
                "wp_cursor_shape_manager_v1" => {
                    let mgr: WpCursorShapeManagerV1 = proxy.bind(name, version.min(1), qh, ());
                    info!(%version, "bound wp_cursor_shape_manager_v1");
                    state.cursor_shape_manager = Some(mgr);
                }
                "zwlr_output_manager_v1" => {
                    let mgr: ZwlrOutputManagerV1 = proxy.bind(name, version.min(4), qh, ());
                    info!(%version, "bound zwlr_output_manager_v1");
                    state.output_config.manager = Some(mgr);
                }
                "zwp_virtual_keyboard_manager_v1" => {
                    let mgr: ZwpVirtualKeyboardManagerV1 = proxy.bind(name, version.min(1), qh, ());
                    info!(%version, "bound zwp_virtual_keyboard_manager_v1");
                    state.virtual_keyboard.manager = Some(mgr);
                }
                "zwlr_virtual_pointer_manager_v1" => {
                    let mgr: ZwlrVirtualPointerManagerV1 = proxy.bind(name, version.min(2), qh, ());
                    info!(%version, "bound zwlr_virtual_pointer_manager_v1");
                    state.virtual_pointer.manager = Some(mgr);
                }
                "river_libinput_config_v1" => {
                    // Keep the proxy alive on AppData so its device events
                    // keep firing. Events drive the natural-scroll apply in
                    // client/input.rs.
                    let cfg: RiverLibinputConfigV1 = proxy.bind(name, version.min(1), qh, ());
                    info!(%version, "bound river_libinput_config_v1");
                    state.libinput_config = Some(cfg);
                }
                "zwlr_screencopy_manager_v1" => {
                    use crate::protocol::wlr_screencopy_unstable_v1::zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1;
                    let mgr: ZwlrScreencopyManagerV1 = proxy.bind(name, version.min(3), qh, ());
                    info!(%version, "bound zwlr_screencopy_manager_v1");
                    state.screenshot.manager = Some(mgr);
                }
                "wl_shm" => {
                    let shm: wl_shm::WlShm = proxy.bind(name, version.min(1), qh, ());
                    info!(%version, "bound wl_shm");
                    state.screenshot.shm = Some(shm);
                }
                "wl_compositor" => {
                    // Needed for float-shadow decoration surfaces
                    // (`get_decoration_below`).
                    let comp: wayland_client::protocol::wl_compositor::WlCompositor =
                        proxy.bind(name, version.min(4), qh, ());
                    info!(%version, "bound wl_compositor");
                    state.shadow.compositor = Some(comp);
                }
                "wl_output" => {
                    // Screencopy needs a real wl_output (river_output_v1 is a
                    // different object). V1 uses the first bound output.
                    let output: wl_output::WlOutput = proxy.bind(name, version.min(4), qh, ());
                    info!(%version, "bound wl_output for screencopy");
                    state.screenshot.outputs.push(output);
                }
                "river_layer_shell_v1" => {
                    use crate::protocol::river_layer_shell_v1::river_layer_shell_v1::RiverLayerShellV1;
                    let mgr: RiverLayerShellV1 = proxy.bind(name, version.min(1), qh, ());
                    info!(%version, "bound river_layer_shell_v1 (layer-shell clients enabled)");
                    state.layer_shell.manager = Some(mgr);
                    // Seat/outputs may already exist if the WM global was
                    // processed first — attach children now.
                    layer_shell::attach_existing(state, qh);
                }
                _ => {}
            }
        }
    }
}

// ---------- Stub dispatches for interfaces where we don't act on events ----------

impl Dispatch<wl_seat::WlSeat, ()> for AppData {
    fn event(
        _: &mut Self,
        _: &wl_seat::WlSeat,
        _: wl_seat::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_output::WlOutput, ()> for AppData {
    fn event(
        _: &mut Self,
        _: &wl_output::WlOutput,
        _: wl_output::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<RiverNodeV1, ()> for AppData {
    fn event(
        _: &mut Self,
        _: &RiverNodeV1,
        _: <RiverNodeV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<RiverOutputV1, ()> for AppData {
    fn event(
        state: &mut Self,
        _: &RiverOutputV1,
        event: <RiverOutputV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use crate::protocol::river_window_management_v1::river_output_v1::Event;
        use sola_bus::topics::{OutputGeometry, Topic};
        // River emits dimensions each time the logical output changes.
        // Forward the first one we see — the shell's zoning code keys on
        // a single output for v1.
        match event {
            Event::Dimensions { width, height, .. } => {
                info!(width, height, "river_output dimensions");
                state.output_size = Some((width, height));
                state
                    .bus
                    .emit(Topic::OutputGeometry(OutputGeometry { width, height }));
            }
            Event::Position { x, y } => {
                info!(x, y, "river_output position");
                state.output_origin = Some((x, y));
            }
            _ => {}
        }
    }
}

impl Dispatch<RiverXkbBindingsV1, ()> for AppData {
    fn event(
        _: &mut Self,
        _: &RiverXkbBindingsV1,
        _: <RiverXkbBindingsV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<RiverXkbBindingsSeatV1, ()> for AppData {
    fn event(
        _: &mut Self,
        _: &RiverXkbBindingsSeatV1,
        _: <RiverXkbBindingsSeatV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

// Shell-surface dispatch: we don't create any in v1 but the scanner may
// still require the impl to exist if the type is ever referenced.
use crate::protocol::river_window_management_v1::river_shell_surface_v1::RiverShellSurfaceV1;
impl Dispatch<RiverShellSurfaceV1, ()> for AppData {
    fn event(
        _: &mut Self,
        _: &RiverShellSurfaceV1,
        _: <RiverShellSurfaceV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

use crate::protocol::river_window_management_v1::river_decoration_v1::RiverDecorationV1;
impl Dispatch<RiverDecorationV1, ()> for AppData {
    fn event(
        _: &mut Self,
        _: &RiverDecorationV1,
        _: <RiverDecorationV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_pointer::WlPointer, ()> for AppData {
    fn event(
        _: &mut Self,
        _: &wl_pointer::WlPointer,
        _: wl_pointer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // The WM owns no surfaces, so its pointer rarely receives events; the
        // pointer object exists only to anchor the cursor-shape device.
    }
}

impl Dispatch<WpCursorShapeManagerV1, ()> for AppData {
    fn event(
        _: &mut Self,
        _: &WpCursorShapeManagerV1,
        _: <WpCursorShapeManagerV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // wp_cursor_shape_manager_v1 has no events.
    }
}

impl Dispatch<WpCursorShapeDeviceV1, ()> for AppData {
    fn event(
        _: &mut Self,
        _: &WpCursorShapeDeviceV1,
        _: <WpCursorShapeDeviceV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // wp_cursor_shape_device_v1 has no events.
    }
}
