//! Per-app float-state tracking: which of an app's windows float, so its
//! frame ([`crate::window_frame`]) knows how to draw.
//!
//! **Policy:** the shell marks every window without a zone assignment as
//! floating and emits sticky `Topic::WindowFloating`. Every kit window draws
//! its frame ([`crate::window_frame`]); the float bit picks how: a tile is
//! opaque with ground-painted corners, a float is transparent there and
//! resizes from its edges. Size is the client's requested size unless a zone
//! or restored float geometry applies.
//!
//! An app doesn't know its own sola-river `window_id`, so we learn it by
//! matching `(app_id, title)` from `Topic::Windows`, then track the float bit
//! from the sticky `Topic::WindowFloating`. Feed [`update`] every bus message
//! (from the app's `bus_subscription` fold); read [`is_floating`] /
//! [`is_floating_any`] in `view`.
//!
//! Typical single-window kit app wiring:
//! 1. `window_settings_transparent(APP_ID)` (ARGB swapchain for float CSD)
//! 2. `FloatState::new(APP_ID)` + `window_id: Option<window::Id>`
//! 3. Boot: `window::latest().map(Msg::WindowReady)`
//! 4. Bus: `float.update(&message)`
//! 5. Theme: [`theme_for`] while floating — tiled is an opaque fill, which
//!    the patched `iced_winit` turns into a Wayland opaque-region (River
//!    GLES scanout). Float uses [`crate::theme::overlay`] and clears it.
//! 6. View: [`crate::window_frame`] around content
//! 7. Handlers: [`drag`] / [`drag_resize`] / [`close_app`]
//!
//! [`update`]: FloatState::update
//! [`is_floating`]: FloatState::is_floating
//! [`is_floating_any`]: FloatState::is_floating_any

use std::collections::{HashMap, HashSet};

use iced::window::Direction;
use iced::{Task, Theme};

use sola_bus::Message;
use sola_bus::topics::Topic;

use crate::components::window_frame::Chrome;

#[derive(Debug, Default)]
pub struct FloatState {
    app_id: String,
    /// This app's surfaces: sola-river `window_id` keyed by window title.
    ids_by_title: HashMap<String, u32>,
    /// Currently-floating `window_id`s (all apps; filtered on read).
    floating: HashSet<u32>,
    /// Windows the shell shows in Cinema (`Topic::WindowChrome`): no frame.
    cinema: HashSet<u32>,
}

impl FloatState {
    pub fn new(app_id: impl Into<String>) -> Self {
        Self {
            app_id: app_id.into(),
            ..Default::default()
        }
    }

    /// Fold one bus message. Call from the app's bus-message update arm.
    pub fn update(&mut self, msg: &Message) {
        match Topic::parse(msg) {
            // Windows is the full list each time — rebuild the title map and
            // prune the float set so closed windows' ids don't linger.
            Some(Topic::Windows(windows)) => {
                self.ids_by_title.clear();
                let mut live = HashSet::new();
                for w in windows {
                    live.insert(w.window_id);
                    if w.app_id == self.app_id {
                        self.ids_by_title.insert(w.title, w.window_id);
                    }
                }
                self.floating.retain(|id| live.contains(id));
                self.cinema.retain(|id| live.contains(id));
            }
            // A retraction is the same topic, not sticky: the window left.
            Some(Topic::WindowChrome(c)) => {
                if msg.sticky && c.cinema {
                    self.cinema.insert(c.window_id);
                } else {
                    self.cinema.remove(&c.window_id);
                }
            }
            Some(Topic::WindowFloating(wf)) => {
                if wf.floating {
                    self.floating.insert(wf.window_id);
                } else {
                    self.floating.remove(&wf.window_id);
                }
            }
            _ => {}
        }
    }

    /// Is this app's surface with `title` currently floating?
    pub fn is_floating(&self, title: &str) -> bool {
        self.ids_by_title
            .get(title)
            .is_some_and(|id| self.floating.contains(id))
    }

    /// Is any of this app's surfaces floating? Convenient for single-window apps.
    pub fn is_floating_any(&self) -> bool {
        self.ids_by_title
            .values()
            .any(|id| self.floating.contains(id))
    }

    /// Is the sola-river window `window_id` floating? For apps with a
    /// window per tab ([`crate::tabs::TabWindows::river_id`]).
    pub fn is_window_floating(&self, window_id: u32) -> bool {
        self.floating.contains(&window_id)
    }

    /// How to frame the sola-river window `window_id`.
    pub fn chrome(&self, window_id: u32) -> Chrome {
        if self.cinema.contains(&window_id) {
            Chrome::Bare
        } else {
            self.is_window_floating(window_id).into()
        }
    }

    /// How to frame this app's window. For single-window apps.
    pub fn chrome_any(&self) -> Chrome {
        if self.ids_by_title.values().any(|id| self.cinema.contains(id)) {
            Chrome::Bare
        } else {
            self.is_floating_any().into()
        }
    }

    /// Any compositor `window_id` belonging to this app, if known.
    pub fn any_window_id(&self) -> Option<u32> {
        self.ids_by_title.values().copied().next()
    }
}

/// Theme while floating: clear `background.base` so rounded corners show
/// the desktop (and Wayland opaque-region is cleared). Zoned: the live
/// theme unchanged (opaque fill → compositor scanout).
pub fn theme_for(floating: bool, theme: &Theme) -> Theme {
    if floating {
        crate::theme::overlay(theme)
    } else {
        theme.clone()
    }
}

/// Begin an interactive move of the app's iced window (titlebar drag).
pub fn drag<Message>(window_id: Option<iced::window::Id>) -> Task<Message> {
    match window_id {
        Some(id) => iced::window::drag(id),
        None => Task::none(),
    }
}

/// Begin an interactive edge/corner resize (a floating frame's edge).
pub fn drag_resize<Message>(
    window_id: Option<iced::window::Id>,
    direction: Direction,
) -> Task<Message> {
    match window_id {
        Some(id) => iced::window::drag_resize(id, direction),
        None => Task::none(),
    }
}

/// Ask the session to close this app (`Topic::CloseApp`).
pub fn close_app(app_id: &str) {
    if let Ok(mut bus) = crate::app::bus().lock() {
        let _ = bus.emit(Topic::CloseApp(app_id.into()));
    }
}

/// Boot task that resolves the app's primary iced window id.
pub fn window_ready_task<Message>(
    to_msg: impl Fn(Option<iced::window::Id>) -> Message + Send + 'static,
) -> Task<Message>
where
    Message: Send + 'static,
{
    iced::window::latest().map(to_msg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sola_bus::topics::{Window, WindowFloating};

    fn win(window_id: u32, app_id: &str, title: &str) -> Window {
        Window {
            window_id,
            app_id: app_id.into(),
            title: title.into(),
            pid: None,
        }
    }

    #[test]
    fn theme_for_tiled_is_opaque_float_is_transparent() {
        let base = crate::default_theme();
        assert!(
            theme_for(false, &base)
                .extended_palette()
                .background
                .base
                .color
                .a
                >= 0.999,
            "tiled fill must be opaque so iced_winit can set opaque-region"
        );
        assert_eq!(
            theme_for(true, &base)
                .extended_palette()
                .background
                .base
                .color
                .a,
            0.0,
            "float overlay must clear the fill so rounded CSD can punch through"
        );
    }

    #[test]
    fn tracks_own_float_by_app_id_and_title() {
        let mut fs = FloatState::new("sola-monitor");
        fs.update(
            &Topic::Windows(vec![
                win(7, "sola-monitor", "Monitor"),
                win(9, "other-app", "Other"),
            ])
            .to_message(),
        );
        assert!(!fs.is_floating_any());

        // our window floats
        fs.update(
            &Topic::WindowFloating(WindowFloating {
                window_id: 7,
                floating: true,
            })
            .to_message(),
        );
        assert!(fs.is_floating_any());
        assert!(fs.is_floating("Monitor"));

        // another app's float does not count as ours
        fs.update(
            &Topic::WindowFloating(WindowFloating {
                window_id: 9,
                floating: true,
            })
            .to_message(),
        );
        assert!(fs.is_floating("Monitor"));
        assert!(!fs.is_floating("Other")); // "Other" isn't ours

        // unfloat clears it
        fs.update(
            &Topic::WindowFloating(WindowFloating {
                window_id: 7,
                floating: false,
            })
            .to_message(),
        );
        assert!(!fs.is_floating_any());
        assert!(!fs.is_floating("Monitor"));
    }

    #[test]
    fn closed_window_drops_from_tracking() {
        let mut fs = FloatState::new("sola-monitor");
        fs.update(&Topic::Windows(vec![win(7, "sola-monitor", "Monitor")]).to_message());
        fs.update(
            &Topic::WindowFloating(WindowFloating {
                window_id: 7,
                floating: true,
            })
            .to_message(),
        );
        assert!(fs.is_floating_any());
        // window closes → Windows no longer lists it
        fs.update(&Topic::Windows(vec![]).to_message());
        assert!(!fs.is_floating_any());
    }

    fn chrome_msg(window_id: u32, cinema: bool, sticky: bool) -> Message {
        let mut msg = Topic::WindowChrome(sola_bus::topics::WindowChrome {
            window_id,
            tiled: true,
            cinema,
            rim: false,
        })
        .to_message();
        msg.sticky = sticky;
        msg
    }

    #[test]
    fn cinema_drops_the_frame_until_it_ends() {
        let mut fs = FloatState::new("sola-monitor");
        fs.update(&Topic::Windows(vec![win(7, "sola-monitor", "Monitor")]).to_message());
        assert_eq!(fs.chrome_any(), Chrome::Tiled);
        fs.update(&chrome_msg(7, true, true));
        assert_eq!(fs.chrome_any(), Chrome::Bare);
        assert_eq!(fs.chrome(7), Chrome::Bare);
        fs.update(&chrome_msg(7, false, true));
        assert_eq!(fs.chrome_any(), Chrome::Tiled);
        fs.update(&chrome_msg(7, true, true));
        // A retraction (the window left its zone record) ends it too.
        fs.update(&chrome_msg(7, true, false));
        assert_eq!(fs.chrome(7), Chrome::Tiled);
    }
}
