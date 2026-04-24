//! Wayland client state and dispatch.
//!
//! A single `AppData` owns every cross-cutting state: bus handle, registries,
//! pending update, proxy caches, and the `QueueHandle`. Each interface
//! implements `Dispatch<T, _>` on `AppData`.

use std::collections::HashMap;

use tracing::info;
use wayland_client::{
    Connection, Dispatch, EventQueue, Proxy, QueueHandle,
    backend::ObjectId,
    protocol::{wl_output, wl_registry, wl_seat},
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
use crate::registry::{ChordRegistry, WindowRegistry};

pub mod binding;
pub mod input;
pub mod manage;
pub mod output_config;
pub mod seat;
pub mod virtual_keyboard;
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
    /// Windows we have already positioned at least once, either via an
    /// explicit shell Frame or our own centered default. Prevents the
    /// default from re-firing across subsequent render passes.
    pub placed: std::collections::HashSet<u32>,
    /// Mode selection state for `zwlr_output_manager_v1` — we use this
    /// protocol to pick the highest resolution ≥60Hz on startup.
    pub output_config: output_config::OutputConfigState,
    /// `zwp_virtual_keyboard_v1` state — used to synthesize Ctrl+C / Ctrl+V
    /// into non-Sola clients when the shell's Meta+C/V chords fire.
    pub virtual_keyboard: virtual_keyboard::VirtualKeyboardState,
    /// Held so its `device` events keep firing; preferences (natural
    /// scroll) are applied in `client/input.rs`.
    pub libinput_config: Option<RiverLibinputConfigV1>,
    pub qh: Option<QueueHandle<Self>>,
    /// Cloned from the wayland `Connection` so bus_tick (running on the
    /// calloop timer source) can flush outgoing wayland requests. Without
    /// this, `manage_dirty` and friends queue forever.
    pub conn: Option<Connection>,
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
            placed: std::collections::HashSet::new(),
            output_config: output_config::OutputConfigState::default(),
            virtual_keyboard: virtual_keyboard::VirtualKeyboardState::default(),
            libinput_config: None,
            qh: None,
            conn: None,
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

    Ok((conn, queue, data))
}

/// Called on every bus tick. Drains bus messages, folds them into `pending`,
/// and if pending is dirty, asks River to start a new manage cycle via
/// `manage_dirty`.
pub fn bus_tick(state: &mut AppData) {
    state.bus.ensure_connected();
    state.bus.drain_notify();
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
            sola_bus::topics::Topic::Frame(f) => {
                tracing::debug!(
                    window_id = f.window_id,
                    w = f.width,
                    h = f.height,
                    "got Frame"
                );
                state
                    .pending
                    .frame(f.window_id, f.x, f.y, f.width, f.height);
            }
            sola_bus::topics::Topic::Focus(t) => {
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
            sola_bus::topics::Topic::Copy(req) => {
                dispatch_clipboard_chord(state, req.window_id, ClipboardAction::Copy);
            }
            sola_bus::topics::Topic::Paste(req) => {
                dispatch_clipboard_chord(state, req.window_id, ClipboardAction::Paste);
            }
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

/// Which clipboard chord fired. Determines the evdev keycode synthesized.
#[derive(Clone, Copy)]
enum ClipboardAction {
    Copy,
    Paste,
}

impl ClipboardAction {
    /// Raw Linux evdev keycode from `include/uapi/linux/input-event-codes.h`.
    fn evdev_keycode(self) -> u32 {
        match self {
            ClipboardAction::Copy => 46,  // KEY_C
            ClipboardAction::Paste => 47, // KEY_V
        }
    }
}

/// Handle `Topic::Copy` / `Topic::Paste` from the shell. The message is
/// broadcast, not addressed to a specific subscriber — sola-app handles
/// it for Sola windows; we handle it for foreign (non-sola) windows by
/// synthesizing Ctrl+C / Ctrl+V at the Wayland seat via the virtual
/// keyboard. If the window isn't in our registry at all (e.g. the shell
/// was slightly faster than River's `window` event) we drop silently.
fn dispatch_clipboard_chord(state: &mut AppData, window_id: u32, action: ClipboardAction) {
    let Some(app_id) = state.registry.app_id_for(window_id).map(|s| s.to_string()) else {
        tracing::debug!(window_id, "clipboard chord for unknown window, ignoring");
        return;
    };
    // Sola apps handle Copy/Paste themselves via the sola-app framework.
    // We only synthesize for foreign clients.
    if app_id.starts_with("sola-") {
        return;
    }
    tracing::info!(
        window_id,
        app_id = %app_id,
        action = ?(match action { ClipboardAction::Copy => "copy", ClipboardAction::Paste => "paste" }),
        "synthesizing Ctrl+key for non-sola client"
    );
    virtual_keyboard::synthesize_ctrl_plus_evdev_key(state, action.evdev_keycode());
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
                "river_libinput_config_v1" => {
                    // Keep the proxy alive on AppData so its device events
                    // keep firing. Events drive the natural-scroll apply in
                    // client/input.rs.
                    let cfg: RiverLibinputConfigV1 = proxy.bind(name, version.min(1), qh, ());
                    info!(%version, "bound river_libinput_config_v1");
                    state.libinput_config = Some(cfg);
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
        if let Event::Dimensions { width, height, .. } = event {
            info!(width, height, "river_output dimensions");
            state.output_size = Some((width, height));
            state
                .bus
                .emit(Topic::OutputGeometry(OutputGeometry { width, height }));
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

use crate::protocol::river_window_management_v1::river_pointer_binding_v1::RiverPointerBindingV1;
impl Dispatch<RiverPointerBindingV1, ()> for AppData {
    fn event(
        _: &mut Self,
        _: &RiverPointerBindingV1,
        _: <RiverPointerBindingV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
