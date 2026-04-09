/// DRM device management.
///
/// Handles the lifecycle of a single GPU: opening the device file, creating
/// the DRM and GBM devices, and tracking per-device state.
///
/// ## Key concepts
///
/// - **DRM device**: Kernel interface for display control (mode setting,
///   page flipping, plane management). Represented by `/dev/dri/cardN`.
///
/// - **GBM device**: Sits on top of DRM and provides GPU buffer allocation.
///   Buffers allocated here are used as render targets and scanout sources.
///
/// - **DrmDeviceFd**: Smithay's wrapper around the DRM file descriptor.
///   It acquires the "DRM master" lock on creation, which grants exclusive
///   display control. Only one process can be DRM master at a time.
///
/// See: https://docs.rs/smithay/0.7.0/smithay/backend/drm/struct.DrmDevice.html
/// See: https://docs.rs/smithay/0.7.0/smithay/backend/allocator/gbm/index.html
use std::path::Path;

use smithay::backend::allocator::gbm::GbmDevice;
use smithay::backend::drm::{DrmDevice, DrmDeviceFd, DrmDeviceNotifier, DrmNode};
use smithay::backend::session::Session;
use smithay::backend::session::libseat::LibSeatSession;
use smithay::reexports::drm::Device as DrmDeviceTrait;
use smithay::reexports::rustix::fs::OFlags;
use smithay::utils::DeviceFd;
use smithay_drm_extras::drm_scanner::DrmScanner;

use crate::output::render::{SolaOutput, SolaOutputManager};

/// Per-GPU device state, stored in `Sola::devices` after full initialization.
pub struct Device {
    /// Manages all DRM outputs (compositors) for this GPU.
    /// Owns the `DrmDevice` internally.
    pub output_manager: SolaOutputManager,
    /// Active display outputs, keyed by CRTC handle.
    pub outputs: std::collections::HashMap<smithay::reexports::drm::control::crtc::Handle, SolaOutput>,
    /// GPU buffer allocator.
    pub gbm: GbmDevice<DrmDeviceFd>,
    /// The render node for this GPU (used to get a renderer from GpuManager).
    pub render_node: DrmNode,
    /// Tracks connector hotplug events (monitors being plugged/unplugged).
    pub scanner: DrmScanner,
    /// Calloop token for the DRM event source (VBlank events).
    pub token: smithay::reexports::calloop::RegistrationToken,
}

/// Open and initialize a DRM + GBM device from a filesystem path.
///
/// The session handle is used to open the device file with the right
/// privileges (via libseat). Returns the raw components that `lib::init_device`
/// will assemble into a `Device`.
pub fn open(
    session: &mut LibSeatSession,
    path: &Path,
    node: DrmNode,
) -> anyhow::Result<(DrmDevice, DrmDeviceNotifier, GbmDevice<DrmDeviceFd>, DrmNode)> {
    // Open the DRM device file via libseat (handles privilege escalation).
    let fd = session.open(
        path,
        OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOCTTY | OFlags::NONBLOCK,
    )?;

    // Wrap in Smithay's DeviceFd (Arc<OwnedFd>) then DrmDeviceFd.
    // DrmDeviceFd acquires the DRM master lock on creation, giving us
    // exclusive control over the display hardware.
    let device_fd = DrmDeviceFd::new(DeviceFd::from(fd));

    // Create the DRM device (kernel mode-setting interface).
    // `true` = disable all connectors initially, so we start from a clean state.
    let (drm, notifier) = DrmDevice::new(device_fd.clone(), true)?;

    // Create the GBM device (GPU buffer allocator) from the same fd.
    let gbm = GbmDevice::new(device_fd)?;

    let render_node = super::gpu::render_node_for(node);
    tracing::info!(?node, ?render_node, ?path, "DRM device opened");

    Ok((drm, notifier, gbm, render_node))
}

/// Check whether a DRM device is an NVIDIA GPU.
///
/// NVIDIA's DRM driver has a known issue: overlay planes cause atomic
/// commit failures. When we detect NVIDIA, we must disable overlay plane
/// usage and stick to primary + cursor planes only.
pub fn is_nvidia(drm: &DrmDevice) -> bool {
    // `DrmDeviceTrait::get_driver` comes from the `drm` crate. We import
    // the trait explicitly because `DrmDevice` doesn't re-export it directly.
    drm.get_driver()
        .map(|driver| {
            let name = driver.name().to_string_lossy().to_lowercase();
            let desc = driver.description().to_string_lossy().to_lowercase();
            name.contains("nvidia") || desc.contains("nvidia")
        })
        .unwrap_or(false)
}
