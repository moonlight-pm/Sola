//! TOML config for sola-kvm (`~/.config/sola-kvm/config.toml`).

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::layout::{Align, Layout, LayoutSpec, Side};

/// Top-level config file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub peer: Peer,
    pub layout: LayoutSection,
    pub motion: Motion,
    pub bind: Bind,
    /// Primary output size. Phase C will pull this from sola-river /
    /// bus; until then these defaults (or overrides) drive layout math.
    pub primary: Primary,
    /// Text clipboard sync over TCP (same host/port as UDP peer).
    pub clipboard: Clipboard,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            peer: Peer::default(),
            layout: LayoutSection::default(),
            motion: Motion::default(),
            bind: Bind::default(),
            primary: Primary::default(),
            clipboard: Clipboard::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Peer {
    pub host: String,
    pub port: u16,
}

impl Default for Peer {
    fn default() -> Self {
        Self {
            // Desk default: ember LAN IP (update if DHCP changes).
            host: "10.0.0.133".into(),
            port: 4242,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LayoutSection {
    pub side: Side,
    pub align: Align,
    pub mac_width: i32,
    pub mac_height: i32,
    pub offset_x: Option<i32>,
    pub offset_y: Option<i32>,
}

impl Default for LayoutSection {
    fn default() -> Self {
        Self {
            side: Side::Right,
            align: Align::Bottom,
            mac_width: 2560,
            mac_height: 2880,
            offset_x: None,
            offset_y: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Motion {
    pub scale: f32,
    /// Must be within this many primary pixels of the shared edge before enter
    /// is allowed (default 64). Fights estimated-cursor drift.
    pub edge_band: i32,
    /// Outward rel motion while parked on the edge before enter (default 48).
    /// Feels like pushing through a soft barrier.
    pub enter_push: f32,
}

impl Default for Motion {
    fn default() -> Self {
        Self {
            scale: 1.0,
            edge_band: 64,
            enter_push: 48.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Bind {
    /// Optional emergency release key names (edge-return is primary).
    pub release: Vec<String>,
}

impl Default for Bind {
    fn default() -> Self {
        Self {
            release: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Primary {
    pub width: i32,
    pub height: i32,
}

impl Default for Primary {
    fn default() -> Self {
        // Current novus U4025QW logical size.
        Self {
            width: 5120,
            height: 2160,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Clipboard {
    pub enable: bool,
    /// Max UTF-8 bytes per Offer (default 1 MiB).
    pub max_bytes: u32,
    pub sync_on_enter: bool,
    pub sync_on_leave: bool,
}

impl Default for Clipboard {
    fn default() -> Self {
        Self {
            enable: true,
            max_bytes: 1_048_576,
            sync_on_enter: true,
            sync_on_leave: true,
        }
    }
}

impl Config {
    /// Default path: `$XDG_CONFIG_HOME/sola-kvm/config.toml` or
    /// `~/.config/sola-kvm/config.toml`.
    pub fn default_path() -> PathBuf {
        if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
            return PathBuf::from(xdg).join("sola-kvm").join("config.toml");
        }
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home)
                .join(".config")
                .join("sola-kvm")
                .join("config.toml");
        }
        PathBuf::from("sola-kvm-config.toml")
    }

    /// Load from path, or defaults if the file is missing.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = fs::read_to_string(path).map_err(|e| ConfigError::Io {
            path: path.display().to_string(),
            msg: e.to_string(),
        })?;
        Self::parse(&text)
    }

    /// Parse TOML text.
    pub fn parse(text: &str) -> Result<Self, ConfigError> {
        toml::from_str(text).map_err(|e| ConfigError::Parse(e.to_string()))
    }

    /// Build a [`Layout`] from this config (using `primary` size).
    pub fn layout(&self) -> Layout {
        Layout::compute(&LayoutSpec {
            primary_w: self.primary.width,
            primary_h: self.primary.height,
            mac_w: self.layout.mac_width,
            mac_h: self.layout.mac_height,
            side: self.layout.side,
            align: self.layout.align,
            scale: self.motion.scale,
            offset_x: self.layout.offset_x,
            offset_y: self.layout.offset_y,
            edge_band: self.motion.edge_band,
            enter_push: self.motion.enter_push,
        })
    }

    /// Peer socket address string `host:port`.
    pub fn peer_addr(&self) -> String {
        format!("{}:{}", self.peer.host, self.peer.port)
    }

    /// Write example config to disk (creates parent dirs).
    pub fn write_example(path: &Path) -> Result<(), ConfigError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| ConfigError::Io {
                path: parent.display().to_string(),
                msg: e.to_string(),
            })?;
        }
        let text = toml::to_string_pretty(&Self::default())
            .map_err(|e| ConfigError::Parse(e.to_string()))?;
        fs::write(path, text).map_err(|e| ConfigError::Io {
            path: path.display().to_string(),
            msg: e.to_string(),
        })?;
        Ok(())
    }
}

#[derive(Debug)]
pub enum ConfigError {
    Io { path: String, msg: String },
    Parse(String),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { path, msg } => write!(f, "config io {path}: {msg}"),
            Self::Parse(s) => write!(f, "config parse: {s}"),
        }
    }
}

impl std::error::Error for ConfigError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_design_example() {
        let text = r#"
[peer]
host = "10.0.0.21"
port = 4242

[layout]
side = "right"
align = "bottom"
mac_width = 2560
mac_height = 2880

[motion]
scale = 1.25

[bind]
release = []

[primary]
width = 5120
height = 2160
"#;
        let cfg = Config::parse(text).unwrap();
        assert_eq!(cfg.peer.host, "10.0.0.21");
        assert_eq!(cfg.peer.port, 4242);
        assert!((cfg.motion.scale - 1.25).abs() < f32::EPSILON);
        let layout = cfg.layout();
        assert_eq!(layout.origin_x, 5120);
        assert_eq!(layout.origin_y, -720);
    }

    #[test]
    fn missing_file_is_default() {
        let path = PathBuf::from("/tmp/sola-kvm-does-not-exist-xyz.toml");
        let cfg = Config::load(&path).unwrap();
        assert_eq!(cfg.peer.port, 4242);
    }

    #[test]
    fn roundtrip_write_example() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        Config::write_example(&path).unwrap();
        let cfg = Config::load(&path).unwrap();
        assert_eq!(cfg, Config::default());
    }
}
