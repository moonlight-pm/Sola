//! Enumerate install targets for the wizard.
//!
//! Uses `lsblk -J` when available. Falls back to synthetic demo disks so
//! the UI can be dogfooded without real block devices (or when `lsblk`
//! is missing). Real apply still requires an explicit opt-in later.

use std::process::Command;

use serde::Deserialize;

/// A disk the user may erase and install onto.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Disk {
    /// Kernel name, e.g. `nvme0n1` or `vda`.
    pub name: String,
    /// Path like `/dev/nvme0n1`.
    pub path: String,
    /// Human size, e.g. `512G`.
    pub size: String,
    /// Optional model / transport hint.
    pub model: String,
    /// True when this entry is synthetic (UI demo).
    pub demo: bool,
}

#[derive(Debug, Deserialize)]
struct LsblkOut {
    blockdevices: Vec<LsblkDev>,
}

#[derive(Debug, Deserialize)]
struct LsblkDev {
    name: String,
    #[serde(default)]
    path: Option<String>,
    /// Bytes as number (`-b`) or already-human string without `-b`.
    #[serde(default)]
    size: Option<serde_json::Value>,
    #[serde(default)]
    model: Option<String>,
    #[serde(rename = "type", default)]
    kind: Option<String>,
}

impl LsblkDev {
    fn kind(&self) -> &str {
        self.kind.as_deref().unwrap_or("disk")
    }

    fn size_label(&self) -> String {
        match &self.size {
            Some(serde_json::Value::Number(n)) => n
                .as_u64()
                .map(|b| human_bytes(&b.to_string()))
                .unwrap_or_else(|| n.to_string()),
            Some(serde_json::Value::String(s)) => {
                // Already human from lsblk without -b, or numeric string.
                if s.chars().all(|c| c.is_ascii_digit()) {
                    human_bytes(s)
                } else {
                    s.clone()
                }
            }
            _ => "?".into(),
        }
    }
}

/// List candidate whole disks (not partitions).
pub fn list_disks() -> Vec<Disk> {
    match list_lsblk() {
        Ok(disks) if !disks.is_empty() => disks,
        Ok(_) => {
            tracing::warn!("lsblk returned no disks; using demo list");
            demo_disks()
        }
        Err(e) => {
            tracing::warn!(error = %e, "lsblk failed; using demo list");
            demo_disks()
        }
    }
}

fn list_lsblk() -> Result<Vec<Disk>, String> {
    let out = Command::new("lsblk")
        .args([
            "-J",
            "-b",
            "-o",
            "NAME,PATH,SIZE,MODEL,TYPE",
            // Top-level only; we still filter by type.
        ])
        .output()
        .map_err(|e| format!("spawn lsblk: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "lsblk exit {}: {}",
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    let parsed: LsblkOut = serde_json::from_slice(&out.stdout)
        .map_err(|e| format!("parse lsblk json: {e}"))?;

    let mut disks = Vec::new();
    for dev in parsed.blockdevices {
        // Skip loop/rom; keep disk + some virtual disks (vda).
        let kind = dev.kind();
        if kind != "disk" {
            continue;
        }
        // Skip obvious non-targets.
        if dev.name.starts_with("loop") || dev.name.starts_with("sr") {
            continue;
        }
        let path = dev
            .path
            .clone()
            .unwrap_or_else(|| format!("/dev/{}", dev.name));
        let size = dev.size_label();
        let model = dev
            .model
            .clone()
            .unwrap_or_default()
            .trim()
            .to_string();
        disks.push(Disk {
            name: dev.name,
            path,
            size,
            model,
            demo: false,
        });
    }
    Ok(disks)
}

fn human_bytes(raw: &str) -> String {
    // lsblk -b gives bytes as a decimal string.
    let Ok(n) = raw.trim().parse::<u64>() else {
        return raw.to_string();
    };
    const K: f64 = 1024.0;
    let n = n as f64;
    if n >= K * K * K * K {
        format!("{:.1}T", n / (K * K * K * K))
    } else if n >= K * K * K {
        format!("{:.1}G", n / (K * K * K))
    } else if n >= K * K {
        format!("{:.1}M", n / (K * K))
    } else {
        format!("{n:.0}B")
    }
}

fn demo_disks() -> Vec<Disk> {
    vec![
        Disk {
            name: "vda".into(),
            path: "/dev/vda".into(),
            size: "64G".into(),
            model: "QEMU HARDDISK (demo)".into(),
            demo: true,
        },
        Disk {
            name: "nvme0n1".into(),
            path: "/dev/nvme0n1".into(),
            size: "1.0T".into(),
            model: "Demo NVMe (not real)".into(),
            demo: true,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_bytes_g() {
        assert_eq!(human_bytes("1073741824"), "1.0G");
    }
}
