//! The agent islands' card: opening it (the reply field gets the keyboard)
//! and what you pick, sent back to Projects (`Topic::AgentIslandAction`).

use sola_bus::topics::{AgentIslandAction, IdleKeep, IslandAction, Topic};

use super::{Msg, Panel, Shell, with_bus};

impl Shell {
    /// An agent island was clicked: grow its card, or close it when it
    /// shows. A card with a reply field takes the keyboard.
    pub(crate) fn open_agent_island(&mut self, key: String) -> iced::Task<Msg> {
        let showing = self.menu_open
            && self.open_panel == Some(Panel::AgentIsland)
            && self.agents.open.as_deref() == Some(key.as_str());
        let _ = self.dismiss_open_menu();
        if !showing {
            self.agents.open = Some(key);
            self.agents.reply.clear();
            self.menu_open = true;
            self.set_open_panel(Some(Panel::AgentIsland));
        }
        self.emit_overlay_frames();
        self.emit_composition();
        self.emit_registered_chords();
        let reply = self.agents.opened().is_some_and(|i| i.reply);
        if showing || !reply {
            return iced::Task::none();
        }
        if let Some(wid) = self.lookup_window_id(Self::APP_ID, "menu") {
            with_bus(|bus| {
                let _ = bus.emit(Topic::Focus(sola_bus::topics::FocusTarget { window_id: wid }));
            });
        }
        iced::widget::operation::focus::<Msg>(iced::widget::Id::new(
            crate::agents::view::REPLY_INPUT_ID,
        ))
    }

    /// Send what was picked on the open card, and close it. Projects types
    /// it into the session (or pulls its tab up) and takes the island down.
    pub(crate) fn agent_island_act(&mut self, action: IslandAction) -> iced::Task<Msg> {
        let Some(key) = self.agents.open.take() else {
            return iced::Task::none();
        };
        tracing::info!(%key, ?action, "agent island");
        with_bus(|bus| {
            let _ = bus.emit(Topic::AgentIslandAction(AgentIslandAction { key, action }));
        });
        let _ = self.dismiss_open_menu();
        self.emit_overlay_frames();
        self.emit_composition();
        self.emit_registered_chords();
        iced::Task::none()
    }

    /// Tell sola-river what its idle fade keeps in sight: the top row's
    /// left group (the flower and the agent islands, what you may want to
    /// answer), the row's full height, a gap past its end
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

    /// `Topic::AgentIsland`: fold it; a card whose island went closes.
    pub(crate) fn on_agent_island(&mut self, message: &sola_bus::Message) {
        if !self.agents.update(message) {
            return;
        }
        self.emit_idle_keep(false);
        let card_gone = self.open_panel == Some(Panel::AgentIsland) && self.agents.opened().is_none();
        let list_empty = self.open_panel == Some(Panel::AgentIslands)
            && self.agents.islands.is_empty();
        if self.menu_open && (card_gone || list_empty) {
            let _ = self.dismiss_open_menu();
            self.emit_overlay_frames();
            self.emit_composition();
            self.emit_registered_chords();
        }
    }
}
