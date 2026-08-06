//! Install apply pipeline — **dry-run only** for the visual dogfood build.
//!
//! Real disk partitioning / nixos-install will land with the ISO path.
//! This module exists so the wizard can animate a believable progress
//! sequence without touching storage.

use std::time::Duration;

/// One step shown on the Installing screen.
#[derive(Debug, Clone)]
pub struct ProgressStep {
    pub label: &'static str,
    pub dwell: Duration,
}

/// Fixed policy applied after a real install (documented for the UI).
pub const HOSTNAME: &str = "sola";
pub const LOCALE: &str = "en_US.UTF-8";
pub const KEYBOARD: &str = "us (Mac)";

/// Progress steps for the dry-run installer.
pub fn dry_run_steps(username: &str, disk_path: &str) -> Vec<ProgressStep> {
    let _ = (username, disk_path);
    vec![
        ProgressStep {
            label: "Preparing disk…",
            dwell: Duration::from_millis(700),
        },
        ProgressStep {
            label: "Writing system…",
            dwell: Duration::from_millis(1100),
        },
        ProgressStep {
            label: "Installing Sola…",
            dwell: Duration::from_millis(900),
        },
        ProgressStep {
            label: "Creating user…",
            dwell: Duration::from_millis(500),
        },
        ProgressStep {
            label: "Detecting timezone…",
            dwell: Duration::from_millis(600),
        },
        ProgressStep {
            label: "Finishing…",
            dwell: Duration::from_millis(400),
        },
    ]
}
