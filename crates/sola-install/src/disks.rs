//! Enumerate install targets for the wizard.
//!
//! Uses `lsblk -J` when available. Falls back to synthetic demo disks so
//! the UI can be dogfooded without real block devices (or when `lsblk`
//! is missing). The live root disk is always filtered out of real lists.

use std::path::PathBuf;
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

/// List candidate whole disks (not partitions), excluding the live root disk.
pub fn list_disks() -> Vec<Disk> {
    let live = live_root_disk();
    if let Some(ref d) = live {
        tracing::info!(%d, "live root disk (excluded from targets)");
    }
    match list_lsblk() {
        Ok(disks) if !disks.is_empty() => disks
            .into_iter()
            .filter(|d| {
                if d.demo {
                    return true;
                }
                if let Some(ref live) = live {
                    let a = canonicalize_dev(&d.path);
                    let b = canonicalize_dev(live);
                    if a == b {
                        tracing::info!(disk = %d.path, "skip live root disk");
                        return false;
                    }
                }
                true
            })
            .collect(),
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

/// Parent disk of the filesystem mounted at `/`, e.g. `/dev/vda`.
pub fn live_root_disk() -> Option<String> {
    let out = Command::new("findmnt")
        .args(["-n", "-o", "SOURCE", "/"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let src = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if src.is_empty() {
        return None;
    }
    let src = canonicalize_dev(&src);
    // SOURCE is often a partition; ask lsblk for the parent disk.
    let out = Command::new("lsblk")
        .args(["-no", "PKNAME", &src])
        .output()
        .ok()?;
    if out.status.success() {
        let pk = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !pk.is_empty() {
            return Some(format!("/dev/{pk}"));
        }
    }
    // Already a whole disk?
    Some(src)
}

fn canonicalize_dev(path: &str) -> String {
    PathBuf::from(path)
        .canonicalize()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| path.to_string())
}

fn list_lsblk() -> Result<Vec<Disk>, String> {
    let out = Command::new("lsblk")
        .args([
            "-J",
            "-b",
            "-o",
            "NAME,PATH,SIZE,MODEL,TYPE",
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
        let kind = dev.kind();
        if kind != "disk" {
            continue;
        }
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
            name: "vdb".into(),
            path: "/dev/vdb".into(),
            size: "20G".into(),
            model: "QEMU target (demo)".into(),
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
