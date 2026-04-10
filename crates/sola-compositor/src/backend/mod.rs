/// Hardware backend modules.
///
/// These modules handle the low-level hardware interaction: session management
/// (libseat), GPU discovery (udev), DRM device control, and input handling
/// (libinput).
pub mod device;
pub mod gpu;
pub mod input;
pub mod session;
pub mod socket;
pub mod udev;
