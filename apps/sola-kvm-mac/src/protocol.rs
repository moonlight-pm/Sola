//! Wire format for sola-kvm UDP packets (v1) — Mac-side decoder.
//!
//! **Must stay byte-compatible with** `crates/sola-kvm/src/protocol.rs`.
//! Owned by Phase C on the server; this module is a deliberate mirror so the
//! Mac agent does not depend on the Sola workspace.
//!
//! Layout (all little-endian):
//! ```text
//! magic u32 = 0x4b564d31   // LE bytes on wire: 31 4d 56 4b
//! version u8 = 1
//! type u8
//! seq u32
//! payload…
//! ```

use std::io::{Cursor, Read};

/// Magic: design constant `0x4b564d31` (ASCII-ish “KVM1” as hex digits).
pub const MAGIC: u32 = 0x4b_56_4d_31;

/// Protocol version byte.
pub const VERSION: u8 = 1;

/// Fixed header size: magic(4) + version(1) + type(1) + seq(4) = 10.
pub const HEADER_LEN: usize = 10;

/// Maximum reasonable UDP payload we will decode.
pub const MAX_PACKET_LEN: usize = 64;

/// Wire packet type tags.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PacketType {
    Enter = 1,
    Leave = 2,
    Motion = 3,
    Button = 4,
    Key = 5,
    Scroll = 6,
    Modifiers = 7,
}

impl PacketType {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            1 => Some(Self::Enter),
            2 => Some(Self::Leave),
            3 => Some(Self::Motion),
            4 => Some(Self::Button),
            5 => Some(Self::Key),
            6 => Some(Self::Scroll),
            7 => Some(Self::Modifiers),
            _ => None,
        }
    }
}

/// Which edge of the virtual Mac rect the pointer crossed on enter.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Left = 0,
    Right = 1,
    Top = 2,
    Bottom = 3,
}

impl Edge {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Left),
            1 => Some(Self::Right),
            2 => Some(Self::Top),
            3 => Some(Self::Bottom),
            _ => None,
        }
    }
}

/// Fully-decoded KVM event (after header validation).
#[derive(Debug, Clone, PartialEq)]
pub enum Packet {
    /// Enter remote mode at Mac-local absolute coords.
    Enter { edge: Edge, x: i32, y: i32 },
    /// Leave remote mode; server restores local pointer.
    Leave,
    /// Absolute pointer in Mac-local space (prefer warp/set).
    Motion { x: i32, y: i32 },
    /// Mouse button. `button`: 0=left, 1=right, 2=middle; `pressed`: 0/1.
    Button { button: u8, pressed: u8 },
    /// Linux evdev keycode + press/release.
    Key { keycode: u32, pressed: u8 },
    /// Scroll deltas (line/pixel scale as sent by novus).
    Scroll { dx: f32, dy: f32 },
    /// Explicit modifier mask (optional; keys usually carry mods via press).
    Modifiers { mask: u32 },
}

/// Decode error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    TooShort { need: usize, got: usize },
    BadMagic(u32),
    BadVersion(u8),
    UnknownType(u8),
    BadEdge(u8),
    TruncatedPayload { ty: u8 },
    Io(String),
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooShort { need, got } => {
                write!(f, "packet too short: need {need}, got {got}")
            }
            Self::BadMagic(m) => write!(f, "bad magic: 0x{m:08x}"),
            Self::BadVersion(v) => write!(f, "unsupported version: {v}"),
            Self::UnknownType(t) => write!(f, "unknown packet type: {t}"),
            Self::BadEdge(e) => write!(f, "bad edge: {e}"),
            Self::TruncatedPayload { ty } => {
                write!(f, "truncated payload for type {ty}")
            }
            Self::Io(s) => write!(f, "decode io: {s}"),
        }
    }
}

impl std::error::Error for DecodeError {}

/// Decode one packet from a UDP datagram. Returns `(seq, packet)`.
pub fn decode(bytes: &[u8]) -> Result<(u32, Packet), DecodeError> {
    if bytes.len() < HEADER_LEN {
        return Err(DecodeError::TooShort {
            need: HEADER_LEN,
            got: bytes.len(),
        });
    }

    let mut cur = Cursor::new(bytes);
    let magic = read_u32(&mut cur)?;
    if magic != MAGIC {
        return Err(DecodeError::BadMagic(magic));
    }
    let version = read_u8(&mut cur)?;
    if version != VERSION {
        return Err(DecodeError::BadVersion(version));
    }
    let ty_raw = read_u8(&mut cur)?;
    let ty = PacketType::from_u8(ty_raw).ok_or(DecodeError::UnknownType(ty_raw))?;
    let seq = read_u32(&mut cur)?;

    let packet = match ty {
        PacketType::Enter => {
            let edge_raw = read_u8(&mut cur).map_err(|_| DecodeError::TruncatedPayload {
                ty: ty_raw,
            })?;
            let edge = Edge::from_u8(edge_raw).ok_or(DecodeError::BadEdge(edge_raw))?;
            let x = read_i32(&mut cur).map_err(|_| DecodeError::TruncatedPayload {
                ty: ty_raw,
            })?;
            let y = read_i32(&mut cur).map_err(|_| DecodeError::TruncatedPayload {
                ty: ty_raw,
            })?;
            Packet::Enter { edge, x, y }
        }
        PacketType::Leave => Packet::Leave,
        PacketType::Motion => {
            let x = read_i32(&mut cur).map_err(|_| DecodeError::TruncatedPayload {
                ty: ty_raw,
            })?;
            let y = read_i32(&mut cur).map_err(|_| DecodeError::TruncatedPayload {
                ty: ty_raw,
            })?;
            Packet::Motion { x, y }
        }
        PacketType::Button => {
            let button = read_u8(&mut cur).map_err(|_| DecodeError::TruncatedPayload {
                ty: ty_raw,
            })?;
            let pressed = read_u8(&mut cur).map_err(|_| DecodeError::TruncatedPayload {
                ty: ty_raw,
            })?;
            Packet::Button { button, pressed }
        }
        PacketType::Key => {
            let keycode = read_u32(&mut cur).map_err(|_| DecodeError::TruncatedPayload {
                ty: ty_raw,
            })?;
            let pressed = read_u8(&mut cur).map_err(|_| DecodeError::TruncatedPayload {
                ty: ty_raw,
            })?;
            Packet::Key { keycode, pressed }
        }
        PacketType::Scroll => {
            let dx = read_f32(&mut cur).map_err(|_| DecodeError::TruncatedPayload {
                ty: ty_raw,
            })?;
            let dy = read_f32(&mut cur).map_err(|_| DecodeError::TruncatedPayload {
                ty: ty_raw,
            })?;
            Packet::Scroll { dx, dy }
        }
        PacketType::Modifiers => {
            let mask = read_u32(&mut cur).map_err(|_| DecodeError::TruncatedPayload {
                ty: ty_raw,
            })?;
            Packet::Modifiers { mask }
        }
    };

    Ok((seq, packet))
}

// --- little-endian primitives ------------------------------------------------

fn read_u8(cur: &mut Cursor<&[u8]>) -> Result<u8, DecodeError> {
    let mut b = [0u8; 1];
    cur.read_exact(&mut b)
        .map_err(|e| DecodeError::Io(e.to_string()))?;
    Ok(b[0])
}

fn read_u32(cur: &mut Cursor<&[u8]>) -> Result<u32, DecodeError> {
    let mut b = [0u8; 4];
    cur.read_exact(&mut b)
        .map_err(|e| DecodeError::Io(e.to_string()))?;
    Ok(u32::from_le_bytes(b))
}

fn read_i32(cur: &mut Cursor<&[u8]>) -> Result<i32, DecodeError> {
    let mut b = [0u8; 4];
    cur.read_exact(&mut b)
        .map_err(|e| DecodeError::Io(e.to_string()))?;
    Ok(i32::from_le_bytes(b))
}

fn read_f32(cur: &mut Cursor<&[u8]>) -> Result<f32, DecodeError> {
    let mut b = [0u8; 4];
    cur.read_exact(&mut b)
        .map_err(|e| DecodeError::Io(e.to_string()))?;
    Ok(f32::from_le_bytes(b))
}

/// Encode helpers used only by unit tests (golden vectors / roundtrip).
#[cfg(test)]
pub mod encode {
    use super::*;
    use std::io::Write;

    pub fn encode(seq: u32, packet: &Packet) -> Vec<u8> {
        let mut buf = Vec::with_capacity(MAX_PACKET_LEN);
        buf.extend_from_slice(&MAGIC.to_le_bytes());
        buf.push(VERSION);
        buf.push(packet_type(packet) as u8);
        buf.extend_from_slice(&seq.to_le_bytes());
        match packet {
            Packet::Enter { edge, x, y } => {
                buf.push(*edge as u8);
                buf.extend_from_slice(&x.to_le_bytes());
                buf.extend_from_slice(&y.to_le_bytes());
            }
            Packet::Leave => {}
            Packet::Motion { x, y } => {
                buf.extend_from_slice(&x.to_le_bytes());
                buf.extend_from_slice(&y.to_le_bytes());
            }
            Packet::Button { button, pressed } => {
                buf.push(*button);
                buf.push(*pressed);
            }
            Packet::Key { keycode, pressed } => {
                buf.extend_from_slice(&keycode.to_le_bytes());
                buf.push(*pressed);
            }
            Packet::Scroll { dx, dy } => {
                buf.extend_from_slice(&dx.to_le_bytes());
                buf.extend_from_slice(&dy.to_le_bytes());
            }
            Packet::Modifiers { mask } => {
                buf.extend_from_slice(&mask.to_le_bytes());
            }
        }
        let _ = buf.flush();
        buf
    }

    fn packet_type(p: &Packet) -> PacketType {
        match p {
            Packet::Enter { .. } => PacketType::Enter,
            Packet::Leave => PacketType::Leave,
            Packet::Motion { .. } => PacketType::Motion,
            Packet::Button { .. } => PacketType::Button,
            Packet::Key { .. } => PacketType::Key,
            Packet::Scroll { .. } => PacketType::Scroll,
            Packet::Modifiers { .. } => PacketType::Modifiers,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use encode::encode;

    fn roundtrip(seq: u32, packet: Packet) {
        let bytes = encode(seq, &packet);
        let (got_seq, got) = decode(&bytes).expect("decode");
        assert_eq!(got_seq, seq);
        assert_eq!(got, packet);
    }

    #[test]
    fn magic_matches_server_constant() {
        assert_eq!(MAGIC, 0x4b_56_4d_31);
        assert_eq!(MAGIC.to_le_bytes(), [0x31, 0x4d, 0x56, 0x4b]);
    }

    #[test]
    fn roundtrip_enter() {
        roundtrip(
            1,
            Packet::Enter {
                edge: Edge::Right,
                x: 100,
                y: 2000,
            },
        );
    }

    #[test]
    fn roundtrip_leave() {
        roundtrip(42, Packet::Leave);
    }

    #[test]
    fn roundtrip_motion_abs() {
        roundtrip(7, Packet::Motion { x: 1280, y: 1440 });
    }

    #[test]
    fn roundtrip_button() {
        roundtrip(
            3,
            Packet::Button {
                button: 0,
                pressed: 1,
            },
        );
    }

    #[test]
    fn roundtrip_key_a() {
        // KEY_A = 30 on Linux evdev
        roundtrip(
            9,
            Packet::Key {
                keycode: 30,
                pressed: 1,
            },
        );
    }

    #[test]
    fn roundtrip_scroll() {
        roundtrip(
            11,
            Packet::Scroll {
                dx: 0.0,
                dy: -3.5,
            },
        );
    }

    #[test]
    fn roundtrip_modifiers() {
        roundtrip(12, Packet::Modifiers { mask: 0x05 });
    }

    #[test]
    fn reject_short() {
        let err = decode(&[0u8; 5]).unwrap_err();
        assert!(matches!(err, DecodeError::TooShort { .. }));
    }

    #[test]
    fn reject_bad_magic() {
        let mut bytes = encode(0, &Packet::Leave);
        bytes[0] = 0;
        let err = decode(&bytes).unwrap_err();
        assert!(matches!(err, DecodeError::BadMagic(_)));
    }

    #[test]
    fn reject_bad_version() {
        let mut bytes = encode(0, &Packet::Leave);
        bytes[4] = 99;
        let err = decode(&bytes).unwrap_err();
        assert!(matches!(err, DecodeError::BadVersion(99)));
    }

    #[test]
    fn reject_unknown_type() {
        let mut bytes = encode(0, &Packet::Leave);
        bytes[5] = 99;
        let err = decode(&bytes).unwrap_err();
        assert!(matches!(err, DecodeError::UnknownType(99)));
    }

    #[test]
    fn reject_truncated_motion() {
        let bytes = encode(0, &Packet::Motion { x: 1, y: 2 });
        let err = decode(&bytes[..bytes.len() - 1]).unwrap_err();
        assert!(matches!(err, DecodeError::TruncatedPayload { ty: 3 }));
    }

    #[test]
    fn enter_wire_layout_sizes() {
        // header 10 + edge 1 + x 4 + y 4 = 19
        let bytes = encode(
            0,
            &Packet::Enter {
                edge: Edge::Left,
                x: 0,
                y: 0,
            },
        );
        assert_eq!(bytes.len(), 19);
    }

    #[test]
    fn leave_wire_is_header_only() {
        let bytes = encode(0, &Packet::Leave);
        assert_eq!(bytes.len(), HEADER_LEN);
    }

    /// Golden bytes matching `crates/sola-kvm` encode of:
    /// seq=0, Enter { edge: Right, x: 100, y: 200 }
    #[test]
    fn golden_enter_matches_server_layout() {
        let bytes = encode(
            0,
            &Packet::Enter {
                edge: Edge::Right,
                x: 100,
                y: 200,
            },
        );
        // magic LE
        assert_eq!(&bytes[0..4], &[0x31, 0x4d, 0x56, 0x4b]);
        assert_eq!(bytes[4], 1); // version
        assert_eq!(bytes[5], 1); // Enter
        assert_eq!(&bytes[6..10], &0u32.to_le_bytes()); // seq
        assert_eq!(bytes[10], 1); // Edge::Right
        assert_eq!(&bytes[11..15], &100i32.to_le_bytes());
        assert_eq!(&bytes[15..19], &200i32.to_le_bytes());
    }

    /// Golden Key packet as produced by `sola-kvm send-test` (KEY_A=30 press).
    #[test]
    fn golden_key_a_press() {
        let bytes = encode(
            4,
            &Packet::Key {
                keycode: 30,
                pressed: 1,
            },
        );
        assert_eq!(bytes.len(), HEADER_LEN + 4 + 1);
        assert_eq!(bytes[5], 5); // Key
        assert_eq!(&bytes[6..10], &4u32.to_le_bytes());
        assert_eq!(&bytes[10..14], &30u32.to_le_bytes());
        assert_eq!(bytes[14], 1);
    }
}
