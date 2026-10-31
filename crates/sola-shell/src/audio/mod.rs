//! Menubar volume: PipeWire graph + WirePlumber `wpctl`.
//! See docs/specs/2026-08-29-shell-audio-menubar-design.md.

mod meter;
mod pw;
pub mod view;
pub mod wave;

use iced::Subscription;
use iced::futures::Stream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Output,
    Input,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Device {
    pub id: u32,
    pub name: String,
    pub kind: Kind,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapshot {
    pub available: bool,
    pub sinks: Vec<Device>,
    pub sources: Vec<Device>,
    pub default_sink: Option<u32>,
    pub default_source: Option<u32>,
    pub sink_volume: f32,
    pub sink_mute: bool,
    pub source_volume: f32,
    pub source_mute: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BarIcon {
    pub name: &'static str,
    pub muted: bool,
}

pub fn bar_icon(snap: &Snapshot) -> Option<BarIcon> {
    if !snap.available {
        return None;
    }
    if snap.sink_mute || snap.sink_volume <= 0.001 {
        return Some(BarIcon {
            name: "lucide/volume-x",
            muted: true,
        });
    }
    let name = if snap.sink_volume < 0.34 {
        "lucide/volume"
    } else if snap.sink_volume < 0.67 {
        "lucide/volume-1"
    } else {
        "lucide/volume-2"
    };
    Some(BarIcon { name, muted: false })
}

#[derive(Clone, Debug)]
pub enum Command {
    Refresh,
    SetSinkVolume(f32),
    SetSourceVolume(f32),
    SetSinkMute(bool),
    SetSourceMute(bool),
    SetDefault(u32),
}

#[derive(Clone, Debug)]
pub enum Event {
    Snapshot(Snapshot),
    /// Meter went live — one present so the spectrum canvas can start its
    /// `RedrawRequest::At` loop. Not a 16 ms iced timer.
    Kick,
}

#[derive(Clone, Debug)]
pub enum UiMsg {
    OutputVolume(f32),
    InputVolume(f32),
    ToggleOutputMute,
    ToggleInputMute,
    SetDefaultSink(u32),
    SetDefaultSource(u32),
}

#[derive(Clone, Debug, Default)]
pub struct Ui {
    pub snapshot: Snapshot,
}

impl Ui {
    pub fn on_event(&mut self, ev: Event) {
        match ev {
            Event::Snapshot(s) => self.snapshot = s,
            Event::Kick => {}
        }
    }

    pub fn update(&mut self, msg: UiMsg) -> Option<Command> {
        match msg {
            UiMsg::OutputVolume(v) => {
                let v = (v / 100.0).clamp(0.0, 1.0);
                self.snapshot.sink_volume = v;
                Some(Command::SetSinkVolume(v))
            }
            UiMsg::InputVolume(v) => {
                let v = (v / 100.0).clamp(0.0, 1.0);
                self.snapshot.source_volume = v;
                Some(Command::SetSourceVolume(v))
            }
            UiMsg::ToggleOutputMute => {
                let next = !self.snapshot.sink_mute;
                self.snapshot.sink_mute = next;
                Some(Command::SetSinkMute(next))
            }
            UiMsg::ToggleInputMute => {
                let next = !self.snapshot.source_mute;
                self.snapshot.source_mute = next;
                Some(Command::SetSourceMute(next))
            }
            UiMsg::SetDefaultSink(id) => {
                self.snapshot.default_sink = Some(id);
                Some(Command::SetDefault(id))
            }
            UiMsg::SetDefaultSource(id) => {
                self.snapshot.default_source = Some(id);
                Some(Command::SetDefault(id))
            }
        }
    }
}

static CMD: OnceLock<Mutex<Option<std::sync::mpsc::Sender<Command>>>> = OnceLock::new();

fn cmd_slot() -> &'static Mutex<Option<std::sync::mpsc::Sender<Command>>> {
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
    Subscription::run(audio_stream)
}

fn audio_stream() -> impl Stream<Item = Event> {
    let (tx, rx) = iced::futures::channel::mpsc::unbounded::<Event>();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<Command>();
    if let Ok(mut g) = cmd_slot().lock() {
        *g = Some(cmd_tx);
    }
    std::thread::Builder::new()
        .name("sola-audio".into())
        .spawn({
            let tx = tx.clone();
            move || worker(tx, cmd_rx)
        })
        .ok();
    meter::spawn(tx);
    rx
}

fn worker(
    event_tx: iced::futures::channel::mpsc::UnboundedSender<Event>,
    cmd_rx: std::sync::mpsc::Receiver<Command>,
) {
    let last = Arc::new(Mutex::new(Snapshot::default()));
    refresh(&event_tx, &last);

    let stop = Arc::new(AtomicBool::new(false));
    std::thread::Builder::new()
        .name("sola-audio-poll".into())
        .spawn({
            let event_tx = event_tx.clone();
            let last = last.clone();
            let stop = stop.clone();
            move || {
                while !stop.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_secs(1));
                    if stop.load(Ordering::Relaxed) {
                        break;
                    }
                    refresh(&event_tx, &last);
                }
            }
        })
        .ok();

    while let Ok(cmd) = cmd_rx.recv() {
        apply(cmd, &last);
        while let Ok(more) = cmd_rx.try_recv() {
            apply(more, &last);
        }
        let g = last.lock().unwrap_or_else(|e| e.into_inner());
        publish(&event_tx, &g);
    }
    stop.store(true, Ordering::Relaxed);
}

fn apply(cmd: Command, last: &Mutex<Snapshot>) {
    match cmd {
        Command::Refresh => {}
        Command::SetSinkVolume(v) => {
            let id = lock_snap(last).default_sink;
            if let Some(id) = id {
                if pw::set_volume(id, v) {
                    lock_snap(last).sink_volume = v;
                }
            }
        }
        Command::SetSourceVolume(v) => {
            let id = lock_snap(last).default_source;
            if let Some(id) = id {
                if pw::set_volume(id, v) {
                    lock_snap(last).source_volume = v;
                }
            }
        }
        Command::SetSinkMute(m) => {
            let id = lock_snap(last).default_sink;
            if let Some(id) = id {
                if pw::set_mute(id, m) {
                    lock_snap(last).sink_mute = m;
                }
            }
        }
        Command::SetSourceMute(m) => {
            let id = lock_snap(last).default_source;
            if let Some(id) = id {
                if pw::set_mute(id, m) {
                    lock_snap(last).source_mute = m;
                }
            }
        }
        Command::SetDefault(id) => {
            if !pw::set_default(id) {
                return;
            }
            let mut g = lock_snap(last);
            if g.sinks.iter().any(|d| d.id == id) {
                g.default_sink = Some(id);
            } else if g.sources.iter().any(|d| d.id == id) {
                g.default_source = Some(id);
            }
        }
    }
}

fn refresh(
    event_tx: &iced::futures::channel::mpsc::UnboundedSender<Event>,
    last: &Mutex<Snapshot>,
) {
    match pw::snapshot() {
        Ok(snap) => {
            let mut g = lock_snap(last);
            merge_snapshot(&mut g, snap);
            publish(event_tx, &g);
        }
        Err(e) => {
            tracing::warn!("audio snapshot: {e}");
            let mut g = lock_snap(last);
            if pw::pipewire_socket_present() {
                g.available = true;
                publish(event_tx, &g);
            } else if !g.available {
                *g = Snapshot::default();
                publish(event_tx, &g);
            }
        }
    }
}

/// Keep last defaults/volumes when `wpctl` did not answer, and keep last
/// endpoints when this poll's graph was empty (partial `pw-cli` miss).
fn merge_snapshot(last: &mut Snapshot, new: Snapshot) {
    let keep_sink = new.default_sink.is_none();
    let keep_source = new.default_source.is_none();
    let keep_devices = new.sinks.is_empty()
        && new.sources.is_empty()
        && (!last.sinks.is_empty() || !last.sources.is_empty());
    let sinks = if keep_devices {
        last.sinks.clone()
    } else {
        new.sinks
    };
    let sources = if keep_devices {
        last.sources.clone()
    } else {
        new.sources
    };
    let default_sink = if keep_sink {
        last.default_sink
    } else {
        new.default_sink
    };
    let default_source = if keep_source {
        last.default_source
    } else {
        new.default_source
    };
    let sink_volume = if keep_sink {
        last.sink_volume
    } else {
        new.sink_volume
    };
    let sink_mute = if keep_sink {
        last.sink_mute
    } else {
        new.sink_mute
    };
    let source_volume = if keep_source {
        last.source_volume
    } else {
        new.source_volume
    };
    let source_mute = if keep_source {
        last.source_mute
    } else {
        new.source_mute
    };
    *last = Snapshot {
        available: new.available,
        sinks,
        sources,
        default_sink,
        default_source,
        sink_volume,
        sink_mute,
        source_volume,
        source_mute,
    };
}

fn lock_snap(last: &Mutex<Snapshot>) -> std::sync::MutexGuard<'_, Snapshot> {
    last.lock().unwrap_or_else(|e| e.into_inner())
}

fn publish(event_tx: &iced::futures::channel::mpsc::UnboundedSender<Event>, last: &Snapshot) {
    meter::set_target(if last.available {
        last.default_sink
    } else {
        None
    });
    let _ = event_tx.unbounded_send(Event::Snapshot(last.clone()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hide_when_unavailable() {
        assert!(bar_icon(&Snapshot::default()).is_none());
    }

    #[test]
    fn mute_and_level_icons() {
        let mut s = Snapshot {
            available: true,
            sink_volume: 0.9,
            sink_mute: false,
            ..Snapshot::default()
        };
        assert_eq!(bar_icon(&s).unwrap().name, "lucide/volume-2");
        s.sink_volume = 0.5;
        assert_eq!(bar_icon(&s).unwrap().name, "lucide/volume-1");
        s.sink_volume = 0.1;
        assert_eq!(bar_icon(&s).unwrap().name, "lucide/volume");
        s.sink_mute = true;
        assert!(bar_icon(&s).unwrap().muted);
        assert_eq!(bar_icon(&s).unwrap().name, "lucide/volume-x");
    }

    #[test]
    fn slider_percent_to_command() {
        let mut ui = Ui::default();
        ui.snapshot.available = true;
        let cmd = ui.update(UiMsg::OutputVolume(45.0));
        assert!(matches!(cmd, Some(Command::SetSinkVolume(v)) if (v - 0.45).abs() < 0.001));
        assert!((ui.snapshot.sink_volume - 0.45).abs() < 0.001);
    }

    #[test]
    fn kick_does_not_clobber_snapshot() {
        let mut ui = Ui::default();
        ui.snapshot.available = true;
        ui.snapshot.sink_volume = 0.5;
        ui.on_event(Event::Kick);
        assert!(ui.snapshot.available);
        assert!((ui.snapshot.sink_volume - 0.5).abs() < 0.001);
    }

    #[test]
    fn set_default_sink_is_optimistic() {
        let mut ui = Ui::default();
        ui.snapshot.available = true;
        ui.snapshot.default_sink = Some(1);
        let cmd = ui.update(UiMsg::SetDefaultSink(9));
        assert!(matches!(cmd, Some(Command::SetDefault(9))));
        assert_eq!(ui.snapshot.default_sink, Some(9));
    }

    #[test]
    fn set_default_source_is_optimistic() {
        let mut ui = Ui::default();
        ui.snapshot.available = true;
        ui.snapshot.default_source = Some(2);
        let cmd = ui.update(UiMsg::SetDefaultSource(8));
        assert!(matches!(cmd, Some(Command::SetDefault(8))));
        assert_eq!(ui.snapshot.default_source, Some(8));
    }

    #[test]
    fn merge_keeps_last_volume_when_wpctl_silent() {
        let mut last = Snapshot {
            available: true,
            default_sink: Some(56),
            sink_volume: 0.4,
            sink_mute: false,
            sinks: vec![Device {
                id: 56,
                name: "HDMI".into(),
                kind: Kind::Output,
            }],
            ..Snapshot::default()
        };
        merge_snapshot(
            &mut last,
            Snapshot {
                available: true,
                sinks: vec![Device {
                    id: 56,
                    name: "HDMI".into(),
                    kind: Kind::Output,
                }],
                ..Snapshot::default()
            },
        );
        assert!(last.available);
        assert_eq!(last.default_sink, Some(56));
        assert!((last.sink_volume - 0.4).abs() < 0.001);
        assert_eq!(last.sinks[0].name, "HDMI");
    }

    #[test]
    fn merge_keeps_last_devices_on_empty_graph() {
        let mut last = Snapshot {
            available: true,
            sinks: vec![Device {
                id: 163,
                name: "WH-CH520".into(),
                kind: Kind::Output,
            }],
            ..Snapshot::default()
        };
        merge_snapshot(
            &mut last,
            Snapshot {
                available: true,
                ..Snapshot::default()
            },
        );
        assert_eq!(last.sinks[0].id, 163);
    }
}
