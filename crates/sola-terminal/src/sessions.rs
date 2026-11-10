//! The terminal's own record of its tabs, kept in its state file: what the
//! next run restores. Nothing else reads it, so it is not on the bus.

use sola_bus::topics::TerminalSession;
use sola_core::state_file::StateFile;

pub struct Sessions {
    file: StateFile,
    records: Vec<TerminalSession>,
}

/// This run's records: the file's, else what the bus kept before the move.
pub fn first_of(
    file: Option<Vec<TerminalSession>>,
    legacy: Option<Vec<TerminalSession>>,
) -> Vec<TerminalSession> {
    file.or(legacy).unwrap_or_default()
}

impl Sessions {
    pub fn load() -> Self {
        let file = StateFile::new("terminal", "sessions.json");
        let records = first_of(file.load(), sola_bus::state::legacy_section("TerminalSession"));
        Self { file, records }
    }

    /// The records as saved, in tab order.
    pub fn all(&self) -> Vec<TerminalSession> {
        self.records.clone()
    }

    /// Set the record for `s.id` and save.
    pub fn put(&mut self, s: TerminalSession) {
        match self.records.iter_mut().find(|r| r.id == s.id) {
            Some(r) if *r == s => return,
            Some(r) => *r = s,
            None => self.records.push(s),
        }
        self.file.save(&self.records);
    }

    /// Drop the record for `id` and save.
    pub fn remove(&mut self, id: &str) {
        let before = self.records.len();
        self.records.retain(|r| r.id != id);
        if self.records.len() != before {
            self.file.save(&self.records);
        }
    }

    #[cfg(test)]
    fn at(path: std::path::PathBuf) -> Self {
        let file = StateFile::at(path);
        let records = file.load().unwrap_or_default();
        Self { file, records }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(id: &str, ordinal: u32) -> TerminalSession {
        TerminalSession {
            id: id.into(),
            tmux_session: format!("sola-{id}"),
            cwd: None,
            ordinal,
            layout: None,
        }
    }

    #[test]
    fn saves_replaces_and_removes() {
        let dir = std::env::temp_dir().join(format!("sola-terminal-sessions-{}", std::process::id()));
        let path = dir.join("sessions.json");
        let _ = std::fs::remove_dir_all(&dir);

        let mut s = Sessions::at(path.clone());
        s.put(rec("a", 1));
        s.put(rec("b", 2));
        s.put(TerminalSession { cwd: Some("/x".into()), ..rec("a", 1) });
        assert_eq!(Sessions::at(path.clone()).all().len(), 2);
        assert_eq!(Sessions::at(path.clone()).all()[0].cwd.as_deref(), Some("/x"));

        s.remove("a");
        assert_eq!(Sessions::at(path.clone()).all(), vec![rec("b", 2)]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_file_wins_over_the_legacy_copy() {
        assert_eq!(first_of(Some(vec![rec("a", 1)]), Some(vec![rec("b", 2)])), vec![rec("a", 1)]);
        assert_eq!(first_of(None, Some(vec![rec("b", 2)])), vec![rec("b", 2)]);
        assert_eq!(first_of(Some(vec![]), Some(vec![rec("b", 2)])), vec![]);
        assert!(first_of(None, None).is_empty());
    }
}
