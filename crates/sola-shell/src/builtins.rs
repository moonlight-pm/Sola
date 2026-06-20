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
            // The dispatcher (`sola-browser`) selects the engine via
            // --engine / $SOLA_BROWSER_ENGINE / default-wpe and exec()s
            // the sibling binary. Each engine reports an engine-specific
            // app_id ("sola-browser-wpe" / "sola-browser-cef") so the shell
            // tracks both as distinct apps and either can run at the same time.
            app_id: "sola-browser-wpe".into(),
            label: "Browser (WPE)".into(),
            command: "/opt/sola/bin/sola-browser --engine wpe".into(),
            icon: "lucide/globe".into(),
        },
        Application {
            app_id: "sola-browser-cef".into(),
            label: "Browser (CEF)".into(),
            command: "/opt/sola/bin/sola-browser --engine cef".into(),
            icon: "lucide/earth".into(),
        },
        Application {
            app_id: "sola-kit".into(),
            label: "Kit".into(),
            command: "/opt/sola/bin/sola-kit".into(),
            icon: "lucide/palette".into(),
        },
    ]
}
