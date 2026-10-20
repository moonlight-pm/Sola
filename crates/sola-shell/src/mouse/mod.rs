//! Menubar mouse battery: asks a Lamzu receiver over hidraw.
//!
//! The kernel exposes no battery for this receiver (no `power_supply`),
//! so the shell asks the mouse itself through the vendor protocol
//! ([`compx`]). One thread — same idea as the stats sampler — asking once
//! a minute while the mouse answers and every 10 s while it sleeps (a
//! sleeping mouse ignores the question). The chip shows only once the
//! mouse has answered, and dims when it has been silent for
//! [`STALE_AFTER`] (asleep, off, or out of range). Needs RW on the
//! receiver's hidraw node (udev rule in `nix/module.nix`).

mod compx;

pub use compx::Battery;

use iced::Subscription;
use iced::futures::Stream;
use std::time::{Duration, Instant};

const POLL: Duration = Duration::from_secs(60);
/// A sleeping mouse does not answer (the receiver stays silent too), so
/// ask more often until it does — the chip then shows soon after it wakes.
const RETRY: Duration = Duration::from_secs(10);
/// Silence after the last answer before the chip dims.
pub const STALE_AFTER: Duration = Duration::from_secs(10 * 60);
/// At or below: warning tint.
pub const LOW_PCT: u8 = 20;
/// At or below: danger tint.
pub const CRIT_PCT: u8 = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    /// The mouse answered.
    Reading(Battery),
    /// A receiver is plugged in but no answer came (asleep, off, or no access).
    Silent,
    /// No known receiver is plugged in.
    Gone,
}

/// Menubar-side state, folded from [`Event`]s.
#[derive(Clone, Debug, Default)]
pub struct Ui {
    pub battery: Option<Battery>,
    last_answer: Option<Instant>,
    pub stale: bool,
}

impl Ui {
    pub fn on_event(&mut self, ev: Event, now: Instant) {
        match ev {
            Event::Reading(b) => {
                self.battery = Some(b);
                self.last_answer = Some(now);
                self.stale = false;
            }
            Event::Silent => {
                self.stale = self
                    .last_answer
                    .is_some_and(|t| now.duration_since(t) >= STALE_AFTER);
            }
            Event::Gone => *self = Self::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Normal,
    Low,
    Critical,
    /// No answer for a while — the number is old.
    Stale,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BarChip {
    pub icon: &'static str,
    pub label: String,
    pub tone: Tone,
}

/// The menubar chip, or `None` until the mouse has answered.
pub fn bar_chip(ui: &Ui) -> Option<BarChip> {
    let b = ui.battery?;
    let tone = if ui.stale {
        Tone::Stale
    } else if b.charging {
        Tone::Normal
    } else if b.percent <= CRIT_PCT {
        Tone::Critical
    } else if b.percent <= LOW_PCT {
        Tone::Low
    } else {
        Tone::Normal
    };
    Some(BarChip {
        icon: if b.charging {
            "lucide/battery-charging"
        } else {
            "lucide/mouse"
        },
        label: format!("{}%", b.percent),
        tone,
    })
}

pub fn subscription() -> Subscription<Event> {
    Subscription::run(mouse_stream)
}

fn mouse_stream() -> impl Stream<Item = Event> {
    let (tx, rx) = iced::futures::channel::mpsc::unbounded::<Event>();
    let spawned = std::thread::Builder::new()
        .name("sola-mouse".into())
        .spawn(move || {
            let mut warned = false;
            loop {
                let ev = ask(&mut warned);
                if tx.unbounded_send(ev).is_err() {
                    break;
                }
                std::thread::sleep(match ev {
                    Event::Reading(_) => POLL,
                    Event::Silent | Event::Gone => RETRY,
                });
            }
        });
    if let Err(e) = spawned {
        tracing::error!("mouse battery thread: {e}");
    }
    rx
}

/// One round: find receivers, ask each until one answers. Open/IO errors
/// (usually a missing udev rule) are logged once, not every minute.
fn ask(warned: &mut bool) -> Event {
    let nodes = compx::find();
    if nodes.is_empty() {
        return Event::Gone;
    }
    for node in &nodes {
        match compx::read_battery(node) {
            Ok(Some(b)) => {
                tracing::debug!(node = %node.display(), percent = b.percent, charging = b.charging, "mouse battery");
                *warned = false;
                return Event::Reading(b);
            }
            Ok(None) => {
                tracing::debug!(node = %node.display(), "mouse battery: no answer");
            }
            Err(e) if !*warned => {
                tracing::warn!(node = %node.display(), "mouse battery: cannot ask the receiver: {e} (needs the hidraw udev rule from nix/module.nix)");
                *warned = true;
            }
            Err(_) => {}
        }
    }
    Event::Silent
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bat(percent: u8, charging: bool) -> Battery {
        Battery { percent, charging }
    }

    #[test]
    fn hidden_until_first_answer() {
        let mut ui = Ui::default();
        assert_eq!(bar_chip(&ui), None);
        ui.on_event(Event::Silent, Instant::now());
        assert_eq!(bar_chip(&ui), None);
    }

    #[test]
    fn tone_follows_level() {
        let t0 = Instant::now();
        let mut ui = Ui::default();
        for (pct, tone) in [
            (45, Tone::Normal),
            (LOW_PCT + 1, Tone::Normal),
            (LOW_PCT, Tone::Low),
            (CRIT_PCT, Tone::Critical),
        ] {
            ui.on_event(Event::Reading(bat(pct, false)), t0);
            let chip = bar_chip(&ui).unwrap();
            assert_eq!(chip.tone, tone, "{pct}%");
            assert_eq!(chip.label, format!("{pct}%"));
            assert_eq!(chip.icon, "lucide/mouse");
        }
    }

    #[test]
    fn charging_is_never_alarming() {
        let mut ui = Ui::default();
        ui.on_event(Event::Reading(bat(5, true)), Instant::now());
        let chip = bar_chip(&ui).unwrap();
        assert_eq!(chip.tone, Tone::Normal);
        assert_eq!(chip.icon, "lucide/battery-charging");
    }

    #[test]
    fn silence_dims_after_stale_window_then_answer_clears() {
        let t0 = Instant::now();
        let mut ui = Ui::default();
        ui.on_event(Event::Reading(bat(45, false)), t0);
        ui.on_event(Event::Silent, t0 + STALE_AFTER - Duration::from_secs(1));
        assert_eq!(bar_chip(&ui).unwrap().tone, Tone::Normal);
        ui.on_event(Event::Silent, t0 + STALE_AFTER);
        let chip = bar_chip(&ui).unwrap();
        assert_eq!(chip.tone, Tone::Stale);
        assert_eq!(chip.label, "45%");
        ui.on_event(Event::Reading(bat(44, false)), t0 + STALE_AFTER * 2);
        assert_eq!(bar_chip(&ui).unwrap().tone, Tone::Normal);
    }

    #[test]
    fn unplugged_receiver_hides_chip() {
        let mut ui = Ui::default();
        ui.on_event(Event::Reading(bat(45, false)), Instant::now());
        ui.on_event(Event::Gone, Instant::now());
        assert_eq!(bar_chip(&ui), None);
    }
}
