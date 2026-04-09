/// GPU discovery and renderer management.
///
/// Uses `udev` to find DRM (Direct Rendering Manager) GPU devices attached
/// to the current seat, and `GpuManager` to manage OpenGL ES renderers for
/// those GPUs.
///
/// ## Key concepts
///
/// - **DRM node**: A file in `/dev/dri/` representing a GPU. "Primary" nodes
///   (`card0`) handle display output; "render" nodes (`renderD128`) handle
///   GPU compute/rendering without display privileges.
///
/// - **GpuManager**: Smithay's abstraction over one or more GPUs. Even with a
///   single GPU, it provides `single_renderer()` to create a scoped OpenGL
///   renderer. For multi-GPU setups it handles buffer copying between devices.
///
/// - **GbmGlesBackend**: The specific GPU backend strategy — GBM (Generic
///   Buffer Management) for buffer allocation + GLES (OpenGL ES) for rendering.
///   This is the standard path for modern Linux GPUs including NVIDIA.
///
/// See: https://docs.rs/smithay/0.7.0/smithay/backend/udev/index.html
/// See: https://docs.rs/smithay/0.7.0/smithay/backend/renderer/multigpu/index.html
use std::path::Path;

use smithay::backend::drm::{DrmDeviceFd, DrmNode, NodeType};
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::backend::renderer::multigpu::GpuManager;
use smithay::backend::renderer::multigpu::gbm::GbmGlesBackend;
use smithay::backend::udev;

/// The concrete GpuManager type used throughout Sola.
///
/// Generic params:
/// - `GlesRenderer` — the OpenGL ES renderer implementation
/// - `DrmDeviceFd` — the file descriptor type for GBM devices
pub type SolaGpuManager = GpuManager<GbmGlesBackend<GlesRenderer, DrmDeviceFd>>;

/// Find the primary GPU for the given seat name.
///
/// Uses udev to scan for DRM devices. Returns the DRM node of the primary
/// GPU, which is the one connected to the displays.
pub fn find_primary(seat: &str) -> anyhow::Result<DrmNode> {
    let path = udev::primary_gpu(seat)?
        .ok_or_else(|| anyhow::anyhow!("no GPU found for seat '{seat}'"))?;

    let node = DrmNode::from_path(&path)?;
    tracing::info!(?node, ?path, "found primary GPU");
    Ok(node)
}

/// Create a new GpuManager.
///
/// The manager starts empty — GPUs are added later via `add_node()` as DRM
/// devices are discovered.
pub fn create_manager() -> anyhow::Result<SolaGpuManager> {
    let backend: GbmGlesBackend<GlesRenderer, DrmDeviceFd> = GbmGlesBackend::default();
    let manager = GpuManager::new(backend)?;
    Ok(manager)
}

/// Get the render node for a DRM node, falling back to the node itself.
///
/// Render nodes (`/dev/dri/renderD128`) are preferred because they don't
/// require DRM master privileges for GPU operations. If no render node
/// exists (rare), we fall back to the primary node.
pub fn render_node_for(node: DrmNode) -> DrmNode {
    node.node_with_type(NodeType::Render)
        .and_then(|n| n.ok())
        .unwrap_or(node)
}

/// Check if a DRM device has any connected displays by reading sysfs.
///
/// This avoids opening the DRM device (which acquires DRM master and
/// triggers Smithay initialization) for GPUs that have no displays
/// attached. Opening and immediately dropping a DRM device causes noisy
/// "Failed to restore previous state" errors from Smithay's cleanup code.
///
/// Returns `true` if at least one connector reports "connected" status.
pub fn has_connected_display(path: &Path) -> bool {
    has_connected_display_in(path, Path::new("/sys/class/drm"))
}

/// Inner implementation that accepts a sysfs root for testability.
fn has_connected_display_in(dev_path: &Path, sysfs_dir: &Path) -> bool {
    // dev_path is like /dev/dri/card2 — extract "card2" and look in
    // sysfs_dir/card2-*/status for any "connected" connector.
    let dev_name = match dev_path.file_name().and_then(|n| n.to_str()) {
        Some(name) => name,
        None => return true, // Can't check, assume yes to be safe.
    };

    let entries = match std::fs::read_dir(sysfs_dir) {
        Ok(e) => e,
        Err(_) => return true, // Can't read sysfs, assume yes to be safe.
    };

    // Look for directories named "card2-DP-10", "card2-HDMI-A-1", etc.
    let prefix = format!("{dev_name}-");
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if !name_str.starts_with(&prefix) {
            continue;
        }

        let status_path = entry.path().join("status");
        if let Ok(status) = std::fs::read_to_string(&status_path) {
            if status.trim() == "connected" {
                return true;
            }
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Create a mock sysfs tree in a temp directory.
    /// Each entry is (connector_name, status).
    fn mock_sysfs(entries: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (name, status) in entries {
            let conn_dir = dir.path().join(name);
            fs::create_dir_all(&conn_dir).unwrap();
            fs::write(conn_dir.join("status"), status).unwrap();
        }
        dir
    }

    #[test]
    fn detects_connected_display() {
        let sysfs = mock_sysfs(&[
            ("card2-DP-10", "connected"),
            ("card2-DP-11", "disconnected"),
        ]);
        assert!(has_connected_display_in(
            Path::new("/dev/dri/card2"),
            sysfs.path()
        ));
    }

    #[test]
    fn detects_no_connected_display() {
        let sysfs = mock_sysfs(&[
            ("card1-DP-1", "disconnected"),
            ("card1-DP-2", "disconnected"),
        ]);
        assert!(!has_connected_display_in(
            Path::new("/dev/dri/card1"),
            sysfs.path()
        ));
    }

    #[test]
    fn ignores_other_cards() {
        let sysfs = mock_sysfs(&[
            ("card1-DP-1", "connected"),
            ("card2-DP-10", "disconnected"),
        ]);
        // card2 has no connected displays, even though card1 does.
        assert!(!has_connected_display_in(
            Path::new("/dev/dri/card2"),
            sysfs.path()
        ));
    }

    #[test]
    fn empty_sysfs_returns_false() {
        let sysfs = mock_sysfs(&[]);
        assert!(!has_connected_display_in(
            Path::new("/dev/dri/card0"),
            sysfs.path()
        ));
    }

    #[test]
    fn missing_sysfs_assumes_connected() {
        // If sysfs doesn't exist, assume yes to be safe.
        assert!(has_connected_display_in(
            Path::new("/dev/dri/card0"),
            Path::new("/nonexistent/sysfs/path")
        ));
    }

    #[test]
    fn handles_status_with_trailing_newline() {
        let sysfs = mock_sysfs(&[("card0-HDMI-A-1", "connected\n")]);
        assert!(has_connected_display_in(
            Path::new("/dev/dri/card0"),
            sysfs.path()
        ));
    }
}
