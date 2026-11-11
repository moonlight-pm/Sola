//! The wire of remote view (`sola-kvm view <host>`): another machine's
//! screen in a window here, with this keyboard and pointer driving it.
//!
//! The viewer opens two TCP connections to the host's view port, each its
//! own paired Noise session (`link::Conn`), and says which it is in the first
//! frame. One stream carries one direction, so input never waits behind the
//! picture and neither side shares a Noise state between threads:
//!
//! - `Video`, host to viewer: [`Down`]. `Display` first, then `Picture`s,
//!   and a `Cursor` whenever the host's pointer changes.
//! - `Input`, viewer to host: [`Up`]. Events are KVM1 packets, the same
//!   bytes the UDP input path carries.
//!
//! From version 3 a frame whose tag this side does not know decodes to
//! `None` and is skipped, so an added message needs no new version; a changed
//! one does.
//!
//! Self-contained (no `crate::` paths): the Mac agent includes this file.

use std::io;

pub const VERSION: u8 = 3;
/// The view port is the KVM port plus this.
pub const PORT_OFFSET: u16 = 1;
/// The largest frame either side reads (a key frame at 8K fits well inside).
pub const MAX_FRAME: u32 = 64 << 20;
/// The most clipboard text either side sends.
pub const MAX_CLIPBOARD: usize = 8 << 20;

const DISPLAY: u8 = 1;
const PICTURE: u8 = 2;
const REFUSED: u8 = 3;
const CLIPBOARD: u8 = 4;
const CURSOR: u8 = 5;
const KEYFRAME: u8 = 1;
const EVENT: u8 = 2;
const CLIPBOARD_SET: u8 = 3;
const CLIPBOARD_GET: u8 = 4;
const ACK: u8 = 5;
const SIZE: u8 = 6;
const MIRROR: u8 = 7;
/// A picture's fields before its data.
const PICTURE_HEADER: usize = 27;
/// A pointer image's fields before its pixels.
const CURSOR_HEADER: usize = 8;
/// The largest pointer image, in pixels a side.
pub const MAX_CURSOR: u16 = 256;

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

/// One encoded picture and what the host knows of its way here.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Picture {
    pub key: bool,
    /// From 1, per video stream; [`Up::Ack`] names it.
    pub seq: u32,
    pub pts_us: u64,
    /// Capture to the start of sending, on the host's clock.
    pub age_us: u32,
    /// The host's estimate of the trip here, from acks; 0 before the first.
    pub net_us: u32,
    /// The encoder's bitrate.
    pub kbps: u32,
    /// Capture frames skipped since the last picture.
    pub skipped: u16,
    pub data: Vec<u8>,
}

/// The host's pointer, for the viewer to draw at its own: the image in the
/// picture's pixels, premultiplied RGBA with the top row first, and the
/// pixel in it that points.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Cursor {
    pub width: u16,
    pub height: u16,
    pub hot_x: u16,
    pub hot_y: u16,
    pub rgba: Vec<u8>,
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
    Picture(Picture),
    /// The host will not send its screen, and why, in words for the window.
    Refused(String),
    /// The host's clipboard text, answering [`Up::ClipboardGet`].
    Clipboard(String),
    /// The host's pointer changed. Sent only when the picture leaves the
    /// pointer out; until one comes, the picture has it.
    Cursor(Cursor),
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
            Self::Picture(p) => {
                let mut out = Vec::with_capacity(1 + PICTURE_HEADER + p.data.len());
                out.push(PICTURE);
                out.push(u8::from(p.key));
                out.extend_from_slice(&p.seq.to_le_bytes());
                out.extend_from_slice(&p.pts_us.to_le_bytes());
                for v in [p.age_us, p.net_us, p.kbps] {
                    out.extend_from_slice(&v.to_le_bytes());
                }
                out.extend_from_slice(&p.skipped.to_le_bytes());
                out.extend_from_slice(&p.data);
                out
            }
            Self::Refused(why) => {
                let mut out = vec![REFUSED];
                out.extend_from_slice(why.as_bytes());
                out
            }
            Self::Clipboard(text) => {
                let mut out = vec![CLIPBOARD];
                out.extend_from_slice(text.as_bytes());
                out
            }
            Self::Cursor(c) => {
                let mut out = Vec::with_capacity(1 + CURSOR_HEADER + c.rgba.len());
                out.push(CURSOR);
                for v in [c.width, c.height, c.hot_x, c.hot_y] {
                    out.extend_from_slice(&v.to_le_bytes());
                }
                out.extend_from_slice(&c.rgba);
                out
            }
        }
    }

    /// `None`: a message this side does not know.
    pub fn decode(bytes: &[u8]) -> io::Result<Option<Self>> {
        let (&tag, rest) = bytes.split_first().ok_or_else(|| bad("an empty frame".into()))?;
        let short = || bad(format!("a short frame from the host (tag {tag}, {} bytes)", rest.len()));
        Ok(Some(match tag {
            DISPLAY if rest.len() == 16 => Self::Display {
                width: u32_at(rest, 0),
                height: u32_at(rest, 4),
                points_w: u32_at(rest, 8),
                points_h: u32_at(rest, 12),
            },
            PICTURE if rest.len() >= PICTURE_HEADER => Self::Picture(Picture {
                key: rest[0] != 0,
                seq: u32_at(rest, 1),
                pts_us: u64::from_le_bytes(rest[5..13].try_into().expect("8")),
                age_us: u32_at(rest, 13),
                net_us: u32_at(rest, 17),
                kbps: u32_at(rest, 21),
                skipped: u16::from_le_bytes(rest[25..27].try_into().expect("2")),
                data: rest[PICTURE_HEADER..].to_vec(),
            }),
            CURSOR if rest.len() >= CURSOR_HEADER => {
                let cursor = Cursor {
                    width: u16_at(rest, 0),
                    height: u16_at(rest, 2),
                    hot_x: u16_at(rest, 4),
                    hot_y: u16_at(rest, 6),
                    rgba: rest[CURSOR_HEADER..].to_vec(),
                };
                let pixels = usize::from(cursor.width) * usize::from(cursor.height);
                if cursor.width > MAX_CURSOR || cursor.height > MAX_CURSOR || cursor.rgba.len() != pixels * 4 {
                    return Err(bad(format!(
                        "a pointer image that does not add up ({}x{}, {} bytes)",
                        cursor.width,
                        cursor.height,
                        cursor.rgba.len()
                    )));
                }
                Self::Cursor(cursor)
            }
            DISPLAY | PICTURE | CURSOR => return Err(short()),
            REFUSED => Self::Refused(String::from_utf8_lossy(rest).into_owned()),
            CLIPBOARD => Self::Clipboard(String::from_utf8_lossy(rest).into_owned()),
            _ => return Ok(None),
        }))
    }
}

/// The viewer's picture area: logical pixels, and the window's scale.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Size {
    pub width: u32,
    pub height: u32,
    pub scale_percent: u16,
}

/// Viewer to host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Up {
    /// Send a key picture next (the decoder lost its place).
    Keyframe,
    /// One KVM1 packet.
    Event(Vec<u8>),
    /// Put this text on the host's clipboard.
    ClipboardSet(String),
    /// Send the host's clipboard text ([`Down::Clipboard`]).
    ClipboardGet,
    /// Picture `seq` is decoded; `held_us` from its last byte read to this.
    Ack { seq: u32, held_us: u32 },
    /// Make the screen this size, when the host can; sent first and again
    /// whenever it changes.
    Size(Size),
    /// Show the host's own screen (`true`), or extend it with one the
    /// window's size (`false`, when this never comes); sent with the size.
    Mirror(bool),
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
            Self::ClipboardSet(text) => {
                let mut out = Vec::with_capacity(1 + text.len());
                out.push(CLIPBOARD_SET);
                out.extend_from_slice(text.as_bytes());
                out
            }
            Self::ClipboardGet => vec![CLIPBOARD_GET],
            Self::Ack { seq, held_us } => {
                let mut out = vec![ACK];
                out.extend_from_slice(&seq.to_le_bytes());
                out.extend_from_slice(&held_us.to_le_bytes());
                out
            }
            Self::Size(size) => {
                let mut out = vec![SIZE];
                out.extend_from_slice(&size.width.to_le_bytes());
                out.extend_from_slice(&size.height.to_le_bytes());
                out.extend_from_slice(&size.scale_percent.to_le_bytes());
                out
            }
            Self::Mirror(mirror) => vec![MIRROR, u8::from(*mirror)],
        }
    }

    /// `None`: a message this side does not know.
    pub fn decode(bytes: &[u8]) -> io::Result<Option<Self>> {
        Ok(Some(match bytes.split_first() {
            Some((&KEYFRAME, [])) => Self::Keyframe,
            Some((&EVENT, packet)) => Self::Event(packet.to_vec()),
            Some((&CLIPBOARD_SET, text)) => Self::ClipboardSet(String::from_utf8_lossy(text).into_owned()),
            Some((&CLIPBOARD_GET, [])) => Self::ClipboardGet,
            Some((&ACK, rest)) if rest.len() == 8 => Self::Ack {
                seq: u32_at(rest, 0),
                held_us: u32_at(rest, 4),
            },
            Some((&SIZE, rest)) if rest.len() == 10 => Self::Size(Size {
                width: u32_at(rest, 0),
                height: u32_at(rest, 4),
                scale_percent: u16_at(rest, 8),
            }),
            Some((&MIRROR, [mirror])) => Self::Mirror(*mirror != 0),
            Some((&(KEYFRAME | CLIPBOARD_GET | ACK | SIZE | MIRROR), _)) | None => {
                return Err(bad("a bad frame from the viewer".into()));
            }
            Some(_) => return Ok(None),
        }))
    }
}

fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(bytes[at..at + 2].try_into().expect("2"))
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
            Down::Picture(Picture {
                key: true,
                seq: 1,
                pts_us: 1 << 40,
                age_us: 9_000,
                net_us: 700,
                kbps: 50_000,
                skipped: 2,
                data: vec![0, 0, 0, 1, 0x40],
            }),
            Down::Picture(Picture {
                seq: u32::MAX,
                pts_us: 7,
                ..Picture::default()
            }),
            Down::Refused("Screen Recording is off".into()),
            Down::Clipboard("copied on the host".into()),
            Down::Cursor(Cursor {
                width: 2,
                height: 1,
                hot_x: 1,
                hot_y: 0,
                rgba: vec![0, 0, 0, 255, 255, 255, 255, 255],
            }),
            Down::Cursor(Cursor::default()),
        ] {
            assert_eq!(Down::decode(&msg.encode()).unwrap(), Some(msg));
        }
    }

    #[test]
    fn up_round_trips() {
        for msg in [
            Up::Keyframe,
            Up::Event(vec![1, 2, 3]),
            Up::ClipboardSet("copied here".into()),
            Up::ClipboardGet,
            Up::Ack {
                seq: 812,
                held_us: 3_100,
            },
            Up::Size(Size {
                width: 3635,
                height: 2102,
                scale_percent: 100,
            }),
            Up::Mirror(true),
            Up::Mirror(false),
        ] {
            assert_eq!(Up::decode(&msg.encode()).unwrap(), Some(msg));
        }
    }

    #[test]
    fn a_message_this_side_does_not_know_is_skipped() {
        assert_eq!(Down::decode(&[200, 1, 2, 3]).unwrap(), None);
        assert_eq!(Up::decode(&[200]).unwrap(), None);
    }

    #[test]
    fn a_pointer_image_must_be_its_size() {
        let mut cursor = Cursor {
            width: 2,
            height: 2,
            rgba: vec![0; 2 * 2 * 4],
            ..Cursor::default()
        };
        assert!(Down::decode(&Down::Cursor(cursor.clone()).encode()).is_ok());
        cursor.rgba.pop();
        assert!(Down::decode(&Down::Cursor(cursor.clone()).encode()).is_err());
        cursor.width = MAX_CURSOR + 1;
        cursor.rgba = vec![0; usize::from(cursor.width) * 2 * 4];
        assert!(Down::decode(&Down::Cursor(cursor).encode()).is_err());
    }

    #[test]
    fn short_frames_are_errors_not_panics() {
        assert!(Down::decode(&[]).is_err());
        assert!(Down::decode(&[DISPLAY, 1, 2]).is_err());
        assert!(Down::decode(&[PICTURE, 1]).is_err());
        assert!(Down::decode(&[PICTURE; PICTURE_HEADER]).is_err());
        assert!(Down::decode(&[CURSOR, 1, 0, 1]).is_err());
        assert!(Up::decode(&[]).is_err());
        assert!(Up::decode(&[KEYFRAME, 9]).is_err());
        assert!(Up::decode(&[ACK, 1, 2, 3]).is_err());
        assert!(Up::decode(&[SIZE, 1, 2, 3]).is_err());
        assert!(Up::decode(&[MIRROR]).is_err());
    }
}
