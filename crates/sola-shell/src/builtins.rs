//! Apps that ship with Sola. The shell seeds these into its launcher
//! catalog at startup, ahead of any user-configured entries with the
//! same `app_id` (so a stray duplicate in `applications.json` can't
//! shadow them). They are not stored on disk and can't be edited from
//! the settings panel.
//!
//! Kept inside `sola-shell` (rather than `sola-core`) so editing the
//! list only rebuilds the shell, not every crate in the workspace.

use sola_bus::topics::Application;
use sola_core::env;

fn kit_command(name: &str) -> String {
    env::bin_path(name).display().to_string()
}

pub fn builtin_apps() -> Vec<Application> {
    vec![
        Application {
            app_id: "sola-settings".into(),
            label: "Settings".into(),
            command: kit_command("sola-settings"),
            icon: "lucide/settings".into(),
            ..Default::default()
        },
        Application {
            app_id: "sola-monitor".into(),
            label: "Monitor".into(),
            command: kit_command("sola-monitor"),
            icon: "lucide/monitor".into(),
            ..Default::default()
        },
        Application {
            app_id: "sola-terminal".into(),
            label: "Terminal".into(),
            command: kit_command("sola-terminal"),
            icon: "lucide/terminal".into(),
            ..Default::default()
        },
        Application {
            app_id: "sola-workspaces".into(),
            label: "Workspaces".into(),
            command: kit_command("sola-workspaces"),
            // `folders` — stacked project groups. Distinct from Terminal
            // (`terminal`).
            icon: "lucide/folders".into(),
            ..Default::default()
        },
        Application {
            // Single-crate browser: iced chrome + CEF engine; app_id matches
            // the binary name (`sola-browser`).
            app_id: "sola-browser".into(),
            label: "Browser".into(),
            command: kit_command("sola-browser"),
            icon: "lucide/globe".into(),
            ..Default::default()
        },
        Application {
            app_id: "sola-kit".into(),
            label: "Kit".into(),
            command: kit_command("sola-kit"),
            icon: "lucide/palette".into(),
            ..Default::default()
        },
        Application {
            app_id: "sola-preview".into(),
            label: "Preview".into(),
            command: kit_command("sola-preview"),
            icon: "lucide/image".into(),
            ..Default::default()
        },
        Application {
            app_id: "sola-paint".into(),
            label: "Paint".into(),
            command: kit_command("sola-paint"),
            // `brush` — simple 20px silhouette. Distinct from Preview
            // (`image`) and Kit (`palette`). The detailed `paintbrush`
            // glyph turns to noise at launcher density.
            icon: "lucide/brush".into(),
            ..Default::default()
        },
        Application {
            app_id: "sola-mail".into(),
            label: "Mail".into(),
            command: kit_command("sola-mail"),
            icon: "lucide/mail".into(),
            ..Default::default()
        },
        Application {
            app_id: "sola-calendar".into(),
            label: "Calendar".into(),
            command: "/opt/sola/bin/sola-calendar".into(),
            icon: "lucide/calendar".into(),
            ..Default::default()
        },
        Application {
            app_id: "sola-arcade".into(),
            label: "Arcade".into(),
            command: kit_command("sola-arcade"),
            icon: "lucide/gamepad-2".into(),
            ..Default::default()
        },
        Application {
            app_id: "sola-scope".into(),
            label: "Scope".into(),
            command: kit_command("sola-scope"),
            icon: "lucide/scan".into(),
            ..Default::default()
        },
        Application {
            app_id: "sola-spotify".into(),
            label: "Spotify".into(),
            command: kit_command("sola-spotify"),
            icon: "lucide/disc".into(),
            ..Default::default()
        },
    ]
}
