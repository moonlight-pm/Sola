//! Blitz spike — browser sidebar chrome (Scratch HTML/CSS, Rust events).
//! Isolated crate; do not install; do not merge to master.

mod app;
mod iced_label;
mod strip;
mod tabs;

use dioxus_native::{LogicalSize, WindowAttributes};
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn main() {
    boot("sola-blitz-spike");
    // Blitz loads <img> via tokio::spawn. Without a runtime those fetches never run
    // and we get empty boxes (cyan borders, no bitmap).
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    let _enter = rt.enter();
    tracing::info!("tokio runtime entered for image fetches");
    let attrs = WindowAttributes::default()
        .with_title("Blitz spike · cosmic-text")
        .with_surface_size(LogicalSize::new(720.0, 640.0));
    dioxus_native::launch_cfg(app::app, vec![], vec![Box::new(attrs)]);
}

/// Kit-shaped pre-window boot without pulling `sola-kit` / iced.
fn boot(app_id: &str) {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        format!(
            "{}=info,sola_blitz_spike=info,blitz_net=info,blitz_dom=info",
            app_id.replace('-', "_")
        )
        .into()
    });
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .init();
    tracing::info!("{app_id} starting");
    let socket = activate_wayland_session(20_000);
    tracing::info!(socket = %socket, "wayland socket resolved");
    if wait_for_wayland_socket(&socket, 10_000) {
        tracing::info!(socket = %socket, "wayland socket ready");
    } else {
        tracing::warn!(socket = %socket, "wayland socket not present after 10s — connecting anyway");
    }
    activate_gpu_env();
}

fn runtime_dir() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
}

fn activate_wayland_session(timeout_ms: u64) -> String {
    let display = resolve_wayland_display(timeout_ms);
    // SAFETY: single-threaded pre-init, same contract as sola_core::env.
    unsafe { std::env::set_var("WAYLAND_DISPLAY", &display) };
    display
}

fn resolve_wayland_display(timeout_ms: u64) -> String {
    let start = Instant::now();
    let interval = Duration::from_millis(500);
    loop {
        let path = runtime_dir().join("sola-wayland");
        if let Ok(raw) = std::fs::read_to_string(&path) {
            let name = raw.trim();
            if !name.is_empty() {
                return name.to_string();
            }
        }
        if start.elapsed().as_millis() as u64 >= timeout_ms {
            break;
        }
        std::thread::sleep(interval);
    }
    if let Ok(v) = std::env::var("WAYLAND_DISPLAY") {
        if !v.is_empty() {
            return v;
        }
    }
    "wayland-0".to_string()
}

fn wait_for_wayland_socket(display: &str, timeout_ms: u64) -> bool {
    let path = runtime_dir().join(display);
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    let step = Duration::from_millis(50);
    loop {
        if path.exists() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(step);
    }
}

fn activate_gpu_env() {
    // SAFETY: single-threaded pre-init, same contract as sola_core::env.
    unsafe {
        if std::env::var_os("__EGL_VENDOR_LIBRARY_DIRS").is_none() {
            std::env::set_var(
                "__EGL_VENDOR_LIBRARY_DIRS",
                "/run/opengl-driver/share/glvnd/egl_vendor.d",
            );
        }
        if std::env::var_os("LIBVA_DRIVERS_PATH").is_none() {
            std::env::set_var("LIBVA_DRIVERS_PATH", "/run/opengl-driver/lib/dri");
        }
        if std::env::var_os("VK_ICD_FILENAMES").is_none() {
            std::env::set_var(
                "VK_ICD_FILENAMES",
                "/run/opengl-driver/share/vulkan/icd.d/nvidia_icd.x86_64.json",
            );
        }
        if std::env::var_os("GSETTINGS_BACKEND").is_none() {
            std::env::set_var("GSETTINGS_BACKEND", "memory");
        }
    }
}
