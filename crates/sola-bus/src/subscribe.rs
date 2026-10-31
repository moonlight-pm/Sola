//! The subscribe frames. A client names the kinds it wants
//! (`$subscribe-names`, a postcard list of [`TopicKind::as_str`] names),
//! so a bus and a client from different builds agree on them however
//! `define_topics!` numbers the kinds. For a bus from before names it also
//! sends the numbered `$subscribe`, in the numbering frozen in
//! [`NUMBERED`]. A bus ignores the numbered frames of a connection that
//! has sent names.

use std::io;

use crate::Message;
use crate::topic::{decode_payload, encode_payload};
use crate::topics::TopicKind;

/// The kinds by number as the last build before names numbered them
/// (`next` 83df4558, the Sola in Oath's desk output): a numbered
/// `$subscribe` carries indices into this list. Frozen: never reorder or
/// drop a name, and add none. A kind removed since stays, and decodes to
/// nothing (`CustomTheme`).
pub const NUMBERED: &[&str] = &[
    "MouseLeft", "Shutdown", "LaunchApp", "LaunchResult", "UserAppExited",
    "CloseApp", "ClientConnected", "ClientDisconnected", "Composition", "Frame",
    "Focus", "MouseEntered", "MouseClicked", "Chord", "ChordReleased",
    "MenuAction", "OpenUrl", "OpenImage", "AppToast", "AppNotification",
    "NotificationActivate", "ShowTab", "CloseWindow", "CloseTab",
    "AgentIslandAction", "ShowWindow", "Windows", "AppHidden", "OutputGeometry",
    "WindowGeometry", "WindowFloating", "RegisteredChords", "SetAppMenu",
    "MailStatus", "WindowParent", "Tab", "TabWindow", "TabActivity",
    "WindowChrome", "Workspaces", "WindowWorkspace", "AgentIsland",
    "ThemeVariations", "IdleKeep", "IdleFade", "Zones", "FloatGeometry",
    "SessionApps", "MailConfig", "Theme", "CustomTheme", "Application",
    "TerminalConfig", "MonitorConfig", "TerminalSession", "BrowserConfig",
    "BrowserHistory", "BrowserTab", "PaintSession", "CalendarConfig",
    "TabLayout", "Background", "Appearance", "CalendarAgenda",
];

/// The frames that subscribe to `kinds`: the names, then the numbered set
/// (only the kinds [`NUMBERED`] has).
pub fn frames(kinds: &[TopicKind]) -> [Message; 2] {
    let names: Vec<&str> = kinds.iter().map(|k| k.as_str()).collect();
    let numbered: Vec<u32> = kinds
        .iter()
        .filter_map(|k| NUMBERED.iter().position(|n| *n == k.as_str()))
        .map(|i| i as u32)
        .collect();
    [
        Message::with_payload(crate::CONTROL_SUBSCRIBE_NAMES, encode_payload(&names)),
        Message::with_payload(crate::CONTROL_SUBSCRIBE, encode_payload(&numbered)),
    ]
}

/// A `$subscribe-names` frame. Names this build does not know (a newer
/// client's kinds) are skipped.
pub fn decode_names(msg: &Message) -> io::Result<Vec<TopicKind>> {
    let names: Vec<String> = decode_payload(msg)?;
    let kinds: Vec<TopicKind> = names.iter().filter_map(|n| TopicKind::from_str(n)).collect();
    if kinds.len() < names.len() {
        tracing::warn!(
            unknown = names.len() - kinds.len(),
            kept = kinds.len(),
            "subscribe: skipped unknown topic names"
        );
    }
    Ok(kinds)
}

/// A numbered `$subscribe` frame, read in [`NUMBERED`]. Numbers past it
/// and kinds removed since are skipped; a frame with nothing usable is an
/// error, so the connection keeps its previous set.
pub fn decode_numbered(msg: &Message) -> io::Result<Vec<TopicKind>> {
    let numbers: Vec<u32> = decode_payload(msg)?;
    let kinds: Vec<TopicKind> = numbers
        .iter()
        .filter_map(|&d| NUMBERED.get(d as usize))
        .filter_map(|n| TopicKind::from_str(n))
        .collect();
    if kinds.len() < numbers.len() {
        tracing::warn!(
            unknown = numbers.len() - kinds.len(),
            kept = kinds.len(),
            "subscribe: skipped unknown topic numbers"
        );
    }
    if kinds.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "subscribe: no known topic"));
    }
    Ok(kinds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbered_is_this_build_with_custom_theme() {
        // The frozen list differs from this build's order only by the
        // removed CustomTheme. A kind added after the freeze is missing
        // from it, and a bus from before names never sees it.
        let now: Vec<&str> = TopicKind::ALL
            .iter()
            .map(|k| k.as_str())
            .filter(|n| NUMBERED.contains(n))
            .collect();
        let frozen: Vec<&str> = NUMBERED.iter().copied().filter(|n| *n != "CustomTheme").collect();
        assert_eq!(frozen, now);
        assert_eq!(NUMBERED.iter().position(|n| *n == "CustomTheme"), Some(50));
    }

    #[test]
    fn names_round_trip() {
        let kinds = [TopicKind::Theme, TopicKind::TabLayout, TopicKind::Application];
        let [names, _] = frames(&kinds);
        assert_eq!(names.topic, crate::CONTROL_SUBSCRIBE_NAMES);
        assert_eq!(decode_names(&names).unwrap(), kinds);
    }

    #[test]
    fn numbered_uses_the_frozen_numbers() {
        let kinds = [TopicKind::Theme, TopicKind::Application, TopicKind::TabLayout];
        let [_, numbered] = frames(&kinds);
        assert_eq!(numbered.topic, crate::CONTROL_SUBSCRIBE);
        let numbers: Vec<u32> = decode_payload(&numbered).unwrap();
        assert_eq!(numbers, vec![49, 51, 60]);
        assert_eq!(decode_numbered(&numbered).unwrap(), kinds);
    }

    #[test]
    fn old_build_numbers_read_as_it_meant_them() {
        // A shell from 83df4558 asking for TabLayout and Application.
        let msg = Message::with_payload(crate::CONTROL_SUBSCRIBE, encode_payload(&vec![60u32, 51]));
        assert_eq!(decode_numbered(&msg).unwrap(), vec![TopicKind::TabLayout, TopicKind::Application]);
    }

    #[test]
    fn unknown_entries_are_skipped() {
        let msg = Message::with_payload(crate::CONTROL_SUBSCRIBE, encode_payload(&vec![50u32, 999, 0]));
        assert_eq!(decode_numbered(&msg).unwrap(), vec![TopicKind::MouseLeft]);
        let only_removed = Message::with_payload(crate::CONTROL_SUBSCRIBE, encode_payload(&vec![50u32]));
        assert!(decode_numbered(&only_removed).is_err());
        let names = Message::with_payload(
            crate::CONTROL_SUBSCRIBE_NAMES,
            encode_payload(&vec!["Theme".to_string(), "FromTheFuture".to_string()]),
        );
        assert_eq!(decode_names(&names).unwrap(), vec![TopicKind::Theme]);
    }
}
