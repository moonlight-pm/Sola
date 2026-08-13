//! Length-prefixed bincode IPC between iced chrome and a headless CEF helper.
//!
//! One helper process per profile (own `root_cache_path`). The chrome process
//! never maps extra Wayland windows — helpers have no iced / no xdg_toplevel.

use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;

use serde::{Deserialize, Serialize};

use crate::engine::{EditCmd, NavCmd, TabInfo};
use crate::cef::engine::InputEvent;

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
    Shutdown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FromEngine {
    Ready {
        tabs: Vec<TabInfo>,
        active: u64,
    },
    Frame {
        tab_id: u64,
        width: u32,
        height: u32,
        pixels: Vec<u8>,
    },
    Tabs(Vec<TabInfo>),
    Active(u64),
    Cursor(u32),
    Clipboard(String),
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
}
