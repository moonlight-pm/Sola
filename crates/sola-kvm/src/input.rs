//! Input backends that feed [`crate::server::InputEvent`]s into the session.
//!
//! - **`wayland`** — capture through the compositor ([`crate::capture`]); the
//!   default, and the only one for real use
//! - **`feed`** — line protocol on stdin (tests / remote drive)
//! - **`demo`** — scripted enter → motion → leave sequence (smoke without HID)

use std::io::{self, BufRead, BufReader};

use tracing::warn;

use crate::server::InputEvent;

/// How the server obtains HID events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InputBackendKind {
    /// Through the compositor: edge strip, keyboard, pointer lock.
    #[default]
    Wayland,
    /// Stdin line protocol.
    Feed,
    /// Self-driving smoke sequence then idle.
    Demo,
}

impl InputBackendKind {
    /// `evdev` (the old `/dev/input` capture, gone) means `wayland`, so an
    /// old service line keeps working.
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "wayland" | "compositor" | "evdev" | "input" => Some(Self::Wayland),
            "feed" | "stdin" => Some(Self::Feed),
            "demo" => Some(Self::Demo),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Feed protocol
// ---------------------------------------------------------------------------

/// Parse one feed line into an event.
///
/// Lines (whitespace-separated):
/// - `rel <dx> <dy>`
/// - `abs <x> <y>`
/// - `btn <button> <0|1>`
/// - `key <keycode> <0|1|2>`  (`2` = auto-repeat, matching Linux `EV_KEY`)
/// - `scroll <dx> <dy>`
/// - `leave`
/// - `#` comments and blank lines → `None`
pub fn parse_feed_line(line: &str) -> Result<Option<InputEvent>, String> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return Ok(None);
    }
    let mut parts = line.split_whitespace();
    let cmd = parts.next().ok_or_else(|| "empty".to_string())?;
    match cmd {
        "rel" => {
            let dx: f32 = next_parse(&mut parts, "dx")?;
            let dy: f32 = next_parse(&mut parts, "dy")?;
            Ok(Some(InputEvent::PointerRel { dx, dy }))
        }
        "abs" => {
            let x: i32 = next_parse(&mut parts, "x")?;
            let y: i32 = next_parse(&mut parts, "y")?;
            Ok(Some(InputEvent::PointerAbs { x, y }))
        }
        "btn" => {
            let button: u8 = next_parse(&mut parts, "button")?;
            let pressed: u8 = next_parse(&mut parts, "pressed")?;
            Ok(Some(InputEvent::Button {
                button,
                pressed: pressed != 0,
            }))
        }
        "key" => {
            let keycode: u32 = next_parse(&mut parts, "keycode")?;
            let state: u8 = next_parse(&mut parts, "pressed")?;
            Ok(Some(InputEvent::Key {
                keycode,
                pressed: state != 0,
                repeat: state == 2,
            }))
        }
        "scroll" => {
            let dx: f32 = next_parse(&mut parts, "dx")?;
            let dy: f32 = next_parse(&mut parts, "dy")?;
            Ok(Some(InputEvent::Scroll { dx, dy }))
        }
        "leave" => Ok(Some(InputEvent::ForceLeave)),
        other => Err(format!("unknown feed command: {other}")),
    }
}

fn next_parse<T: std::str::FromStr>(
    parts: &mut std::str::SplitWhitespace<'_>,
    name: &str,
) -> Result<T, String>
where
    T::Err: std::fmt::Display,
{
    let s = parts
        .next()
        .ok_or_else(|| format!("missing feed arg: {name}"))?;
    s.parse()
        .map_err(|e| format!("bad feed arg {name}={s}: {e}"))
}

/// Blocking stdin feed: yields events until EOF.
pub struct FeedSource<R: BufRead> {
    reader: R,
}

impl FeedSource<BufReader<io::Stdin>> {
    pub fn stdin() -> Self {
        Self {
            reader: BufReader::new(io::stdin()),
        }
    }
}

impl<R: BufRead> FeedSource<R> {
    pub fn new(reader: R) -> Self {
        Self { reader }
    }

    /// Next event, or `None` on EOF.
    pub fn next_event(&mut self) -> io::Result<Option<InputEvent>> {
        loop {
            let mut line = String::new();
            let n = self.reader.read_line(&mut line)?;
            if n == 0 {
                return Ok(None);
            }
            match parse_feed_line(&line) {
                Ok(Some(ev)) => return Ok(Some(ev)),
                Ok(None) => continue,
                Err(e) => {
                    warn!(%e, line = %line.trim(), "feed parse error; skipping");
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Demo sequence
// ---------------------------------------------------------------------------

/// Scripted smoke events for `server --input demo`.
pub fn demo_events() -> Vec<InputEvent> {
    vec![
        // Start near right edge (seed abs), then push through enter_push barrier.
        InputEvent::PointerAbs { x: 5119, y: 2000 },
        InputEvent::PointerRel { dx: 50.0, dy: 0.0 }, // enter (enter_push default 48)
        InputEvent::PointerRel { dx: 40.0, dy: 10.0 },
        InputEvent::PointerRel { dx: 20.0, dy: -5.0 },
        InputEvent::Button {
            button: 0,
            pressed: true,
        },
        InputEvent::Button {
            button: 0,
            pressed: false,
        },
        InputEvent::Key {
            keycode: 30, // KEY_A
            pressed: true,
            repeat: false,
        },
        InputEvent::Key {
            keycode: 30,
            pressed: false,
            repeat: false,
        },
        InputEvent::Scroll {
            dx: 0.0,
            dy: -1.0,
        },
        // Leave back toward primary (leftward off Mac left edge).
        InputEvent::PointerRel {
            dx: -200.0,
            dy: 0.0,
        },
    ]
}

/// Collapse consecutive relative-motion (and scroll) events so one poll batch
/// does not spray N near-identical UDP Motion packets. Key/button order is
/// preserved relative to motion runs.
pub fn coalesce_input_events(events: Vec<InputEvent>) -> Vec<InputEvent> {
    if events.len() <= 1 {
        return events;
    }
    let mut out: Vec<InputEvent> = Vec::with_capacity(events.len());
    let mut acc_dx = 0.0f32;
    let mut acc_dy = 0.0f32;
    let mut has_rel = false;
    let mut acc_sdx = 0.0f32;
    let mut acc_sdy = 0.0f32;
    let mut has_scroll = false;

    let flush_rel =
        |out: &mut Vec<InputEvent>, dx: &mut f32, dy: &mut f32, has: &mut bool| {
            if *has {
                out.push(InputEvent::PointerRel {
                    dx: *dx,
                    dy: *dy,
                });
                *dx = 0.0;
                *dy = 0.0;
                *has = false;
            }
        };
    let flush_scroll =
        |out: &mut Vec<InputEvent>, dx: &mut f32, dy: &mut f32, has: &mut bool| {
            if *has {
                out.push(InputEvent::Scroll {
                    dx: *dx,
                    dy: *dy,
                });
                *dx = 0.0;
                *dy = 0.0;
                *has = false;
            }
        };

    for ev in events {
        match ev {
            InputEvent::PointerRel { dx, dy } => {
                // Keep motion and scroll as separate streams but flush the
                // other when switching, so order stays intuitive.
                flush_scroll(&mut out, &mut acc_sdx, &mut acc_sdy, &mut has_scroll);
                has_rel = true;
                acc_dx += dx;
                acc_dy += dy;
            }
            InputEvent::Scroll { dx, dy } => {
                flush_rel(&mut out, &mut acc_dx, &mut acc_dy, &mut has_rel);
                has_scroll = true;
                acc_sdx += dx;
                acc_sdy += dy;
            }
            other => {
                flush_rel(&mut out, &mut acc_dx, &mut acc_dy, &mut has_rel);
                flush_scroll(&mut out, &mut acc_sdx, &mut acc_sdy, &mut has_scroll);
                out.push(other);
            }
        }
    }
    flush_rel(&mut out, &mut acc_dx, &mut acc_dy, &mut has_rel);
    flush_scroll(&mut out, &mut acc_sdx, &mut acc_sdy, &mut has_scroll);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_rel() {
        let ev = parse_feed_line("rel 1.5 -2").unwrap().unwrap();
        assert_eq!(
            ev,
            InputEvent::PointerRel {
                dx: 1.5,
                dy: -2.0
            }
        );
    }

    #[test]
    fn parse_key_and_leave() {
        assert!(matches!(
            parse_feed_line("key 30 1").unwrap(),
            Some(InputEvent::Key {
                keycode: 30,
                pressed: true,
                repeat: false,
            })
        ));
        assert!(matches!(
            parse_feed_line("key 30 2").unwrap(),
            Some(InputEvent::Key {
                keycode: 30,
                pressed: true,
                repeat: true,
            })
        ));
        assert!(matches!(
            parse_feed_line("leave").unwrap(),
            Some(InputEvent::ForceLeave)
        ));
    }

    #[test]
    fn coalesce_sums_rel_and_keeps_keys() {
        let events = vec![
            InputEvent::PointerRel { dx: 1.0, dy: 0.0 },
            InputEvent::PointerRel { dx: 2.0, dy: 3.0 },
            InputEvent::Key {
                keycode: 30,
                pressed: true,
                repeat: false,
            },
            InputEvent::PointerRel { dx: 4.0, dy: 0.0 },
            InputEvent::PointerRel { dx: 1.0, dy: 1.0 },
        ];
        let out = coalesce_input_events(events);
        assert_eq!(
            out,
            vec![
                InputEvent::PointerRel { dx: 3.0, dy: 3.0 },
                InputEvent::Key {
                    keycode: 30,
                    pressed: true,
                    repeat: false,
                },
                InputEvent::PointerRel { dx: 5.0, dy: 1.0 },
            ]
        );
    }

    #[test]
    fn parse_comments() {
        assert!(parse_feed_line("# hi").unwrap().is_none());
        assert!(parse_feed_line("").unwrap().is_none());
    }

    #[test]
    fn demo_enters_and_leaves() {
        // Smoke: demo sequence should be non-empty and contain a force path
        // or enough leftward motion — actual enter tested in server tests.
        assert!(demo_events().len() >= 5);
    }
}
