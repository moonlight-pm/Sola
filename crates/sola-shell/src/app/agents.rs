//! What you do with an agent island, sent back to Projects
//! (`Topic::AgentIslandAction`), and the islands changing.

use sola_bus::topics::{AgentIslandAction, IdleKeep, IslandAction, Topic};

use super::{Msg, Panel, Shell, with_bus};

impl Shell {
    /// An island was clicked or closed: Projects brings the session's window
    /// up or lets it go, and takes the island down either way. Going to a
    /// window closes what is open; closing an island leaves it (the +N list
    /// for the next one).
    pub(crate) fn agent_island_act(&mut self, key: String, action: IslandAction) -> iced::Task<Msg> {
        tracing::info!(%key, ?action, "agent island");
        let show = action == IslandAction::Show;
        with_bus(|bus| {
            let _ = bus.emit(Topic::AgentIslandAction(AgentIslandAction { key, action }));
        });
        if show && self.dismiss_open_menu() {
            self.emit_overlay_frames();
            self.emit_composition();
            self.emit_registered_chords();
        }
        iced::Task::none()
    }

    /// Tell sola-river what its idle fade keeps in sight: the top row's
    /// left group (the flower and the agent islands, what you may want to
    /// get back to), the row's full height, a gap past its end
    /// (`Topic::IdleKeep`). Sent when it moves; `again` sends it anyway
    /// (after a bus or sola-river restart).
    pub(crate) fn emit_idle_keep(&mut self, again: bool) {
        let end = crate::agents::view::left_group_end(self) + crate::menubar::ISLAND_GAP;
        let keep = IdleKeep {
            x: 0,
            y: 0,
            w: end.ceil() as u32,
            h: crate::menubar::WINDOW_HEIGHT,
        };
        if !again && self.agents.keep_sent == Some(keep) {
            return;
        }
        self.agents.keep_sent = Some(keep);
        with_bus(|bus| {
            let _ = bus.emit(Topic::IdleKeep(keep));
        });
    }

    /// `Topic::AgentIsland`: fold it; an open +N list follows, and goes
    /// with its last row.
    pub(crate) fn on_agent_island(&mut self, message: &sola_bus::Message) {
        if !self.agents.update(message) {
            return;
        }
        self.emit_idle_keep(false);
        if !self.menu_open || self.open_panel != Some(Panel::AgentIslands) {
            return;
        }
        if crate::agents::view::more(self) == 0 {
            let _ = self.dismiss_open_menu();
            self.emit_composition();
            self.emit_registered_chords();
        }
        self.emit_overlay_frames();
    }
}
