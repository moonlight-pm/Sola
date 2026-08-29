//! Menubar Bluetooth: in-process BlueZ client + the panel model.
//!
//! See docs/specs/2026-08-29-shell-bluetooth-menubar-design.md.

mod agent;
mod bluez;
pub mod view;

use iced::Subscription;
use iced::futures::Stream;
use std::sync::{Mutex, OnceLock};

pub use bluez::{parse_managed_objects, snapshot_from_parts};

/// D-Bus object path of a BlueZ device (`/org/bluez/hci0/dev_…`).
pub type DevicePath = String;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapshot {
    pub adapter: Option<Adapter>,
    pub devices: Vec<Device>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Adapter {
    pub path: String,
    pub address: String,
    pub alias: String,
    pub powered: bool,
    pub discovering: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Device {
    pub path: DevicePath,
    pub address: String,
    pub alias: String,
    pub paired: bool,
    pub connected: bool,
    pub rssi: Option<i16>,
    pub battery_pct: Option<u8>,
}

impl Device {
    /// Battery readout for the panel. `None` when BlueZ did not expose it.
    pub fn battery_label(&self) -> Option<String> {
        self.battery_pct.map(|n| format!("{n}%"))
    }
}

impl Snapshot {
    pub fn connected(&self) -> Vec<&Device> {
        let mut v: Vec<&Device> = self.devices.iter().filter(|d| d.connected).collect();
        v.sort_by(|a, b| a.alias.to_lowercase().cmp(&b.alias.to_lowercase()));
        v
    }

    pub fn paired_idle(&self) -> Vec<&Device> {
        let mut v: Vec<&Device> = self
            .devices
            .iter()
            .filter(|d| d.paired && !d.connected)
            .collect();
        v.sort_by(|a, b| a.alias.to_lowercase().cmp(&b.alias.to_lowercase()));
        v
    }

    /// Unpaired devices (inquiry). Shown only while adding.
    pub fn nearby(&self) -> Vec<&Device> {
        let mut v: Vec<&Device> = self.devices.iter().filter(|d| !d.paired).collect();
        v.sort_by(|a, b| match (b.rssi, a.rssi) {
            (Some(br), Some(ar)) => br.cmp(&ar).then_with(|| a.alias.cmp(&b.alias)),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => a.alias.to_lowercase().cmp(&b.alias.to_lowercase()),
        });
        v
    }
}

/// Bar glyph for a snapshot. `None` → hide the chip (no adapter).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BarIcon {
    pub name: &'static str,
    pub muted: bool,
}

pub fn bar_icon(snap: &Snapshot) -> Option<BarIcon> {
    let adapter = snap.adapter.as_ref()?;
    if !adapter.powered {
        return Some(BarIcon {
            name: "lucide/bluetooth-off",
            muted: true,
        });
    }
    if snap.devices.iter().any(|d| d.connected) {
        Some(BarIcon {
            name: "lucide/bluetooth-connected",
            muted: false,
        })
    } else {
        Some(BarIcon {
            name: "lucide/bluetooth",
            muted: false,
        })
    }
}

#[derive(Clone, Debug)]
pub enum Command {
    Refresh,
    SetPowered(bool),
    SetDiscovering(bool),
    Connect(DevicePath),
    Disconnect(DevicePath),
    Pair(DevicePath),
    AgentReply { id: u64, reply: AgentReply },
}

#[derive(Clone, Debug)]
pub enum AgentReply {
    Accept,
    Reject,
    Pin(String),
    Passkey(u32),
}

#[derive(Clone, Debug)]
pub enum Event {
    Snapshot(Snapshot),
    AgentPrompt(AgentPrompt),
    AgentCleared,
    Notice(Option<String>),
    Busy(Option<DevicePath>),
}

#[derive(Clone, Debug)]
pub struct AgentPrompt {
    pub id: u64,
    pub device_path: DevicePath,
    pub device_name: String,
    pub kind: AgentKind,
}

#[derive(Clone, Debug)]
pub enum AgentKind {
    ConfirmPasskey(u32),
    RequestPin,
    RequestPasskey,
    DisplayPin(String),
    DisplayPasskey { passkey: u32, entered: u16 },
    Authorize,
}

#[derive(Clone, Debug)]
pub enum UiMsg {
    Power(bool),
    Add,
    DoneAdding,
    Disconnect(DevicePath),
    Connect(DevicePath),
    Pair(DevicePath),
    PinInput(String),
    AgentAccept,
    AgentReject,
}

#[derive(Clone, Debug, Default)]
pub struct Ui {
    pub snapshot: Snapshot,
    pub adding: bool,
    pub prompt: Option<AgentPrompt>,
    pub pin_input: String,
    pub notice: Option<String>,
    pub busy_path: Option<DevicePath>,
}

impl Ui {
    pub fn on_event(&mut self, ev: Event) {
        match ev {
            Event::Snapshot(s) => self.snapshot = s,
            Event::AgentPrompt(p) => {
                self.prompt = Some(p);
                self.pin_input.clear();
            }
            Event::AgentCleared => {
                self.prompt = None;
                self.pin_input.clear();
            }
            Event::Notice(n) => self.notice = n,
            Event::Busy(p) => self.busy_path = p,
        }
    }

    pub fn on_close(&mut self) {
        self.adding = false;
        self.prompt = None;
        self.pin_input.clear();
        self.notice = None;
        // busy_path is left: an in-flight pair/connect still completes.
    }

    /// Toggle the panel. Returns whether it is now open.
    pub fn toggle_open(&mut self, currently_open: bool) -> bool {
        if currently_open {
            self.on_close();
            false
        } else {
            true
        }
    }

    pub fn update(&mut self, msg: UiMsg) -> Option<Command> {
        match msg {
            UiMsg::Power(on) => Some(Command::SetPowered(on)),
            UiMsg::Add => {
                self.adding = true;
                self.notice = None;
                Some(Command::SetDiscovering(true))
            }
            UiMsg::DoneAdding => {
                self.adding = false;
                Some(Command::SetDiscovering(false))
            }
            UiMsg::Disconnect(path) => Some(Command::Disconnect(path)),
            UiMsg::Connect(path) => {
                self.busy_path = Some(path.clone());
                Some(Command::Connect(path))
            }
            UiMsg::Pair(path) => {
                self.busy_path = Some(path.clone());
                Some(Command::Pair(path))
            }
            UiMsg::PinInput(s) => {
                self.pin_input = s;
                None
            }
            UiMsg::AgentAccept => {
                let Some(p) = self.prompt.take() else {
                    return None;
                };
                let reply = match p.kind {
                    AgentKind::RequestPin => AgentReply::Pin(std::mem::take(&mut self.pin_input)),
                    AgentKind::RequestPasskey => {
                        let n = self.pin_input.trim().parse::<u32>().unwrap_or(0);
                        self.pin_input.clear();
                        AgentReply::Passkey(n)
                    }
                    _ => AgentReply::Accept,
                };
                Some(Command::AgentReply { id: p.id, reply })
            }
            UiMsg::AgentReject => {
                let Some(p) = self.prompt.take() else {
                    return None;
                };
                self.pin_input.clear();
                Some(Command::AgentReply {
                    id: p.id,
                    reply: AgentReply::Reject,
                })
            }
        }
    }
}

static CMD: OnceLock<Mutex<Option<tokio::sync::mpsc::UnboundedSender<Command>>>> = OnceLock::new();

fn cmd_slot() -> &'static Mutex<Option<tokio::sync::mpsc::UnboundedSender<Command>>> {
    CMD.get_or_init(|| Mutex::new(None))
}

pub fn send(cmd: Command) {
    if let Ok(g) = cmd_slot().lock() {
        if let Some(tx) = g.as_ref() {
            let _ = tx.send(cmd);
        }
    }
}

pub fn subscription() -> Subscription<Event> {
    Subscription::run(bt_stream)
}

fn bt_stream() -> impl Stream<Item = Event> {
    let (tx, rx) = iced::futures::channel::mpsc::unbounded::<Event>();
    let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<Command>();
    if let Ok(mut g) = cmd_slot().lock() {
        *g = Some(cmd_tx);
    }

    std::thread::Builder::new()
        .name("sola-bt".into())
        .spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    tracing::error!("bluetooth runtime: {e}");
                    return;
                }
            };
            rt.block_on(bluez::worker(tx, cmd_rx));
        })
        .ok();

    rx
}

#[cfg(test)]
mod tests {
    use super::*;

    fn adapter(powered: bool) -> Adapter {
        Adapter {
            path: "/org/bluez/hci0".into(),
            address: "00:11:22:33:44:55".into(),
            alias: "hci0".into(),
            powered,
            discovering: false,
        }
    }

    fn dev(alias: &str, paired: bool, connected: bool, battery: Option<u8>) -> Device {
        Device {
            path: format!("/org/bluez/hci0/dev_{alias}"),
            address: "AA:BB:CC:DD:EE:FF".into(),
            alias: alias.into(),
            paired,
            connected,
            rssi: None,
            battery_pct: battery,
        }
    }

    #[test]
    fn hide_when_no_adapter() {
        assert!(bar_icon(&Snapshot::default()).is_none());
    }

    #[test]
    fn off_icon_when_powered_off() {
        let snap = snapshot_from_parts(
            Some(adapter(false)),
            vec![dev("buds", true, true, Some(80))],
        );
        let icon = bar_icon(&snap).expect("adapter present");
        assert_eq!(icon.name, "lucide/bluetooth-off");
        assert!(icon.muted);
    }

    #[test]
    fn on_icon_without_connected() {
        let snap = snapshot_from_parts(Some(adapter(true)), vec![dev("kbd", true, false, None)]);
        let icon = bar_icon(&snap).unwrap();
        assert_eq!(icon.name, "lucide/bluetooth");
        assert!(!icon.muted);
    }

    #[test]
    fn connected_icon() {
        let snap =
            snapshot_from_parts(Some(adapter(true)), vec![dev("buds", true, true, Some(40))]);
        let icon = bar_icon(&snap).unwrap();
        assert_eq!(icon.name, "lucide/bluetooth-connected");
        assert!(!icon.muted);
    }

    #[test]
    fn battery_omitted_when_absent() {
        assert!(dev("mouse", true, true, None).battery_label().is_none());
    }

    #[test]
    fn battery_shown_when_present() {
        assert_eq!(
            dev("buds", true, true, Some(72)).battery_label().as_deref(),
            Some("72%")
        );
    }

    #[test]
    fn lists_split_connected_paired_nearby() {
        let snap = snapshot_from_parts(
            Some(adapter(true)),
            vec![
                dev("Zebra", true, true, None),
                dev("Alpha", true, false, None),
                Device {
                    path: "/org/bluez/hci0/dev_near".into(),
                    address: "11:22:33:44:55:66".into(),
                    alias: "Speaker".into(),
                    paired: false,
                    connected: false,
                    rssi: Some(-40),
                    battery_pct: None,
                },
            ],
        );
        assert_eq!(
            snap.connected()
                .iter()
                .map(|d| d.alias.as_str())
                .collect::<Vec<_>>(),
            ["Zebra"]
        );
        assert_eq!(
            snap.paired_idle()
                .iter()
                .map(|d| d.alias.as_str())
                .collect::<Vec<_>>(),
            ["Alpha"]
        );
        assert_eq!(
            snap.nearby()
                .iter()
                .map(|d| d.alias.as_str())
                .collect::<Vec<_>>(),
            ["Speaker"]
        );
    }

    #[test]
    fn panel_toggle_closes_adding() {
        let mut ui = Ui {
            adding: true,
            pin_input: "1234".into(),
            prompt: Some(AgentPrompt {
                id: 1,
                device_path: "/x".into(),
                device_name: "x".into(),
                kind: AgentKind::RequestPin,
            }),
            ..Ui::default()
        };
        assert!(!ui.toggle_open(true));
        assert!(!ui.adding);
        assert!(ui.prompt.is_none());
        assert!(ui.pin_input.is_empty());
        assert!(ui.toggle_open(false));
    }

    #[test]
    fn disconnect_command_is_not_unpair() {
        let mut ui = Ui::default();
        let cmd = ui.update(UiMsg::Disconnect("/dev".into()));
        assert!(matches!(cmd, Some(Command::Disconnect(ref p)) if p == "/dev"));
        assert!(!matches!(cmd, Some(Command::Pair(_))));
    }

    #[test]
    fn add_starts_discovery_done_stops() {
        let mut ui = Ui::default();
        assert!(matches!(
            ui.update(UiMsg::Add),
            Some(Command::SetDiscovering(true))
        ));
        assert!(ui.adding);
        assert!(matches!(
            ui.update(UiMsg::DoneAdding),
            Some(Command::SetDiscovering(false))
        ));
        assert!(!ui.adding);
    }
}
