//! The wire of remote view (`sola-kvm view <host>`): another machine's
//! screen in a window here, with this keyboard and pointer driving it.
//!
//! The viewer opens two TCP connections to the host's view port, each its
//! own paired Noise session (`link::Conn`), and says which it is in the first
//! frame. One stream carries one direction, so input never waits behind the
//! picture and neither side shares a Noise state between threads:
//!
//! - `Video`, host to viewer: [`Down`]. `Display` first, then `Picture`s.
//! - `Input`, viewer to host: [`Up`]. Events are KVM1 packets, the same
//!   bytes the UDP input path carries.
//!
//! Self-contained (no `crate::` paths): the Mac agent includes this file.

use std::io;

pub const VERSION: u8 = 1;
/// The view port is the KVM port plus this.
pub const PORT_OFFSET: u16 = 1;
/// The largest frame either side reads (a key frame at 8K fits well inside).
pub const MAX_FRAME: u32 = 64 << 20;

const DISPLAY: u8 = 1;
const PICTURE: u8 = 2;
const REFUSED: u8 = 3;
const KEYFRAME: u8 = 1;
const EVENT: u8 = 2;

/// Which direction a connection is; its first frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stream {
    Video,
    Input,
}

impl Stream {
    pub fn encode(self) -> [u8; 2] {
        [
            VERSION,
            match self {
                Self::Video => 1,
                Self::Input => 2,
            },
        ]
    }

    pub fn decode(bytes: &[u8]) -> io::Result<Self> {
        match bytes {
            [VERSION, 1] => Ok(Self::Video),
            [VERSION, 2] => Ok(Self::Input),
            [v, _] if *v != VERSION => Err(bad(format!(
                "the other side speaks view version {v}, this one {VERSION}"
            ))),
            _ => Err(bad("not a view stream".into())),
        }
    }
}

/// Host to viewer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Down {
    /// The screen being sent: `width` by `height` pixels in the picture,
    /// `points_w` by `points_h` in the coordinates input is sent in.
    Display {
        width: u32,
        height: u32,
        points_w: u32,
        points_h: u32,
    },
    /// One encoded picture (HEVC, Annex B; a key picture carries its
    /// parameter sets).
    Picture { key: bool, pts_us: u64, data: Vec<u8> },
    /// The host will not send its screen, and why, in words for the window.
    Refused(String),
}

impl Down {
    pub fn encode(&self) -> Vec<u8> {
        match self {
            Self::Display {
                width,
                height,
                points_w,
                points_h,
            } => {
                let mut out = vec![DISPLAY];
                for v in [width, height, points_w, points_h] {
                    out.extend_from_slice(&v.to_le_bytes());
                }
                out
            }
            Self::Picture { key, pts_us, data } => {
                let mut out = Vec::with_capacity(10 + data.len());
                out.push(PICTURE);
                out.push(u8::from(*key));
                out.extend_from_slice(&pts_us.to_le_bytes());
                out.extend_from_slice(data);
                out
            }
            Self::Refused(why) => {
                let mut out = vec![REFUSED];
                out.extend_from_slice(why.as_bytes());
                out
            }
        }
    }

    pub fn decode(bytes: &[u8]) -> io::Result<Self> {
        let (&tag, rest) = bytes.split_first().ok_or_else(|| bad("an empty frame".into()))?;
        match tag {
            DISPLAY if rest.len() == 16 => Ok(Self::Display {
                width: u32_at(rest, 0),
                height: u32_at(rest, 4),
                points_w: u32_at(rest, 8),
                points_h: u32_at(rest, 12),
            }),
            PICTURE if rest.len() >= 9 => Ok(Self::Picture {
                key: rest[0] != 0,
                pts_us: u64::from_le_bytes(rest[1..9].try_into().expect("8")),
                data: rest[9..].to_vec(),
            }),
            REFUSED => Ok(Self::Refused(String::from_utf8_lossy(rest).into_owned())),
            _ => Err(bad(format!("a bad frame from the host (tag {tag}, {} bytes)", rest.len()))),
        }
    }
}

/// Viewer to host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Up {
    /// Send a key picture next (the decoder lost its place).
    Keyframe,
    /// One KVM1 packet.
    Event(Vec<u8>),
}

impl Up {
    pub fn encode(&self) -> Vec<u8> {
        match self {
            Self::Keyframe => vec![KEYFRAME],
            Self::Event(packet) => {
                let mut out = Vec::with_capacity(1 + packet.len());
                out.push(EVENT);
                out.extend_from_slice(packet);
                out
            }
        }
    }

    pub fn decode(bytes: &[u8]) -> io::Result<Self> {
        match bytes.split_first() {
            Some((&KEYFRAME, [])) => Ok(Self::Keyframe),
            Some((&EVENT, packet)) => Ok(Self::Event(packet.to_vec())),
            _ => Err(bad("a bad frame from the viewer".into())),
        }
    }
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().expect("4"))
}

fn bad(msg: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_round_trips_and_names_a_version_mismatch() {
        for s in [Stream::Video, Stream::Input] {
            assert_eq!(Stream::decode(&s.encode()).unwrap(), s);
        }
        let err = Stream::decode(&[VERSION + 1, 1]).unwrap_err();
        assert!(err.to_string().contains("version"), "{err}");
    }

    #[test]
    fn down_round_trips() {
        for msg in [
            Down::Display {
                width: 3024,
                height: 1964,
                points_w: 1512,
                points_h: 982,
            },
            Down::Picture {
                key: true,
                pts_us: 1 << 40,
                data: vec![0, 0, 0, 1, 0x40],
            },
            Down::Picture {
                key: false,
                pts_us: 7,
                data: Vec::new(),
            },
            Down::Refused("Screen Recording is off".into()),
        ] {
            assert_eq!(Down::decode(&msg.encode()).unwrap(), msg);
        }
    }

    #[test]
    fn up_round_trips() {
        for msg in [Up::Keyframe, Up::Event(vec![1, 2, 3])] {
            assert_eq!(Up::decode(&msg.encode()).unwrap(), msg);
        }
    }

    #[test]
    fn short_frames_are_errors_not_panics() {
        assert!(Down::decode(&[]).is_err());
        assert!(Down::decode(&[DISPLAY, 1, 2]).is_err());
        assert!(Down::decode(&[PICTURE, 1]).is_err());
        assert!(Up::decode(&[]).is_err());
        assert!(Up::decode(&[KEYFRAME, 9]).is_err());
    }
}
