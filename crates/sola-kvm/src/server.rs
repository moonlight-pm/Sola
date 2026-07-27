//! Pure server state machine: local ↔ remote edge enter/leave + packet emit.
//!
//! No Wayland or `/dev/input` here — those live in `input` backends that feed
//! [`InputEvent`]s. Unit tests drive the machine directly.

use std::collections::BTreeSet;

use crate::layout::Layout;
use crate::protocol::{Edge, Packet};

/// Mode of the KVM session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    /// Pointer is on the real primary output; no exclusive grab.
    Local,
    /// Pointer is over the virtual Mac rect; exclusive grab + UDP spray.
    Remote {
        /// Mac-local absolute cursor.
        mx: i32,
        my: i32,
    },
}

/// Side effects for the process layer (grab / ungrab / warp).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SideEffect {
    /// Entered remote mode — exclusive-grab pointer+keyboard; suppress Meta chords.
    Grab,
    /// Left remote mode — release grab; restore chords; warp local pointer.
    Release {
        /// Primary-space point where the local pointer should reappear.
        warp_primary: (i32, i32),
    },
}

/// Result of applying one input event.
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub packets: Vec<Packet>,
    pub effects: Vec<SideEffect>,
}

impl Step {
    fn empty() -> Self {
        Self {
            packets: Vec::new(),
            effects: Vec::new(),
        }
    }

    fn packets(packets: Vec<Packet>) -> Self {
        Self {
            packets,
            effects: Vec::new(),
        }
    }
}

/// Raw HID / synthetic events the server understands.
#[derive(Debug, Clone, PartialEq)]
pub enum InputEvent {
    /// Absolute pointer in primary space (optional seed / warp from compositor).
    PointerAbs { x: i32, y: i32 },
    /// Relative pointer motion (libinput/evdev deltas, pre-scale).
    PointerRel { dx: f32, dy: f32 },
    /// Mouse button. `button`: 0=left, 1=right, 2=middle.
    Button { button: u8, pressed: bool },
    /// Linux evdev keycode.
    Key { keycode: u32, pressed: bool },
    /// Scroll deltas.
    Scroll { dx: f32, dy: f32 },
    /// Emergency leave (config release chord or CLI).
    ForceLeave,
}

/// Pure KVM session: layout + mode + virtual cursor + pressed-key bookkeeping.
#[derive(Debug, Clone)]
pub struct Session {
    pub layout: Layout,
    pub mode: Mode,
    /// Estimated primary-space cursor while local (edge detection).
    pub local_x: i32,
    pub local_y: i32,
    /// Keys currently down (evdev codes) — released synthetically on leave.
    pressed_keys: BTreeSet<u32>,
    /// Buttons currently down — released synthetically on leave.
    pressed_buttons: BTreeSet<u8>,
}

impl Session {
    /// Start local, pointer seeded at primary center.
    pub fn new(layout: Layout) -> Self {
        let local_x = layout.primary_w / 2;
        let local_y = layout.primary_h / 2;
        Self {
            layout,
            mode: Mode::Local,
            local_x,
            local_y,
            pressed_keys: BTreeSet::new(),
            pressed_buttons: BTreeSet::new(),
        }
    }

    /// Start local at a known primary position (tests / compositor seed).
    pub fn with_local_pos(layout: Layout, x: i32, y: i32) -> Self {
        let (x, y) = layout.clamp_primary(x, y);
        Self {
            layout,
            mode: Mode::Local,
            local_x: x,
            local_y: y,
            pressed_keys: BTreeSet::new(),
            pressed_buttons: BTreeSet::new(),
        }
    }

    pub fn is_remote(&self) -> bool {
        matches!(self.mode, Mode::Remote { .. })
    }

    pub fn mac_pos(&self) -> Option<(i32, i32)> {
        match self.mode {
            Mode::Remote { mx, my } => Some((mx, my)),
            Mode::Local => None,
        }
    }

    /// Apply one event; returns packets to send and grab side-effects.
    pub fn handle(&mut self, event: InputEvent) -> Step {
        match event {
            InputEvent::PointerAbs { x, y } => self.on_pointer_abs(x, y),
            InputEvent::PointerRel { dx, dy } => self.on_pointer_rel(dx, dy),
            InputEvent::Button { button, pressed } => self.on_button(button, pressed),
            InputEvent::Key { keycode, pressed } => self.on_key(keycode, pressed),
            InputEvent::Scroll { dx, dy } => self.on_scroll(dx, dy),
            InputEvent::ForceLeave => self.leave_remote(None),
        }
    }

    fn on_pointer_abs(&mut self, x: i32, y: i32) -> Step {
        match self.mode {
            Mode::Local => {
                // If compositor reports a point already inside the virtual
                // Mac (unusual without a real second output), enter.
                if self.layout.contains_primary(x, y) {
                    let (mx, my) = self.layout.to_mac_local(x, y);
                    return self.enter_remote(mx, my);
                }
                let (cx, cy) = self.layout.clamp_primary(x, y);
                self.local_x = cx;
                self.local_y = cy;
                Step::empty()
            }
            Mode::Remote { .. } => {
                // Absolute primary while remote is ignored — remote uses rel.
                Step::empty()
            }
        }
    }

    fn on_pointer_rel(&mut self, dx: f32, dy: f32) -> Step {
        match self.mode {
            Mode::Local => {
                if let Some((mx, my)) =
                    self.layout
                        .try_enter_from_motion(self.local_x, self.local_y, dx, dy)
                {
                    return self.enter_remote(mx, my);
                }
                let (nx, ny) = self.layout.apply_local_motion(self.local_x, self.local_y, dx, dy);
                let (cx, cy) = self.layout.clamp_primary(nx, ny);
                self.local_x = cx;
                self.local_y = cy;
                Step::empty()
            }
            Mode::Remote { mx, my } => {
                let (nx, ny) = self.layout.integrate_motion(mx, my, dx, dy);
                if let Some(warp) = self.layout.leave_toward_primary(nx, ny) {
                    return self.leave_remote(Some((nx, ny, warp)));
                }
                // Clamp soft: stay on Mac if motion leaves away from primary
                // (far edges of the virtual display).
                let cx = nx.clamp(0, self.layout.mac_w.saturating_sub(1));
                let cy = ny.clamp(0, self.layout.mac_h.saturating_sub(1));
                // If we had to clamp because we left a non-return edge, park
                // on that edge rather than wrapping.
                self.mode = Mode::Remote { mx: cx, my: cy };
                Step::packets(vec![Packet::Motion { x: cx, y: cy }])
            }
        }
    }

    fn on_button(&mut self, button: u8, pressed: bool) -> Step {
        if !self.is_remote() {
            return Step::empty();
        }
        if pressed {
            self.pressed_buttons.insert(button);
        } else {
            self.pressed_buttons.remove(&button);
        }
        Step::packets(vec![Packet::Button {
            button,
            pressed: if pressed { 1 } else { 0 },
        }])
    }

    fn on_key(&mut self, keycode: u32, pressed: bool) -> Step {
        if !self.is_remote() {
            return Step::empty();
        }
        if pressed {
            self.pressed_keys.insert(keycode);
        } else {
            self.pressed_keys.remove(&keycode);
        }
        Step::packets(vec![Packet::Key {
            keycode,
            pressed: if pressed { 1 } else { 0 },
        }])
    }

    fn on_scroll(&mut self, dx: f32, dy: f32) -> Step {
        if !self.is_remote() {
            return Step::empty();
        }
        Step::packets(vec![Packet::Scroll { dx, dy }])
    }

    fn enter_remote(&mut self, mx: i32, my: i32) -> Step {
        let mx = mx.clamp(0, self.layout.mac_w.saturating_sub(1));
        let my = my.clamp(0, self.layout.mac_h.saturating_sub(1));
        self.mode = Mode::Remote { mx, my };
        let edge: Edge = self.layout.enter_edge();
        Step {
            packets: vec![
                Packet::Enter {
                    edge,
                    x: mx,
                    y: my,
                },
                // Immediate abs position so clients that ignore Enter coords still track.
                Packet::Motion { x: mx, y: my },
            ],
            effects: vec![SideEffect::Grab],
        }
    }

    /// Leave remote mode. `after_motion` is `(mx, my, warp_primary)` when
    /// leave was triggered by cursor exit; `None` for force leave (warp to
    /// last local seed on shared edge).
    fn leave_remote(&mut self, after_motion: Option<(i32, i32, (i32, i32))>) -> Step {
        if !self.is_remote() {
            return Step::empty();
        }

        let warp = match after_motion {
            Some((_, _, w)) => w,
            None => match self.mode {
                Mode::Remote { mx, my } => {
                    // Prefer the natural return edge; fall back to projecting
                    // the current Mac point onto the shared primary edge.
                    self.layout
                        .leave_toward_primary(mx - 1, my)
                        .or_else(|| self.layout.leave_toward_primary(mx + 1, my))
                        .or_else(|| self.layout.leave_toward_primary(mx, my - 1))
                        .or_else(|| self.layout.leave_toward_primary(mx, my + 1))
                        .unwrap_or_else(|| {
                            let (px, py) = self.layout.to_primary(mx, my);
                            self.layout.clamp_primary(px, py)
                        })
                }
                Mode::Local => (self.local_x, self.local_y),
            },
        };

        let mut packets = Vec::new();

        // Stuck-modifier / stuck-button recovery before Leave so the client
        // still injects key-ups while it considers itself remote.
        for &button in &self.pressed_buttons {
            packets.push(Packet::Button {
                button,
                pressed: 0,
            });
        }
        for &keycode in &self.pressed_keys {
            packets.push(Packet::Key {
                keycode,
                pressed: 0,
            });
        }
        self.pressed_buttons.clear();
        self.pressed_keys.clear();

        packets.push(Packet::Leave);

        self.mode = Mode::Local;
        let (wx, wy) = self.layout.clamp_primary(warp.0, warp.1);
        self.local_x = wx;
        self.local_y = wy;

        Step {
            packets,
            effects: vec![SideEffect::Release {
                warp_primary: (wx, wy),
            }],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{Align, LayoutSpec, Side};

    fn desk() -> Layout {
        Layout::compute(&LayoutSpec {
            primary_w: 5120,
            primary_h: 2160,
            mac_w: 2560,
            mac_h: 2880,
            side: Side::Right,
            align: Align::Bottom,
            scale: 1.0,
            offset_x: None,
            offset_y: None,
        })
    }

    #[test]
    fn enter_on_right_edge_motion() {
        let mut s = Session::with_local_pos(desk(), 5119, 2000);
        let step = s.handle(InputEvent::PointerRel { dx: 3.0, dy: 0.0 });
        assert!(s.is_remote());
        assert!(matches!(step.effects.as_slice(), [SideEffect::Grab]));
        assert!(matches!(
            step.packets[0],
            Packet::Enter {
                edge: Edge::Right,
                x: 0,
                y: 2720
            }
        ));
        assert_eq!(s.mac_pos(), Some((0, 2720)));
    }

    #[test]
    fn local_motion_stays_local() {
        let mut s = Session::with_local_pos(desk(), 100, 100);
        let step = s.handle(InputEvent::PointerRel { dx: 10.0, dy: 5.0 });
        assert!(!s.is_remote());
        assert!(step.packets.is_empty());
        assert_eq!((s.local_x, s.local_y), (110, 105));
    }

    #[test]
    fn remote_motion_emits_abs() {
        let mut s = Session::with_local_pos(desk(), 5119, 2000);
        s.handle(InputEvent::PointerRel { dx: 2.0, dy: 0.0 });
        let step = s.handle(InputEvent::PointerRel { dx: 5.0, dy: -3.0 });
        assert_eq!(step.packets.len(), 1);
        match step.packets[0] {
            Packet::Motion { x, y } => {
                assert_eq!(x, 5);
                assert_eq!(y, 2717);
            }
            ref p => panic!("expected Motion, got {p:?}"),
        }
    }

    #[test]
    fn leave_toward_primary_releases() {
        let mut s = Session::with_local_pos(desk(), 5119, 1000);
        s.handle(InputEvent::PointerRel { dx: 2.0, dy: 0.0 });
        assert!(s.is_remote());
        // Mac enter at mx=0; move left off Mac.
        let step = s.handle(InputEvent::PointerRel { dx: -5.0, dy: 0.0 });
        assert!(!s.is_remote());
        assert!(step.packets.iter().any(|p| matches!(p, Packet::Leave)));
        assert!(matches!(
            step.effects.as_slice(),
            [SideEffect::Release { .. }]
        ));
        // Local pointer restored on right edge.
        assert_eq!(s.local_x, 5119);
    }

    #[test]
    fn stuck_keys_released_on_leave() {
        let mut s = Session::with_local_pos(desk(), 5119, 1000);
        s.handle(InputEvent::PointerRel { dx: 2.0, dy: 0.0 });
        s.handle(InputEvent::Key {
            keycode: 29, // Ctrl
            pressed: true,
        });
        s.handle(InputEvent::Key {
            keycode: 56, // Alt
            pressed: true,
        });
        s.handle(InputEvent::Button {
            button: 0,
            pressed: true,
        });
        let step = s.handle(InputEvent::ForceLeave);
        // button-up, two key-ups, Leave
        let ups: Vec<_> = step
            .packets
            .iter()
            .filter(|p| {
                matches!(
                    p,
                    Packet::Key { pressed: 0, .. } | Packet::Button { pressed: 0, .. }
                )
            })
            .collect();
        assert_eq!(ups.len(), 3);
        assert!(matches!(step.packets.last(), Some(Packet::Leave)));
        assert!(!s.is_remote());
        assert!(s.pressed_keys.is_empty());
        assert!(s.pressed_buttons.is_empty());
    }

    #[test]
    fn buttons_and_keys_ignored_while_local() {
        let mut s = Session::with_local_pos(desk(), 100, 100);
        let step = s.handle(InputEvent::Key {
            keycode: 30,
            pressed: true,
        });
        assert!(step.packets.is_empty());
        let step = s.handle(InputEvent::Button {
            button: 0,
            pressed: true,
        });
        assert!(step.packets.is_empty());
    }

    #[test]
    fn motion_scale_applied_while_remote() {
        let base = desk();
        // rebuild with scale 2
        let layout = Layout::compute(&LayoutSpec {
            primary_w: base.primary_w,
            primary_h: base.primary_h,
            mac_w: base.mac_w,
            mac_h: base.mac_h,
            side: base.side,
            align: base.align,
            scale: 2.0,
            offset_x: None,
            offset_y: None,
        });
        let mut s = Session::with_local_pos(layout, 5119, 1000);
        s.handle(InputEvent::PointerRel { dx: 2.0, dy: 0.0 });
        let (mx0, my0) = s.mac_pos().unwrap();
        s.handle(InputEvent::PointerRel { dx: 3.0, dy: 0.0 });
        let (mx1, _) = s.mac_pos().unwrap();
        assert_eq!(mx1 - mx0, 6); // 3 * scale 2
        let _ = my0;
    }

    #[test]
    fn scroll_while_remote() {
        let mut s = Session::with_local_pos(desk(), 5119, 1000);
        s.handle(InputEvent::PointerRel { dx: 2.0, dy: 0.0 });
        let step = s.handle(InputEvent::Scroll {
            dx: 0.0,
            dy: -1.5,
        });
        assert_eq!(
            step.packets,
            vec![Packet::Scroll {
                dx: 0.0,
                dy: -1.5
            }]
        );
    }
}
