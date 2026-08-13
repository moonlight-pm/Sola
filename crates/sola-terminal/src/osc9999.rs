//! OSC `9999` agent-status side channel.
//!
//! Agents may emit `\x1b]9999;{json}\x07` (or ST-terminated). The sequence
//! must be stripped from the PTY byte stream so it never lands in the grid.
//! Status is **never** inferred from OSC 0/2 titles.

use std::sync::{Mutex, OnceLock, mpsc};

use iced::Subscription;
use iced::futures::Stream;
use serde::Deserialize;

const PREFIX: &[u8] = b"\x1b]9999;";
const MAX_PENDING: usize = 64 * 1024;

/// Parsed OSC 9999 payload. Unknown fields are ignored.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct OscStatus {
    pub state: OscState,
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default, rename = "agentType", alias = "agent_type")]
    pub agent_type: Option<String>,
    #[serde(default, rename = "toolName", alias = "tool_name")]
    pub tool_name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OscState {
    Working,
    Waiting,
    Done,
    #[serde(other)]
    Idle,
}

/// Incremental stripper for one PTY reader.
#[derive(Default)]
pub struct OscScanner {
    pending: Vec<u8>,
}

impl OscScanner {
    pub fn new() -> Self {
        Self::default()
    }

    /// Return displayable bytes plus any complete status payloads.
    pub fn feed(&mut self, chunk: &[u8]) -> (Vec<u8>, Vec<OscStatus>) {
        let mut combined = std::mem::take(&mut self.pending);
        combined.extend_from_slice(chunk);
        let mut clean = Vec::with_capacity(combined.len());
        let mut payloads = Vec::new();
        let mut cursor = 0usize;

        while cursor < combined.len() {
            let Some(rel) = find_subslice(&combined[cursor..], PREFIX) else {
                let tail = &combined[cursor..];
                let keep = partial_prefix_len(tail);
                clean.extend_from_slice(&tail[..tail.len() - keep]);
                self.pending = tail[tail.len() - keep..].to_vec();
                break;
            };
            clean.extend_from_slice(&combined[cursor..cursor + rel]);
            let payload_at = cursor + rel + PREFIX.len();
            match find_terminator(&combined[payload_at..]) {
                None => {
                    let rest = &combined[cursor + rel..];
                    if rest.len() > MAX_PENDING {
                        self.pending.clear();
                    } else {
                        self.pending = rest.to_vec();
                    }
                    break;
                }
                Some((term_rel, term_len)) => {
                    let raw = &combined[payload_at..payload_at + term_rel];
                    if let Ok(text) = std::str::from_utf8(raw) {
                        if let Ok(status) = serde_json::from_str::<OscStatus>(text) {
                            payloads.push(status);
                        }
                    }
                    cursor = payload_at + term_rel + term_len;
                }
            }
        }

        (clean, payloads)
    }
}

fn find_subslice(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn find_terminator(hay: &[u8]) -> Option<(usize, usize)> {
    let bel = hay.iter().position(|&b| b == 0x07);
    let st = find_subslice(hay, b"\x1b\\");
    match (bel, st) {
        (None, None) => None,
        (Some(b), None) => Some((b, 1)),
        (None, Some(s)) => Some((s, 2)),
        (Some(b), Some(s)) if b <= s => Some((b, 1)),
        (Some(_), Some(s)) => Some((s, 2)),
    }
}

fn partial_prefix_len(tail: &[u8]) -> usize {
    let max = PREFIX.len().saturating_sub(1).min(tail.len());
    for k in (1..=max).rev() {
        if tail.ends_with(&PREFIX[..k]) {
            return k;
        }
    }
    0
}

// ── iced channel (taken once, like title_subscription) ──────────────────────

static OSC_TX: OnceLock<mpsc::Sender<(String, OscStatus)>> = OnceLock::new();
static OSC_RX: Mutex<Option<mpsc::Receiver<(String, OscStatus)>>> = Mutex::new(None);

fn ensure_channel() {
    OSC_TX.get_or_init(|| {
        let (tx, rx) = mpsc::channel();
        *OSC_RX.lock().unwrap() = Some(rx);
        tx
    });
}

pub fn try_sender() -> Option<mpsc::Sender<(String, OscStatus)>> {
    OSC_TX.get().cloned()
}

/// Create the channel if a subscriber will take it.
pub fn sender() -> mpsc::Sender<(String, OscStatus)> {
    ensure_channel();
    OSC_TX.get().unwrap().clone()
}

pub fn subscription() -> Subscription<(String, OscStatus)> {
    Subscription::run(osc_stream)
}

fn osc_stream() -> impl Stream<Item = (String, OscStatus)> {
    ensure_channel();
    let rx_opt = OSC_RX.lock().unwrap().take();
    let (iced_tx, iced_rx) = iced::futures::channel::mpsc::unbounded();
    match rx_opt {
        Some(std_rx) => {
            std::thread::spawn(move || {
                while !iced_tx.is_closed() {
                    match std_rx.recv() {
                        Ok(pair) => {
                            if iced_tx.unbounded_send(pair).is_err() {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
            });
        }
        None => drop(iced_tx),
    }
    iced_rx
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed_all(chunks: &[&[u8]]) -> (Vec<u8>, Vec<OscStatus>) {
        let mut s = OscScanner::new();
        let mut clean = Vec::new();
        let mut payloads = Vec::new();
        for c in chunks {
            let (d, p) = s.feed(c);
            clean.extend(d);
            payloads.extend(p);
        }
        (clean, payloads)
    }

    #[test]
    fn strips_bel_payload_and_keeps_surrounding() {
        let raw = b"hello\x1b]9999;{\"state\":\"working\"}\x07world";
        let (clean, payloads) = feed_all(&[raw]);
        assert_eq!(clean, b"helloworld");
        assert_eq!(payloads.len(), 1);
        assert_eq!(payloads[0].state, OscState::Working);
    }

    #[test]
    fn strips_st_terminator() {
        let raw = b"\x1b]9999;{\"state\":\"done\"}\x1b\\";
        let (clean, payloads) = feed_all(&[raw]);
        assert!(clean.is_empty());
        assert_eq!(payloads[0].state, OscState::Done);
    }

    #[test]
    fn split_across_reads() {
        let a = b"pre\x1b]9999;{\"sta";
        let b = b"te\":\"waiting\"}\x07post";
        let (clean, payloads) = feed_all(&[a, b]);
        assert_eq!(clean, b"prepost");
        assert_eq!(payloads[0].state, OscState::Waiting);
    }

    #[test]
    fn never_treats_osc_title_as_status() {
        let raw = b"\x1b]0;grok working\x07\x1b]2;title\x07plain";
        let (clean, payloads) = feed_all(&[raw]);
        assert!(payloads.is_empty());
        assert!(clean.windows(4).any(|w| w == b"\x1b]0;"));
        assert!(clean.ends_with(b"plain"));
    }

    #[test]
    fn ignores_malformed_json() {
        let raw = b"\x1b]9999;not-json\x07ok";
        let (clean, payloads) = feed_all(&[raw]);
        assert_eq!(clean, b"ok");
        assert!(payloads.is_empty());
    }
}
