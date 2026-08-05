//! CLIP1 wire protocol — TCP, little-endian.
//!
//! Separate from KVM1 UDP input. Same peer port is fine (different transport).

use std::io::{self, Read, Write};

/// ASCII `CLIP` as u32 (`0x43_4c_49_50`).
pub const MAGIC: u32 = 0x43_4c_49_50;
pub const VERSION: u8 = 1;

/// MIME: UTF-8 plain text only in v1.
pub const MIME_TEXT_UTF8: u8 = 1;

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MsgType {
    Hello = 1,
    Offer = 2,
    Empty = 3,
    Ack = 4,
}

impl MsgType {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            1 => Some(Self::Hello),
            2 => Some(Self::Offer),
            3 => Some(Self::Empty),
            4 => Some(Self::Ack),
            _ => None,
        }
    }
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Novus = 1,
    Ember = 2,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AckStatus {
    Ok = 0,
    TooLarge = 1,
    Reject = 2,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    Hello { role: Role },
    /// UTF-8 text body (may be empty — prefer [`Message::Empty`] for clear).
    Offer { hash: u32, text: String },
    Empty,
    Ack { of_seq: u32, status: AckStatus },
}

/// FNV-1a 32-bit over UTF-8 bytes — stable, no extra crate.
pub fn hash_text(text: &str) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for &b in text.as_bytes() {
        h ^= u32::from(b);
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

pub fn write_message(w: &mut dyn Write, seq: u32, msg: &Message) -> io::Result<()> {
    let mut hdr = [0u8; 10];
    hdr[0..4].copy_from_slice(&MAGIC.to_le_bytes());
    hdr[4] = VERSION;
    match msg {
        Message::Hello { role } => {
            hdr[5] = MsgType::Hello as u8;
            hdr[6..10].copy_from_slice(&seq.to_le_bytes());
            w.write_all(&hdr)?;
            w.write_all(&[*role as u8])?;
        }
        Message::Offer { hash, text } => {
            let bytes = text.as_bytes();
            let len = bytes.len() as u32;
            hdr[5] = MsgType::Offer as u8;
            hdr[6..10].copy_from_slice(&seq.to_le_bytes());
            w.write_all(&hdr)?;
            w.write_all(&[MIME_TEXT_UTF8])?;
            w.write_all(&len.to_le_bytes())?;
            w.write_all(&hash.to_le_bytes())?;
            w.write_all(bytes)?;
        }
        Message::Empty => {
            hdr[5] = MsgType::Empty as u8;
            hdr[6..10].copy_from_slice(&seq.to_le_bytes());
            w.write_all(&hdr)?;
        }
        Message::Ack { of_seq, status } => {
            hdr[5] = MsgType::Ack as u8;
            hdr[6..10].copy_from_slice(&seq.to_le_bytes());
            w.write_all(&hdr)?;
            w.write_all(&of_seq.to_le_bytes())?;
            w.write_all(&[*status as u8])?;
        }
    }
    w.flush()
}

pub fn read_message(r: &mut dyn Read, max_bytes: u32) -> io::Result<(u32, Message)> {
    let mut hdr = [0u8; 10];
    r.read_exact(&mut hdr)?;
    let magic = u32::from_le_bytes(hdr[0..4].try_into().unwrap());
    if magic != MAGIC {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("bad CLIP magic {magic:#x}"),
        ));
    }
    if hdr[4] != VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("bad CLIP version {}", hdr[4]),
        ));
    }
    let ty = MsgType::from_u8(hdr[5]).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unknown CLIP type {}", hdr[5]),
        )
    })?;
    let seq = u32::from_le_bytes(hdr[6..10].try_into().unwrap());

    let msg = match ty {
        MsgType::Hello => {
            let mut b = [0u8; 1];
            r.read_exact(&mut b)?;
            let role = match b[0] {
                1 => Role::Novus,
                2 => Role::Ember,
                o => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("bad role {o}"),
                    ))
                }
            };
            Message::Hello { role }
        }
        MsgType::Offer => {
            let mut fixed = [0u8; 9]; // mime + len + hash
            r.read_exact(&mut fixed)?;
            let mime = fixed[0];
            if mime != MIME_TEXT_UTF8 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("unsupported mime {mime}"),
                ));
            }
            let len = u32::from_le_bytes(fixed[1..5].try_into().unwrap());
            let hash = u32::from_le_bytes(fixed[5..9].try_into().unwrap());
            if len > max_bytes {
                // Drain residual so stream stays aligned, then error.
                let mut left = len as usize;
                let mut sink = [0u8; 4096];
                while left > 0 {
                    let n = left.min(sink.len());
                    r.read_exact(&mut sink[..n])?;
                    left -= n;
                }
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("offer too large: {len} > {max_bytes}"),
                ));
            }
            let mut body = vec![0u8; len as usize];
            if len > 0 {
                r.read_exact(&mut body)?;
            }
            let text = String::from_utf8(body).map_err(|e| {
                io::Error::new(io::ErrorKind::InvalidData, format!("offer not utf-8: {e}"))
            })?;
            // Trust hash for cache identity; recompute for skip logic on write path.
            let _ = hash;
            Message::Offer {
                hash: hash_text(&text),
                text,
            }
        }
        MsgType::Empty => Message::Empty,
        MsgType::Ack => {
            let mut b = [0u8; 5];
            r.read_exact(&mut b)?;
            let of_seq = u32::from_le_bytes(b[0..4].try_into().unwrap());
            let status = match b[4] {
                0 => AckStatus::Ok,
                1 => AckStatus::TooLarge,
                _ => AckStatus::Reject,
            };
            Message::Ack { of_seq, status }
        }
    };
    Ok((seq, msg))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn roundtrip_offer_and_hash() {
        let text = "hello clipboard 🦀";
        let h = hash_text(text);
        let msg = Message::Offer {
            hash: h,
            text: text.into(),
        };
        let mut buf = Vec::new();
        write_message(&mut buf, 7, &msg).unwrap();
        let (seq, got) = read_message(&mut Cursor::new(buf), 1_048_576).unwrap();
        assert_eq!(seq, 7);
        assert_eq!(got, msg);
    }

    #[test]
    fn roundtrip_empty_hello_ack() {
        for (seq, msg) in [
            (1, Message::Empty),
            (2, Message::Hello { role: Role::Novus }),
            (
                3,
                Message::Ack {
                    of_seq: 9,
                    status: AckStatus::Ok,
                },
            ),
        ] {
            let mut buf = Vec::new();
            write_message(&mut buf, seq, &msg).unwrap();
            let (s, got) = read_message(&mut Cursor::new(buf), 1024).unwrap();
            assert_eq!(s, seq);
            assert_eq!(got, msg);
        }
    }
}
