/// GPU discovery and renderer management.
///
/// Uses `udev` to find DRM GPU devices and `GpuManager` to manage
/// OpenGL ES renderers.
///
/// See: https://docs.rs/smithay/0.7.0/smithay/backend/udev/index.html
/// See: https://docs.rs/smithay/0.7.0/smithay/backend/renderer/multigpu/index.html
use std::path::Path;

use smithay::backend::drm::{DrmDeviceFd, DrmNode, NodeType};
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::backend::renderer::multigpu::GpuManager;
use smithay::backend::renderer::multigpu::gbm::GbmGlesBackend;
use smithay::backend::udev;

use crate::error::GpuError;

/// The concrete GpuManager type used throughout State.
pub type SolaGpuManager = GpuManager<GbmGlesBackend<GlesRenderer, DrmDeviceFd>>;

/// Find the primary GPU for the given seat name.
pub fn find_primary(seat: &str) -> Result<DrmNode, GpuError> {
    let path = udev::primary_gpu(seat)
        .map_err(|e| GpuError::NodeResolution {
            path: format!("/dev/dri (seat={seat})").into(),
            reason: e.to_string(),
        })?
        .ok_or_else(|| GpuError::NotFound {
            seat: seat.to_string(),
        })?;

    let node = DrmNode::from_path(&path).map_err(|e| GpuError::NodeResolution {
        path: path.clone(),
        reason: e.to_string(),
    })?;

    tracing::info!(?node, ?path, "found primary GPU");
    Ok(node)
}

/// Create a new GpuManager.
pub fn create_manager() -> Result<SolaGpuManager, GpuError> {
    let backend: GbmGlesBackend<GlesRenderer, DrmDeviceFd> = GbmGlesBackend::default();
    GpuManager::new(backend).map_err(|e| GpuError::ManagerCreation(format!("{e:?}")))
}

/// Get the render node for a DRM node, falling back to the node itself.
pub fn render_node_for(node: DrmNode) -> DrmNode {
    node.node_with_type(NodeType::Render)
        .and_then(|n| n.ok())
        .unwrap_or(node)
}

/// Check if a DRM device has any connected displays by reading sysfs.
///
/// Avoids opening devices that have no displays attached.
pub fn has_connected_display(path: &Path) -> bool {
    has_connected_display_in(path, Path::new("/sys/class/drm"))
}

/// Inner implementation that accepts a sysfs root for testability.
fn has_connected_display_in(dev_path: &Path, sysfs_dir: &Path) -> bool {
    let dev_name = match dev_path.file_name().and_then(|n| n.to_str()) {
        Some(name) => name,
        None => return true,
    };

    let entries = match std::fs::read_dir(sysfs_dir) {
        Ok(e) => e,
        Err(_) => return true,
    };

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
        let sysfs = mock_sysfs(&[("card1-DP-1", "connected"), ("card2-DP-10", "disconnected")]);
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
