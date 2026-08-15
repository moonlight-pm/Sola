//! Length-prefixed bincode IPC between iced chrome and a headless CEF helper.
//!
//! Control messages stay on `engine.sock`. Pixel frames use a **separate**
//! `engine.frame.sock` so an 8–12 MiB blit cannot stall input / tab / cursor
//! messages behind a single writer lock.

use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;

use serde::{Deserialize, Serialize};

use crate::cef::engine::InputEvent;
use crate::cef::paint::DirtyRect;
use crate::engine::{EditCmd, NavCmd, PageContext, TabInfo};

/// Safety cap (4K BGRA ≈ 33 MiB). Larger is treated as a corrupt peer.
const MAX_MSG: u32 = 64 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ToEngine {
    Resize {
        width: u32,
        height: u32,
        scale: f64,
    },
    Input(InputEvent),
    Focus(bool),
    /// Whether this helper is the front profile. `false` hides every tab so
    /// parked profiles stop compositing and stop sending frames.
    SetFront(bool),
    Nav(NavCmd),
    Edit(EditCmd),
    PasteText(String),
    EvaluateJs(String),
    OpenTab {
        id: u64,
        url: String,
        title: String,
    },
    CloseTab(u64),
    SetActiveTab(u64),
    CancelDownload {
        id: u32,
    },
    Shutdown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FromEngine {
    Ready {
        tabs: Vec<TabInfo>,
        active: u64,
    },
    Tabs(Vec<TabInfo>),
    Active(u64),
    Cursor(u32),
    Clipboard(String),
    /// Composition caret in view pixels. `w == 0` clears the last box.
    ImeCaret {
        x: i32,
        y: i32,
        w: i32,
        h: i32,
    },
    Download(DownloadEvent),
    WebAuthn(WebAuthnEvent),
    /// Page right-click — chrome shows the kit context menu.
    PageContext(PageContext),
}

/// Helper → chrome WebAuthn intercept (page lives in the engine process).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebAuthnEvent {
    pub id: u64,
    pub action: String,
    pub origin: String,
    pub rp_id: String,
    pub public_key_json: String,
}

/// One download update from a helper. `id` is CEF's per-process download id.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadEvent {
    pub id: u32,
    pub filename: String,
    pub path: String,
    pub url: String,
    pub received: i64,
    pub total: i64,
    /// 0..=100, or `-1` if CEF does not know the size.
    pub percent: i32,
    pub state: DownloadPhase,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum DownloadPhase {
    Progress,
    Complete,
    Canceled,
    Failed,
}

/// Header for one raw frame on the dedicated frame socket. Pixels follow
/// as a bare BGRA blob (not bincode) so we do not walk 8 MiB through serde.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameMeta {
    pub tab_id: u64,
    pub width: u32,
    pub height: u32,
    /// Empty = the pixel blob is a full frame. Otherwise `pixels` is still
    /// a full `width × height` BGRA buffer (damage already applied by the
    /// helper); chrome may upload only these rects.
    pub dirty: Vec<DirtyRect>,
}

pub fn write_msg<T: Serialize>(stream: &mut UnixStream, msg: &T) -> io::Result<()> {
    let buf = bincode::serialize(msg).map_err(io::Error::other)?;
    let len = u32::try_from(buf.len()).map_err(io::Error::other)?;
    if len > MAX_MSG {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("ipc message too large: {len}"),
        ));
    }
    stream.write_all(&len.to_le_bytes())?;
    stream.write_all(&buf)?;
    stream.flush()
}

pub fn read_msg<T: for<'de> Deserialize<'de>>(stream: &mut UnixStream) -> io::Result<T> {
    let mut len_b = [0u8; 4];
    stream.read_exact(&mut len_b)?;
    let len = u32::from_le_bytes(len_b);
    if len == 0 || len > MAX_MSG {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("ipc length {len} out of range"),
        ));
    }
    let mut buf = vec![0u8; len as usize];
    stream.read_exact(&mut buf)?;
    bincode::deserialize(&buf).map_err(io::Error::other)
}

/// Write one CPU frame: small bincode header + raw BGRA. No serde on pixels.
pub fn write_frame(stream: &mut UnixStream, meta: &FrameMeta, pixels: &[u8]) -> io::Result<()> {
    let header = bincode::serialize(meta).map_err(io::Error::other)?;
    let hlen = u32::try_from(header.len()).map_err(io::Error::other)?;
    let plen = u32::try_from(pixels.len()).map_err(io::Error::other)?;
    if plen > MAX_MSG {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("frame too large: {plen}"),
        ));
    }
    stream.write_all(&hlen.to_le_bytes())?;
    stream.write_all(&header)?;
    stream.write_all(&plen.to_le_bytes())?;
    stream.write_all(pixels)?;
    stream.flush()
}

pub fn read_frame(stream: &mut UnixStream) -> io::Result<(FrameMeta, Vec<u8>)> {
    let mut hlen_b = [0u8; 4];
    stream.read_exact(&mut hlen_b)?;
    let hlen = u32::from_le_bytes(hlen_b);
    if hlen == 0 || hlen > 1024 * 1024 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("frame header length {hlen} out of range"),
        ));
    }
    let mut header = vec![0u8; hlen as usize];
    stream.read_exact(&mut header)?;
    let meta: FrameMeta = bincode::deserialize(&header).map_err(io::Error::other)?;
    let mut plen_b = [0u8; 4];
    stream.read_exact(&mut plen_b)?;
    let plen = u32::from_le_bytes(plen_b);
    if plen == 0 || plen > MAX_MSG {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("frame payload {plen} out of range"),
        ));
    }
    let mut pixels = vec![0u8; plen as usize];
    stream.read_exact(&mut pixels)?;
    Ok((meta, pixels))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixStream as Pair;

    #[test]
    fn round_trip_control() {
        let (mut a, mut b) = Pair::pair().unwrap();
        let msg = ToEngine::Nav(NavCmd::Reload);
        write_msg(&mut a, &msg).unwrap();
        let got: ToEngine = read_msg(&mut b).unwrap();
        match got {
            ToEngine::Nav(NavCmd::Reload) => {}
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn round_trip_set_front() {
        let (mut a, mut b) = Pair::pair().unwrap();
        write_msg(&mut a, &ToEngine::SetFront(false)).unwrap();
        let got: ToEngine = read_msg(&mut b).unwrap();
        assert!(matches!(got, ToEngine::SetFront(false)));
    }

    #[test]
    fn round_trip_raw_frame() {
        let (mut a, mut b) = Pair::pair().unwrap();
        let meta = FrameMeta {
            tab_id: 7,
            width: 2,
            height: 1,
            dirty: vec![DirtyRect {
                x: 0,
                y: 0,
                w: 1,
                h: 1,
            }],
        };
        let pixels = vec![1, 2, 3, 4, 5, 6, 7, 8];
        write_frame(&mut a, &meta, &pixels).unwrap();
        let (got, pix) = read_frame(&mut b).unwrap();
        assert_eq!(got.tab_id, 7);
        assert_eq!(got.width, 2);
        assert_eq!(pix, pixels);
    }

    #[test]
    fn round_trip_ime_caret() {
        let (mut a, mut b) = Pair::pair().unwrap();
        write_msg(
            &mut a,
            &FromEngine::ImeCaret {
                x: 8,
                y: 16,
                w: 2,
                h: 18,
            },
        )
        .unwrap();
        let got: FromEngine = read_msg(&mut b).unwrap();
        match got {
            FromEngine::ImeCaret { x, y, w, h } => {
                assert_eq!((x, y, w, h), (8, 16, 2, 18));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn round_trip_download() {
        let (mut a, mut b) = Pair::pair().unwrap();
        write_msg(
            &mut a,
            &FromEngine::Download(DownloadEvent {
                id: 3,
                filename: "a.pdf".into(),
                path: "/tmp/a.pdf".into(),
                url: "https://ex/a.pdf".into(),
                received: 10,
                total: 100,
                percent: 10,
                state: DownloadPhase::Progress,
            }),
        )
        .unwrap();
        let got: FromEngine = read_msg(&mut b).unwrap();
        match got {
            FromEngine::Download(ev) => {
                assert_eq!(ev.id, 3);
                assert_eq!(ev.filename, "a.pdf");
                assert_eq!(ev.percent, 10);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn round_trip_webauthn() {
        let (mut a, mut b) = Pair::pair().unwrap();
        write_msg(
            &mut a,
            &FromEngine::WebAuthn(WebAuthnEvent {
                id: 9,
                action: "get".into(),
                origin: "https://exchange.gemini.com".into(),
                rp_id: "gemini.com".into(),
                public_key_json: "{}".into(),
            }),
        )
        .unwrap();
        let got: FromEngine = read_msg(&mut b).unwrap();
        match got {
            FromEngine::WebAuthn(ev) => {
                assert_eq!(ev.id, 9);
                assert_eq!(ev.rp_id, "gemini.com");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn round_trip_page_context() {
        let (mut a, mut b) = Pair::pair().unwrap();
        write_msg(
            &mut a,
            &FromEngine::PageContext(PageContext {
                link_url: Some("https://ex/a".into()),
                editable: true,
                can_go_back: true,
                ..PageContext::default()
            }),
        )
        .unwrap();
        let got: FromEngine = read_msg(&mut b).unwrap();
        match got {
            FromEngine::PageContext(ctx) => {
                assert_eq!(ctx.link_url.as_deref(), Some("https://ex/a"));
                assert!(ctx.editable);
                assert!(ctx.can_go_back);
            }
            other => panic!("{other:?}"),
        }
    }
}
