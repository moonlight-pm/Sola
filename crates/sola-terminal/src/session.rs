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

#[cfg(test)]
mod tests {
    use super::*;

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
}
