//! Advertise `image/png` immediately; fill the pipe when a paste arrives.
//!
//! Wayland data-control is pull-based. Slack ⌘V opens an fd; we can hold
//! that fd until encode finishes (macOS promised PNG). `wl-clipboard-rs`
//! materializes bytes *before* offering, so screenshots used that crate
//! only after encode — paste had nothing to wait on.

use std::io::{self, Write};

use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use rustix::fs::{OFlags, fcntl_setfl};
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, event_created_child};
use wayland_protocols_wlr::data_control::v1::client::{
    zwlr_data_control_device_v1::{self, ZwlrDataControlDeviceV1},
    zwlr_data_control_manager_v1::ZwlrDataControlManagerV1,
    zwlr_data_control_offer_v1::ZwlrDataControlOfferV1,
    zwlr_data_control_source_v1::{self, ZwlrDataControlSourceV1},
};

use super::Error;

const MIME_PNG: &str = "image/png";
const FULFILL_TIMEOUT: Duration = Duration::from_secs(8);

enum Slot {
    Pending,
    Ready(Arc<[u8]>),
    Failed,
}

/// Clipboard source that is already advertised as `image/png`.
///
/// Call [`fulfill`](Self::fulfill) with PNG bytes (or [`fail`](Self::fail))
/// when capture/encode finishes. A paste that arrives first blocks on the
/// compositor pipe until then.
#[derive(Clone)]
pub struct PngOffer {
    slot: Arc<(Mutex<Slot>, Condvar)>,
}

impl PngOffer {
    pub fn fulfill(&self, bytes: Vec<u8>) {
        let (lock, cv) = &*self.slot;
        let mut g = lock.lock().unwrap();
        *g = Slot::Ready(Arc::from(bytes));
        cv.notify_all();
    }

    pub fn fail(&self) {
        let (lock, cv) = &*self.slot;
        let mut g = lock.lock().unwrap();
        if matches!(*g, Slot::Pending) {
            *g = Slot::Failed;
        }
        cv.notify_all();
    }
}

/// Offer `image/png` now. Returns after the compositor has the source.
pub fn offer_png() -> Result<PngOffer, Error> {
    let slot = Arc::new((Mutex::new(Slot::Pending), Condvar::new()));
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let slot_thread = slot.clone();
    thread::Builder::new()
        .name("sola-clip-png".into())
        .spawn(move || serve(slot_thread, ready_tx))
        .map_err(|e| Error::Wayland(format!("clipboard thread: {e}")))?;
    match ready_rx.recv_timeout(Duration::from_secs(2)) {
        Ok(Ok(())) => Ok(PngOffer { slot }),
        Ok(Err(e)) => Err(e),
        Err(_) => Err(Error::Wayland("clipboard offer timed out".into())),
    }
}

fn serve(slot: Arc<(Mutex<Slot>, Condvar)>, ready: mpsc::SyncSender<Result<(), Error>>) {
    if let Err(e) = serve_inner(slot, &ready) {
        let _ = ready.send(Err(e));
    }
}

fn serve_inner(
    slot: Arc<(Mutex<Slot>, Condvar)>,
    ready: &mpsc::SyncSender<Result<(), Error>>,
) -> Result<(), Error> {
    let conn = Connection::connect_to_env().map_err(|e| Error::Wayland(e.to_string()))?;
    let (globals, mut queue) = registry_queue_init::<State>(&conn)
        .map_err(|e| Error::Wayland(format!("clipboard registry: {e}")))?;
    let qh = queue.handle();

    let manager: ZwlrDataControlManagerV1 = globals
        .bind(&qh, 1..=2, ())
        .map_err(|_| Error::Wayland("wlr-data-control not available".into()))?;

    let registry = globals.registry();
    let seats: Vec<WlSeat> = globals.contents().with_list(|list| {
        list.iter()
            .filter(|g| g.interface == WlSeat::interface().name && g.version >= 2)
            .map(|g| registry.bind(g.name, 2, &qh, ()))
            .collect()
    });
    if seats.is_empty() {
        return Err(Error::Wayland("no seats".into()));
    }

    let mut state = State {
        slot,
        source: None,
        quit: false,
    };

    let devices: Vec<ZwlrDataControlDeviceV1> = seats
        .iter()
        .map(|seat| manager.get_data_device(seat, &qh, ()))
        .collect();

    let source = manager.create_data_source(&qh, ());
    source.offer(MIME_PNG.to_string());
    for device in &devices {
        device.set_selection(Some(&source));
    }
    state.source = Some(source);

    queue
        .roundtrip(&mut state)
        .map_err(|e| Error::Wayland(format!("clipboard roundtrip: {e}")))?;
    let _ = ready.send(Ok(()));

    while !state.quit {
        queue
            .blocking_dispatch(&mut state)
            .map_err(|e| Error::Wayland(format!("clipboard dispatch: {e}")))?;
    }
    Ok(())
}

fn wait_bytes(slot: &Arc<(Mutex<Slot>, Condvar)>) -> Result<Arc<[u8]>, Error> {
    let (lock, cv) = &**slot;
    let mut g = lock.lock().unwrap();
    let deadline = Instant::now() + FULFILL_TIMEOUT;
    while matches!(*g, Slot::Pending) {
        let now = Instant::now();
        if now >= deadline {
            return Err(Error::Wayland("clipboard fulfill timed out".into()));
        }
        let (gg, wait) = cv.wait_timeout(g, deadline - now).unwrap();
        g = gg;
        if wait.timed_out() && matches!(*g, Slot::Pending) {
            return Err(Error::Wayland("clipboard fulfill timed out".into()));
        }
    }
    match &*g {
        Slot::Ready(b) => Ok(Arc::clone(b)),
        Slot::Failed => Err(Error::Wayland("screenshot encode failed".into())),
        Slot::Pending => unreachable!(),
    }
}

fn write_fd(fd: std::os::fd::OwnedFd, bytes: &[u8]) -> io::Result<()> {
    fcntl_setfl(&fd, OFlags::empty()).map_err(io::Error::from)?;
    let mut file = std::fs::File::from(fd);
    file.write_all(bytes)?;
    Ok(())
}

struct State {
    slot: Arc<(Mutex<Slot>, Condvar)>,
    source: Option<ZwlrDataControlSourceV1>,
    quit: bool,
}

impl Dispatch<WlRegistry, GlobalListContents> for State {
    fn event(
        _state: &mut Self,
        _proxy: &WlRegistry,
        _event: <WlRegistry as wayland_client::Proxy>::Event,
        _data: &GlobalListContents,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlSeat, ()> for State {
    fn event(
        _state: &mut Self,
        _proxy: &WlSeat,
        _event: <WlSeat as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwlrDataControlManagerV1, ()> for State {
    fn event(
        _state: &mut Self,
        _proxy: &ZwlrDataControlManagerV1,
        _event: <ZwlrDataControlManagerV1 as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwlrDataControlOfferV1, ()> for State {
    fn event(
        _state: &mut Self,
        proxy: &ZwlrDataControlOfferV1,
        _event: <ZwlrDataControlOfferV1 as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        proxy.destroy();
    }
}

impl Dispatch<ZwlrDataControlDeviceV1, ()> for State {
    fn event(
        _state: &mut Self,
        _proxy: &ZwlrDataControlDeviceV1,
        event: zwlr_data_control_device_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let zwlr_data_control_device_v1::Event::DataOffer { id } = event {
            id.destroy();
        }
    }

    event_created_child!(State, ZwlrDataControlDeviceV1, [
        zwlr_data_control_device_v1::EVT_DATA_OFFER_OPCODE => (ZwlrDataControlOfferV1, ()),
    ]);
}

impl Dispatch<ZwlrDataControlSourceV1, ()> for State {
    fn event(
        state: &mut Self,
        proxy: &ZwlrDataControlSourceV1,
        event: zwlr_data_control_source_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_data_control_source_v1::Event::Send { mime_type, fd } => {
                if mime_type != MIME_PNG {
                    return;
                }
                match wait_bytes(&state.slot) {
                    Ok(bytes) => {
                        if let Err(e) = write_fd(fd, &bytes) {
                            tracing::warn!(%e, "clipboard send failed");
                        }
                    }
                    Err(e) => tracing::warn!(%e, "clipboard send has no png"),
                }
            }
            zwlr_data_control_source_v1::Event::Cancelled => {
                proxy.destroy();
                state.source = None;
                state.quit = true;
            }
            _ => {}
        }
    }
}
