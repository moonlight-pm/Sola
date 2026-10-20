//! Upload to Oath's default origin (`store.oath.moonlight.pm`, an R2
//! bucket) through its S3 API with curl `--aws-sigv4`, with the write key
//! Oath's own publish uses: `OATH_ORIGIN_S3`, `OATH_ORIGIN_KEY_ID`,
//! `OATH_ORIGIN_SECRET` from the environment or `~/.config/oath/origin.env`.
//!
//! Layout, as Oath publishes: `pkg/<name>/<hash>.tar` (a tar of the
//! tree's contents) and `plan/<name>/<plan hash>.plan`.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Where machines fetch from (public read).
pub const PUBLIC: &str = "https://store.oath.moonlight.pm";

pub struct Origin {
    s3: String,
    id: String,
    secret: String,
}

impl Origin {
    pub fn from_env() -> Result<Self, String> {
        let file = config_file();
        let mut vars = BTreeMap::new();
        if let Ok(text) = std::fs::read_to_string(&file) {
            for line in text.lines().map(str::trim) {
                if line.starts_with('#') {
                    continue;
                }
                if let Some((k, v)) = line.split_once('=') {
                    vars.insert(k.trim().to_string(), v.trim().to_string());
                }
            }
        }
        let get = |k: &str| {
            std::env::var(k)
                .ok()
                .filter(|v| !v.is_empty())
                .or_else(|| vars.get(k).cloned())
                .ok_or_else(|| format!("{k} is in neither the environment nor {}", file.display()))
        };
        let s3 = get("OATH_ORIGIN_S3")?.trim_end_matches('/').to_string();
        if !s3.starts_with("https://") {
            return Err("OATH_ORIGIN_S3 must be an https:// bucket URL".into());
        }
        Ok(Self {
            s3,
            id: get("OATH_ORIGIN_KEY_ID")?,
            secret: get("OATH_ORIGIN_SECRET")?,
        })
    }

    /// Upload `file` as `key`. Keys named by a hash never change.
    pub fn put(&self, key: &str, file: &Path) -> Result<(), String> {
        let url = format!("{}/{key}", self.s3);
        let ctype = if key.ends_with(".tar") {
            "Content-Type: application/x-tar"
        } else {
            "Content-Type: text/plain; charset=utf-8"
        };
        let mut child = Command::new("curl")
            .args(["-fsS", "--aws-sigv4", "aws:amz:auto:s3", "-K", "-", "-T"])
            .arg(file)
            .args(["-H", ctype])
            .args(["-H", "Cache-Control: public, max-age=31536000, immutable"])
            .arg(&url)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("curl: {e}"))?;
        // The key goes on stdin (`-K -`), not in argv.
        child
            .stdin
            .take()
            .ok_or("curl stdin")?
            .write_all(format!("user = \"{}:{}\"\n", self.id, self.secret).as_bytes())
            .map_err(|e| format!("curl stdin: {e}"))?;
        let out = child.wait_with_output().map_err(|e| format!("curl: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "put {key}: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        Ok(())
    }
}

/// True when the public origin already serves `key`.
pub fn published(key: &str) -> bool {
    Command::new("curl")
        .args(["-fsSI", "-o", "/dev/null"])
        .arg(format!("{PUBLIC}/{key}"))
        .status()
        .is_ok_and(|s| s.success())
}

fn config_file() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_default()
        .join("oath/origin.env")
}
