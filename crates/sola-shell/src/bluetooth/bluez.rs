//! zbus talk to `org.bluez`. Parsing is unit-tested; the worker never
//! runs on the iced thread.

use std::collections::HashMap;
use std::time::Duration;

use iced::futures::StreamExt;
use zbus::fdo::ManagedObjects;
use zbus::zvariant::{ObjectPath, OwnedValue, Value};

use super::agent::{AGENT_PATH, Agent, AgentInner};
use super::{Adapter, Command, Device, Event, Snapshot, looks_like_address};

const POLL: Duration = Duration::from_secs(4);
const RETRY: Duration = Duration::from_secs(5);

#[zbus::proxy(interface = "org.bluez.Adapter1", default_service = "org.bluez")]
trait Adapter1 {
    fn start_discovery(&self) -> zbus::Result<()>;
    fn stop_discovery(&self) -> zbus::Result<()>;
    fn set_discovery_filter(&self, properties: HashMap<&str, Value<'_>>) -> zbus::Result<()>;

    #[zbus(property)]
    fn powered(&self) -> zbus::Result<bool>;
    #[zbus(property)]
    fn set_powered(&self, value: bool) -> zbus::Result<()>;
    #[zbus(property)]
    fn set_pairable(&self, value: bool) -> zbus::Result<()>;
}

#[zbus::proxy(interface = "org.bluez.Device1", default_service = "org.bluez")]
trait Device1 {
    fn connect(&self) -> zbus::Result<()>;
    fn disconnect(&self) -> zbus::Result<()>;
    fn pair(&self) -> zbus::Result<()>;
}

#[zbus::proxy(
    interface = "org.bluez.AgentManager1",
    default_service = "org.bluez",
    default_path = "/org/bluez"
)]
trait AgentManager1 {
    fn register_agent(&self, path: ObjectPath<'_>, capability: &str) -> zbus::Result<()>;
    fn unregister_agent(&self, path: ObjectPath<'_>) -> zbus::Result<()>;
    fn request_default_agent(&self, path: ObjectPath<'_>) -> zbus::Result<()>;
}

pub fn snapshot_from_parts(adapter: Option<Adapter>, mut devices: Vec<Device>) -> Snapshot {
    devices.sort_by(|a, b| a.path.cmp(&b.path));
    Snapshot { adapter, devices }
}

pub fn parse_managed_objects(objects: &ManagedObjects) -> Snapshot {
    let mut adapter: Option<Adapter> = None;
    let mut devices: HashMap<String, Device> = HashMap::new();
    let mut batteries: HashMap<String, u8> = HashMap::new();

    for (path, ifaces) in objects {
        let path_s = path.as_str().to_string();
        for (iface, props) in ifaces {
            match iface.as_str() {
                "org.bluez.Adapter1" => {
                    if adapter.is_none() {
                        adapter = Some(Adapter {
                            path: path_s.clone(),
                            address: prop_str(props, "Address").unwrap_or_default(),
                            alias: prop_str(props, "Alias")
                                .or_else(|| prop_str(props, "Name"))
                                .unwrap_or_else(|| path_s.clone()),
                            powered: prop_bool(props, "Powered").unwrap_or(false),
                            discovering: prop_bool(props, "Discovering").unwrap_or(false),
                        });
                    }
                }
                "org.bluez.Device1" => {
                    let address = prop_str(props, "Address").unwrap_or_default();
                    let name =
                        prop_str(props, "Name").filter(|s| !s.is_empty() && !looks_like_address(s));
                    let alias = prop_str(props, "Alias")
                        .filter(|s| !s.is_empty())
                        .or_else(|| name.clone())
                        .unwrap_or_else(|| address.clone());
                    devices.insert(
                        path_s.clone(),
                        Device {
                            path: path_s.clone(),
                            address,
                            alias,
                            name,
                            address_type: prop_str(props, "AddressType"),
                            icon: prop_str(props, "Icon"),
                            appearance: prop_u16(props, "Appearance"),
                            class: prop_u32(props, "Class"),
                            uuids: prop_str_list(props, "UUIDs"),
                            paired: prop_bool(props, "Paired").unwrap_or(false),
                            connected: prop_bool(props, "Connected").unwrap_or(false),
                            rssi: prop_i16(props, "RSSI"),
                            battery_pct: None,
                        },
                    );
                }
                "org.bluez.Battery1" => {
                    if let Some(pct) = prop_u8(props, "Percentage") {
                        batteries.insert(path_s.clone(), pct);
                    }
                }
                _ => {}
            }
        }
    }

    for (path, pct) in batteries {
        if let Some(d) = devices.get_mut(&path) {
            d.battery_pct = Some(pct);
        }
    }

    let mut list: Vec<Device> = devices.into_values().collect();
    list.sort_by(|a, b| a.alias.to_lowercase().cmp(&b.alias.to_lowercase()));
    Snapshot {
        adapter,
        devices: list,
    }
}

fn prop_bool(props: &HashMap<String, OwnedValue>, key: &str) -> Option<bool> {
    props.get(key).and_then(as_bool)
}

fn prop_str(props: &HashMap<String, OwnedValue>, key: &str) -> Option<String> {
    props.get(key).and_then(as_str)
}

fn prop_u8(props: &HashMap<String, OwnedValue>, key: &str) -> Option<u8> {
    props.get(key).and_then(as_u8)
}

fn prop_i16(props: &HashMap<String, OwnedValue>, key: &str) -> Option<i16> {
    props.get(key).and_then(as_i16)
}

fn prop_u16(props: &HashMap<String, OwnedValue>, key: &str) -> Option<u16> {
    props.get(key).and_then(as_u16)
}

fn prop_u32(props: &HashMap<String, OwnedValue>, key: &str) -> Option<u32> {
    props.get(key).and_then(as_u32)
}

fn prop_str_list(props: &HashMap<String, OwnedValue>, key: &str) -> Vec<String> {
    props.get(key).map(as_str_list).unwrap_or_default()
}

fn as_bool(v: &OwnedValue) -> Option<bool> {
    match &**v {
        Value::Bool(b) => Some(*b),
        _ => None,
    }
}

fn as_str(v: &OwnedValue) -> Option<String> {
    match &**v {
        Value::Str(s) => Some(s.to_string()),
        _ => None,
    }
}

fn as_u8(v: &OwnedValue) -> Option<u8> {
    match &**v {
        Value::U8(n) => Some(*n),
        Value::U16(n) => u8::try_from(*n).ok(),
        Value::U32(n) => u8::try_from(*n).ok(),
        _ => None,
    }
}

fn as_i16(v: &OwnedValue) -> Option<i16> {
    match &**v {
        Value::I16(n) => Some(*n),
        Value::I32(n) => i16::try_from(*n).ok(),
        Value::I64(n) => i16::try_from(*n).ok(),
        _ => None,
    }
}

fn as_u16(v: &OwnedValue) -> Option<u16> {
    match &**v {
        Value::U16(n) => Some(*n),
        Value::U32(n) => u16::try_from(*n).ok(),
        Value::U8(n) => Some(u16::from(*n)),
        _ => None,
    }
}

fn as_u32(v: &OwnedValue) -> Option<u32> {
    match &**v {
        Value::U32(n) => Some(*n),
        Value::U16(n) => Some(u32::from(*n)),
        Value::U8(n) => Some(u32::from(*n)),
        _ => None,
    }
}

fn as_str_list(v: &OwnedValue) -> Vec<String> {
    match &**v {
        Value::Array(arr) => arr
            .inner()
            .iter()
            .filter_map(|item| match item {
                Value::Str(s) => Some(s.to_string()),
                _ => None,
            })
            .collect(),
        Value::Str(s) => vec![s.to_string()],
        _ => Vec::new(),
    }
}

pub async fn worker(
    event_tx: iced::futures::channel::mpsc::UnboundedSender<Event>,
    mut cmd_rx: tokio::sync::mpsc::UnboundedReceiver<Command>,
) {
    loop {
        match session(&event_tx, &mut cmd_rx).await {
            SessionEnd::Shutdown => return,
            SessionEnd::Disconnected => {
                let _ = event_tx.unbounded_send(Event::Snapshot(Snapshot::default()));
                tokio::time::sleep(RETRY).await;
            }
        }
    }
}

enum SessionEnd {
    Shutdown,
    Disconnected,
}

async fn session(
    event_tx: &iced::futures::channel::mpsc::UnboundedSender<Event>,
    cmd_rx: &mut tokio::sync::mpsc::UnboundedReceiver<Command>,
) -> SessionEnd {
    let inner = AgentInner::new(event_tx.clone());
    let agent = Agent {
        inner: inner.clone(),
    };

    let conn = match zbus::connection::Builder::system() {
        Ok(b) => match b.serve_at(AGENT_PATH, agent) {
            Ok(b) => match b.build().await {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!("bluetooth system bus: {e}");
                    return SessionEnd::Disconnected;
                }
            },
            Err(e) => {
                tracing::warn!("bluetooth agent export: {e}");
                return SessionEnd::Disconnected;
            }
        },
        Err(e) => {
            tracing::warn!("bluetooth system bus builder: {e}");
            return SessionEnd::Disconnected;
        }
    };

    let om = match zbus::fdo::ObjectManagerProxy::builder(&conn)
        .destination("org.bluez")
        .and_then(|b| b.path("/"))
    {
        Ok(b) => match b.build().await {
            Ok(p) => p,
            Err(e) => {
                tracing::debug!("bluetooth ObjectManager: {e}");
                return SessionEnd::Disconnected;
            }
        },
        Err(e) => {
            tracing::debug!("bluetooth ObjectManager builder: {e}");
            return SessionEnd::Disconnected;
        }
    };

    register_agent(&conn).await;

    let mut added = om.receive_interfaces_added().await.ok();
    let mut removed = om.receive_interfaces_removed().await.ok();

    let mut discovering = false;
    push_snapshot(&om, event_tx, &inner).await;

    loop {
        tokio::select! {
            cmd = cmd_rx.recv() => {
                let Some(cmd) = cmd else {
                    return SessionEnd::Shutdown;
                };
                match handle_cmd(cmd, &conn, &om, event_tx, &inner, &mut discovering).await {
                    CmdResult::Ok => {}
                    CmdResult::Gone => return SessionEnd::Disconnected,
                }
            }
            _ = tokio::time::sleep(if discovering {
                Duration::from_secs(1)
            } else {
                POLL
            }) => {
                push_snapshot(&om, event_tx, &inner).await;
            }
            added_ev = async {
                match added.as_mut() {
                    Some(s) => s.next().await,
                    None => std::future::pending().await,
                }
            } => {
                if added_ev.is_none() {
                    added = None;
                } else {
                    push_snapshot(&om, event_tx, &inner).await;
                }
            }
            removed_ev = async {
                match removed.as_mut() {
                    Some(s) => s.next().await,
                    None => std::future::pending().await,
                }
            } => {
                if removed_ev.is_none() {
                    removed = None;
                } else {
                    push_snapshot(&om, event_tx, &inner).await;
                }
            }
        }
    }
}

enum CmdResult {
    Ok,
    Gone,
}

async fn handle_cmd(
    cmd: Command,
    conn: &zbus::Connection,
    om: &zbus::fdo::ObjectManagerProxy<'_>,
    event_tx: &iced::futures::channel::mpsc::UnboundedSender<Event>,
    inner: &std::sync::Arc<AgentInner>,
    discovering: &mut bool,
) -> CmdResult {
    match cmd {
        Command::Refresh => {
            push_snapshot(om, event_tx, inner).await;
        }
        Command::SetPowered(on) => {
            if !on && *discovering {
                let _ = stop_discovery(conn, om).await;
                *discovering = false;
            }
            match adapter_proxy(conn, om).await {
                Ok(Some(a)) => {
                    if let Err(e) = a.set_powered(on).await {
                        tracing::warn!("bluetooth set powered: {e}");
                        let _ = event_tx.unbounded_send(Event::Notice(Some(
                            "Couldn't change Bluetooth power.".into(),
                        )));
                    }
                }
                Ok(None) => {
                    let _ = event_tx.unbounded_send(Event::Snapshot(Snapshot::default()));
                    return CmdResult::Gone;
                }
                Err(e) => {
                    tracing::debug!("bluetooth adapter: {e}");
                    return CmdResult::Gone;
                }
            }
            push_snapshot(om, event_tx, inner).await;
        }
        Command::SetDiscovering(on) => {
            if on == *discovering {
                if on {
                    push_snapshot(om, event_tx, inner).await;
                }
                return CmdResult::Ok;
            }
            if on {
                match start_discovery(conn, om).await {
                    Ok(()) => *discovering = true,
                    Err(e) => {
                        tracing::warn!("bluetooth start discovery: {e}");
                        let _ = event_tx.unbounded_send(Event::Notice(Some(
                            "Couldn't search for devices.".into(),
                        )));
                    }
                }
            } else {
                let _ = stop_discovery(conn, om).await;
                *discovering = false;
            }
            push_snapshot(om, event_tx, inner).await;
        }
        Command::Disconnect(path) => {
            match device_proxy(conn, &path).await {
                Ok(d) => {
                    if let Err(e) = d.disconnect().await {
                        tracing::warn!(%path, "bluetooth disconnect: {e}");
                        let _ = event_tx
                            .unbounded_send(Event::Notice(Some("Couldn't disconnect.".into())));
                    }
                }
                Err(e) => tracing::debug!(%path, "bluetooth device: {e}"),
            }
            let _ = event_tx.unbounded_send(Event::Busy(None));
            push_snapshot(om, event_tx, inner).await;
        }
        Command::Connect(path) => {
            let conn = conn.clone();
            let event_tx = event_tx.clone();
            let inner = inner.clone();
            tokio::spawn(async move {
                match device_proxy(&conn, &path).await {
                    Ok(d) => {
                        if let Err(e) = d.connect().await {
                            tracing::warn!(%path, "bluetooth connect: {e}");
                            let _ = event_tx
                                .unbounded_send(Event::Notice(Some("Couldn't connect.".into())));
                        } else {
                            let _ = event_tx.unbounded_send(Event::Notice(None));
                        }
                    }
                    Err(e) => tracing::debug!(%path, "bluetooth device: {e}"),
                }
                let _ = event_tx.unbounded_send(Event::Busy(None));
                if let Ok(om) = object_manager(&conn).await {
                    push_snapshot(&om, &event_tx, &inner).await;
                }
            });
        }
        Command::Pair(path) => {
            let conn = conn.clone();
            let event_tx = event_tx.clone();
            let inner = inner.clone();
            tokio::spawn(async move {
                match device_proxy(&conn, &path).await {
                    Ok(d) => {
                        if let Err(e) = d.pair().await {
                            // Already paired is not fatal — still try Connect.
                            tracing::debug!(%path, "bluetooth pair: {e}");
                        }
                        if let Err(e) = d.connect().await {
                            tracing::warn!(%path, "bluetooth connect after pair: {e}");
                            let _ = event_tx
                                .unbounded_send(Event::Notice(Some("Couldn't connect.".into())));
                        } else {
                            let _ = event_tx.unbounded_send(Event::Notice(None));
                        }
                    }
                    Err(e) => tracing::debug!(%path, "bluetooth device: {e}"),
                }
                let _ = event_tx.unbounded_send(Event::Busy(None));
                if let Ok(om) = object_manager(&conn).await {
                    push_snapshot(&om, &event_tx, &inner).await;
                }
            });
        }
        Command::AgentReply { id, reply } => {
            inner.complete(id, reply).await;
        }
    }
    CmdResult::Ok
}

async fn register_agent(conn: &zbus::Connection) {
    let mgr = match AgentManager1Proxy::new(conn).await {
        Ok(m) => m,
        Err(e) => {
            tracing::debug!("bluetooth AgentManager: {e}");
            return;
        }
    };
    let path = match ObjectPath::try_from(AGENT_PATH) {
        Ok(p) => p,
        Err(_) => return,
    };
    if let Err(e) = mgr.register_agent(path.clone(), "KeyboardDisplay").await {
        tracing::debug!("bluetooth RegisterAgent: {e}");
        return;
    }
    if let Err(e) = mgr.request_default_agent(path).await {
        tracing::debug!("bluetooth RequestDefaultAgent: {e}");
    } else {
        tracing::info!("bluetooth agent registered");
    }
}

async fn push_snapshot(
    om: &zbus::fdo::ObjectManagerProxy<'_>,
    event_tx: &iced::futures::channel::mpsc::UnboundedSender<Event>,
    inner: &AgentInner,
) {
    match om.get_managed_objects().await {
        Ok(objs) => {
            let snap = parse_managed_objects(&objs);
            if let Ok(mut names) = inner.names.lock() {
                names.clear();
                for d in &snap.devices {
                    names.insert(d.path.clone(), d.alias.clone());
                }
            }
            let _ = event_tx.unbounded_send(Event::Snapshot(snap));
        }
        Err(e) => {
            tracing::debug!("bluetooth GetManagedObjects: {e}");
            let _ = event_tx.unbounded_send(Event::Snapshot(Snapshot::default()));
        }
    }
}

async fn object_manager(
    conn: &zbus::Connection,
) -> zbus::Result<zbus::fdo::ObjectManagerProxy<'static>> {
    zbus::fdo::ObjectManagerProxy::builder(conn)
        .destination("org.bluez")?
        .path("/")?
        .build()
        .await
}

async fn adapter_proxy(
    conn: &zbus::Connection,
    om: &zbus::fdo::ObjectManagerProxy<'_>,
) -> zbus::Result<Option<Adapter1Proxy<'static>>> {
    let objs = om.get_managed_objects().await?;
    let snap = parse_managed_objects(&objs);
    let Some(adapter) = snap.adapter else {
        return Ok(None);
    };
    let path = ObjectPath::try_from(adapter.path)?;
    let proxy = Adapter1Proxy::builder(conn).path(path)?.build().await?;
    Ok(Some(proxy))
}

async fn device_proxy(conn: &zbus::Connection, path: &str) -> zbus::Result<Device1Proxy<'static>> {
    let path = ObjectPath::try_from(path.to_string())?;
    Device1Proxy::builder(conn).path(path)?.build().await
}

async fn start_discovery(
    conn: &zbus::Connection,
    om: &zbus::fdo::ObjectManagerProxy<'_>,
) -> zbus::Result<()> {
    let Some(a) = adapter_proxy(conn, om).await? else {
        return Err(zbus::Error::Failure("no adapter".into()));
    };
    let mut filter: HashMap<&str, Value<'_>> = HashMap::new();
    filter.insert("DuplicateData", Value::Bool(false));
    filter.insert("Transport", Value::from("auto"));
    // Drop distant BLE beacons; a pairing device on the desk is louder.
    filter.insert("RSSI", Value::I16(-80));
    let _ = a.set_discovery_filter(filter).await;
    let _ = a.set_pairable(true).await;
    match a.start_discovery().await {
        Ok(()) => Ok(()),
        Err(e) if already_discovering(&e) => Ok(()),
        Err(e) => Err(e),
    }
}

async fn stop_discovery(
    conn: &zbus::Connection,
    om: &zbus::fdo::ObjectManagerProxy<'_>,
) -> zbus::Result<()> {
    let Some(a) = adapter_proxy(conn, om).await? else {
        return Ok(());
    };
    let _ = a.set_pairable(false).await;
    match a.stop_discovery().await {
        Ok(()) => Ok(()),
        Err(e) if not_discovering(&e) => Ok(()),
        Err(e) => Err(e),
    }
}

fn already_discovering(e: &zbus::Error) -> bool {
    let s = e.to_string();
    s.contains("InProgress") || s.contains("Already")
}

fn not_discovering(e: &zbus::Error) -> bool {
    let s = e.to_string();
    s.contains("NotReady") || s.contains("Failed") || s.contains("InProgress")
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbus::names::OwnedInterfaceName;
    use zbus::zvariant::{OwnedObjectPath, Str};

    fn ov_bool(v: bool) -> OwnedValue {
        Value::from(v).try_to_owned().expect("bool")
    }
    fn ov_str(s: &str) -> OwnedValue {
        Value::from(Str::from(s)).try_to_owned().expect("str")
    }
    fn ov_u8(n: u8) -> OwnedValue {
        Value::from(n).try_to_owned().expect("u8")
    }
    fn ov_i16(n: i16) -> OwnedValue {
        Value::from(n).try_to_owned().expect("i16")
    }

    fn path(s: &str) -> OwnedObjectPath {
        OwnedObjectPath::try_from(s).expect("path")
    }
    fn iface(s: &str) -> OwnedInterfaceName {
        OwnedInterfaceName::try_from(s).expect("iface")
    }

    #[test]
    fn parse_adapter_devices_and_optional_battery() {
        let mut objects: ManagedObjects = HashMap::new();

        let mut adapter_props = HashMap::new();
        adapter_props.insert("Address".into(), ov_str("00:11:22:33:44:55"));
        adapter_props.insert("Alias".into(), ov_str("Host"));
        adapter_props.insert("Powered".into(), ov_bool(true));
        adapter_props.insert("Discovering".into(), ov_bool(false));
        let mut adapter_ifaces = HashMap::new();
        adapter_ifaces.insert(iface("org.bluez.Adapter1"), adapter_props);
        objects.insert(path("/org/bluez/hci0"), adapter_ifaces);

        let mut dev_props = HashMap::new();
        dev_props.insert("Address".into(), ov_str("AA:BB:CC:DD:EE:FF"));
        dev_props.insert("Alias".into(), ov_str("AirPods Pro"));
        dev_props.insert("Paired".into(), ov_bool(true));
        dev_props.insert("Connected".into(), ov_bool(true));
        let mut bat = HashMap::new();
        bat.insert("Percentage".into(), ov_u8(72));
        let mut dev_ifaces = HashMap::new();
        dev_ifaces.insert(iface("org.bluez.Device1"), dev_props);
        dev_ifaces.insert(iface("org.bluez.Battery1"), bat);
        objects.insert(path("/org/bluez/hci0/dev_AA_BB_CC_DD_EE_FF"), dev_ifaces);

        let mut idle_props = HashMap::new();
        idle_props.insert("Address".into(), ov_str("11:22:33:44:55:66"));
        idle_props.insert("Name".into(), ov_str("Keychron"));
        idle_props.insert("Paired".into(), ov_bool(true));
        idle_props.insert("Connected".into(), ov_bool(false));
        let mut idle_ifaces = HashMap::new();
        idle_ifaces.insert(iface("org.bluez.Device1"), idle_props);
        objects.insert(path("/org/bluez/hci0/dev_11_22_33_44_55_66"), idle_ifaces);

        let mut near_props = HashMap::new();
        near_props.insert("Address".into(), ov_str("DE:AD:BE:EF:00:01"));
        near_props.insert("Alias".into(), ov_str("Speaker"));
        near_props.insert("Paired".into(), ov_bool(false));
        near_props.insert("Connected".into(), ov_bool(false));
        near_props.insert("RSSI".into(), ov_i16(-42));
        let mut near_ifaces = HashMap::new();
        near_ifaces.insert(iface("org.bluez.Device1"), near_props);
        objects.insert(path("/org/bluez/hci0/dev_DE_AD_BE_EF_00_01"), near_ifaces);

        let snap = parse_managed_objects(&objects);
        let ad = snap.adapter.expect("adapter");
        assert!(ad.powered);
        assert_eq!(ad.alias, "Host");

        let air = snap
            .devices
            .iter()
            .find(|d| d.alias == "AirPods Pro")
            .expect("airpods");
        assert!(air.connected && air.paired);
        assert_eq!(air.battery_pct, Some(72));

        let key = snap
            .devices
            .iter()
            .find(|d| d.alias == "Keychron")
            .expect("keychron");
        assert!(key.paired && !key.connected);
        assert_eq!(key.battery_pct, None);

        let spk = snap
            .devices
            .iter()
            .find(|d| d.alias == "Speaker")
            .expect("speaker");
        assert!(!spk.paired);
        assert_eq!(spk.rssi, Some(-42));
    }

    #[test]
    fn parse_empty_is_no_adapter() {
        let objects: ManagedObjects = HashMap::new();
        let snap = parse_managed_objects(&objects);
        assert!(snap.adapter.is_none());
        assert!(snap.devices.is_empty());
    }
}
