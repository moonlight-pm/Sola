/// DRM device management.
///
/// Handles opening GPU devices, creating DRM/GBM objects, and tracking
/// per-device state.
///
/// See: https://docs.rs/smithay/0.7.0/smithay/backend/drm/struct.DrmDevice.html
/// See: https://docs.rs/smithay/0.7.0/smithay/backend/allocator/gbm/index.html
use std::collections::HashMap;
use std::path::Path;

use smithay::backend::allocator::gbm::GbmDevice;
use smithay::backend::drm::{DrmDevice, DrmDeviceFd, DrmDeviceNotifier, DrmNode};
use smithay::backend::session::libseat::LibSeatSession;
use smithay::backend::session::Session;
use smithay::reexports::drm::control::crtc;
use smithay::reexports::drm::Device as DrmDeviceTrait;
use smithay::reexports::rustix::fs::OFlags;
use smithay::utils::DeviceFd;
use smithay_drm_extras::drm_scanner::DrmScanner;

use crate::error::DeviceError;
use crate::types::{SolaOutput, SolaOutputManager};

/// Per-GPU device state, stored in `Sola::devices` after full initialization.
pub struct Device {
    /// Manages all DRM outputs (compositors) for this GPU.
    pub output_manager: SolaOutputManager,
    /// Active display outputs, keyed by CRTC handle.
    pub outputs: HashMap<crtc::Handle, SolaOutput>,
    /// GPU buffer allocator.
    pub gbm: GbmDevice<DrmDeviceFd>,
    /// The render node for this GPU (used to get a renderer from GpuManager).
    pub render_node: DrmNode,
    /// Whether a page flip is pending (waiting for VBlank).
    /// When true, `render_all` skips this device to avoid competing with
    /// the VBlank-driven render loop.
    pub frame_pending: bool,
    /// Tracks connector hotplug events.
    pub scanner: DrmScanner,
    /// Calloop token for the DRM event source (VBlank events).
    pub token: smithay::reexports::calloop::RegistrationToken,
}

/// Open and initialize a DRM + GBM device from a filesystem path.
pub fn open(
    session: &mut LibSeatSession,
    path: &Path,
    node: DrmNode,
) -> Result<(DrmDevice, DrmDeviceNotifier, GbmDevice<DrmDeviceFd>, DrmNode), DeviceError> {
    let fd = session
        .open(
            path,
            OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOCTTY | OFlags::NONBLOCK,
        )
        .map_err(|e| DeviceError::Open {
            path: path.to_owned(),
            reason: e.to_string(),
        })?;

    let device_fd = DrmDeviceFd::new(DeviceFd::from(fd));

    let (drm, notifier) =
        DrmDevice::new(device_fd.clone(), true).map_err(|e| DeviceError::DrmInit {
            path: path.to_owned(),
            reason: e.to_string(),
        })?;

    let gbm = GbmDevice::new(device_fd).map_err(|e| DeviceError::GbmInit {
        path: path.to_owned(),
        source: e,
    })?;

    let render_node = super::gpu::render_node_for(node);
    tracing::info!(?node, ?render_node, ?path, "DRM device opened");

    Ok((drm, notifier, gbm, render_node))
}

/// Check whether a DRM device is an NVIDIA GPU.
///
/// NVIDIA's DRM driver has a known issue: overlay planes cause atomic
/// commit failures. We disable overlay planes when this returns true.
pub fn is_nvidia(drm: &DrmDevice) -> bool {
    drm.get_driver()
        .map(|driver| {
            let name = driver.name().to_string_lossy().to_lowercase();
            let desc = driver.description().to_string_lossy().to_lowercase();
            name.contains("nvidia") || desc.contains("nvidia")
        })
        .unwrap_or(false)
}
