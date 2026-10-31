//! A pane's desk menu (`sola_kit::desk_menu`): Copy and Paste, and the
//! vault's rows to type a value into the session (a `sudo` or `ssh`
//! prompt). Terminal and Projects both keep one [`PaneMenu`] and route a
//! pane's right-click and the bus through it.

use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::Term;
use iced::Point;
use sola_bus::Message;
use sola_kit::desk_menu::{self, DeskMenuContext, DeskPick};

use crate::emulator::Listener;

/// The rows' ids: the same as the Edit menu's, so they show its keys.
pub const COPY: &str = "copy";
pub const PASTE: &str = "paste";

/// The desk menu open on a pane, until its answer.
#[derive(Debug, Default)]
pub struct PaneMenu {
    open: Option<(u64, String)>,
}

impl PaneMenu {
    /// Ask for the desk menu on `pane` at `at` (window logical px).
    pub fn open(&mut self, pane: &str, at: Point, term: &FairMutex<Term<Listener>>) {
        let selection = has_selection(term);
        let token = desk_menu::next_token();
        self.open = Some((token, pane.to_string()));
        desk_menu::open(
            token,
            at.x,
            at.y,
            DeskMenuContext {
                editable: true,
                secret: false,
                selection,
                site: None,
            },
            vec![
                desk_menu::item(COPY, "Copy", selection),
                desk_menu::item(PASTE, "Paste", true),
            ],
            Vec::new(),
        );
    }

    /// The answer to the open menu and its pane, when `message` is it.
    pub fn answer(&mut self, message: &Message) -> Option<(String, DeskPick)> {
        let pick = desk_menu::pick_of(message)?;
        if self.open.as_ref().map(|(token, _)| *token) != Some(pick.token) {
            return None;
        }
        let (_, pane) = self.open.take()?;
        Some((pane, pick.pick))
    }
}

/// Something is selected in the pane.
fn has_selection(term: &FairMutex<Term<Listener>>) -> bool {
    term.lock()
        .selection
        .as_ref()
        .is_some_and(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sola_bus::topics::{DeskMenuPick, Topic};

    fn pick(token: u64) -> Message {
        Topic::DeskMenuPick(DeskMenuPick {
            app_id: "sola-terminal".into(),
            pid: std::process::id(),
            token,
            pick: DeskPick::Action(COPY.into()),
        })
        .to_message()
    }

    #[test]
    fn only_the_open_menus_answer_is_taken_once() {
        let mut menu = PaneMenu {
            open: Some((5, "pane-a".into())),
        };
        assert!(menu.answer(&pick(4)).is_none());
        assert_eq!(
            menu.answer(&pick(5)),
            Some(("pane-a".into(), DeskPick::Action(COPY.into())))
        );
        assert!(menu.answer(&pick(5)).is_none());
    }
}
