//! Menubar volume: PipeWire graph + WirePlumber `wpctl`.
//! See docs/specs/2026-08-29-shell-audio-menubar-design.md.

mod meter;
mod pw;
pub mod view;
pub mod wave;

use iced::Subscription;
use iced::futures::Stream;
use std::sync::{Mutex, OnceLock};
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
            UiMsg::SetDefaultSink(id) => Some(Command::SetDefault(id)),
            UiMsg::SetDefaultSource(id) => Some(Command::SetDefault(id)),
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
    push(&event_tx);
    loop {
        match cmd_rx.recv_timeout(Duration::from_secs(1)) {
            Ok(cmd) => {
                apply(cmd);
                while let Ok(more) = cmd_rx.try_recv() {
                    apply(more);
                }
                push(&event_tx);
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => push(&event_tx),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
        }
    }
}

fn apply(cmd: Command) {
    let snap = pw::snapshot();
    match cmd {
        Command::Refresh => {}
        Command::SetSinkVolume(v) => {
            if let Some(id) = snap.default_sink {
                let _ = pw::set_volume(id, v);
            }
        }
        Command::SetSourceVolume(v) => {
            if let Some(id) = snap.default_source {
                let _ = pw::set_volume(id, v);
            }
        }
        Command::SetSinkMute(m) => {
            if let Some(id) = snap.default_sink {
                let _ = pw::set_mute(id, m);
            }
        }
        Command::SetSourceMute(m) => {
            if let Some(id) = snap.default_source {
                let _ = pw::set_mute(id, m);
            }
        }
        Command::SetDefault(id) => {
            let _ = pw::set_default(id);
        }
    }
}

fn push(event_tx: &iced::futures::channel::mpsc::UnboundedSender<Event>) {
    let snap = pw::snapshot();
    meter::set_target(if snap.available {
        snap.default_sink
    } else {
        None
    });
    let _ = event_tx.unbounded_send(Event::Snapshot(snap));
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
}
