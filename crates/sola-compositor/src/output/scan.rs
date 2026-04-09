/// Output connector scanning and display discovery.
///
/// When a GPU is initialized (or a monitor is hot-plugged), we scan its DRM
/// connectors to find connected displays. Each connected connector + CRTC
/// pair becomes a Wayland output that clients can render to.
///
/// ## Key concepts
///
/// - **Connector**: A physical display port (HDMI, DisplayPort, etc.)
/// - **CRTC**: A display pipeline that reads from a framebuffer and drives
///   a connector. One CRTC can drive one connector at a time.
/// - **Mode**: A resolution + refresh rate combination (e.g., 2560x1440@144Hz).
///
/// See: https://docs.rs/smithay-drm-extras/0.1.0/smithay_drm_extras/drm_scanner/index.html
use smithay::backend::drm::DrmDevice;
use smithay::reexports::drm::control::ModeTypeFlags;
use smithay_drm_extras::drm_scanner::{DrmScanEvent, DrmScanner};

/// Scan a DRM device's connectors for connected displays.
///
/// Returns a list of (connector, crtc, mode, name) tuples for each
/// connected display. Called during initial device setup.
pub fn find_connected_outputs(
    scanner: &mut DrmScanner,
    drm: &DrmDevice,
) -> Vec<(
    smithay::reexports::drm::control::connector::Info,
    smithay::reexports::drm::control::crtc::Handle,
    smithay::reexports::drm::control::Mode,
    String,
)> {
    let scan_result = match scanner.scan_connectors(drm) {
        Ok(result) => result,
        Err(err) => {
            tracing::error!(?err, "failed to scan connectors");
            return vec![];
        }
    };

    let mut outputs = vec![];
    for event in scan_result {
        if let DrmScanEvent::Connected { connector, crtc } = event {
            if let Some(crtc) = crtc {
                let mode = connector
                    .modes()
                    .iter()
                    .find(|m| m.mode_type().contains(ModeTypeFlags::PREFERRED))
                    .or_else(|| connector.modes().first())
                    .copied();

                if let Some(mode) = mode {
                    let name = format!(
                        "{}-{}",
                        connector.interface().as_str(),
                        connector.interface_id()
                    );

                    tracing::info!(
                        %name,
                        width = mode.size().0,
                        height = mode.size().1,
                        refresh = mode.vrefresh(),
                        "display connected"
                    );

                    outputs.push((connector, crtc, mode, name));
                } else {
                    tracing::warn!(?connector, "connected display has no modes");
                }
            }
        }
    }

    outputs
}
