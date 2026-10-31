//! The desk menu as the shell runs it ([`crate::desk_menu`]): an app's
//! `DeskMenuOpen` puts the menu over the app's window, the menu overlay
//! takes the keyboard, and the answer goes back as one `DeskMenuPick` —
//! after the keyboard is back with the app. The vault is asked off the
//! iced thread.

use std::time::Duration;

use iced::Task;
use sola_bus::topics::{DeskMenuOpen, DeskMenuPick, DeskPick, FocusTarget, Topic};

use super::{Msg, Panel, Shell, with_bus};
use crate::desk_menu::{self, DeskMenu, DeskMsg, Key, Outcome, Target, VaultView, vault};

/// The right press that opened the menu can reach the shell after it
/// (sola-river and the app race on the bus): that click is not a click
/// elsewhere.
const OPENING_CLICK: Duration = Duration::from_millis(400);

/// Vault's app id: Sign in and Unlock bring it forward.
const VAULT_APP: &str = "sola-vault";

impl Shell {
    /// An app asks for the desk menu at a right-click.
    pub(crate) fn on_desk_menu_open(&mut self, open: DeskMenuOpen) -> Task<Msg> {
        // Whatever the menu overlay shows gives way (a desk menu still
        // open ends unpicked).
        let _ = self.dismiss_open_menu();
        let window = self.desk_window(&open);
        let anchor = self.desk_anchor(window, open.x, open.y);
        let main = desk_menu::rows::main_rows(&open, |edit| {
            self.keys.common_key(desk_menu::rows::edit_action(edit))
        });
        self.desk_generation = self.desk_generation.wrapping_add(1);
        let generation = self.desk_generation;
        let site = open.context.site.clone();
        let editable = open.context.editable;
        tracing::info!(
            app_id = %open.app_id,
            pid = open.pid,
            token = open.token,
            ?window,
            editable,
            "desk menu"
        );
        self.desk = Some(DeskMenu::new(open, main, window, anchor, generation));
        self.menu_open = true;
        self.current_open_index = None;
        self.current_open_is_system = false;
        self.set_open_panel(Some(Panel::DeskMenu));
        self.desk_relayout();
        self.emit_composition();
        self.emit_registered_chords();
        // Live already (another panel was up): the keyboard now; else when
        // iced reports the live size (`commit_overlay_show`).
        if self.overlay_iced_live[0] {
            self.desk_take_keyboard();
        }
        if !editable {
            return Task::none();
        }
        Task::perform(vault::off_thread(move || vault::ask(site)), move |view| {
            Msg::Desk(DeskMsg::Asked {
                generation,
                view: view.unwrap_or(VaultView::Unavailable),
            })
        })
    }

    pub(crate) fn on_desk(&mut self, msg: DeskMsg) -> Task<Msg> {
        match msg {
            DeskMsg::Hover { level, row } => {
                let outcome = self.desk.as_mut().map(|d| d.hover(level, row));
                self.desk_outcome(outcome)
            }
            DeskMsg::Press { level, row } => {
                let outcome = self.desk.as_mut().map(|d| d.press(level, row));
                self.desk_outcome(outcome)
            }
            DeskMsg::Dismiss => Task::done(Msg::CloseMenu),
            DeskMsg::SearchInput(query) => {
                if let Some(desk) = self.desk.as_mut() {
                    desk.set_query(query);
                }
                self.desk_relayout();
                Task::none()
            }
            DeskMsg::Asked { generation, view } => {
                if let Some(desk) = self.desk.as_mut().filter(|d| d.generation == generation) {
                    desk.vault = view;
                    desk.settle();
                }
                self.desk_relayout();
                Task::none()
            }
            DeskMsg::Listed { generation, items } => {
                if let Some(desk) = self.desk.as_mut().filter(|d| d.generation == generation) {
                    if let Some(search) = desk.search.as_mut() {
                        search.items = Some(items);
                        let query = search.query.clone();
                        desk.set_query(query);
                    }
                }
                self.desk_relayout();
                Task::none()
            }
            DeskMsg::Delivered { to, result } => match result {
                Ok(ticket) => {
                    send(&to, DeskPick::VaultInsert { ticket: ticket.0 });
                    Task::none()
                }
                Err(message) => {
                    tracing::warn!(app_id = %to.app_id, error = %message, "desk menu: no delivery");
                    send(&to, DeskPick::Dismissed);
                    self.menubar.push_toast(format!("Vault: {message}"));
                    let generation = self.menubar.toast_generation;
                    Task::perform(tokio::time::sleep(Duration::from_secs(5)), move |_| {
                        Msg::ToastExpire(generation)
                    })
                }
            },
        }
    }

    /// Arrows and Return while the menu has the keyboard.
    pub(crate) fn desk_key(&mut self, key: Key) -> Task<Msg> {
        let outcome = self.desk.as_mut().map(|d| d.key(key));
        self.desk_outcome(outcome)
    }

    fn desk_outcome(&mut self, outcome: Option<Outcome>) -> Task<Msg> {
        match outcome.unwrap_or(Outcome::Nothing) {
            Outcome::Nothing => Task::none(),
            Outcome::Moved => {
                self.desk_relayout();
                self.desk_field_focus()
            }
            Outcome::Pick(pick) => {
                if let Some(desk) = self.desk_close() {
                    send(&desk.target(), pick);
                }
                Task::none()
            }
            Outcome::Deliver { item, field } => {
                let Some(desk) = self.desk_close() else {
                    return Task::none();
                };
                let to = desk.target();
                let pid = to.pid;
                Task::perform(
                    vault::off_thread(move || vault::deliver(item, field, pid)),
                    move |result| {
                        Msg::Desk(DeskMsg::Delivered {
                            to: to.clone(),
                            result: result.unwrap_or_else(|| Err("The vault went away.".into())),
                        })
                    },
                )
            }
            Outcome::OpenVault => {
                if let Some(desk) = self.desk_close() {
                    send(&desk.target(), DeskPick::Dismissed);
                }
                self.launcher.prior_focus = self.focused_window_id;
                self.launch_from_launcher(Some(VAULT_APP))
            }
            Outcome::Search => {
                self.desk_relayout();
                let generation = self.desk_generation;
                Task::batch([
                    self.desk_field_focus(),
                    Task::perform(vault::off_thread(vault::list), move |items| {
                        Msg::Desk(DeskMsg::Listed {
                            generation,
                            items: items.unwrap_or_else(|| Err("The vault went away.".into())),
                        })
                    }),
                ])
            }
        }
    }

    /// The search field takes the keyboard while the keys are in its
    /// results and lets it go in a result's values, where ← and → are the
    /// menu's (a focused field keeps them for its caret).
    fn desk_field_focus(&mut self) -> Task<Msg> {
        let Some(desk) = self.desk.as_mut() else {
            return Task::none();
        };
        let want = desk.wants_field();
        if want == desk.field_focused {
            return Task::none();
        }
        desk.field_focused = want;
        if want {
            iced::widget::operation::focus(iced::widget::Id::new(desk_menu::SEARCH_INPUT_ID))
        } else {
            iced::advanced::widget::operate(
                iced::advanced::widget::operation::focusable::unfocus(),
            )
        }
    }

    /// The menu's sheets on the output, while it is open.
    pub(crate) fn desk_placed(&self) -> Option<desk_menu::place::Placed> {
        let (w, h) = self.zoning.output_size?;
        Some(self.desk.as_ref()?.placed((w as f32, h as f32)))
    }

    /// Frame the overlay again when the sheets' union moved or grew.
    fn desk_relayout(&mut self) {
        if let Some(desk) = self.desk.as_mut() {
            desk.settle();
        }
        let window = self.desk_placed().map(|p| p.window);
        if window != self.desk_frame_sent {
            self.desk_frame_sent = window;
            self.emit_overlay_frames();
        }
    }

    /// The menu overlay takes the keyboard (arrows, Return, the search).
    pub(crate) fn desk_take_keyboard(&self) {
        if self.desk.is_none() {
            return;
        }
        if let Some(wid) = self.lookup_window_id(Self::APP_ID, "menu") {
            with_bus(|bus| {
                let _ = bus.emit(Topic::Focus(FocusTarget { window_id: wid }));
            });
        }
    }

    /// Close the menu for a pick, the keyboard back with the app first.
    fn desk_close(&mut self) -> Option<DeskMenu> {
        let desk = self.desk.take()?;
        let _ = self.dismiss_open_menu();
        self.emit_overlay_frames();
        self.emit_composition();
        self.emit_registered_chords();
        self.desk_return_keyboard(desk.window);
        Some(desk)
    }

    /// The menu went with nothing picked (Escape, a press elsewhere,
    /// another menu): the app hears so. Called as the panel changes.
    pub(crate) fn desk_dismissed(&mut self) {
        let Some(desk) = self.desk.take() else {
            return;
        };
        self.desk_frame_sent = None;
        self.desk_return_keyboard(desk.window);
        send(&desk.target(), DeskPick::Dismissed);
    }

    fn desk_return_keyboard(&mut self, window: Option<u32>) {
        self.desk_frame_sent = None;
        let known = |wid: &u32| self.known_windows.iter().any(|w| w.window_id == *wid);
        if let Some(wid) = window.filter(known).or(self.focused_window_id) {
            with_bus(|bus| {
                let _ = bus.emit(Topic::Focus(FocusTarget { window_id: wid }));
            });
        }
    }

    /// The press that opened the menu, arriving after it.
    pub(crate) fn desk_opening_click(&self, window_id: u32) -> bool {
        self.desk
            .as_ref()
            .is_some_and(|d| d.window == Some(window_id) && d.opened_at.elapsed() < OPENING_CLICK)
    }

    /// The app's window went away under its menu.
    pub(crate) fn desk_window_gone(&mut self) -> bool {
        let gone = self.desk.as_ref().and_then(|d| d.window).is_some_and(|wid| {
            !self.known_windows.iter().any(|w| w.window_id == wid)
        });
        if gone {
            let _ = self.dismiss_open_menu();
            self.emit_overlay_frames();
            self.emit_composition();
            self.emit_registered_chords();
        }
        gone
    }

    /// The window the menu hangs over: the one last clicked, focused or
    /// under the pointer that is the app's (by its process), else any of
    /// the app's.
    fn desk_window(&self, open: &DeskMenuOpen) -> Option<u32> {
        let ours = |wid: &u32| {
            self.known_windows.iter().any(|w| {
                w.window_id == *wid
                    && match w.pid {
                        Some(pid) => pid == open.pid,
                        None => w.app_id == open.app_id,
                    }
            })
        };
        [
            self.last_clicked_window,
            self.focused_window_id,
            self.pointer_window_id,
        ]
        .into_iter()
        .flatten()
        .find(ours)
        .or_else(|| {
            self.known_windows
                .iter()
                .find(|w| w.pid == Some(open.pid))
                .map(|w| w.window_id)
        })
    }

    /// The click in output pixels: the window's place plus the click in
    /// it. A window the shell has no place for: the output's middle.
    fn desk_anchor(&self, window: Option<u32>, x: i32, y: i32) -> (f32, f32) {
        match window.and_then(|w| self.zoning.live_geometry.get(&w)) {
            Some(g) => ((g.x + x) as f32, (g.y + y) as f32),
            None => {
                tracing::warn!(?window, "desk menu: no place for the window; centred");
                let (w, h) = self.zoning.output_size.unwrap_or((1920, 1080));
                ((w / 2) as f32, (h / 2) as f32)
            }
        }
    }
}

/// The answer to the app's menu.
fn send(to: &Target, pick: DeskPick) {
    with_bus(|bus| {
        let _ = bus.emit(Topic::DeskMenuPick(DeskMenuPick {
            app_id: to.app_id.clone(),
            pid: to.pid,
            token: to.token,
            pick,
        }));
    });
}
