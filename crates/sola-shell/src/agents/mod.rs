//! Agent islands in the top row (`docs/specs/2026-10-23-agent-islands-design.md`):
//! one island per Projects session that asks or has finished
//! (`Topic::AgentIsland`, sticky per session), left after the flower.
//! Clicking one grows its card under it ([`view::card`]): the ask and its
//! choices, or the last message and a reply; Show pulls the session's tab
//! up. What you pick goes back as `Topic::AgentIslandAction`, and Projects
//! types it into the session.

pub mod view;

use sola_bus::Message;
use sola_bus::topics::{AgentIsland, Topic};

/// The islands and the card open.
#[derive(Default)]
pub struct AgentIslands {
    /// Newest first.
    pub islands: Vec<AgentIsland>,
    /// The island whose card shows.
    pub open: Option<String>,
    /// The card's reply field.
    pub reply: String,
}

impl AgentIslands {
    /// Fold `Topic::AgentIsland` (sticky: add or replace; retracted:
    /// remove). Returns true when the islands changed.
    pub fn update(&mut self, msg: &Message) -> bool {
        let Some(Topic::AgentIsland(island)) = Topic::parse(msg) else {
            return false;
        };
        self.islands.retain(|i| i.key != island.key);
        if msg.sticky {
            self.islands.push(island);
            self.islands.sort_by(|a, b| b.since.cmp(&a.since));
        } else if self.open.as_deref() == Some(island.key.as_str()) {
            self.open = None;
        }
        true
    }

    pub fn get(&self, key: &str) -> Option<&AgentIsland> {
        self.islands.iter().find(|i| i.key == key)
    }

    /// The island whose card shows.
    pub fn opened(&self) -> Option<&AgentIsland> {
        self.get(self.open.as_deref()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sola_bus::topics::IslandState;

    fn island(key: &str, since: u64) -> AgentIsland {
        AgentIsland {
            key: key.into(),
            name: key.into(),
            project: "Sola".into(),
            state: IslandState::Done,
            title: "finished".into(),
            body: String::new(),
            choices: Vec::new(),
            reply: true,
            since,
        }
    }

    #[test]
    fn islands_are_newest_first_and_go_when_retracted() {
        let sticky = |i: AgentIsland| {
            let mut m = Topic::AgentIsland(i).to_message();
            m.sticky = true;
            m
        };
        let mut a = AgentIslands::default();
        assert!(a.update(&sticky(island("a", 1))));
        assert!(a.update(&sticky(island("b", 2))));
        assert_eq!(a.islands[0].key, "b");
        a.open = Some("a".into());
        let mut gone = Topic::AgentIsland(island("a", 1)).to_message();
        gone.sticky = false;
        assert!(a.update(&gone));
        assert_eq!(a.islands.len(), 1);
        assert_eq!(a.open, None);
    }
}
