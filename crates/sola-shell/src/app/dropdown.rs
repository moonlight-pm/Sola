//! The drop-down terminal over an app that fills the screen
//! (`Topic::DropDown`, `solactl dropdown`; Joshua, 2026-11-15, for WoW in
//! Cinema). While an app that has one shows in Cinema, the shell grabs its
//! key; a press starts `sola-dropdown` running its command, and its window
//! goes across the top of the screen, after the app in the composition, so
//! rivulet draws it over the fullscreen app (Oath dc481d4), and takes the
//! keyboard. The app stays fullscreen: sola-river keeps a fullscreen window
//! so while the focused window is composed above it. The next press closes
//! it and gives the keyboard back; so does the app leaving Cinema.

use std::collections::HashMap;

use iced::Task;
use sola_bus::topics::{DropDown, FrameUpdate, Topic};
use sola_core::KeyChord;

use super::{Msg, Shell, with_bus};

/// The drop-down program's app id.
pub const APP_ID: &str = "sola-dropdown";

#[derive(Default)]
pub struct DropDowns {
    /// Each app's drop-down, by app id.
    by_app: HashMap<String, DropDown>,
    /// The one down now.
    down: Option<Down>,
    /// Counts starts, so a late end of an earlier one is not this one's.
    starts: u64,
}

struct Down {
    /// The window of the app it is over.
    over: u32,
    /// How much of the screen's height it takes, in percent.
    height: u8,
    /// Its window, once it maps.
    window: Option<u32>,
    /// Sent away before its window mapped: close it as it maps.
    closing: bool,
    start: u64,
}

impl Shell {
    /// `Topic::DropDown`: an app's drop-down set, or taken away (`None`).
    pub(crate) fn on_drop_down(&mut self, app_id: &str, drop: Option<DropDown>) {
        match drop {
            Some(d) => {
                tracing::info!(%app_id, key = %d.key.display(), height = d.height, "drop-down set");
                self.drop_down.by_app.insert(app_id.to_string(), d);
            }
            None => {
                self.drop_down.by_app.remove(app_id);
            }
        }
        self.emit_registered_chords();
    }

    /// The window in Cinema and its app's drop-down, when it has one.
    fn drop_down_here(&self) -> Option<(u32, &DropDown)> {
        let wid = self.cinema_shown?;
        let app = self.app_id_for_window(wid)?;
        self.drop_down.by_app.get(&app).map(|d| (wid, d))
    }

    /// The key River grabs for it: while an app with one is in Cinema.
    pub(crate) fn drop_down_key(&self) -> Option<KeyChord> {
        self.drop_down_here().map(|(_, d)| d.key)
    }

    /// A chord: bring the drop-down down or send it away, if it is its key.
    pub(crate) fn drop_down_chord(&mut self, chord: &KeyChord) -> Option<Task<Msg>> {
        let (over, d) = self.drop_down_here()?;
        if d.key != *chord {
            return None;
        }
        if self.drop_down.down.is_some() {
            self.send_drop_down_away();
            return Some(Task::none());
        }
        let (command, height) = (d.command.clone(), d.height);
        self.drop_down.starts += 1;
        let start = self.drop_down.starts;
        self.drop_down.down = Some(Down {
            over,
            height,
            window: None,
            closing: false,
            start,
        });
        let exe = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|dir| dir.join(APP_ID)))
            .unwrap_or_else(|| APP_ID.into());
        tracing::info!(over, %command, "drop-down");
        Some(Task::perform(
            async move {
                tokio::task::spawn_blocking(move || {
                    std::process::Command::new(&exe)
                        .args(["--", "sh", "-c", &command])
                        .status()
                        .map_err(|e| format!("{}: {e}", exe.display()))
                })
                .await
            },
            move |done| {
                let result = done.map_err(|e| e.to_string()).and_then(|r| r);
                if let Err(e) = &result {
                    tracing::error!(%e, "drop-down did not start");
                }
                Msg::DropDownEnded(start)
            },
        ))
    }

    /// Close it and give the keyboard back to the app under it.
    fn send_drop_down_away(&mut self) {
        let Some(down) = self.drop_down.down.as_mut() else {
            return;
        };
        let Some(window) = down.window else {
            down.closing = true;
            return;
        };
        let over = down.over;
        self.drop_down.down = None;
        with_bus(|bus| {
            let _ = bus.emit(Topic::CloseWindow(window));
        });
        self.emit_composition();
        if self.cinema_shown == Some(over) {
            if let Some(app) = self.app_id_for_window(over) {
                self.focus_window(over, &app);
            }
        }
        self.emit_registered_chords();
    }

    /// Its program ended. If its window never mapped, it is not down.
    pub(crate) fn on_drop_down_ended(&mut self, start: u64) {
        if self
            .drop_down
            .down
            .as_ref()
            .is_some_and(|d| d.start == start && d.window.is_none())
        {
            self.drop_down.down = None;
        }
    }

    /// Whether `app_id` is the drop-down's: never placed in a zone or listed.
    pub(crate) fn is_drop_down_app(app_id: &str) -> bool {
        app_id == APP_ID
    }

    /// Its window mapped: it goes over the app, or closes if it was sent
    /// away meanwhile. Whether it should take the keyboard once composed.
    pub(crate) fn drop_down_mapped(&mut self, wid: u32) -> bool {
        let Some(down) = self.drop_down.down.as_mut().filter(|d| d.window.is_none()) else {
            // Not one the shell started now: close it.
            with_bus(|bus| {
                let _ = bus.emit(Topic::CloseWindow(wid));
            });
            return false;
        };
        down.window = Some(wid);
        if down.closing {
            self.send_drop_down_away();
            return false;
        }
        true
    }

    /// Its window closed (the command ended, or it was sent away): the
    /// keyboard goes back to the app under it, when it had it. The window
    /// that should have the keyboard now.
    pub(crate) fn drop_down_closed(&mut self, removed: &[u32]) -> Option<u32> {
        let down = self.drop_down.down.as_ref()?;
        let window = down.window.filter(|w| removed.contains(w))?;
        let over = down.over;
        self.drop_down.down = None;
        (self.focused_window_id == Some(window)).then(|| {
            self.focused_window_id = Some(over);
            over
        })
    }

    /// Its window, while it is down over the window in Cinema.
    pub(crate) fn drop_down_window(&self, cinema: u32) -> Option<u32> {
        self.drop_down.down.as_ref().filter(|d| d.over == cinema)?.window
    }

    /// The window it should have the keyboard, once composed over the app.
    pub(crate) fn focus_drop_down(&mut self, wid: u32) {
        self.focus_window(wid, APP_ID);
    }

    /// Whether one is down: the pointer over the app under it does not
    /// take the keyboard from it.
    pub(crate) fn drop_down_is_down(&self) -> bool {
        self.drop_down.down.is_some()
    }

    /// Across the top of the screen, its height's share of it.
    pub(crate) fn drop_down_frame(&self) -> Option<FrameUpdate> {
        let down = self.drop_down.down.as_ref()?;
        let (w, h) = self.zoning.output_size?;
        Some(FrameUpdate {
            window_id: down.window?,
            x: 0,
            y: 0,
            width: w.max(1),
            height: (h * i32::from(down.height) / 100).max(1),
            fullscreen: false,
        })
    }

    /// Cinema changed: one down over an app that left it goes away.
    pub(crate) fn drop_down_on_cinema(&mut self, cinema: Option<u32>) {
        if self.drop_down.down.as_ref().is_some_and(|d| Some(d.over) != cinema) {
            tracing::info!(?cinema, "drop-down: its app left Cinema");
            let down = self.drop_down.down.as_mut().expect("checked");
            match down.window {
                Some(window) => {
                    self.drop_down.down = None;
                    with_bus(|bus| {
                        let _ = bus.emit(Topic::CloseWindow(window));
                    });
                }
                None => down.closing = true,
            }
        }
    }
}
