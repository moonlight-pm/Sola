use std::collections::HashSet;

/// Whether a bus-delivered TerminalSession should be admitted or retracted.
#[derive(Debug, PartialEq, Eq)]
pub enum Admit {
    Yes,
    Retract,
}

/// Decide whether to admit or retract a TerminalSession during initial
/// sticky replay. `None` (tmux state unknown) admits everything — never
/// nuke tabs on a transient tmux glitch.
pub fn reconcile_admit(live_tmux: &Option<HashSet<String>>, tmux_session: &str) -> Admit {
    match live_tmux {
        Some(set) if !set.contains(tmux_session) => Admit::Retract,
        _ => Admit::Yes,
    }
}

/// Whether a delivered sticky TerminalSession should be boot-reconciled.
/// We only reconcile sessions we're seeing for the first time (boot replay);
/// a session we already track is our own echo / a re-emit and must never be
/// retracted against the stale boot snapshot.
pub fn admit_session(
    was_present: bool,
    live_tmux: &Option<HashSet<String>>,
    tmux_session: &str,
) -> Admit {
    if was_present {
        return Admit::Yes;
    }
    reconcile_admit(live_tmux, tmux_session)
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- reconcile_admit (existing) ---

    #[test]
    fn admits_tab_when_tmux_alive() {
        let live = Some(HashSet::from(["sola-a".to_string()]));
        assert_eq!(reconcile_admit(&live, "sola-a"), Admit::Yes);
    }

    #[test]
    fn retracts_tab_when_tmux_gone() {
        let live: Option<HashSet<String>> = Some(HashSet::new());
        assert_eq!(reconcile_admit(&live, "sola-a"), Admit::Retract);
    }

    #[test]
    fn admits_everything_when_tmux_unknown() {
        assert_eq!(reconcile_admit(&None, "sola-a"), Admit::Yes);
    }

    // --- admit_session (new) ---

    /// The bug case: a tab we already track (was_present=true) must NEVER be
    /// retracted, even if its tmux session name is absent from the boot snapshot.
    #[test]
    fn tracked_tab_always_admitted_even_when_tmux_gone() {
        let live: Option<HashSet<String>> = Some(HashSet::new()); // tmux "gone"
        assert_eq!(admit_session(true, &live, "sola-new"), Admit::Yes);
    }

    /// Boot cleanup still works: an unseen session whose tmux is gone retracts.
    #[test]
    fn unseen_tab_retracted_when_tmux_gone() {
        let live: Option<HashSet<String>> = Some(HashSet::new());
        assert_eq!(admit_session(false, &live, "sola-a"), Admit::Retract);
    }

    /// A live persisted session on boot is admitted.
    #[test]
    fn unseen_tab_admitted_when_tmux_alive() {
        let live = Some(HashSet::from(["sola-a".to_string()]));
        assert_eq!(admit_session(false, &live, "sola-a"), Admit::Yes);
    }

    /// When tmux state is unknown (None), admit everything — never nuke on glitch.
    #[test]
    fn unseen_tab_admitted_when_tmux_unknown() {
        assert_eq!(admit_session(false, &None, "sola-a"), Admit::Yes);
    }
}
