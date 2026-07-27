//! Wire format for sola-kvm UDP packets (v1).
//!
//! Trusted LAN only — no TLS, no handshake. See
//! `docs/specs/2026-07-27-sola-kvm-design.md` §4.

use std::io::{Cursor, Read, Write};

/// Magic: ASCII `KVM1` as little-endian u32 (`0x4b564d31`).
pub const MAGIC: u32 = 0x4b_56_4d_31;

/// Protocol version byte.
pub const VERSION: u8 = 1;

/// Fixed header size: magic(4) + version(1) + type(1) + seq(4) = 10.
pub const HEADER_LEN: usize = 10;

/// Maximum reasonable UDP payload we will encode/decode.
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
/// Matches layout [`crate::layout::Side`] numbering for convenience.
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
    /// Absolute pointer in Mac-local space (preferred over relative).
    Motion { x: i32, y: i32 },
    /// Mouse button. `button`: 0=left, 1=right, 2=middle; `pressed`: 0/1.
    Button { button: u8, pressed: u8 },
    /// Linux evdev keycode + press/release.
    Key { keycode: u32, pressed: u8 },
    /// Scroll deltas (Mac/client units; typically line or pixel scale).
    Scroll { dx: f32, dy: f32 },
    /// Explicit modifier mask (optional; keys usually carry mods via press).
    Modifiers { mask: u32 },
}

/// Encode error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EncodeError {
    Io(String),
}

impl std::fmt::Display for EncodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(s) => write!(f, "encode io: {s}"),
        }
    }
}

impl std::error::Error for EncodeError {}

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

/// Encode a packet with the given sequence number into a new buffer.
pub fn encode(seq: u32, packet: &Packet) -> Result<Vec<u8>, EncodeError> {
    let mut buf = Vec::with_capacity(MAX_PACKET_LEN);
    encode_into(seq, packet, &mut buf)?;
    Ok(buf)
}

/// Encode into an existing buffer (appended).
pub fn encode_into(seq: u32, packet: &Packet, buf: &mut Vec<u8>) -> Result<(), EncodeError> {
    let ty = packet_type(packet);
    write_u32(buf, MAGIC)?;
    write_u8(buf, VERSION)?;
    write_u8(buf, ty as u8)?;
    write_u32(buf, seq)?;

    match packet {
        Packet::Enter { edge, x, y } => {
            write_u8(buf, *edge as u8)?;
            write_i32(buf, *x)?;
            write_i32(buf, *y)?;
        }
        Packet::Leave => {}
        Packet::Motion { x, y } => {
            write_i32(buf, *x)?;
            write_i32(buf, *y)?;
        }
        Packet::Button { button, pressed } => {
            write_u8(buf, *button)?;
            write_u8(buf, *pressed)?;
        }
        Packet::Key { keycode, pressed } => {
            write_u32(buf, *keycode)?;
            write_u8(buf, *pressed)?;
        }
        Packet::Scroll { dx, dy } => {
            write_f32(buf, *dx)?;
            write_f32(buf, *dy)?;
        }
        Packet::Modifiers { mask } => {
            write_u32(buf, *mask)?;
        }
    }
    Ok(())
}

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

// --- little-endian primitives ------------------------------------------------

fn write_u8(buf: &mut Vec<u8>, v: u8) -> Result<(), EncodeError> {
    buf.write_all(&[v]).map_err(|e| EncodeError::Io(e.to_string()))
}

fn write_u32(buf: &mut Vec<u8>, v: u32) -> Result<(), EncodeError> {
    buf.write_all(&v.to_le_bytes())
        .map_err(|e| EncodeError::Io(e.to_string()))
}

fn write_i32(buf: &mut Vec<u8>, v: i32) -> Result<(), EncodeError> {
    buf.write_all(&v.to_le_bytes())
        .map_err(|e| EncodeError::Io(e.to_string()))
}

fn write_f32(buf: &mut Vec<u8>, v: f32) -> Result<(), EncodeError> {
    buf.write_all(&v.to_le_bytes())
        .map_err(|e| EncodeError::Io(e.to_string()))
}

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

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(seq: u32, packet: Packet) {
        let bytes = encode(seq, &packet).expect("encode");
        let (got_seq, got) = decode(&bytes).expect("decode");
        assert_eq!(got_seq, seq);
        assert_eq!(got, packet);
    }

    #[test]
    fn magic_matches_design_constant() {
        // Design §4.2: magic u32 = 0x4b564d31 ("KVM1" as hex digits).
        // Encoded little-endian on the wire.
        assert_eq!(MAGIC, 0x4b_56_4d_31);
        let on_wire = MAGIC.to_le_bytes();
        assert_eq!(on_wire, [0x31, 0x4d, 0x56, 0x4b]);
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
    fn roundtrip_key() {
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
        let mut bytes = encode(0, &Packet::Leave).unwrap();
        bytes[0] = 0;
        let err = decode(&bytes).unwrap_err();
        assert!(matches!(err, DecodeError::BadMagic(_)));
    }

    #[test]
    fn reject_bad_version() {
        let mut bytes = encode(0, &Packet::Leave).unwrap();
        bytes[4] = 99; // version slot
        let err = decode(&bytes).unwrap_err();
        assert!(matches!(err, DecodeError::BadVersion(99)));
    }

    #[test]
    fn reject_unknown_type() {
        let mut bytes = encode(0, &Packet::Leave).unwrap();
        bytes[5] = 99; // type slot
        let err = decode(&bytes).unwrap_err();
        assert!(matches!(err, DecodeError::UnknownType(99)));
    }

    #[test]
    fn reject_truncated_motion() {
        let bytes = encode(0, &Packet::Motion { x: 1, y: 2 }).unwrap();
        // Drop last byte of y
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
        )
        .unwrap();
        assert_eq!(bytes.len(), 19);
    }

    #[test]
    fn leave_wire_is_header_only() {
        let bytes = encode(0, &Packet::Leave).unwrap();
        assert_eq!(bytes.len(), HEADER_LEN);
    }
}
