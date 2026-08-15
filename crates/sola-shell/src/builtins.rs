//! Apps that ship with Sola. The shell seeds these into its launcher
//! catalog at startup, ahead of any user-configured entries with the
//! same `app_id` (so a stray duplicate in `applications.json` can't
//! shadow them). They are not stored on disk and can't be edited from
//! the settings panel.
//!
//! Kept inside `sola-shell` (rather than `sola-core`) so editing the
//! list only rebuilds the shell, not every crate in the workspace.

use sola_bus::topics::Application;

pub fn builtin_apps() -> Vec<Application> {
    vec![
        Application {
            app_id: "sola-settings".into(),
            label: "Settings".into(),
            command: "/opt/sola/bin/sola-settings".into(),
            icon: "lucide/settings".into(),
        },
        Application {
            app_id: "sola-monitor".into(),
            label: "Monitor".into(),
            command: "/opt/sola/bin/sola-monitor".into(),
            icon: "lucide/monitor".into(),
        },
        Application {
            app_id: "sola-terminal".into(),
            label: "Terminal".into(),
            command: "/opt/sola/bin/sola-terminal".into(),
            icon: "lucide/terminal".into(),
        },
        Application {
            // Single-crate browser: iced chrome + CEF engine; app_id matches
            // the binary name (`sola-browser`).
            app_id: "sola-browser".into(),
            label: "Browser".into(),
            command: "/opt/sola/bin/sola-browser".into(),
            icon: "lucide/globe".into(),
        },
        Application {
            app_id: "sola-kit".into(),
            label: "Kit".into(),
            command: "/opt/sola/bin/sola-kit".into(),
            icon: "lucide/palette".into(),
        },
        Application {
            app_id: "sola-agent".into(),
            label: "Agent".into(),
            command: "/opt/sola/bin/sola-agent".into(),
            icon: "lucide/bot".into(),
        },
        Application {
            app_id: "sola-preview".into(),
            label: "Preview".into(),
            command: "/opt/sola/bin/sola-preview".into(),
            icon: "lucide/image".into(),
        },
        Application {
            app_id: "sola-paint".into(),
            label: "Paint".into(),
            command: "/opt/sola/bin/sola-paint".into(),
            // `brush` — simple 20px silhouette. Distinct from Preview
            // (`image`) and Kit (`palette`). The detailed `paintbrush`
            // glyph turns to noise at launcher density.
            icon: "lucide/brush".into(),
        },
        Application {
            app_id: "sola-mail".into(),
            label: "Mail".into(),
            command: "/opt/sola/bin/sola-mail".into(),
            icon: "lucide/mail".into(),
        },
        Application {
            app_id: "sola-arcade".into(),
            label: "Arcade".into(),
            command: "/opt/sola/bin/sola-arcade".into(),
            icon: "lucide/gamepad-2".into(),
        },
    ]
}
