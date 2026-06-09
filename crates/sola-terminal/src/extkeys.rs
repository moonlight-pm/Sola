//! xterm **modifyOtherKeys** (XTMODKEYS) state, tracked from the PTY output
//! stream so the input encoder can emit CSI-u sequences for modified keys.
//!
//! # Why this exists
//!
//! Inside this terminal, applications run under **tmux**. tmux does not pass
//! the kitty keyboard protocol through to inner apps; instead, when an inner
//! app (e.g. Claude Code) requests extended keys, tmux enables *its own*
//! extended-keys negotiation with the OUTER terminal (us) using modifyOtherKeys
//! — `CSI > 4 ; 2 m` to enable, `CSI > 4 m` to disable (tmux's `Eneks`/`Dseks`
//! capabilities, gated on the `extkeys` terminal-feature). Once enabled, tmux
//! expects us to report modified keys such as Shift+Enter as `CSI 13 ; 2 u`,
//! which it then forwards to the inner app. That is how Shift+Enter becomes
//! distinct from Enter through tmux.
//!
//! # Why scan the byte stream
//!
//! The `vte` parser *does* decode `CSI > 4 ; Pv m` and calls
//! `Handler::set_modify_other_keys`, but `alacritty_terminal::Term` leaves that
//! hook at its default no-op — the engine exposes no `TermMode` bit for it. So
//! we watch the same bytes the parser sees and record the level in a
//! process-wide registry keyed by tab id; [`crate::input`] consults it when
//! encoding a key press.
//!
//! Levels: `0` = off, `1` = enable-except-well-defined, `2` = enable-all. Any
//! non-zero level turns on CSI-u encoding of modified keys.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

// ── Process-wide per-tab level registry ─────────────────────────────────────

static REGISTRY: OnceLock<Mutex<HashMap<String, u8>>> = OnceLock::new();

fn registry() -> &'static Mutex<HashMap<String, u8>> {
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Record the modifyOtherKeys level for `tab_id` (called from the reader thread
/// as the scanner observes XTMODKEYS sequences).
pub fn set_level(tab_id: &str, level: u8) {
    registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(tab_id.to_string(), level);
}

/// Current modifyOtherKeys level for `tab_id` (`0` if unknown/disabled).
pub fn level(tab_id: &str) -> u8 {
    registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(tab_id)
        .copied()
        .unwrap_or(0)
}

/// Drop a tab's entry (on tab close) so a reused id can't inherit stale state.
pub fn clear(tab_id: &str) {
    registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(tab_id);
}

// ── Incremental XTMODKEYS scanner ───────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// Scanning for ESC.
    Ground,
    /// Saw ESC, expecting `[`.
    Esc,
    /// Saw `ESC [`, inspecting the next byte.
    CsiStart,
    /// Inside a CSI that isn't `CSI > … m`; consume to the final byte.
    OtherCsi,
    /// Saw `ESC [ >`, collecting parameter bytes until the final byte.
    Params,
}

/// Stream scanner that recognises `CSI > 4 ; Pv m` (and `CSI > 4 m`) across
/// arbitrarily-split byte chunks, ignoring all other escape sequences. State
/// persists between [`feed`](Self::feed) calls so a sequence straddling two PTY
/// reads is still matched.
pub struct Scanner {
    state: State,
    params: Vec<u8>,
}

impl Scanner {
    pub fn new() -> Self {
        Self {
            state: State::Ground,
            params: Vec::new(),
        }
    }

    /// Feed a chunk of PTY output. Returns the new modifyOtherKeys level if a
    /// complete `CSI > 4 ; Pv m` (or `CSI > 4 m`) sequence completed in this
    /// chunk — the *last* one, if several. Returns `None` otherwise.
    pub fn feed(&mut self, bytes: &[u8]) -> Option<u8> {
        let mut latest = None;
        for &b in bytes {
            // ESC always restarts sequence recognition, from any state.
            if b == 0x1b {
                self.state = State::Esc;
                self.params.clear();
                continue;
            }
            match self.state {
                State::Ground => {}
                State::Esc => {
                    self.state = if b == b'[' { State::CsiStart } else { State::Ground };
                }
                State::CsiStart => {
                    self.state = if b == b'>' {
                        self.params.clear();
                        State::Params
                    } else if is_final(b) {
                        State::Ground
                    } else {
                        State::OtherCsi
                    };
                }
                State::OtherCsi => {
                    if is_final(b) {
                        self.state = State::Ground;
                    }
                }
                State::Params => {
                    if b.is_ascii_digit() || b == b';' {
                        self.params.push(b);
                    } else if is_final(b) {
                        if b == b'm' {
                            if let Some(level) = parse_xtmodkeys(&self.params) {
                                latest = Some(level);
                            }
                        }
                        self.state = State::Ground;
                        self.params.clear();
                    } else {
                        // Unexpected byte inside the parameter run — abandon.
                        self.state = State::Ground;
                        self.params.clear();
                    }
                }
            }
        }
        latest
    }
}

impl Default for Scanner {
    fn default() -> Self {
        Self::new()
    }
}

/// A CSI final byte is in the range `0x40..=0x7e` (`@`..`~`).
fn is_final(b: u8) -> bool {
    (0x40..=0x7e).contains(&b)
}

/// Parse the parameter bytes between `>` and `m`. Returns the level only for
/// `Pp == 4` (modifyOtherKeys) with `Pv` in `0..=2`; the missing `Pv` in
/// `CSI > 4 m` is treated as `0` (disable), matching xterm/vte.
fn parse_xtmodkeys(params: &[u8]) -> Option<u8> {
    let text = std::str::from_utf8(params).ok()?;
    let mut parts = text.split(';');
    let pp: u8 = parts.next().unwrap_or("").parse().ok()?;
    if pp != 4 {
        return None;
    }
    let pv: u8 = match parts.next() {
        None | Some("") => 0,
        Some(s) => s.parse().ok()?,
    };
    (pv <= 2).then_some(pv)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn level_of(seq: &[u8]) -> Option<u8> {
        Scanner::new().feed(seq)
    }

    #[test]
    fn enable_all_is_level_2() {
        assert_eq!(level_of(b"\x1b[>4;2m"), Some(2));
    }

    #[test]
    fn enable_except_well_defined_is_level_1() {
        assert_eq!(level_of(b"\x1b[>4;1m"), Some(1));
    }

    #[test]
    fn bare_disable_is_level_0() {
        // tmux's `Dseks` capability.
        assert_eq!(level_of(b"\x1b[>4m"), Some(0));
    }

    #[test]
    fn explicit_reset_is_level_0() {
        assert_eq!(level_of(b"\x1b[>4;0m"), Some(0));
    }

    #[test]
    fn non_modifyotherkeys_private_m_ignored() {
        // `CSI > 0 m` etc. is not modifyOtherKeys.
        assert_eq!(level_of(b"\x1b[>0m"), None);
    }

    #[test]
    fn kitty_push_csi_gt_u_ignored() {
        // `CSI > 1 u` (kitty) must not be mistaken for XTMODKEYS.
        assert_eq!(level_of(b"\x1b[>1u"), None);
    }

    #[test]
    fn ordinary_sgr_ignored() {
        // `CSI 4 m` (underline) has no `>` and must not match.
        assert_eq!(level_of(b"\x1b[4m"), None);
    }

    #[test]
    fn private_mode_set_ignored() {
        // `CSI ? 2004 h` (bracketed paste) shares the private-marker shape.
        assert_eq!(level_of(b"\x1b[?2004h"), None);
    }

    #[test]
    fn embedded_in_surrounding_output() {
        let mut s = Scanner::new();
        assert_eq!(s.feed(b"hello\x1b[1mworld\x1b[>4;2mtail"), Some(2));
    }

    #[test]
    fn split_across_chunks() {
        let mut s = Scanner::new();
        assert_eq!(s.feed(b"\x1b[>4"), None);
        assert_eq!(s.feed(b";2m"), Some(2));
    }

    #[test]
    fn split_mid_escape() {
        let mut s = Scanner::new();
        assert_eq!(s.feed(b"\x1b"), None);
        assert_eq!(s.feed(b"[>"), None);
        assert_eq!(s.feed(b"4;1"), None);
        assert_eq!(s.feed(b"m"), Some(1));
    }

    #[test]
    fn last_of_several_wins() {
        // Enable then disable in one chunk → net disabled.
        let mut s = Scanner::new();
        assert_eq!(s.feed(b"\x1b[>4;2m\x1b[>4m"), Some(0));
    }

    #[test]
    fn esc_resyncs_mid_other_csi() {
        // An unrelated CSI interrupted by a fresh ESC should still match.
        let mut s = Scanner::new();
        assert_eq!(s.feed(b"\x1b[38;5;2\x1b[>4;2m"), Some(2));
    }

    #[test]
    fn registry_roundtrip() {
        set_level("tab-xyz", 2);
        assert_eq!(level("tab-xyz"), 2);
        set_level("tab-xyz", 0);
        assert_eq!(level("tab-xyz"), 0);
        clear("tab-xyz");
        assert_eq!(level("tab-xyz"), 0);
    }
}
