//! A session's drop-down terminal (Joshua, 2026-11-12: "a button to 'drop
//! down' a terminal of 25 lines where i can do one off commands, it starts
//! in the same folder as the session. and then i can easily dismiss it"):
//! a shell over the top of the session's window, under its bar. The bar's
//! button or Super+J shows and hides it; while it shows, keys go to it.
//! The session's pane keeps its size underneath, so its agent does not
//! redraw.
//!
//! It is a tmux session of its own on the session's computer,
//! `drop-sws-<id>`, which nothing lists as a session (those are `sws-`).
//! Hiding it, closing the window or quitting Projects keep it. The session
//! closing ends it: the tmux config's `session-closed` hook
//! ([`sola_terminal::tmux`]), wherever and however it closed. `exit` in it
//! ends it too, and the next one starts fresh in the session's folder.

use iced::widget::{Space, canvas, column, container, opaque};
use iced::{Element, Length, Shadow, Task, Vector};
use sola_kit::components::divider::horizontal_divider;
use sola_terminal::emulator::{self, Emulator, Listener};
use sola_terminal::pty::{self, PtyBackend};
use sola_terminal::state::PaneRuntime;
use sola_terminal::term_view;

use crate::link::Link;
use crate::{App, Msg, catalog};

/// How far it drops, in rows.
const ROWS: u16 = 25;
/// Rows of the session left showing under it when the window is short.
const LEAVE: u16 = 6;
/// Its pane id: `drop:<session>`.
const PREFIX: &str = "drop:";

pub fn pane_id(tab: &str) -> String {
    format!("{PREFIX}{tab}")
}

/// The session a drop-down pane id belongs to.
pub fn tab_of(pane: &str) -> Option<&str> {
    pane.strip_prefix(PREFIX)
}

/// Its tmux session, on the session's computer: the hook's name for it,
/// `drop-` and the session's.
pub fn session_name(tab: &str) -> String {
    format!("drop-{}{}", crate::TMUX_PREFIX, catalog::bare_id(tab))
}

/// Its rows, over a session `session_rows` tall.
pub fn rows(session_rows: u16) -> u16 {
    ROWS.min(session_rows.saturating_sub(LEAVE)).max(1)
}

impl App {
    /// The pane keys go to: the focused session's drop-down while it shows.
    pub(crate) fn key_pane(&self) -> String {
        if self.drops.contains(&self.focused) {
            pane_id(&self.focused)
        } else {
            self.focused.clone()
        }
    }

    /// Show or hide session `tab`'s drop-down.
    pub(crate) fn toggle_drop(&mut self, tab: &str) -> Task<Msg> {
        if self.drops.remove(tab) {
            return Task::none();
        }
        if self.attach_drop(tab) {
            self.drops.insert(tab.to_string());
        }
        Task::none()
    }

    /// Its grid: the session's columns, [`rows`] rows.
    pub(crate) fn drop_grid(&self, tab: &str) -> (u16, u16) {
        let (cols, session_rows) = self.pane_grid(tab);
        (cols, rows(session_rows))
    }

    /// Attach session `tab`'s drop-down, making it if it is not there.
    /// Whether it is attached.
    fn attach_drop(&mut self, tab: &str) -> bool {
        let id = pane_id(tab);
        if self.runtimes.contains_key(&id) {
            self.resize_all_panes();
            return true;
        }
        let Some(ws) = self.tab_for_pane(tab) else {
            return false;
        };
        let host = ws.host.clone();
        let cwd = ws.path.to_string_lossy().into_owned();
        if let Some(h) = &host {
            if !matches!(self.links.get(h), Some(Link::Up { .. })) {
                tracing::info!(tab, host = %h, "no drop-down: its computer is not linked");
                return false;
            }
        }
        let session = session_name(tab);
        let (cols, rows) = self.drop_grid(tab);
        let listener = Listener::new(
            id.clone(),
            pty::pty_write_sender(),
            emulator::notify_sender(),
            emulator::title_sender(),
        );
        let em = Emulator::new(cols, rows, listener);
        let (term, cursor) = (em.term(), em.cursor_snap());
        let (notify, exit) = (emulator::notify_sender(), emulator::exit_sender());
        let backend = match &host {
            None => PtyBackend::spawn_or_attach(
                &id, &session, cols, rows, Some(&cwd), term, cursor, notify, exit,
            ),
            Some(h) => PtyBackend::spawn_remote(
                &id, h, &session, cols, rows, &cwd, term, cursor, notify, exit, &[], &[], &[],
            ),
        };
        let backend = match backend {
            Ok(b) => b,
            Err(e) => {
                tracing::error!(tab, %session, "drop-down terminal: {e}");
                return false;
            }
        };
        tracing::info!(tab, %session, %cwd, "drop-down terminal");
        backend.resize_client(cols, rows);
        backend.sigwinch();
        self.runtimes.insert(
            id.clone(),
            PaneRuntime {
                emulator: em,
                backend,
                cache: canvas::Cache::default(),
            },
        );
        self.pane_grids.insert(id, (cols, rows));
        true
    }

    /// Its client ended: `exit` in it, or its session or ssh gone. It
    /// hides; the next Super+J attaches it again, or makes a new one.
    pub(crate) fn on_drop_exit(&mut self, id: &str) -> Task<Msg> {
        tracing::info!(pane = %id, "drop-down terminal exited");
        self.runtimes.remove(id);
        self.pane_grids.remove(id);
        if let Some(tab) = tab_of(id) {
            self.drops.remove(tab);
        }
        Task::none()
    }

    /// Let go of session `tab`'s drop-down here (its window closing): its
    /// tmux session stays, for the next time.
    pub(crate) fn forget_drop(&mut self, tab: &str) {
        let id = pane_id(tab);
        self.runtimes.remove(&id);
        self.pane_grids.remove(&id);
        self.drops.remove(tab);
    }

    /// Session `tab`'s drop-down, over the top of its pane, when it shows.
    pub(crate) fn drop_view(&self, tab: &str) -> Option<Element<'_, Msg>> {
        if !self.drops.contains(tab) {
            return None;
        }
        let id = pane_id(tab);
        let rt = self.runtimes.get(&id)?;
        let (_, rows) = self.pane_grids.get(&id).copied().unwrap_or_else(|| self.drop_grid(tab));
        let bg = self.palette.bg;
        // Clipped, as the session's pane is.
        let panel = container(self.term_canvas(&id, rt))
            .width(Length::Fill)
            .height(Length::Fixed(term_view::height_for(rows, self.metrics)))
            .clip(true)
            .style(move |_| container::Style {
                background: Some(bg.into()),
                ..container::Style::default()
            });
        let dropped = container(column![panel, horizontal_divider()]).style(|_| container::Style {
            shadow: Shadow {
                color: iced::Color::from_rgba(0.0, 0.0, 0.0, 0.35),
                offset: Vector::new(0.0, 4.0),
                blur_radius: 14.0,
            },
            ..container::Style::default()
        });
        // Opaque: the session's pane under it takes no clicks or wheel
        // there; below it, the session's pane is as it was.
        Some(column![opaque(dropped), Space::new().height(Length::Fill)].into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn its_pane_id_names_its_session() {
        assert_eq!(tab_of(&pane_id("ws-main@novus")), Some("ws-main@novus"));
        assert_eq!(tab_of("ws-main"), None);
    }

    #[test]
    fn its_tmux_session_is_the_sessions_name_after_drop() {
        assert_eq!(session_name("ws-main"), "drop-sws-ws-main");
        assert_eq!(session_name("ws-main@novus"), "drop-sws-ws-main");
    }

    #[test]
    fn it_drops_25_rows_and_leaves_some_of_a_short_session_showing() {
        assert_eq!(rows(60), 25);
        assert_eq!(rows(31), 25);
        assert_eq!(rows(20), 14);
        assert_eq!(rows(3), 1);
    }
}
