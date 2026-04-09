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
    // path is like /dev/dri/card2 — extract "card2" and look in
    // /sys/class/drm/card2-*/status for any "connected" connector.
    let dev_name = match path.file_name().and_then(|n| n.to_str()) {
        Some(name) => name,
        None => return true, // Can't check, assume yes to be safe.
    };

    let sysfs_dir = Path::new("/sys/class/drm");
    let entries = match std::fs::read_dir(sysfs_dir) {
        Ok(e) => e,
        Err(_) => return true,
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
